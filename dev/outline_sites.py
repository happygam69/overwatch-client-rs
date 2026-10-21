"""Outline the function behind each address and disassemble around it.

Reads the v2b capture (or whatever OW_DUMPS names), walks backwards from each
address to the nearest padding boundary, and prints the annotated window.  The
starts this prints are what Ghidra needs to `create_function`; the capture
holds loader-final addresses, so the image Ghidra has must be the same session.

Usage:
    OW_DUMPS=dumps_v2b python outline_sites.py 0x7ff7c536825c 0x7ff7c5368690 ...
"""

from __future__ import annotations

import os
import sys

import regions as R

from disasm_field_sites import annotate, md, outline_function


def main() -> int:
    space = R.Space(R.load(R.capture_dir("dumps_v2b")))
    for text in sys.argv[1:]:
        addr = int(text, 16)
        reg = space.find(addr)
        where = f"region {reg.base:#x} protect {reg.protect:#x}" if reg else "no region"
        start, end = outline_function(space, addr)
        print(f"\n=== {addr:#018x}  ({where})")
        print(f"    outline: {start:#018x} .. {end:#018x}  ({end - start:#x} bytes)")

        # Head of the function, so the prologue and the argument setup show.
        head = space.read(start, 0x80)
        if head:
            print("    -- head --")
            for ins in md.disasm(head, start):
                print(f"    {ins.address:012x}  {annotate(space, ins)}")

        # A window centred on the site itself.
        print("    -- around the site --")
        before = min(0x80, addr - start)
        data = space.read(addr - before, before + 0x60)
        if not data:
            print("    (no bytes)")
            continue
        for ins in md.disasm(data, addr - before):
            marker = "  <<<" if ins.address <= addr < ins.address + ins.size else ""
            print(f"    {ins.address:012x}  {annotate(space, ins)}{marker}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
