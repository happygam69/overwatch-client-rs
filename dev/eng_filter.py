"""Filtered view of the engine dump: keep only decision-relevant instructions."""
from __future__ import annotations
import re
import sys

DUMP = "dev/engine_dump.asm"
KEEP_MNEM = {
    "call", "test", "je", "jne", "jmp", "cmp", "cmove", "cmovne", "sete", "setne",
    "movabs", "xor", "add", "sub", "ror", "shl", "shr", "or", "and", "lea", "ret",
    "jae", "jbe", "jg", "jl", "jge", "jle", "js", "jns",
}
STOREISH = re.compile(r"^mov\s+(qword|dword) ptr \[(?!rsp)(\w+)( \+ 0x[0-9a-f]+)?\],")
ECX = re.compile(r"^mov\s+ecx, (0x[0-9a-f]+)$")


def main() -> int:
    lo = int(sys.argv[1], 16) if len(sys.argv) > 1 else 0
    hi = int(sys.argv[2], 16) if len(sys.argv) > 2 else 1 << 64
    for line in open(DUMP, encoding="utf-8"):
        line = line.rstrip("\n")
        if not line.strip():
            continue
        addr, rest = line.split("  ", 1)
        a = int(addr, 16)
        if not (lo <= a <= hi):
            continue
        rest = rest.strip()
        mnem = rest.split(" ")[0]
        keep = mnem in KEEP_MNEM or STOREISH.match(rest) or ECX.match(rest)
        if mnem == "mov" and rest.startswith("mov") and " ptr [rip" in rest:
            keep = True
        if mnem in ("lea",) and "[rip" not in rest:
            keep = False
        if mnem == "mov" and not keep:
            pass
        if keep:
            print(f"{addr}  {rest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
