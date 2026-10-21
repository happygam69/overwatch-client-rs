"""Dump memory around an address from the captured regions.

    python peek.py 0x7ff757721900 [length]
"""

from __future__ import annotations

import sys

import regions as R


def main() -> int:
    space = R.Space(R.load())
    addr = int(sys.argv[1], 16)
    length = int(sys.argv[2], 16) if len(sys.argv) > 2 else 0x80

    reg = space.find(addr)
    print(f"{addr:#x}: {'in ' + reg.path.split(chr(92))[-1] if reg else 'NOT CAPTURED'}")

    data = space.read(addr, length)
    if data is None:
        # Fall back to whatever tail of the region exists
        data = space.read(addr, 1)
        if data is None:
            print("  (not present)")
            return 1
        print("  (partial)")

    for off in range(0, len(data), 16):
        row = data[off : off + 16]
        hexs = " ".join(f"{b:02x}" for b in row)
        text = "".join(chr(b) if 32 <= b < 127 else "." for b in row)
        print(f"  {addr + off:#014x}  {hexs:<47}  {text}")
        # show pointers that land in a captured region
        for i in range(0, len(row) - 7, 8):
            q = int.from_bytes(row[i : i + 8], "little")
            if 0x10000 < q < 0x800000000000 and space.find(q):
                s = space.cstring(q)
                if s:
                    print(f"                    ^ qword -> {q:#x} \"{s[:60]}\"")
                else:
                    print(f"                    ^ qword -> {q:#x}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
