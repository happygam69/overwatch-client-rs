"""Audit the ground-truth image's function bookkeeping: `.pdata` facts, bridge counts and the name table.

    python dev/audit_image.py                      # fast audit (about a minute)
    python dev/audit_image.py --registry           # + full bridge registry sweep (~20 min, 8 ways)
    python dev/audit_image.py --no-bridge          # image and name table only

It answers one open question: do the image, the bridge and `docs/function-names.csv`
agree on what the program's 201k+ `.pdata` bodies are? Reports, in order:

  - the oracle image (path, size, sha256) and its `.pdata`: entry count, unique
    beginnings, chained/fragmented entry count, body-size histogram;
  - the Ghidra bridge (if up): total functions, custom-named, thunks, externals,
    and with `--registry` how many of its functions sit on a `.pdata` beginning
    versus further analyser discoveries;
  - the name table (docs/function-names.csv): rows and how many are applied.

Read-only against the image and the project.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import pathlib
import struct
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
IMG = ROOT / "dev" / "data" / "offline_recover" / "module_offline.bin"
PDATA_RVA = 0x450B000
IMAGE_BASE = 0x140000000
TABLE = ROOT / "docs" / "function-names.csv"

HEAD = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HEAD))
import gh  # noqa: E402


def parse_pdata(data: bytes) -> dict:
    """Read the plaintext .pdata run and report its structure."""
    img_offset = PDATA_RVA  # flat image: file offset == RVA
    if len(data) < img_offset + 12:
        raise SystemExit(f"image too small for .pdata at {PDATA_RVA:#x}")
    # Walk entries while they look like RUNTIME_FUNCTION triplets: begin < end,
    # end < image size, unwind_rva < image size, begins non-decreasing.
    n = (len(data) - img_offset) // 12
    count = 0
    prev_begin = 0
    begins: list[int] = []
    sizes: list[int] = []
    buckets = {lo: 0 for lo in (0, 64, 256, 1024, 4096, 16384, 65536)}
    img_size = 0x4838000
    while count < min(n, 400_000):
        begin, end, unwind = struct.unpack_from("<III", data, img_offset + count * 12)
        if begin >= end or end >= img_size or unwind >= img_size:
            break
        if count and begin < prev_begin:
            break
        prev_begin = begin
        begins.append(begin)
        sizes.append(end - begin)
        size = end - begin
        b = 65536
        for lo in (0, 64, 256, 1024, 4096, 16384):
            if size < lo:
                break
            b = lo
        buckets[b] += 1
        count += 1
    unique = {begin for begin in begins}
    chains = len(begins) - len(unique)
    sha = hashlib.sha256(data).hexdigest()
    return {
        "image_bytes": len(data),
        "sha256_16": sha[:16],
        "entries": count,
        "unique_begins": len(unique),
        "chained_entries": chains,
        "largest": max(sizes) if sizes else 0,
        "buckets": buckets,
    }


def bridge_facts() -> dict:
    facts: dict[str, object] = {}
    r = gh.call("/get_function_count")
    facts["total"] = r.get("function_count", r)
    for key, params in {
        "custom_named": {"has_custom_name": "true"},
        "thunks": {"is_thunk": "true"},
        "external": {"is_external": "true"},
    }.items():
        try:
            r = gh.call("/find_functions", {**params, "limit": "1"})
            facts[key] = r.get("total", "?")
        except Exception as exc:  # bridge down mid-run
            facts[key] = f"unavailable: {exc}"
    names_here: set[str] = set()
    off = 0
    limit = 1000
    while True:
        r = gh.call(
            "/find_functions",
            {"has_custom_name": "true", "limit": str(limit), "offset": str(off), "sort_by": "ADDRESS"},
        )
        for fn in r.get("functions", []):
            names_here.add(fn.get("name", ""))
        if r.get("offset", off) + r.get("count", 0) >= r.get("total", 0) or not r.get("count"):
            break
        off = r.get("offset", off) + r.get("count", 0)
    facts["names_in_program"] = len(names_here)
    facts["_names_set"] = names_here
    return facts


def name_table(names_in_program: set[str] | None) -> dict:
    rows = []
    with TABLE.open(encoding="utf-8", newline="") as f:
        for row in csv.DictReader(f):
            rows.append(row)
    return {
        "rows": len(rows),
        "names_in_program": (
            sum(1 for r in rows if r["name"] in names_in_program) if names_in_program else None
        ),
    }


def pdata_begins(data: bytes) -> list[int]:
    """Return every `.pdata` entry's begin RVA (no validation - parse_pdata did it)."""
    n = (len(data) - PDATA_RVA) // 12
    begins = []
    prev = 0
    for i in range(n):
        begin, end, unwind = struct.unpack_from("<III", data, PDATA_RVA + i * 12)
        if begin >= end or end >= 0x4838000 or unwind >= 0x4838000 or (i and begin < prev):
            break
        prev = begin
        begins.append(begin)
    return begins


def registry_sweep(begins: set[int]) -> dict:
    """Page the whole bridge function registry (slow); classify it against .pdata.

    Pages are fetched eight at a time; the bridge mostly serialises and the
    wall time is what it is (~5 s per 1000-entry page).
    """
    import concurrent.futures as cf

    first = gh.call("/find_functions", {"limit": "1"})
    total = int(first.get("total", 0))
    offsets = list(range(0, total, 1000))

    def page(offset: int) -> list[str]:
        r = gh.call(
            "/find_functions", {"limit": "1000", "offset": str(offset), "sort_by": "ADDRESS"}
        )
        return [fn.get("address", "") for fn in r.get("functions", [])]

    addrs: list[str] = []
    with cf.ThreadPoolExecutor(8) as ex:
        for part in ex.map(page, offsets):
            addrs.extend(part)
    at_pdata = 0
    extras: list[str] = []
    for addr in addrs:
        try:
            a = int(addr, 16)
        except ValueError:
            continue
        # the bridge reports absolute addresses; .pdata stores RVAs
        if a < IMAGE_BASE:
            continue
        if a - IMAGE_BASE in begins:
            at_pdata += 1
        elif len(extras) < 5:
            extras.append(a)
    return {"paged": len(addrs), "at_pdata": at_pdata, "extras": extras}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--no-bridge", action="store_true")
    ap.add_argument("--registry", action="store_true",
                    help="also sweep the full function registry (about 20 min)")
    args = ap.parse_args()

    data = IMG.read_bytes()
    p = parse_pdata(data)
    print(f"image      : {IMG.relative_to(ROOT)}  {p['image_bytes']:,} B  sha256 {p['sha256_16']}\u2026")
    print(
        f".pdata     : {p['entries']:,} entries, {p['unique_begins']:,} unique beginnings, "
        f"{p['chained_entries']:,} chained, largest body {p['largest']:,} B"
    )
    print("size histogram (body bytes):")
    cuts = [(0, 64), (64, 256), (256, 1024), (1024, 4096), (4096, 16384), (16384, 65536), (65536, 1 << 40)]
    shown = 0
    for lo, hi in cuts:
        n = p["buckets"].get(lo, 0)
        shown += n
        print(f"  [{lo:>6}, {hi:>6})  {n:>8,}")
    if shown != p["entries"]:
        print(f"  (bucket sum {shown:,} vs entries {p['entries']:,})")

    bf: dict | None = None
    if not args.no_bridge:
        try:
            bf = bridge_facts()
            names = bf.pop("_names_set")
            bf["csv"] = name_table(names)
        except Exception as exc:
            print(f"bridge     : unavailable ({exc}) - run with the GUI bridge up for the rest")
            bf = None

    if bf is not None:
        begins = set(pdata_begins(data))
        if args.registry:
            s = registry_sweep(begins)
            extra = s["paged"] - s["at_pdata"]
            print(
                f"registry   : {s['paged']:,} paged; {s['at_pdata']:,} on a .pdata "
                f"beginning; {extra:,} analyser extras"
            )
            if s["extras"]:
                print(f"  extras sample: {', '.join(f'{v:#x}' for v in s['extras'])}")
        else:
            print(
                f"registry   : not swept (add --registry, ~20 min); function count "
                f"{bf['total']:,} vs {p['entries']:,} .pdata beginnings"
            )

    if bf is not None:
        print("bridge:")
        print(f"  functions        : {bf['total']:,}")
        print(f"  custom-named     : {bf['custom_named']}")
        print(f"  thunks           : {bf['thunks']}")
        print(f"  externals        : {bf['external']}")
        csv_facts = bf["csv"]
        print(
            f"  name table       : {csv_facts['rows']:,} rows, "
            f"{csv_facts['names_in_program'] if csv_facts['names_in_program'] is not None else '?'} applied"
        )

    first = struct.unpack_from("<III", data, PDATA_RVA)
    print(f"first entry: begin {IMAGE_BASE + first[0]:#x} end {IMAGE_BASE + first[1]:#x}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
