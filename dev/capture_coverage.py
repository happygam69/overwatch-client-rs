"""Do we have the code for this address?

    python dev/capture_coverage.py <file-with-addresses>
    python dev/capture_coverage.py 0x7ff7c4ff0060 0x7ff7c6e84610

Answers, for every address asked about:

  * is the page in the **standing image** (`D:/ow-dumps/module_after_diff.bin`,
    the v2b image the docs are written against) warm - that is, does it hold
    code rather than zeros?
  * if it is not, which **run** captured it, and which run saw it stay
    **encrypted** (ciphertext, never decrypted).

An address on disk is a *v2b* address: base `0x7ff7c4d20000`.

The point is to answer "is the behaviour we still need readable?" with a
measurement instead of an assumption. A page that every run saw encrypted is
a capture gap: no amount of analysis recovers it, only a better capture (or a
different route to the same behaviour) does.
"""

from __future__ import annotations

import csv
import pathlib
import sys

V2B_BASE = 0x7FF7C4D20000
# The flat standing image is 0x4962000 bytes; the client's .text is 0x42E0000 of it.
V2B_SIZE = 0x4962000
PAGE = 4096
IMAGE = pathlib.Path(r"D:\ow-dumps\module_after_diff.bin")
# A newer merge of the same runs can be measured the same way:
#   python dev/warm_merge.py --base ... --out D:\ow-dumps\module_after_diff_v2.bin <runs>
IMAGE_V2 = pathlib.Path(r"D:\ow-dumps\module_after_diff_v2.bin")
RUNS = pathlib.Path(r"D:\ow-dumps\live_page_diff")
# The loader is a different module with its own base; the loader's own RVA is
# the address, and the warm loader image is flat (a file offset is an RVA).
LOADER_WARM = pathlib.Path(r"D:\ow-dumps\loader_after_diff.bin")


def warm_runs() -> dict[int, list[str]]:
    """v2b page -> the runs that captured it as readable code.

    A page only counts when it is *code*: the run's own entropy has to be in
    the code band (0.5..7.2). A page of uniform filler (4096 non-zero bytes,
    zero entropy) is readable memory, not code, and the merge rightly leaves
    it out.
    """
    out: dict[int, list[str]] = {}
    for run in sorted(RUNS.glob("run_*")):
        csv_path = run / "warm.csv"
        if not csv_path.is_file():
            continue
        with csv_path.open(encoding="utf-8", newline="") as f:
            for row in csv.DictReader(f):
                if row.get("module") != "Overwatch.exe":
                    continue
                try:
                    v2b = int(row["v2b"], 16)
                    nonzero = int(row["nonzero"])
                    entropy = float(row["entropy"])
                except (KeyError, ValueError):
                    continue
                if nonzero >= 64 and 0.5 <= entropy <= 7.2:
                    out.setdefault(v2b, []).append(run.name)
    return out


def encrypted_runs() -> dict[int, list[str]]:
    """v2b page -> the runs that saw it stay encrypted."""
    out: dict[int, list[str]] = {}
    for run in sorted(RUNS.glob("run_*")):
        csv_path = run / "encrypted.csv"
        if not csv_path.is_file():
            continue
        with csv_path.open(encoding="utf-8", newline="") as f:
            for row in csv.DictReader(f):
                if row.get("module") != "Overwatch.exe":
                    continue
                try:
                    v2b = int(row["v2b"], 16)
                except (KeyError, ValueError):
                    continue
                out.setdefault(v2b, []).append(run.name)
    return out


def image_page_state(image: pathlib.Path, base: int, addr: int) -> str:
    """warm / cold / outside - is this page code in the image?"""
    if addr < base:
        return "outside"
    off = addr - base
    if off + PAGE > image.stat().st_size:
        return "outside"
    with image.open("rb") as f:
        f.seek(off)
        data = f.read(PAGE)
    if not data:
        return "outside"
    if data.count(b"\x00") == len(data):
        return "cold"
    return "warm"


def deltas() -> list[tuple[str, int]]:
    """The run deltas: session address = v2b + delta."""
    out = []
    for run in sorted(RUNS.glob("run_*")):
        meta = run / "meta.json"
        if not meta.is_file():
            continue
        try:
            import json

            d = int(json.loads(meta.read_text(encoding="utf-8"))["delta"])
        except Exception:
            continue
        out.append((run.name, d))
    return out


DELTAS = deltas()

# The older frames the docs also use, from `docs/formats/character-controller.md`:
#   exec  = v2b - 0x71820000
#   range = v2b - 0x17AEC0000
OLD_FRAMES = {
    "exec": 0x71820000,
    "range": 0x17AEC0000,
}


def to_v2b(addr: int) -> tuple[int, str]:
    """Normalise an address to the v2b frame, saying which frame it came from."""
    if V2B_BASE <= addr < V2B_BASE + V2B_SIZE:
        return addr, "v2b"
    for name, d in DELTAS:
        v2b = addr - d
        if V2B_BASE <= v2b < V2B_BASE + V2B_SIZE:
            return v2b, f"session {name[4:12]}"
    for name, delta in OLD_FRAMES.items():
        v2b = addr + delta
        if V2B_BASE <= v2b < V2B_BASE + V2B_SIZE:
            return v2b, name
    return addr, "not in the v2b image"


def block_map(block: int = 0x200000) -> list[tuple[int, int, int]]:
    """(block rva, code bytes, total bytes) for the standing image."""
    out = []
    size = IMAGE.stat().st_size
    with IMAGE.open("rb") as f:
        off = 0
        while off < size:
            f.seek(off)
            data = f.read(block)
            if not data:
                break
            zeros = data.count(b"\x00")
            out.append((off, len(data) - zeros, len(data)))
            off += block
    return out


def merge_gain() -> None:
    """How much each run would add to the standing image if it were merged."""
    size = IMAGE.stat().st_size
    with IMAGE.open("rb") as f:
        for run in sorted(RUNS.glob("run_*")):
            csv_path = run / "warm.csv"
            if not csv_path.is_file():
                continue
            total = 0
            missing = 0
            for row in csv.DictReader(csv_path.open(encoding="utf-8", newline="")):
                if row.get("module") != "Overwatch.exe":
                    continue
                try:
                    v2b = int(row["v2b"], 16)
                    nonzero = int(row["nonzero"])
                except (KeyError, ValueError):
                    continue
                if nonzero <= 16:
                    continue
                off = v2b - V2B_BASE
                if off < 0 or off + PAGE > size:
                    continue
                total += 1
                f.seek(off)
                if f.read(PAGE).count(b"\x00") == PAGE:
                    missing += 1
            pct = 100 * missing / total if total else 0
            print(f"{run.name:<24} {total:>6} warm pages, {missing:>6} not in the image ({pct:.1f}%)")


def gaps() -> None:
    """The cold pages of the standing image: recoverable by re-merging, or not."""
    size = IMAGE.stat().st_size
    warm = warm_runs()
    enc = encrypted_runs()
    with IMAGE.open("rb") as f:
        cold = 0
        recoverable = 0
        never = 0
        recovered_rvas: list[int] = []
        for off in range(0, size, PAGE):
            f.seek(off)
            data = f.read(PAGE)
            if not data or data.count(b"\x00") != len(data):
                continue  # not a cold page
            cold += 1
            v2b = V2B_BASE + off
            if v2b in warm:
                recoverable += 1
                recovered_rvas.append(off)
            else:
                never += 1
    print(f"standing image: {cold} cold pages")
    print(f"  recoverable by re-merging the existing runs: {recoverable} "
          f"({100 * recoverable / cold:.1f}%)")
    print(f"  never captured (encrypted in every run):      {never} "
          f"({100 * never / cold:.1f}%)")
    # the recoverable ones, as contiguous ranges
    ranges: list[tuple[int, int]] = []
    for off in sorted(recovered_rvas):
        if ranges and off - ranges[-1][1] <= PAGE:
            ranges[-1] = (ranges[-1][0], off + PAGE)
        else:
            ranges.append((off, off + PAGE))
    print(f"\nrecoverable ranges (rva .. rva, v2b):")
    for lo, hi in ranges[:20]:
        print(f"  {lo:#010x}..{hi:#010x}  v2b {V2B_BASE + lo:#014x}")
    if len(ranges) > 20:
        print(f"  ... and {len(ranges) - 20} more")
    # the pages no run ever decrypted
    never_rvas: list[int] = []
    with IMAGE.open("rb") as f:
        for off in range(0, size, PAGE):
            f.seek(off)
            data = f.read(PAGE)
            if not data or data.count(b"\x00") != len(data):
                continue
            if (V2B_BASE + off) not in warm:
                never_rvas.append(off)
    never_ranges: list[tuple[int, int]] = []
    for off in sorted(never_rvas):
        if never_ranges and off - never_ranges[-1][1] <= PAGE:
            never_ranges[-1] = (never_ranges[-1][0], off + PAGE)
        else:
            never_ranges.append((off, off + PAGE))
    print(f"\nnever decrypted by any run ({len(never_rvas)} pages, "
          f"{len(never_ranges)} ranges):")
    for lo, hi in never_ranges:
        print(f"  {lo:#010x}..{hi:#010x}  v2b {V2B_BASE + lo:#014x}")


def main() -> int:
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return 2
    if args[0] == "--gaps":
        gaps()
        return 0
    if args[0] == "--merge-gain":
        merge_gain()
        return 0
    if args[0] == "--map":
        rows = block_map()
        print(f"{'rva':<12} {'code %':<9} {'zero %':<8} v2b")
        total_code = sum(r[1] for r in rows)
        total = sum(r[2] for r in rows)
        for rva, code, tot in rows:
            print(
                f"{rva:#010x} {100 * code / tot:>7.1f}% {100 * (tot - code) / tot:>7.1f}% "
                f"{V2B_BASE + rva:#014x}"
            )
        print(f"\nwhole image: {100 * total_code / total:.1f}% non-zero bytes")
        return 0
    wanted: list[tuple[str, int]] = []
    if pathlib.Path(args[0]).is_file():
        for line in pathlib.Path(args[0]).read_text(encoding="utf-8").splitlines():
            line = line.split("#", 1)[0].strip()
            if not line:
                continue
            label, _, addr = line.rpartition(" ")
            wanted.append((label.strip(), int(addr.strip(), 16)))
    else:
        wanted = [("", int(a, 16)) for a in args]

    warm = warm_runs()
    enc = encrypted_runs()
    print(f"{'address':<34} {'image':<8} {'runs':<6} note")
    cold = 0
    for label, addr in wanted:
        v2b, frame = to_v2b(addr)
        page = v2b & ~(PAGE - 1)
        state = image_page_state(IMAGE, V2B_BASE, v2b)
        runs = warm.get(page, [])
        note = ""
        if state == "warm":
            note = "in the standing image"
        elif runs:
            note = f"captured in {len(runs)} run(s), not merged"
        elif page in enc:
            note = "ENCRYPTED in every run that saw it"
        else:
            note = "no run captured it"
        if state != "warm":
            cold += 1
        print(
            f"{(label + ' ' if label else ''):<22}{addr:#018x} {state:<8} {len(runs):<6} "
            f"{note} [{frame}]"
        )
    print(f"\n{len(wanted) - cold} of {len(wanted)} addresses are readable in the standing image")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
