"""Check cited addresses and function-name entries against the one address frame.

    python dev/frame_check.py --check     # gate mode: fail on a stale-frame citation
    python dev/frame_check.py             # report only
    python dev/frame_check.py --selftest  # prove the detector detects

Rule 9 of AGENTS.md: an address is `0x140000000 + RVA` in the current image, or
a function name from `docs/function-names.csv`. This tool enforces it
mechanically on the markdown docs (and the name table), with three detectable
defects:

  1. a runtime-frame address (`0x7ff6...` / `0x7ff7...`, a 12-hex-digit VA from
     a live capture) cited in a file that is not declared history - the classic
     paste from a capture-era probe output;
  2. a 10-hex-digit current-frame token (`0x14...`) whose value lies outside
     the image extents - a wrong-frame address;
  3. a `function-names.csv` address that lies inside a `.pdata` function range
     but is not that range's beginning - an offcut or a local call target, not
     a function entry;
  4. a `function-names.csv` address that is the start of a `UNW_FLAG_CHAININFO`
     continuation - a chain fragment is part of its parent function, so a name
     there names a body that does not begin at that address.

Rule 3 and 4 need the executable image, so they are skipped with a clear message
when neither `OW_EXE` nor the default install path nor the offline recovery
image is present (CI has no install). Everything else hex in the docs is an id, hash,
GUID, mask or RVA spelling and is somebody else's namespace; this tool does not
judge it. The three history files allowed to discuss runtime addresses are
`docs/provenance.md`, `docs/ow-tank-1to1.md` and `docs/casc-1to1.md` (they
explain why those numbers are not citable). Read-only.
"""

from __future__ import annotations

import argparse
import bisect
import csv
import os
import pathlib
import re
import struct
import tempfile

# The current image's frame and extents (docs/ghidra-project.md).
IMAGE_BASE = 0x140000000
IMAGE_SIZE = 0x4838000
RUNTIME_SHAPE = re.compile(r"0x7ff[0-9a-f]{7,11}\b", re.IGNORECASE)
# A current-frame token is 0x14 followed by 7..12 hex digits (the image's own
# addresses are exactly 9); the value must then sit inside the extents.
CURRENT_SHAPE = re.compile(r"0x14[0-9a-f]{7,12}\b", re.IGNORECASE)
NAME_TABLE_NAME = "function-names.csv"

# Files allowed to cite runtime-frame addresses: they are the ones that say
# why such citations are dead ends, quoting them as history. Paths relative
# to the scanned root, POSIX style.
HISTORY_FILES = ("docs/ow-tank-1to1.md", "docs/casc-1to1.md", "docs/provenance.md")

ROOT_FILES = ("STATUS-handoff.md", "README.md", "MODLOG.md")


def detect_faults(text: str, rel: str, image_base: int, image_end: int) -> list[str]:
    """The two rules, in the shape both the selftest and the gate use."""
    faults = []
    history = rel in HISTORY_FILES
    for lineno, line in enumerate(text.splitlines(), 1):
        for m in RUNTIME_SHAPE.finditer(line):
            if history:
                continue
            faults.append(f"{rel}:{lineno}: runtime-frame address {m.group(0)} outside the history files")
        for m in CURRENT_SHAPE.finditer(line):
            v = int(m.group(0), 16)
            if not image_base <= v < image_end:
                faults.append(f"{rel}:{lineno}: 0x14-frame address {m.group(0)} outside the image extents")
    return faults


def scan_file(path: pathlib.Path, root: pathlib.Path, image_base: int, image_end: int) -> list[str]:
    rel = path.relative_to(root).as_posix()
    return detect_faults(path.read_text(encoding="utf-8", errors="replace"), rel, image_base, image_end)


def docs_files(root: pathlib.Path) -> list[pathlib.Path]:
    files = sorted(root.rglob("*.md"))
    for name in ROOT_FILES:
        p = root / name
        if p.is_file():
            files.append(p)
    return files


def check_name_table(path: pathlib.Path, image_base: int, image_end: int) -> list[str]:
    if not path.is_file():
        return []
    faults = []
    with path.open(encoding="utf-8", newline="") as f:
        for lineno, row in enumerate(csv.DictReader(f), 2):
            try:
                addr = int(row["address"], 16)
            except (ValueError, KeyError) as exc:
                faults.append(f"{NAME_TABLE_NAME}:{lineno}: bad address ({exc})")
                continue
            if not image_base <= addr < image_end:
                name = row.get("name", "?")
                faults.append(f"{NAME_TABLE_NAME}:{lineno}: {name} at {row['address']} "
                              f"outside the image extents")
    return faults


# The executable image used for the function-entry check. The install path is
# the authoritative plaintext PE; the offline recovery image is a flat copy of
# the same bytes and is used when the install is not present.
DEFAULT_EXE = pathlib.Path(r"E:\Overwatch\_retail_\Overwatch.exe")
OFFLINE_IMAGE = pathlib.Path(__file__).resolve().parent / "data" / "offline_recover" / "module_offline.bin"


def find_image() -> tuple[pathlib.Path, bytes] | None:
    """The first available executable image, or None (CI has no install)."""
    candidates = []
    env = os.environ.get("OW_EXE")
    if env:
        candidates.append(pathlib.Path(env))
    candidates.extend((DEFAULT_EXE, OFFLINE_IMAGE))
    for path in candidates:
        if path.is_file():
            return path, path.read_bytes()
    return None


def pe_sections(image: bytes) -> dict[str, dict[str, int]]:
    """The PE section table as name -> {va, vsz, raw, rsz}."""
    pe = struct.unpack_from("<I", image, 0x3C)[0]
    nsec = struct.unpack_from("<H", image, pe + 6)[0]
    opt = struct.unpack_from("<H", image, pe + 20)[0]
    s0 = pe + 24 + opt
    out = {}
    for i in range(nsec):
        o = s0 + 40 * i
        name = image[o : o + 8].split(b"\x00")[0].decode("latin-1")
        vsz, va, rsz, ro = struct.unpack_from("<IIII", image, o + 8)
        out[name] = {"va": va, "vsz": vsz, "raw": ro, "rsz": rsz}
    return out


def pdata_ranges(image: bytes) -> tuple[list[tuple[int, int]], int, int, dict[int, int]] | None:
    """The .pdata function ranges in RVA space, plus the .text extent.

    The install's `.pdata` is read at its raw file offset; the offline image is
    flat, so its section data is at `VirtualAddress`. Both are tried and the
    candidate with the most plausible ranges wins - that keeps one parser for
    both spellings without a mode flag. The same spelling then locates the
    unwind records, so the chain parent (rule 4) is read from the right bytes.

    Returns (ranges, text_lo, text_hi, chain_parent) where `chain_parent` maps a
    `UNW_FLAG_CHAININFO` continuation start to the start of the entry its unwind
    record names as parent.
    """
    sections = pe_sections(image)
    pdr = sections.get(".pdata")
    text = sections.get(".text")
    if pdr is None or text is None:
        return None
    text_lo = text["va"]
    text_hi = text["va"] + text["vsz"]
    best: tuple[int, list[tuple[int, int, int]], bool] | None = None
    for offset, size, flat in (
        (pdr["raw"], min(pdr["rsz"], pdr["vsz"]), False),
        (pdr["va"], min(pdr["vsz"], pdr["rsz"]), True),
    ):
        if offset + size > len(image):
            continue
        raw = image[offset : offset + size]
        entries = []
        for i in range(len(raw) // 12):
            beg, end, unwind = struct.unpack_from("<III", raw, 12 * i)
            if not beg or end <= beg:
                continue
            if text_lo <= beg < text_hi and text_lo < end <= text_hi:
                entries.append((beg, end, unwind))
        if entries and (best is None or len(entries) > best[0]):
            best = (len(entries), entries, flat)
    if best is None:
        return None
    _n, entries, flat = best
    chain_parent = {}
    for beg, _end, unwind in entries:
        parent = _chain_parent(image, sections, unwind, flat)
        if parent is not None:
            chain_parent[beg] = parent
    # A real function body cannot start inside another range. Keep the first
    # range when two entries overlap (the client's own chained fragments do).
    ranges = sorted((beg, end) for beg, end, _unwind in entries)
    return ranges, text_lo, text_hi, chain_parent


def _offset_of(sections: dict[str, dict[str, int]], rva: int, flat: bool) -> int | None:
    """File offset of an RVA, for the install (mapped) or the flat image."""
    if flat:
        return rva
    for s in sections.values():
        span = max(s["vsz"], s["rsz"])
        if s["va"] <= rva < s["va"] + span:
            return s["raw"] + (rva - s["va"])
    return None


def _chain_parent(image: bytes, sections: dict[str, dict[str, int]], unwind_rva: int, flat: bool) -> int | None:
    """Start of the parent entry named by a chained UNWIND_INFO, or None.

    UNWIND_INFO byte 0 is version (bits 0-2) then flags (bits 3-7). A record
    whose version is not 1..3 is not UNWIND_INFO, so the read is rejected rather
    than guessed. With `UNW_FLAG_CHAININFO` set, a RUNTIME_FUNCTION follows the
    unwind-code array: 2 bytes per code plus the 4-byte header, aligned. The
    client never puts handler data on a continuation, so a record that carries
    one is left unresolved instead of guessed at.
    """
    off = _offset_of(sections, unwind_rva, flat)
    if off is None or not 0 <= off < len(image) - 3:
        return None
    first = image[off]
    if (first & 0x7) not in (1, 2, 3):
        return None
    flags = (first >> 3) & 0x1F
    if not flags & 0x4:
        return None
    if flags & 0x3:
        return None
    count = image[off + 2]
    tail = off + 4 + 2 * count
    if tail & 1:
        tail += 1
    if tail + 12 > len(image):
        return None
    parent_begin, _parent_end, _parent_unwind = struct.unpack_from("<III", image, tail)
    if not parent_begin or parent_begin == 0:
        return None
    return parent_begin


def chain_root(chain_parent: dict[int, int], begin: int, hops: int = 16) -> int | None:
    """Walk `UNW_FLAG_CHAININFO` parents up to the real function start."""
    cur = begin
    for _ in range(hops):
        nxt = chain_parent.get(cur)
        if nxt is None:
            return cur
        cur = nxt
    return None


def check_name_entries(
    path: pathlib.Path,
    ranges: list[tuple[int, int]],
    text_lo: int,
    text_hi: int,
    chain_parent: dict[int, int] | None = None,
) -> list[str]:
    """Every named function must be a real .pdata range start.

    A named address that is not in any .pdata range is allowed: MSVC omits
    unwind info for leaf functions, and the project records those explicitly.
    An address inside a range but not at its beginning is not a function entry.
    An address that starts a `UNW_FLAG_CHAININFO` continuation is part of its
    parent function, so it is a fault unless the row declares
    `container=<root start>` - and then the declared root is verified against
    the chain the unwind records spell out.
    """
    if not path.is_file() or not ranges:
        return []
    chain_parent = chain_parent or {}
    starts = [beg for beg, _end in ranges]
    faults = []
    with path.open(encoding="utf-8", newline="") as f:
        for lineno, row in enumerate(csv.DictReader(f), 2):
            try:
                addr = int(row["address"], 16) - IMAGE_BASE
            except (ValueError, KeyError):
                continue
            name = row.get("name", "?")
            if not text_lo <= addr < text_hi:
                faults.append(f"{NAME_TABLE_NAME}:{lineno}: {name} at {row['address']} "
                              f"is not in .text")
                continue
            container = (row.get("container") or "").strip()
            if addr in chain_parent:
                if not container:
                    faults.append(
                        f"{NAME_TABLE_NAME}:{lineno}: {name} at {row['address']} starts a "
                        f"chained .pdata continuation, not a function"
                    )
                    continue
                try:
                    want = int(container, 16) - IMAGE_BASE
                except ValueError:
                    faults.append(
                        f"{NAME_TABLE_NAME}:{lineno}: {name} has a bad container {container!r}"
                    )
                    continue
                root = chain_root(chain_parent, addr)
                if root != want:
                    got = "unresolved" if root is None else f"0x{IMAGE_BASE + root:x}"
                    faults.append(
                        f"{NAME_TABLE_NAME}:{lineno}: {name} at {row['address']} declares "
                        f"container {container}, but its chain resolves to {got}"
                    )
                continue
            if container:
                faults.append(
                    f"{NAME_TABLE_NAME}:{lineno}: {name} at {row['address']} declares "
                    f"container {container} but is not a chained continuation"
                )
                continue
            i = bisect.bisect_right(starts, addr) - 1
            if i < 0:
                continue
            beg, end = ranges[i]
            if beg < addr < end:
                faults.append(
                    f"{NAME_TABLE_NAME}:{lineno}: {name} at {row['address']} is inside "
                    f"function {IMAGE_BASE + beg:#x}..{IMAGE_BASE + end:#x}, "
                    f"not the entry"
                )
    return faults


def selftest() -> int:
    """Build a throwaway tree, prove both defects are caught and the history
    exemption holds."""
    good = "ids: 0x080000000f69002e scene map, 0x450b000 rva, 0x6EFDBD73 hash\n"
    doc_current = good + "citing 0x1450000000 wrong frame\n0x140ca7f30 right\n"
    doc_runtime = "captured at 0x7ff64c094b90\n"
    base = 0x140000000
    end = base + 0x4838000
    table = (
        "program,address,doc_address,name,role\n"
        f"Overwatch.exe,{base:#x},{base:#x},A,good\n"
        f"Overwatch.exe,{0x145000000:x},{0x145000000:x},B,wrong\n"
    )
    entry_table = (
        "program,address,doc_address,name,role,container\n"
        f"Overwatch.exe,{base + 0x1080:#x},{base + 0x1080:#x},Inside,offcut,\n"
        f"Overwatch.exe,{base + 0x1000:#x},{base + 0x1000:#x},Entry,real,\n"
        f"Overwatch.exe,{base + 0x2000:#x},{base + 0x2000:#x},Unmarked,fragment,\n"
        f"Overwatch.exe,{base + 0x2200:#x},{base + 0x2200:#x},WrongRoot,fragment,{base + 0x2400:#x}\n"
        f"Overwatch.exe,{base + 0x2400:#x},{base + 0x2400:#x},MarkedOk,fragment,{base + 0x1000:#x}\n"
    )
    with tempfile.TemporaryDirectory() as td:
        tree = pathlib.Path(td)
        (tree / "docs").mkdir()
        (tree / "flat.md").write_text(doc_current + doc_runtime, encoding="utf-8")
        (tree / "docs" / "ow-tank-1to1.md").write_text(doc_runtime, encoding="utf-8")
        (tree / "docs" / "camera.md").write_text(doc_runtime, encoding="utf-8")
        (tree / NAME_TABLE_NAME).write_text(table, encoding="utf-8")
        (tree / "entry-names.csv").write_text(entry_table, encoding="utf-8")
        faults_flat = scan_file(tree / "flat.md", tree, base, end)
        faults_history = scan_file(tree / "docs" / "ow-tank-1to1.md", tree, base, end)
        faults_doc = scan_file(tree / "docs" / "camera.md", tree, base, end)
        faults_table = check_name_table(tree / NAME_TABLE_NAME, base, end)
        faults_entries = check_name_entries(
            tree / "entry-names.csv",
            [(0x1000, 0x1100), (0x2000, 0x2100), (0x2200, 0x2300), (0x2400, 0x2500)],
            0x1000,
            0x3000,
            {0x2000: 0x1000, 0x2200: 0x1000, 0x2400: 0x1000},
        )

    ok = all(
        (
            any("0x14-frame" in x for x in faults_flat),
            any("runtime-frame" in x for x in faults_flat),
            not faults_history,
            any("runtime-frame" in x for x in faults_doc),
            any("outside the image extents" in x for x in faults_table),
            len(faults_flat) == 2,
            any("not the entry" in x for x in faults_entries),
            any("chained .pdata continuation" in x for x in faults_entries),
            any(f"chain resolves to {base + 0x1000:#x}" in x for x in faults_entries),
            not any("MarkedOk" in x for x in faults_entries),
            len(faults_entries) == 3,
        )
    )
    print("selftest: frame detector,", "ok" if ok else "FAILED")
    for x in faults_flat + faults_history + faults_doc + faults_table + faults_entries:
        print("  ", x)
    return 0 if ok else 1


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent.parent
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true", help="exit 1 on any fault")
    ap.add_argument("--selftest", action="store_true", help="prove the detector detects")
    ap.add_argument("--root", type=pathlib.Path, help="scan this tree instead of the repo")
    args = ap.parse_args()
    if args.selftest:
        return selftest()

    where = args.root or root
    image_end = IMAGE_BASE + IMAGE_SIZE
    faults: list[str] = []
    for f in docs_files(where):
        faults.extend(scan_file(f, where, IMAGE_BASE, image_end))
    if where == root:
        faults.extend(check_name_table(root / "docs" / NAME_TABLE_NAME, IMAGE_BASE, image_end))
        image = find_image()
        if image is None:
            print("function entry check: skipped (no image)")
        else:
            image_path, image_bytes = image
            got = pdata_ranges(image_bytes)
            if got is None:
                print("function entry check: no .pdata ranges; skipped")
            else:
                ranges, text_lo, text_hi, chain_parent = got
                entry_faults = check_name_entries(
                    root / "docs" / NAME_TABLE_NAME, ranges, text_lo, text_hi, chain_parent
                )
                faults.extend(entry_faults)
                print(
                    f"function entry check: {len(ranges)} .pdata ranges "
                    f"({len(chain_parent)} chained continuations) from "
                    f"{image_path.name}; {len(entry_faults)} fault(s)"
                )
    for f in faults:
        print(f)
    verdict = "FAULTS" if faults else "ok"
    print(f"frame check: {len(faults)} fault(s); {verdict}")
    if args.check and faults:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
