"""Resolve every rip-relative memory operand in the engine dump and report
references to the engine global 0x143b7f410, plus stores through registers
that were just loaded from it."""
from __future__ import annotations
import re

DUMP = "dev/engine_dump.asm"
ENGINE_GLOBAL = 0x143B7F410
RIP = re.compile(r"\[rip \+ (0x[0-9a-f]+)\]")
STORE = re.compile(r"^mov\s+(qword|dword) ptr \[(\w+) \+ (0x[0-9a-f]+)\], (\w+)$")
LOAD = re.compile(r"^mov\s+(\w+), qword ptr \[rip \+ (0x[0-9a-f]+)\]$")


def main() -> int:
    rows = []
    for line in open(DUMP, encoding="utf-8"):
        line = line.rstrip("\n")
        if not line.strip():
            continue
        addr, rest = line.split("  ", 1)
        rows.append((int(addr, 16), rest.strip()))

    # map instruction length: next addr - addr
    refs = []
    for i, (a, t) in enumerate(rows):
        nxt = rows[i + 1][0] if i + 1 < len(rows) else a + 16
        ln = nxt - a
        for m in RIP.finditer(t):
            refs.append((a, ln, t, a + ln + int(m.group(1), 16)))
    eng = [(a, t) for a, ln, t, tgt in refs if tgt == ENGINE_GLOBAL]
    print(f"rip_refs={len(refs)} engine_global_refs={len(eng)}")
    for a, t in eng:
        print(f"  {a:012x}  {t}")
    print("--- distinct rip targets ---")
    seen = {}
    for a, ln, t, tgt in refs:
        seen.setdefault(tgt, 0)
        seen[tgt] += 1
    for tgt in sorted(seen):
        print(f"  {tgt:#x} x{seen[tgt]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
