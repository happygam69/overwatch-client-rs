"""Read windows of a chunk asset through `ow-tank-look --hex` and decode them.

The chunk payloads are large (the practice range's collision chunk is
21,858,688 bytes) and this script never copies the whole asset: it asks the
existing `ow-tank-look --hex --at --len` dumper for the windows it needs and
parses the hex it prints. Windows are cached per (guid, offset, length) inside
one run.

Usage:
    python dev/chunk11_probe.py header            # payload +0x00 .. +0x60
    python dev/chunk11_probe.py elems             # the 0x58/0x38 arrays
    python dev/chunk11_probe.py at 0x14D3810 0x200
    python dev/chunk11_probe.py walk              # follow the header's RVAs

Nothing here writes to the install: `ow-tank-look` opens it read-only.
"""

from __future__ import annotations

import struct
import subprocess
import sys

GUID = 0x0DD0001100000688


def window(guid: int, at: int, length: int) -> bytes:
    """One hex window of an asset, via the sanctioned dumper."""
    # The dumper takes whole 16-byte rows, so ask for the next multiple up.
    want = (length + 15) & ~15
    out = subprocess.run(
        [
            "cargo", "run", "--release", "-q", "-p", "ow-tank", "--bin", "look",
            "--", "--hex", hex(guid), "--at", hex(at), "--len", str(want),
        ],
        capture_output=True,
        text=True,
    )
    if out.returncode != 0:
        raise SystemExit(f"look failed: {out.stderr[:2000]}")
    buf = bytearray()
    for line in out.stdout.splitlines():
        line = line.strip()
        if not line.startswith("0x"):
            continue
        # "<addr>  xx xx .. xx   [annotation]"
        parts = line.split()
        if len(parts) < 2 or not parts[0].startswith("0x"):
            continue
        for word in parts[1:]:
            if len(word) == 2:
                try:
                    buf.append(int(word, 16))
                except ValueError:
                    pass
            else:
                break
    return bytes(buf[:length])


def header(buf: bytes) -> None:
    q = struct.unpack_from("<6Q", buf, 0)
    counts = struct.unpack_from("<2I", buf, 8)
    print(f"+0x00 u64 {q[0]:#x}")
    print(f"+0x08 countA {counts[0]}   +0x0c countB {counts[1]}")
    print(f"+0x10 u64 {q[2]:#x}   +0x18 u64 {q[3]:#x}")
    print(f"+0x20 arrA  {q[4]:#x}   +0x28 arrB {q[5]:#x}")
    tail = struct.unpack_from("<2Q", buf, 0x30)
    print(f"+0x30 u64 {tail[0]:#018x}   +0x38 u64 {tail[1]:#x}")
    floats = struct.unpack_from("<12f", buf, 0x40)
    print("+0x40 " + " ".join(f"{v:.6g}" for v in floats))


def main(argv: list[str]) -> int:
    if not argv:
        print(__doc__)
        return 1
    cmd = argv[0]
    if cmd == "header":
        header(window(GUID, 0, 0x80))
        return 0
    if cmd == "at":
        at = int(argv[1], 16)
        ln = int(argv[2], 16)
        data = window(GUID, at, ln)
        for i in range(0, len(data), 16):
            row = data[i:i + 16]
            print(f"{at + i:08x}  {row.hex(' ')}  "
                  f"{''.join(chr(b) if 32 <= b < 127 else '.' for b in row)}")
        return 0
    if cmd == "elems":
        data = window(GUID, 0x14D3810, 0x24 * 0x58)
        for i in range(0x24):
            e = struct.unpack_from("<11Q", data, i * 0x58)
            print(f"A[{i:2d}] " + " ".join(f"{v:#012x}" for v in e))
        b = window(GUID, 0x14D8948, 0x38)
        print("B[0]  " + " ".join(f"{v:#012x}" for v in struct.unpack("<7Q", b)))
        return 0
    print(__doc__)
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
