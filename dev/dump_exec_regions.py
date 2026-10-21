"""Dump the executable regions of a running client, with their addresses.

The captures built from this (`dev/dumps_exec`, then flattened) are raw copies of
committed, executable memory at the addresses Windows reports — which is what makes
a live dump line up with the address algebra in `docs/`.

Read-only: OpenProcess(VM_READ | VM_QUERY), ReadProcessMemory only. Output goes to
an absolute path outside the repository.

Usage:
    python dump_exec_regions.py --pid N --out D:/ow-dumps/exec_YYYYMMDD
"""

from __future__ import annotations

import os
import sys

import dump_process as DP

EXECUTABLE = 0x10 | 0x20 | 0x40 | 0x80
SKIP = (DP.PAGE_NOACCESS | DP.PAGE_GUARD)


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else 0
    out = args[args.index("--out") + 1] if "--out" in args else ""
    if not pid or not out:
        print(__doc__)
        return 2
    handle = DP.open_target(pid)
    os.makedirs(out, exist_ok=True)
    chunk = 4 << 20
    saved = failed = count = 0
    manifest = []
    for base, size, protect, kind, _alloc in DP.regions(handle):
        if not (protect & EXECUTABLE) or (protect & SKIP):
            continue
        blob = bytearray()
        off = 0
        while off < size:
            data = DP.read_region(handle, base + off, min(chunk, size - off))
            if not data:
                failed += 1
                blob = bytearray()
                break
            blob += data
            off += chunk
        if not blob:
            continue
        name = f"{base:012x}_{size:08x}_{kind:08x}_{protect:08x}.bin"
        with open(os.path.join(out, name), "wb") as fh:
            fh.write(blob)
        manifest.append((base, len(blob), protect, kind, name))
        saved += len(blob)
        count += 1
    with open(os.path.join(out, "manifest.txt"), "w", encoding="utf-8") as fh:
        fh.write("# base size protect type file\n")
        for base, size, protect, kind, name in manifest:
            fh.write(f"{base:012x} {size:08x} {protect:08x} {kind:08x} {name}\n")
    print(f"{count} executable region(s), {saved/1048576:.1f} MB, {failed} unreadable chunk(s)")
    print(f"output: {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
