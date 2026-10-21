#!/usr/bin/env python3
"""Cross-check our texture decoders against an independent implementation.

`ow-tank-tex --guid 0x… --dump FILE` writes a small container: the level-0
block run, then our own decode of it, with the dimensions and format byte in a
header. This script feeds the *same* block bytes to a third-party decoder and
compares pixel for pixel — the point is to verify our tables and bit layout
without a second opinion from the same head.

Set up the oracle once (it is a wheel with a compiled BC7 decoder, kept out of
the repository):

    pip install --target D:/ow-dumps/bc7oracle texture2ddecoder

Then, per texture:

    cargo run -q -p ow-tank --bin tex -- --guid 0x… --dump D:/ow-dumps/t.dump
    python dev/tex_oracle.py D:/ow-dumps/t.dump

Exit code 0 means every pixel matched; 1 means a mismatch was printed (with the
first few differing pixels), 2 means the container or the oracle was missing.

## BC6H is the exception, and this script will not judge it

BC6H is the high-dynamic-range member of the family and `texture2ddecoder`
cannot check it usefully:

* `decode_bc6` returns **8-bit BGRA**, so every value above 1.0 is flattened to
  255 and every value below 0.0 to 0 — it cannot see the range the format
  exists for. (It also has no signedness argument: it decodes the `UF16`
  variant only, which is the one this install ships.)
* Its `UF16` path is not bit-exact either. Measured 2026-10-17 over 21 real
  BC6H assets (`--dump-spread 300`): 20 of 21 files agree pixel for pixel, and
  one (`0c0000000003e30b`) has 34 differing pixels out of 65,536, every one of
  them the oracle pinning a channel to 255 where the value is below 1.0
  (largest difference 184). Our values on those pixels are **bit-identical to
  `bcdec`**, so the divergence is on the oracle's side, not ours.

So `compare()` refuses BC6H by default and says so (exit code 2 = "cannot
judge"), rather than quietly passing or quietly failing. The check that *is*
the arbiter compares **half floats** against `bcdec` (iOrange's reference
implementation, which implements the specification's arithmetic step for step)
and lives in `dev/bc6h_oracle.py`; `dev/bc6h_tables.py` re-derives the mode,
field and partition tables from the published sources. The 8-bit comparison is
still available, for anyone who wants the number:

    python dev/tex_oracle.py --bc6h D:/ow-dumps/spread/spread_*.dump
    python dev/tex_oracle.py --bc6h --dir D:/ow-dumps/spread
"""

import os
import struct
import sys

ORACLE = os.environ.get("OW_TEX_ORACLE", "D:/ow-dumps/bc7oracle")
BC6H_DXGI = (94, 95, 96)


def load_oracle():
    if ORACLE not in sys.path:
        sys.path.insert(0, ORACLE)
    try:
        import texture2ddecoder  # noqa: F401
    except ImportError as e:
        print(f"oracle not importable from {ORACLE}: {e}")
        print("pip install --target %s texture2ddecoder" % ORACLE)
        sys.exit(2)
    return texture2ddecoder


def read_container(path):
    """The `OWTX` container `ow-tank-tex --dump` / `--dump-spread` writes."""
    raw = open(path, "rb").read()
    if raw[:4] != b"OWTX":
        print(f"{path}: not an OWTX dump")
        return None
    w, h, format_byte = struct.unpack_from("<IIB", raw, 4)
    (size,) = struct.unpack_from("<I", raw, 13)
    blocks = raw[17:17 + size]
    ours = raw[17 + size:17 + size + w * h * 4]
    return dict(w=w, h=h, stored=format_byte, dxgi=stored_to_dxgi(format_byte),
                blocks=blocks, ours=ours)


def stored_to_dxgi(stored):
    """The client stores one value higher from 26 up."""
    return stored - 1 if stored >= 0x1A else stored


def bc6h_is_not_judgeable(dxgi, stored):
    """Say why this oracle cannot arbitrate BC6H, rather than returning a
    verdict it is not entitled to."""
    print(f"{stored} (DXGI {dxgi}) is BC6H: this oracle returns 8-bit BGRA for it, "
          f"so it cannot see an HDR value outside 0..1 and its UF16 path is not "
          f"bit-exact on real assets either. The half-float arbiter is "
          f"dev/bc6h_oracle.py (against bcdec); the tables are re-derived by "
          f"dev/bc6h_tables.py. Run this script with --bc6h for the weak "
          f"8-bit comparison and its numbers.")
    return 2


def compare_bc6h_8bit(path):
    """The weak check, available but never mistaken for the real one: the same
    blocks through `texture2ddecoder.decode_bc6`, compared against our clamped
    8-bit view pixel for pixel. Every difference is printed and counted; none is
    absorbed, so this reports a non-zero exit whenever the two disagree."""
    c = read_container(path)
    if c is None:
        return 2
    if c["dxgi"] not in BC6H_DXGI:
        print(f"{os.path.basename(path)}: DXGI {c['dxgi']} is not BC6H")
        return 2
    oracle = load_oracle()
    w, h = c["w"], c["h"]
    theirs = bytearray(oracle.decode_bc6(bytes(c["blocks"]), w, h))
    for i in range(0, len(theirs), 4):
        theirs[i], theirs[i + 2] = theirs[i + 2], theirs[i]
    ours = c["ours"]
    if len(theirs) != len(ours):
        print(f"size mismatch: oracle {len(theirs)} bytes, ours {len(ours)}")
        return 1
    differing = []
    diff_count = 0
    max_delta = 0
    saturated = 0
    for i in range(0, len(ours), 4):
        a, b = ours[i:i + 4], theirs[i:i + 4]
        if a == b:
            continue
        diff_count += 1
        # Count the one shape of difference measured on this install: the oracle
        # pinning a channel to 255 where our value is below it. Reported, never
        # treated as agreement.
        if b[0] == 255 and a[0] < 255 or b[1] == 255 and a[1] < 255 or b[2] == 255 and a[2] < 255:
            saturated += 1
        for ch in range(4):
            max_delta = max(max_delta, abs(a[ch] - b[ch]))
        if len(differing) < 6:
            px = i // 4
            differing.append(f"  pixel ({px % w},{px // w}): ours {list(a)} oracle {list(b)}")
    if diff_count:
        print(f"{os.path.basename(path)}: {w}x{h} DXGI {c['dxgi']} BC6H: "
              f"{diff_count} of {w * h} pixels differ from the 8-bit oracle"
              f"{f', {saturated} of them the oracle saturating' if saturated else ''}"
              f" (largest channel delta {max_delta})")
        print("\n".join(differing))
        print("  our half floats on this file are the arbiter's business: "
              "dev/bc6h_oracle.py compares them to bcdec")
        return 1
    print(f"{os.path.basename(path)}: {w}x{h} DXGI {c['dxgi']} BC6H: {w * h} "
          f"pixels identical to the 8-bit oracle (a weak check: the half-float "
          f"comparison is dev/bc6h_oracle.py's)")
    return 0


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    if sys.argv[1] == "--bc6h":
        rest = sys.argv[2:]
        if not rest:
            print(__doc__)
            return 2
        paths = []
        for arg in rest:
            if arg == "--dir":
                continue
            if os.path.isdir(arg):
                paths.extend(sorted(os.path.join(arg, f)
                                    for f in os.listdir(arg) if f.endswith(".dump")))
            else:
                paths.append(arg)
        if not paths:
            print("no dumps to compare")
            return 2
        bad = 0
        checked = 0
        skipped = 0
        for p in paths:
            with open(p, "rb") as fh:
                head = fh.read(13)
            if head[:4] != b"OWTX":
                skipped += 1
                continue
            # The format byte sits after the two dimensions, at offset 12.
            stored = head[12]
            if stored_to_dxgi(stored) not in BC6H_DXGI:
                skipped += 1
                continue
            checked += 1
            if compare_bc6h_8bit(p) == 1:
                bad += 1
        print(f"{checked - bad} of {checked} BC6H dumps identical to the 8-bit "
              f"oracle (weak; dev/bc6h_oracle.py is the arbiter); "
              f"{skipped} non-BC6H dump(s) skipped")
        return 1 if bad else 0
    if sys.argv[1] == "--dir":
        paths = sorted(
            os.path.join(sys.argv[2], f)
            for f in os.listdir(sys.argv[2])
            if f.endswith(".dump")
        )
        if not paths:
            print(f"no .dump files in {sys.argv[2]}")
            return 2
        bad = 0
        # "Not judged" is counted apart from "identical": a format this oracle
        # cannot arbitrate (BC6H, the float families) must not be swept into a
        # pass count it never earned.
        judged = 0
        for p in paths:
            rc = compare(p)
            if rc == 1:
                bad += 1
            elif rc == 0:
                judged += 1
        print(f"{judged - bad} of {judged} judged dumps identical to the oracle; "
              f"{len(paths) - judged} not judged by this oracle (see each file's line)")
        return 1 if bad else 0
    return compare(sys.argv[1])


def compare(path: str) -> int:
    c = read_container(path)
    if c is None:
        return 2
    w, h, blocks, ours = c["w"], c["h"], c["blocks"], c["ours"]
    dxgi = c["dxgi"]

    oracle = load_oracle()
    # Only the formats this script can compare: the BCn families. The oracle
    # returns BGRA (measured with a mode-6 block whose channels are distinct:
    # R=255 G=129 B=1 comes back as 1,129,255,255), so its output is swapped
    # back to RGBA before comparing.
    if dxgi in BC6H_DXGI:
        return bc6h_is_not_judgeable(dxgi, c["stored"])
    if dxgi in (70, 71, 72):
        theirs = oracle.decode_bc1(blocks, w, h)
    elif dxgi in (73, 74, 75):
        theirs = oracle.decode_bc2(blocks, w, h)
    elif dxgi in (76, 77, 78):
        theirs = oracle.decode_bc3(blocks, w, h)
    elif dxgi in (79, 80, 81):
        theirs = oracle.decode_bc4(blocks, w, h)
    elif dxgi in (82, 83, 84):
        theirs = oracle.decode_bc5(blocks, w, h)
    elif dxgi in (97, 98, 99):
        theirs = oracle.decode_bc7(blocks, w, h)
    else:
        print(f"DXGI {dxgi} (stored {c['stored']}) is not comparable with this oracle")
        return 2
    theirs = bytearray(theirs)
    for i in range(0, len(theirs), 4):
        theirs[i], theirs[i + 2] = theirs[i + 2], theirs[i]

    if len(theirs) != len(ours):
        print(f"size mismatch: oracle {len(theirs)} bytes, ours {len(ours)}")
        return 1
    differing = []
    expected = 0
    max_delta = 0
    for i in range(0, len(ours), 4):
        a = ours[i : i + 4]
        b = theirs[i : i + 4]
        if a != b:
            # One documented divergence: in BC1's 3-colour mode the oracle
            # paints index 3 opaque black where the D3D specification (and
            # DirectXTex's own DecodeBC1) paint it transparent black. That is a
            # difference in alpha semantics, not in decoding, and it is the one
            # case where the two references disagree with each other.
            if a[3] == 0 and b[3] == 255 and a[:3] == b[:3] == bytes(3):
                expected += 1
                continue
            for c in range(4):
                max_delta = max(max_delta, abs(a[c] - b[c]))
            if len(differing) < 8:
                px = i // 4
                differing.append(
                    f"  pixel ({px % w},{px // w}): ours {list(a)} oracle {list(b)}"
                )
    if differing:
        print(f"{os.path.basename(path)}: {w}x{h} DXGI {dxgi}: MISMATCH")
        print("\n".join(differing))
        print(f"  max channel delta {max_delta}")
        return 1
    note = (
        f" ({expected} pixels differ only in BC1 3-colour alpha, as documented)"
        if expected
        else ""
    )
    print(
        f"{os.path.basename(path)}: {w}x{h} DXGI {dxgi}: {w*h} pixels identical to the oracle{note}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
