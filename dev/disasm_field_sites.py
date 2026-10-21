"""Find every reference to a known field hash in the captured code, and
disassemble around it.

Usage:
    python disasm_field_sites.py list            # where each hash appears
    python disasm_field_sites.py show <hash>     # disassemble one site set
    python disasm_field_sites.py func <hash>     # try to outline the function
"""

from __future__ import annotations

import os
import sys

import capstone

import regions as R

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
md.detail = True


def find_sites(space: R.Space, value: int) -> list[int]:
    needle = value.to_bytes(4, "little")
    hits = []
    for reg in space.regions:
        data = reg.data()
        start = 0
        while True:
            i = data.find(needle, start)
            if i < 0:
                break
            hits.append(reg.base + i)
            start = i + 1
    return hits


def classify(space: R.Space, addr: int) -> str:
    """Look at the bytes just before the immediate to guess the instruction."""
    for back in range(1, 8):
        chunk = space.read(addr - back, back + 4)
        if not chunk:
            continue
        insns = list(md.disasm(chunk, addr - back))
        for ins in insns:
            if ins.address + ins.size == addr + 4:
                return f"{ins.mnemonic} {ins.op_str}"
    return "?"


def annotate(space: R.Space, ins) -> str:
    """Add resolved memory targets to an instruction."""
    extra = []
    for op in ins.operands:
        if op.type == capstone.x86.X86_OP_MEM:
            m = op.mem
            base = 0
            if m.base == capstone.x86.X86_REG_RIP:
                base = ins.address + ins.size
            elif m.base == 0:
                base = 0
            else:
                continue
            target = base + m.disp
            s = space.cstring(target)
            if s and len(s) > 3:
                extra.append(f'"{s[:70]}"')
                continue
            q = space.u64(target)
            if q:
                s2 = space.cstring(q)
                if s2 and len(s2) > 3:
                    extra.append(f'-> "{s2[:70]}"')
                    continue
                r = space.find(q)
                if r is not None and 0x10000 < q < 0x7FFFFFFFFFFF:
                    extra.append(f"-> [{q:#x}]")
                    continue
            if target > 0x10000:
                extra.append(f"[{target:#x}]")
    if extra:
        return f"{ins.mnemonic:<7} {ins.op_str:<46} ; {' '.join(extra)}"
    return f"{ins.mnemonic:<7} {ins.op_str}"


def disasm_window(space: R.Space, addr: int, before: int = 0x180, after: int = 0x120) -> None:
    start = addr - before
    data = space.read(start, before + after)
    if not data:
        print(f"  (no bytes at {start:#x})")
        return
    for ins in md.disasm(data, start):
        marker = "  <<<" if ins.address <= addr < ins.address + ins.size else ""
        print(f"  {ins.address:012x}  {annotate(space, ins)}{marker}")


def outline_function(space: R.Space, addr: int) -> tuple[int, int]:
    """Walk backwards to the last padding/ret, forwards to the next one."""
    start = addr
    limit = addr - 0x2000
    probe = addr - 1
    while probe > limit:
        b = space.read(probe, 1)
        if b is None:
            break
        # int3 padding or a ret/leave boundary is the best guess for a function end
        if b == b"\xcc":
            run = 0
            p = probe
            while p > limit and space.read(p, 1) == b"\xcc":
                run += 1
                p -= 1
            if run >= 2:
                start = probe + 1
                break
        probe -= 1
    end = addr
    p = addr
    cap = addr + 0x4000
    while p < cap:
        chunk = space.read(p, 1)
        if chunk is None:
            break
        if chunk == b"\xcc":
            run = 0
            q = p
            while q < cap and space.read(q, 1) == b"\xcc":
                run += 1
                q += 1
            if run >= 3:
                end = p
                break
        p += 1
    return start, end


def main() -> int:
    space = R.Space(R.load())
    hashes = R.known_hashes(REPO)
    print(f"regions: {len(space.regions)}  known hashes: {len(hashes)}")

    if len(sys.argv) < 2:
        print(__doc__)
        return 1
    cmd = sys.argv[1]

    if cmd == "list":
        for value, name in sorted(hashes.items(), key=lambda kv: kv[1]):
            sites = find_sites(space, value)
            if not sites:
                continue
            kinds = {}
            for a in sites:
                k = classify(space, a).split()[0]
                kinds[k] = kinds.get(k, 0) + 1
            print(f"{value:#010x} {name:<28} {len(sites):3d}  {kinds}")
        return 0

    if cmd in ("show", "func"):
        value = int(sys.argv[2], 16)
        name = hashes.get(value, "?")
        sites = find_sites(space, value)
        print(f"{value:#010x} {name}: {len(sites)} site(s)")
        for a in sites:
            print(f"\n=== site {a:#x}  ({classify(space, a)})")
            if cmd == "func":
                start, end = outline_function(space, a)
                print(f"  function guess: {start:#x} .. {end:#x}  ({end - start} bytes)")
                disasm_window(space, a, before=min(0x600, a - start), after=min(0x400, end - a))
            else:
                disasm_window(space, a)
        return 0

    print(__doc__)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
