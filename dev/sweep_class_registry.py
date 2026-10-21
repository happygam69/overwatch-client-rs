"""Sweep the class-registration thunks into a class registry.

Every runtime class is registered by a small thunk:

    sub rsp, 0x98
    xor r9d, r9d
    cmp byte(ptr) [rip+len-flag], r9b          ; decode-once guard
    je skip
    test dword(ptr) [rip+N], 0xffffff          ; name length
    lea r8, [rip+K]                            ; 8-byte xor key
    loop: [key + i + 0xc] ^= key[(i&7)]        ; decode the name
    ...
    lea rax, [rip+X]                           ; static descriptor pointers
    mov  [rsp+0x88], rax
    mov  r8d, <CLASS_HASH>
    ...
    mov  dword(ptr) [rsp+0x20], <SIZE>
    call 0x7ff64c094b90                        ; the registrar

The sweep finds every call to the registrar (RIP-relative rel32), walks back to
the thunk's `sub rsp, 0x98`, forward-slices the register slots, decodes the
name the thunk encrypts for its own class, and reports hash, size, name, the
static descriptor pointers, and both mapped-image and heap-capture addresses.

Usage:
    python sweep_class_registry.py            # full sweep -> class_registry.txt
    python sweep_class_registry.py 0xADDR     # one thunk, verbose

    OW_DUMPS=dumps_v2 OW_DELTA=0x17aec0000 OW_OUT=class_registry_v2.txt \
        python sweep_class_registry.py        # same sweep, newer capture
"""

from __future__ import annotations

import re
import struct
import sys

import regions as R
import disasm_field_sites as D

REG_CALL = 0x7FF64C09_4B90 + R.DELTA   # mapped-image address of the registrar
# Heap copy of the same call: the heap module is the mapped one shifted by a
# session-specific amount, meaningful only inside dumps_range.  Newer captures
# sweep the mapped copy alone (a second copy only ever produced duplicates).
HEAP_TWIN = None if R.DELTA else REG_CALL + (0x161b4b9c7a0 - 0x7ff64c094b90)


def find_reg_calls(space: R.Space) -> list[int]:
    """All RIP-relative call sites that land on the registrar, in any copy.

    Vectorised: every 4-byte window at every alignment is an int32 candidate,
    the target is window-offset + 4 + value, and the E8 opcode check filters
    real `call rel32` instructions.
    """
    import numpy as np

    sites: list[int] = []
    targets = [REG_CALL] if HEAP_TWIN is None else [REG_CALL, HEAP_TWIN]
    targets = np.array(targets, dtype=np.int64)
    for reg in space.regions:
        d = reg.data()
        n = len(d)
        if n < 8:
            continue
        base = reg.base
        for shift in range(4):
            m = (n - shift - 4) // 4
            if m < 1:
                continue
            # rel32 is signed; sign-extend it so target = end-of-instruction + rel.
            v = np.frombuffer(d[shift:], dtype="<i4", count=m).astype(np.int64)
            offs = base + shift + 4 + np.arange(m, dtype=np.int64) * 4
            cand = offs + v
            hit = cand == targets[0]
            for t in targets[1:]:
                hit |= cand == t
            hit_idx = np.nonzero(hit)[0]
            for i in hit_idx:
                pos = int(i) * 4 + shift
                if pos >= 1 and d[pos - 1] == 0xE8:
                    sites.append(reg.base + pos - 1)
    return sites


def thunk_start(space: R.Space, call: int, limit: int = 0x400) -> int | None:
    """The `sub rsp, 0x98` that begins this thunk."""
    b = space.read(call - limit, limit)
    if not b:
        return None
    pat = b"\x48\x81\xec\x98\x00\x00\x00"
    pos = b.rfind(pat)
    while pos >= 0:
        addr = call - limit + pos
        if addr < call - 4:
            return addr
        pos = b.rfind(pat, 0, pos)
    return None


def slice_thunk(space: R.Space, thunk: int, call: int) -> dict | None:
    """Forward-slice from the thunk start to its call, parse the slots."""
    b = space.read(thunk, call - thunk + 8)
    if not b:
        return None
    out: dict = {
        "class_hash": None,
        "size": None,
        "len_addr": None,
        "key_addr": None,
        "literal_size": None,
        "statics": [],
    }
    rip = 0
    base = thunk
    for ins in D.md.disasm(b, base):
        rip_base = ins.address + ins.size
        ops = ins.op_str
        if ins.mnemonic == "mov" and ops.startswith("r8d, 0x"):
            v = int(ops.split(", ")[1], 16)
            if v > 0xFFFFFF:
                out["class_hash"] = v
        elif ins.mnemonic == "mov" and re.match(
            r"dword ptr \[rsp \+ 0x20\], 0x", ops
        ):
            out["size"] = int(ops.split("], ")[1], 16)
        elif ins.mnemonic == "mov" and re.match(
            r"dword ptr \[rsp \+ 0x28\], \d+$", ops
        ):
            out["literal_size"] = int(ops.split("], ")[1])
        elif ins.mnemonic == "mov" and ops.startswith("eax, dword ptr [rip"):
            m = re.search(r"\+ (0x[0-9a-f]+)\]", ops)
            if m:
                out["len_addr"] = rip_base + int(m.group(1), 16)
        elif ins.mnemonic == "lea" and (ops.startswith("r8, [rip")):
            m = re.search(r"\+ (0x[0-9a-f]+)\]", ops)
            if m:
                out["key_addr"] = rip_base + int(m.group(1), 16)
        elif ins.mnemonic == "lea" and ("[rip" in ops):
            m = re.search(r"\+ (0x[0-9a-f]+)\]", ops)
            if m:
                out["statics"].append(rip_base + int(m.group(1), 16))
        if ins.address >= call:
            break
    return out


def decode_name(space: R.Space, key_addr: int | None, len_addr: int | None) -> str | None:
    """The class name, as the client has already decoded it.

    The thunks describe the transformation (`[key + i + 0xc] ^= key[i&7]`, length
    dword at key+8) but it ran once at process start, so in the capture the
    buffer at key+0xc holds the *plaintext* already. Read that; also fall back
    to running the transform over whatever bytes sit there if the plaintext
    does not read as ASCII (covers regions captured before the decode ran).
    """
    if not key_addr or not len_addr:
        return None
    key = space.read(key_addr, 8)
    n_raw = space.read(key_addr + 8, 4)
    if not key or not n_raw:
        return None
    n = struct.unpack("<I", n_raw)[0] & 0xFFFFFF
    if not 1 <= n <= 96:
        return None
    buf = space.read(key_addr + 0xC, n)
    if not buf or len(buf) < n:
        return None
    candidates = [buf, bytes(buf[i] ^ key[i & 7] for i in range(n))]
    for text_bytes in candidates:
        try:
            text = text_bytes.decode("ascii")
        except UnicodeDecodeError:
            continue
        if all(32 <= ord(c) < 127 for c in text):
            return text
    return None


def main() -> int:
    space = R.Space(R.load(R.capture_dir("dumps_range")))

    if len(sys.argv) > 1:
        addr = int(sys.argv[1], 16)
        reg = space.find(addr) or space.find(addr - 8)
        print(f"target {addr:#x} region {reg.base:#x}" if reg else "unmapped")
        t = thunk_start(space, addr) or addr
        print(f"thunk start {t:#x}")
        info = slice_thunk(space, t, addr)
        print({k: (hex(v) if isinstance(v, int) else v) for k, v in info.items()})
        name = decode_name(space, info.get("key_addr"), info.get("len_addr"))
        print(f"name: {name}")
        return 0

    calls = find_reg_calls(space)
    print(f"registrar call sites: {len(calls)}")

    rows = []
    for site in sorted(set(calls)):
        # only mapped-image copies (the heap copy reports the dwarf data too)
        thunk = thunk_start(space, site)
        if thunk is None:
            continue
        info = slice_thunk(space, thunk, site)
        if not info or not info["class_hash"]:
            continue
        name = decode_name(space, info["key_addr"], info["len_addr"])
        rows.append(
            (
                info["class_hash"],
                info["size"],
                info["literal_size"],
                name,
                thunk,
                site,
                tuple(info["statics"][:6]),
            )
        )

    # Dedupe mapped/heap twins: same hash+size registered twice.
    seen = {}
    for row in rows:
        key = (row[0], row[1])
        seen.setdefault(key, []).append(row)

    out_path = R.out_name("class_registry.txt")
    with open(out_path, "w", encoding="utf-8") as fh:
        fh.write("# class_hash  size  literal_size  name  thunk  call_site  statics\n")
        for (h, sz), group in sorted(seen.items()):
            best = group[0]
            names = sorted({r[3] for r in group if r[3]})
            unk = ",".join(f"{r[5]:#x}" for r in group)
            lit = best[2] if best[2] is not None else 0
            size = sz if sz is not None else 0
            fh.write(
                f"{h:08x} {size:#05x} {lit} "
                f"{names[0] if names else '?'} "
                f"thunk@{best[4]:#x} sites[{unk}] "
                f"statics[{','.join(f'{a:#x}' for a in best[6])}]\n"
            )
    named = sum(1 for (h, _), g in seen.items() if any(r[3] for r in g))
    print(f"classes: {len(seen)} (deduped from {len(rows)} sites), named: {named}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
