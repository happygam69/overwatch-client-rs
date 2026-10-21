"""Correlate a raw mover trace's slots with the recorded key timeline.

The mover capture (`live_trace.py capture` or `record --mover`) records the
component object raw: trace v2, one frame per sample with the object's bytes
and the key state. This tool is the Q3 instrument: it groups frames by key
combination and reports, per 4-byte slot, how the slot's value moves with the
input — which is how the input/desired-velocity slots get identified without a
working decompile (the mover's code is a capture gap; see
`docs/formats/character-controller.md`).

Usage:
    python analyze_mover_trace.py <trace.jsonl> [--size 0x130] [--top 40]

Output:

  * a label census (how many frames of each key combination),
  * slots ranked by the spread between the idle mean and the movement means,
    with the per-label mean printed, so a speed slot (0 -> 5.5) is obvious,
  * slots whose *u32* takes a small set of values that separate by keys, which
    is what an input bitfield looks like.

Read-only; writes nothing.
"""

from __future__ import annotations

import json
import struct
import sys
from collections import Counter, defaultdict


def label_of(keys: dict) -> str:
    w, a, s, d = (keys.get(k, 0) for k in ("w", "a", "s", "d"))
    fwd = "W" if w else ("S" if s else "")
    sid = "D" if d else ("A" if a else "")
    parts = [fwd + sid] if (fwd or sid) else ["idle"]
    if keys.get("ctrl"):
        parts.append("ctrl")
    if keys.get("space"):
        parts.append("jump")
    if keys.get("lmb"):
        parts.append("fire")
    return "+".join(parts)


def f32(b: bytes, off: int) -> float:
    return struct.unpack_from("<f", b, off)[0]


def u32(b: bytes, off: int) -> int:
    return struct.unpack_from("<I", b, off)[0]


def main() -> int:
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return 2
    path = args[0]
    size = int(args[args.index("--size") + 1], 16) if "--size" in args else 0x130
    top = int(args[args.index("--top") + 1]) if "--top" in args else 40

    frames = []  # (label, bytes)
    header = None
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            rec = json.loads(line)
            if "trace" in rec:
                header = rec
                size = int(header.get("size", size))
                continue
            raw = rec.get("raw")
            keys = rec.get("keys")
            if raw is None or keys is None:
                continue
            frames.append((label_of(keys), bytes.fromhex(raw)))

    if not frames:
        print("no raw frames with keys in this trace")
        return 1
    print(f"{path}: {len(frames)} frames, size 0x{size:x}, hz {header.get('hz') if header else '?'}")
    census = Counter(l for l, _ in frames)
    print("label census: " + ", ".join(f"{k}={v}" for k, v in census.most_common()))

    # Per-slot float means per label.
    slots = range(0, size - 3, 4)
    rows = []
    for off in slots:
        per = defaultdict(list)
        for label, b in frames:
            per[label].append(f32(b, off))
        if "idle" not in per:
            continue
        idle = sum(per["idle"]) / len(per["idle"])
        move = {k: sum(v) / len(v) for k, v in per.items() if k != "idle"}
        if not move:
            continue
        spread = max(abs(m - idle) for m in move.values())
        # a walk speed is ~5.5; a bitfield value is tiny and integer-ish
        rows.append((spread, off, idle, move))
    rows.sort(reverse=True)

    print("\n=== slots whose value tracks the input (f32) ===")
    for spread, off, idle, move in rows[:top]:
        parts = ", ".join(f"{k}:{v:+.4g}" for k, v in sorted(move.items())[:6])
        print(f"  +{off:#05x}  idle {idle:+10.5g}   spread {spread:9.4g}   {parts}")

    print("\n=== slots with few distinct u32 values that separate by label ===")
    bit_rows = []
    for off in slots:
        vals = defaultdict(set)
        for label, b in frames:
            vals[label].add(u32(b, off))
        allv = set().union(*vals.values()) if vals else set()
        if not allv or len(allv) > 8:
            continue
        idle_v = vals.get("idle", set())
        sep = any(v not in idle_v for k, s in vals.items() if k != "idle" for v in s)
        if sep:
            bit_rows.append((off, allv, {k: sorted(s) for k, s in sorted(vals.items())}))
    for off, allv, per in bit_rows[:top]:
        shown = ", ".join(f"{k}:{[hex(v) for v in v]}" for k, v in list(per.items())[:6])
        print(f"  +{off:#05x}  values {[hex(v) for v in sorted(allv)]}   {shown}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
