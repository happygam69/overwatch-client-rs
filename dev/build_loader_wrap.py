"""Build the loader's XOR regions as one PE, in the client's address frame.

    python dev/build_loader_wrap.py

The loader's .text is encrypted on disk, so the only readable copy of its code
is the warm flat image (`D:/ow-dumps/loader_after_diff.bin`, a dump of the
loader at its own base, so a file offset is an RVA). Four regions matter for
the page cipher: the applier candidate, its caller, and the two other
pad-base families.

The regions are wrapped in one minimal PE at the *client's* addresses
(`0x180000000` + RVA), so a function's address in the wrap is the address the
docs use. Import the result into the Ghidra project (folder `/ldr`) and the
functions can be read and named there.
"""

from __future__ import annotations

import pathlib
import subprocess
import sys

WARM = pathlib.Path(r"D:\ow-dumps\loader_after_diff.bin")
OUT_DIR = pathlib.Path(r"D:\ow-dumps\loader_regions")
WRAP = OUT_DIR / "loader_xor_regions.exe"
DISK_BASE = 0x180000000

# (name, rva, length)
REGIONS = [
    ("applier", 0x1438000, 0x17000),
    ("caller", 0x1015000, 0x2000),
    ("padb", 0x10C1000, 0x10000),
    ("padc", 0x10B7000, 0x5000),
]


def main() -> int:
    if not WARM.is_file():
        print(f"{WARM} is missing; capture the loader first")
        return 1
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    blob = WARM.read_bytes()
    pairs: list[str] = []
    for name, rva, length in REGIONS:
        data = blob[rva : rva + length]
        if len(data) != length:
            print(f"{name}: only {len(data)} of {length} bytes at {rva:#x}")
            return 1
        path = OUT_DIR / f"{name}_{rva:x}.bin"
        path.write_bytes(data)
        pairs.append(str(path))
        print(f"{name}: rva {rva:#x} +{length:#x} -> {path.name}")
    cmd = [sys.executable, "dev/mkpe.py", str(WRAP)] + [
        x for pair in zip([hex(DISK_BASE + r) for _, r, _ in REGIONS], pairs) for x in pair
    ]
    print(" ".join(cmd))
    subprocess.run(cmd, check=True)
    print(f"wrote {WRAP}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
