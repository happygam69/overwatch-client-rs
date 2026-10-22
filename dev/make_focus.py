"""Build the focus file for the next capture.

    python dev/make_focus.py [--image D:/ow-dumps/module_v21_clean.bin]
                             [--run D:/ow-dumps/live_page_diff/run_...]
                             [--out D:/ow-dumps/live_page_diff/gaps_current.focus]

A capture is spent on what the tool happens to be looking at. The focus file
tells it where to look: the pages the last run could see but never read
(the run's own `gaps.focus`), plus the **cold regions of the current build** -
the code no capture has ever seen, which is the biggest remaining gap.

The output is one span per line, `rva:0xSTART-rva:0xEND`, the format
`dev/live_page_diff.py --focus-file` reads.
"""

from __future__ import annotations

import argparse
import pathlib

PAGE = 4096
TEXT_RVA = 0x1000
TEXT_SIZE = 0x2F2B3D3


def cold_spans(image: pathlib.Path, min_pages: int = 8) -> list[tuple[int, int]]:
    """The cold (all-zero) page runs of the client's .text."""
    blob = image.read_bytes()
    spans = []
    start = None
    for off in range(TEXT_RVA, TEXT_RVA + TEXT_SIZE, PAGE):
        page = blob[off : off + PAGE]
        cold = len(page) == PAGE and page.count(0) == PAGE
        if cold and start is None:
            start = off
        elif not cold and start is not None:
            if (off - start) // PAGE >= min_pages:
                spans.append((start, off))
            start = None
    if start is not None and (TEXT_RVA + TEXT_SIZE - start) // PAGE >= min_pages:
        spans.append((start, TEXT_RVA + TEXT_SIZE))
    return spans


def read_gaps(run: pathlib.Path) -> list[tuple[int, int]]:
    """The spans from a run's own gaps.focus."""
    path = run / "gaps.focus"
    if not path.is_file():
        return []
    out = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        try:
            a, b = line.split("-", 1)
            lo = int(a.split(":", 1)[1], 0)
            hi = int(b.split(":", 1)[1], 0)
        except (ValueError, IndexError):
            continue
        out.append((lo, hi))
    return out


def merge(spans: list[tuple[int, int]]) -> list[tuple[int, int]]:
    """Merge overlapping spans, page aligned."""
    out: list[tuple[int, int]] = []
    for lo, hi in sorted(spans):
        lo &= ~(PAGE - 1)
        hi = (hi + PAGE - 1) & ~(PAGE - 1)
        if out and lo <= out[-1][1]:
            out[-1] = (out[-1][0], max(out[-1][1], hi))
        else:
            out.append((lo, hi))
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--image", default=r"D:\ow-dumps\module_v21_clean.bin")
    ap.add_argument("--run", default="")
    ap.add_argument("--out", default=r"D:\ow-dumps\live_page_diff\gaps_current.focus")
    ap.add_argument("--min-pages", type=int, default=8)
    ap.add_argument("--top", type=int, default=0,
                    help="keep only the N largest cold regions (0 = all)")
    ap.add_argument("--no-cold", action="store_true",
                    help="the run's own gaps only")
    args = ap.parse_args()

    image = pathlib.Path(args.image)
    spans: list[tuple[int, int]] = []
    if not args.no_cold and image.is_file():
        cold = cold_spans(image, args.min_pages)
        if args.top:
            cold = sorted(cold, key=lambda s: s[1] - s[0], reverse=True)[: args.top]
            cold.sort()
        print(f"{image.name}: {len(cold)} cold .text regions "
              f"({sum((b - a) // PAGE for a, b in cold)} pages)")
        spans += cold
    elif not args.no_cold:
        print(f"{image} is missing; only the run's own gaps are used")

    run = pathlib.Path(args.run) if args.run else None
    if run is None:
        runs = sorted(pathlib.Path(r"D:\ow-dumps\live_page_diff").glob("run_*"),
                      key=lambda p: p.stat().st_mtime)
        run = runs[-1] if runs else None
    if run and (run / "gaps.focus").is_file():
        gaps = read_gaps(run)
        print(f"{run.name}: {len(gaps)} spans the run saw but never read")
        spans += gaps

    merged = merge(spans)
    pages = sum((b - a) // PAGE for a, b in merged)
    out = pathlib.Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("w", encoding="utf-8", newline="\n") as f:
        f.write("# focus file for the next capture\n")
        f.write(f"#   python dev/live_page_diff.py --focus-file {out}\n")
        f.write("# the cold .text of the current build plus the last run's own gaps\n")
        for lo, hi in merged:
            f.write(f"rva:0x{lo:x}-rva:0x{hi:x}\n")
    print(f"wrote {out}: {len(merged)} spans, {pages} pages")
    print("  (focus pages are polled every pass - each one costs read budget "
          "continuously, so keep the list as short as it can be)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
