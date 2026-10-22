"""Do we have the code for each anchor, in this build's image?

    python dev/anchor_check.py --old D:/ow-dumps/module_after_diff.bin \
                               --new D:/ow-dumps/module_v21d.bin \
                               --list dev/needed-code.txt

For every anchor the project still needs:

* **here** - the function's bytes are in the new image at the same address
  (the patch did not move it);
* **moved** - the same bytes are in the new image at another address (the
  patch moved the code): the new address is given;
* **cold** - the bytes are nowhere in the new image, and the page they would
  sit on is cold: the code was never captured;
* **rewritten** - the bytes are gone and the patch changed the function: the
  code is in the new build but not the same code, so it has to be found
  again (through the structure that points at it - a vtable, a registration).

The last one is the honest answer for a client that rewrote its own code.
"""

from __future__ import annotations

import argparse
import pathlib

PAGE = 4096
V2B_BASE = 0x7FF7C4D20000
WINDOW = 96


def page_state(blob: bytes, v2b: int) -> str:
    off = v2b - V2B_BASE
    if off < 0 or off + PAGE > len(blob):
        return "outside"
    page = blob[off : off + PAGE]
    return "cold" if page.count(0) == PAGE else "warm"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--old", required=True)
    ap.add_argument("--new", required=True)
    ap.add_argument("--list", default="dev/needed-code.txt")
    args = ap.parse_args()

    old = pathlib.Path(args.old).read_bytes()
    new = pathlib.Path(args.new).read_bytes()

    wanted = []
    for line in pathlib.Path(args.list).read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        system, addr = line.rsplit(" ", 1)
        wanted.append((system, int(addr, 16)))

    counts = {"here": 0, "moved": 0, "rewritten": 0, "cold": 0, "outside": 0}
    print(f"{'system':<11} {'address':<18} {'verdict':<10} where")
    for system, addr in wanted:
        off = addr - V2B_BASE
        needle = old[off : off + WINDOW]
        if len(needle) < WINDOW:
            verdict, where = "outside", ""
        elif needle.count(0) == WINDOW:
            verdict, where = "cold", "the old image had no code there either"
        elif new[off : off + WINDOW] == needle:
            verdict, where = "here", f"same address ({page_state(new, addr)})"
        else:
            at = new.find(needle)
            if at >= 0:
                verdict = "moved"
                where = f"{V2B_BASE + at:#x}"
            else:
                verdict = "rewritten"
                where = f"the page it would sit on is {page_state(new, addr)}"
        counts[verdict] += 1
        print(f"{system:<11} {addr:#018x} {verdict:<10} {where}")

    total = len(wanted)
    print(f"\n{total} anchors: "
          + ", ".join(f"{k} {v}" for k, v in counts.items() if v))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
