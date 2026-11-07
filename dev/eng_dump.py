"""Linear-sweep disassemble a VA range of the offline image, skipping bad bytes.

Usage: python dev/eng_dump.py <start_hex> <end_hex> [outfile]
Writes one instruction per line: "VA  mnemonic op_str".
"""
from __future__ import annotations
import sys
from capstone import CS_ARCH_X86, CS_MODE_64, Cs

IMAGE = "dev/data/offline_recover/module_offline.bin"
BASE = 0x140000000


def main() -> int:
    start = int(sys.argv[1], 16)
    end = int(sys.argv[2], 16)
    out = sys.argv[3] if len(sys.argv) > 3 else None
    with open(IMAGE, "rb") as f:
        f.seek(start - BASE)
        code = f.read(end - start)
    md = Cs(CS_ARCH_X86, CS_MODE_64)
    md.detail = True
    lines = []
    off = 0
    while off < len(code):
        got = None
        for ins in md.disasm(code[off:off + 16], start + off):
            got = ins
            break
        if got is None:
            lines.append(f"{start + off:012x}  .byte {code[off]:02x}")
            off += 1
            continue
        lines.append(f"{start + off:012x}  {got.mnemonic:<8} {got.op_str}")
        off += got.size
    text = "\n".join(lines) + "\n"
    if out:
        with open(out, "w", encoding="utf-8") as f:
            f.write(text)
        print(f"wrote {len(lines)} lines to {out}")
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
