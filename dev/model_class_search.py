"""Search the reference STU class definitions for a class whose declared field
offsets match a set read from the client's own code.

The client's code pins the *offsets* a descriptor's fields live at; the
reference checkout's generated `[STU(...)]` / `[STUField(...)]` declarations
name a candidate class for those offsets. The match is a check on the client's
registration, never a substitute for it: `dev/model_registry.py` gives the
registered size, and the class's constructor is read in the image afterwards.

Usage:
    python dev/model_class_search.py --offset 0x98 --offset 0x110 \
        --offset 0x178 --offset 0x186 --offset 0x194 [--min-size 0x190]

Every offset is compared against the declaration's own field offset.
"""

from __future__ import annotations

import argparse
import collections
import re
from pathlib import Path

CHECKOUT = Path(r"E:\OWLib")
CLASS_RE = re.compile(r"\[STU\(0x([0-9A-Fa-f]+),\s*(\d+)\)\]")
ATTR_RE = re.compile(r"\[STUField\(0x([0-9A-Fa-f]+),\s*(\d+)(?:,\s*([^\]]*))?\)\]")
DECL_RE = re.compile(r"(?:public|internal|private|protected)\s+([\w<>\[\]., ]+?)\s+(\w+)\s*(?:=|;)")


def sources() -> list[Path]:
    dirs = [
        CHECKOUT / "TankLib" / "STU" / "Types",
        CHECKOUT / "TACTLib" / "TankLib" / "STU" / "Types",
    ]
    root = CHECKOUT / "TankLib" / "STU"
    dirs.append(root / "Types")
    out: list[Path] = []
    for d in dict.fromkeys(dirs):
        if d.is_dir():
            out.extend(sorted(d.rglob("*.cs")))
    return out


def parse(path: Path) -> list[dict]:
    text = path.read_text(encoding="utf-8", errors="replace")
    classes = []
    cur = None
    pending = None
    for line in text.splitlines():
        m = CLASS_RE.search(line)
        if m:
            if cur:
                classes.append(cur)
            cur = {"hash": int(m.group(1), 16), "size": int(m.group(2)),
                   "fields": [], "name": None, "path": path}
            pending = None
            continue
        m = ATTR_RE.search(line)
        if m and cur is not None:
            pending = {"hash": int(m.group(1), 16),
                       "offset": int(m.group(2)),
                       "reader": (m.group(3) or "").strip()}
            continue
        if pending is not None and cur is not None:
            d = DECL_RE.search(line)
            if d:
                pending["type"] = d.group(1).strip()
                pending["name"] = d.group(2)
                cur["fields"].append(pending)
                pending = None
        if cur is not None and cur["name"] is None:
            m = re.search(r"class\s+(\w+)", line)
            if m:
                cur["name"] = m.group(1)
    if cur:
        classes.append(cur)
    return classes


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--offset", action="append", default=[])
    ap.add_argument("--min-size", default=None)
    ap.add_argument("--exact-size", default=None)
    args = ap.parse_args()

    want = sorted(int(o, 16) for o in args.offset)
    min_size = int(args.min_size, 16) if args.min_size else 0
    classes = []
    for path in sources():
        classes.extend(parse(path))
    print(f"{len(classes)} declared classes over {len(sources())} files")

    rows = []
    for c in classes:
        offs = {f["offset"] for f in c["fields"]}
        hit = [o for o in want if o in offs]
        score = len(hit)
        if score and c["size"] >= min_size:
            rows.append((score, c))
    rows.sort(key=lambda r: (-r[0], r[1]["size"]))

    for score, c in rows[:60]:
        print(f"{score}/{len(want)} 0x{c['hash']:08X} size {c['size']:>5} "
              f"({c['size']:#x}) {c['name']}  fields {len(c['fields'])}")
        for f in c["fields"]:
            mark = "*" if f["offset"] in want else " "
            print(f"     {mark} +{f['offset']:<5} {f['offset']:#06x} 0x{f['hash']:08X} "
                  f"{f['name']:<32} {f['type']:<22} {f['reader']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
