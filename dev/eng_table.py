"""Reconstruct the engine-slot table of ConstructEngineSubsystems (0x140554220).

Reads the linear-sweep dump written by dev/eng_dump.py and walks it as a
sequence of alloc-blocks:

    xor edx,edx / mov ecx,SIZE / call Alloc
    test rax,rax / je L / mov rcx,rax / call CTOR / jmp L2 / L: mov rax,rbp
    mov rdx,rax / mov [rbx+SLOT],rax / mov rcx,rbx / call 0x141c52d10

Anything that does not fit is reported as an anomaly with its raw window.
"""
from __future__ import annotations
import re
import sys

DUMP = "dev/engine_dump.asm"
ALLOC = "0x141baff30"
RWI = "0x141c52d10"
ATTACH = "0x140bc7590"

STORE_RE = re.compile(r"^mov\s+(qword|dword) ptr \[(\w+) \+ (0x[0-9a-f]+)\], (\w+)$")


def parse(path):
    out = []
    for line in open(path, encoding="utf-8"):
        line = line.rstrip("\n")
        if not line.strip():
            continue
        addr, rest = line.split("  ", 1)
        rest = rest.strip()
        if rest.startswith(".byte"):
            out.append((int(addr, 16), ".byte", rest[6:].strip()))
            continue
        mnem, _, ops = rest.partition(" ")
        out.append((int(addr, 16), mnem, ops.strip()))
    return out


def main() -> int:
    ins = parse(DUMP)
    n = len(ins)
    alloc_idx = [i for i, (_, m, o) in enumerate(ins) if m == "call" and o == ALLOC]
    rwi_idx = [i for i, (_, m, o) in enumerate(ins) if m == "call" and o == RWI]
    attach_idx = [i for i, (_, m, o) in enumerate(ins) if m == "call" and o == ATTACH]
    print(f"instructions={n}")
    print(f"alloc_calls={len(alloc_idx)} rwi_calls={len(rwi_idx)} attach_calls={len(attach_idx)}")
    stores = []
    for i, (a, m, o) in enumerate(ins):
        mm = STORE_RE.match(f"{m} {o}")
        if mm:
            stores.append((i, a, mm.group(2), int(mm.group(3), 16), mm.group(4), mm.group(1)))
    print(f"total_store_shaped={len(stores)}")
    for s in stores:
        print(f"  store {s[1]:012x} base={s[2]} off={s[3]:#05x} src={s[4]} {s[5]}")
    print("--- alloc blocks ---")
    for k, i in enumerate(alloc_idx):
        lo = max(0, i - 6)
        hi = min(n, i + 24)
        # size
        size = "?"
        for j in range(i - 1, max(-1, i - 5), -1):
            if ins[j][1].startswith("mov") and ins[j][2].startswith("ecx,"):
                size = ins[j][2].split(",")[1].strip()
                break
        # find next rwi after i
        nxt = [r for r in rwi_idx if r > i]
        rwi = nxt[0] if nxt else None
        win = range(i, (rwi + 1) if rwi is not None else min(n, i + 24))
        ctor = None
        st = None
        for j in win:
            a, m, o = ins[j]
            if m == "call" and o not in (ALLOC,):
                if ctor is None:
                    ctor = (a, o)
            mm = STORE_RE.match(f"{m} {o}")
            if mm and mm.group(2) == "rbx":
                st = (a, int(mm.group(3), 16), mm.group(4))
        print(f"[{k:02d}] alloc@{ins[i][0]:012x} size={size} ctor={ctor} store={st} rwi@{ins[rwi][0]:012x}" if rwi is not None else f"[{k:02d}] alloc@{ins[i][0]:012x} size={size} ctor={ctor} store={st} rwi=NONE")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
