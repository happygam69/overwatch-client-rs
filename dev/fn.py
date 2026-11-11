"""Find and read the client's functions in the current image.

    python dev/fn.py read <addr>            # everything known about one function
    python dev/fn.py find <query>           # by name, 4CC tag, string or immediate
    python dev/fn.py callers <addr>         # who calls it (whole image)
    python dev/fn.py callees <addr>         # what it calls (whole image)
    python dev/fn.py scope <addr>           # the transitive closure of a function
    python dev/fn.py jobs                   # the asset-type -> job registry
    python dev/fn.py slots <disp>           # who dispatches through a vtable slot

**Why this tool exists.** The client image already carries the answer to
"which functions implement this format", in three places: the asset-type
registry, the chunk tags as immediates, and the STU class table. But the
tools that read those places were one-off probes, and the corpus that holds
the whole-image relationship index had no query interface at all - so finding
a function was a manual trace and reading it was a manual assembly of five
tool invocations. This tool does both in one command.

**What it reads** (all read-only):

* `dev/data/offline_recover/module_offline.bin` - the flat image
  (`offset == RVA`, base `0x140000000`). The `.text` on disk is encrypted,
  so this recovered copy is the only code source that works without Ghidra.
* `dev/data/corpus/corpus.db` - the whole-image index built by
  `python dev/corpus.py index`: `fn` (119,802 chain-root functions with
  their true extents), `edges` (1.87M control transfers), `slot_sites`
  (35,835 memory-indirect dispatches). Without it, `read` still works on
  the image alone but loses the whole-image callers and callees.
* `docs/function-names.csv` - the names a human has already given.
* `docs/**.md` - where a function is already written down.
* `dev/verify_harness/units/*.json` and `ledger.json` - whether a function
  has a differential unit and whether that unit is verified.

**The address frame.** Every address here is the current-image address,
`0x140000000 + RVA`. The verify-harness units live in the `v2b` frame
(`v2b - 0x7ff7c4d20000 == exe - 0x140000000`); the tool converts.

The tool never writes to the install, the image or the Ghidra project, and
it prints no decompiled code into the repo: what it prints goes to the
terminal, and the conclusions the reader draws belong in
`docs/formats/<system>.md` as an address and a sentence.
"""

from __future__ import annotations

import argparse
import bisect
import csv
import json
import os
import pathlib
import re
import sqlite3
import struct
import subprocess
import sys

HEAD = pathlib.Path(__file__).resolve().parent
ROOT = HEAD.parent
sys.path.insert(0, str(HEAD))

import corpus  # noqa: E402  (the disassembly and the .pdata chain walk)

BASE = corpus.IMAGE_BASE
IMG = corpus.IMG
DB = ROOT / "dev" / "data" / "corpus" / "corpus.db"
NAMES = ROOT / "docs" / "function-names.csv"
DOCS = ROOT / "docs"
UNITS = ROOT / "dev" / "verify_harness" / "units"
LEDGER = ROOT / "dev" / "verify_harness" / "ledger.json"

V2B_BASE = 0x7FF7C4D20000
# The decompiler's cache: the client's C for the functions this tool read.
DECOMPILE_DIR = ROOT / "dev" / "data" / "decompiles"

REG_NUM = {
    "rax": 0, "rcx": 1, "rdx": 2, "rbx": 3, "rsp": 4, "rbp": 5, "rsi": 6,
    "rdi": 7, "r8": 8, "r9": 9, "r10": 10, "r11": 11, "r12": 12, "r13": 13,
    "r14": 14, "r15": 15,
}


def _hex(text: str) -> int:
    """A capstone displacement: `+ 0x1f`, `- 0x1b1920b`.

    `int("-0x1b1920b", 16)` is a ValueError - Python will not take a sign
    with a base - so the sign is taken off and applied here.
    """
    token = text.replace(" ", "")
    sign = 1
    if token.startswith("-"):
        sign = -1
        token = token[1:]
    elif token.startswith("+"):
        token = token[1:]
    return sign * int(token, 16)

# .text ends here; strings and tables live above it.
TEXT_END = corpus.TEXT_END

_DATA: bytes | None = None
_CON: sqlite3.Connection | None = None
_CON_TRIED = False
_FUNCS: list[tuple[int, int, int, int]] | None = None
_NAMES: dict[int, dict[str, str]] | None = None


def image() -> bytes:
    global _DATA
    if _DATA is None:
        _DATA = IMG.read_bytes()
    return _DATA


def con() -> sqlite3.Connection | None:
    """The corpus index, or None when it has not been built.

    The database lives under `dev/data/` (git-ignored). Without it the whole
    image can still be read; the whole-image caller and callee queries are
    the only things that need it."""
    global _CON, _CON_TRIED
    if _CON_TRIED:
        return _CON
    _CON_TRIED = True
    if not DB.is_file():
        return None
    try:
        _CON = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    except sqlite3.Error:
        _CON = None
    return _CON


def need_index() -> sqlite3.Connection:
    c = con()
    if c is None:
        raise SystemExit(
            "the corpus index is missing: run `python dev/corpus.py index` "
            "(it reads the image bytes only; no Ghidra needed)"
        )
    return c


def functions() -> list[tuple[int, int, int, int]]:
    """Every function in the image as (begin_rva, end_rva, pieces, opaque).

    The unit is a function, not a `.pdata` entry: the client splits a body
    into a hot part and cold parts chained with `UNW_FLAG_CHAININFO`, and
    only the chain root is a function. `dev/corpus.py index` measures
    119,802 of them over 201,246 entries, and pins that count on every
    `verify`."""
    global _FUNCS
    if _FUNCS is not None:
        return _FUNCS
    c = con()
    if c is not None:
        rows = c.execute("SELECT begin,end,pieces,opaque FROM fn").fetchall()
        if rows:
            _FUNCS = sorted(rows)
            return _FUNCS
    # Fall back to the image: the same chain walk, without the index.
    funcs, _frags = corpus.unions()
    rows = []
    for _idx, (begin, stop, pieces) in funcs.items():
        rows.append((begin, stop, len(pieces), 0))
    _FUNCS = sorted(rows)
    return _FUNCS


def containing(rva: int) -> tuple[int, int, int, int] | None:
    """The function whose extent holds `rva`, or None."""
    rows = functions()
    i = bisect.bisect_right(rows, (rva, 1 << 62, 1 << 62, 1 << 62)) - 1
    if i < 0:
        return None
    begin, end, pieces, opaque = rows[i]
    if begin <= rva < end:
        return rows[i]
    return None


def names() -> dict[int, dict[str, str]]:
    """The name table: address -> row of `docs/function-names.csv`."""
    global _NAMES
    if _NAMES is not None:
        return _NAMES
    out: dict[int, dict[str, str]] = {}
    if NAMES.is_file():
        with NAMES.open(encoding="utf-8", newline="") as fh:
            for row in csv.DictReader(fh):
                raw = (row.get("address") or "").strip()
                if not raw:
                    continue
                try:
                    out[int(raw, 16)] = row
                except ValueError:
                    continue
    _NAMES = out
    return out


def doc_cites(addr: int) -> list[tuple[str, int, str]]:
    """Where an address is already written down in `docs/`."""
    hits: list[tuple[str, int, str]] = []
    needle = re.compile(rf"0x{addr:x}\b", re.IGNORECASE)
    if not DOCS.is_dir():
        return hits
    for path in sorted(DOCS.rglob("*.md")):
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for lineno, line in enumerate(text.splitlines(), 1):
            if needle.search(line):
                hits.append((path.relative_to(ROOT).as_posix(), lineno, line.strip()))
    return hits


def unit_for(addr: int) -> str:
    """The differential unit that tests this function, if any."""
    if not UNITS.is_dir():
        return ""
    want = f"0x{addr:x}"
    for path in sorted(UNITS.glob("*.json")):
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            continue
        spec = data.get("address")
        if isinstance(spec, dict):
            for key in ("exe", "v2b"):
                value = spec.get(key)
                if not isinstance(value, str):
                    continue
                try:
                    a = int(value, 16)
                except ValueError:
                    continue
                if key == "v2b":
                    a = a - V2B_BASE + BASE
                if a == addr:
                    return data.get("id") or path.stem
        elif isinstance(spec, str):
            try:
                if int(spec, 16) == addr:
                    return data.get("id") or path.stem
            except ValueError:
                pass
    return ""


def unit_status(unit_id: str) -> str:
    """The ledger's verdict for a unit (`verified`, `inconclusive`, ...)."""
    if not unit_id or not LEDGER.is_file():
        return ""
    try:
        data = json.loads(LEDGER.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return ""
    for entry in data.get("entries", []):
        if entry.get("unit_id") == unit_id:
            status = entry.get("status") or "?"
            cov = entry.get("coverage")
            if cov is not None:
                status += f" (coverage {cov}%)"
            return status
    return ""


def label(addr: int) -> str:
    """The best name for an address: the name table, else the Ghidra name."""
    row = names().get(addr)
    if row and (row.get("name") or "").strip():
        name = row["name"].strip()
        tag = (row.get("tag") or "").strip()
        return f"{name} [{tag}]" if tag else name
    c = con()
    if c is not None:
        hit = c.execute(
            "SELECT name FROM functions WHERE address=?", (f"{addr:#x}",)
        ).fetchone()
        if hit and hit[0] and not hit[0].startswith("FUN_"):
            return hit[0]
    return f"FUN_{addr:x}"


def parse_addr(text: str) -> int:
    """Accept `0x141b88f10`, `141b88f10` (a hex VA) or an RVA."""
    token = text.strip().lower()
    if token.startswith("0x"):
        token = token[2:]
    if not token or set(token) - set("0123456789abcdef"):
        raise SystemExit(f"not an address: {text}")
    value = int(token, 16)
    if BASE <= value < BASE + TEXT_END:
        return value
    if value < TEXT_END:
        return BASE + value
    raise SystemExit(f"address outside .text: {text}")


# --------------------------------------------------------------------------
# the read path
# --------------------------------------------------------------------------


def body_window(rva: int, default: int = 0x100) -> tuple[int, int]:
    """A printable body for an address the unwind table does not cover.

    Some of the client's bodies have no `.pdata` entry at all; the function
    register has no row for them, but the code is there. The window runs to
    the next function that does have one."""
    rows = functions()
    i = bisect.bisect_right(rows, (rva, 1 << 62, 1 << 62, 1 << 62))
    if i < len(rows):
        return rva, rows[i][0]
    return rva, rva + default


def cmd_read(args) -> int:
    addr = parse_addr(args.addr)
    row = containing(addr - BASE)
    if row is None:
        return _read_uncovered(addr, args.no_asm, args.decompile)
    begin, end, pieces, opaque = row
    begin_va, end_va = BASE + begin, BASE + end

    name_row = names().get(begin_va)
    print(f"{addr:#x}  {label(begin_va)}")
    if name_row and (name_row.get("role") or "").strip():
        print(f"    role: {name_row['role'].strip()}")
    unit = unit_for(begin_va)
    if unit:
        status = unit_status(unit)
        print(f"    unit: {unit}" + (f" - {status}" if status else " (no ledger entry)"))
    cites = doc_cites(begin_va)
    if cites:
        for path, lineno, line in cites[:6]:
            print(f"    docs: {path}:{lineno}  {line[:110]}")
        if len(cites) > 6:
            print(f"    docs: ... and {len(cites) - 6} more")

    print(
        f"    extent: {begin_va:#x}..{end_va:#x}  "
        f"{end - begin} bytes, {pieces} .pdata piece(s)"
    )
    print(
        f"    frames: exe {begin_va:#x}  v2b {V2B_BASE + begin:#x}  "
        "(the verify harness runs the oracle in the v2b frame)"
    )
    if opaque:
        print(
            "    opaque: yes - the linear listing desyncs (a branch lands inside "
            "its own instruction, or a code transfer points outside .text). "
            "Read it in the decompiler."
        )
    if addr != begin_va:
        print(f"    note: {addr:#x} is inside this function, {addr - begin_va:#x} in")

    callers = callers_of(begin_va)
    callees = callees_of(begin_va)
    print(f"    callers: {len(callers)}")
    for site, src, form in callers[:8]:
        print(f"        {src:#x}  {label(src)}  (call site {site:#x}, {form})")
    if len(callers) > 8:
        print(f"        ... and {len(callers) - 8} more")
    print(f"    callees: {len(callees)}")
    for site, target, form in callees[:8]:
        print(f"        {target:#x}  {label(target)}  (at {site:#x}, {form})")
    if len(callees) > 8:
        print(f"        ... and {len(callees) - 8} more")

    if not args.no_asm:
        print()
        print(corpus.disassemble(begin_va, end_va).rstrip())
    if args.decompile:
        text, why = decompile(begin_va if row is None else begin_va)
        print()
        if why:
            print(f"// no decompile: {why}")
        else:
            print("// ===== the client's C (Ghidra) =====")
            print(text.rstrip())
    return 0


def _read_uncovered(addr: int, no_asm: bool, decompile_it: bool = False) -> int:
    """Read a body the unwind table does not cover.

    The client's image has code that no `.pdata` entry describes (the audit
    counts a handful of such bodies). The function register has no row for
    them, so the extent is unknown: the window runs to the next function
    that does have one, and the listing is linear from the address."""
    lo, hi = body_window(addr - BASE)
    name_row = names().get(addr)
    print(f"{addr:#x}  {label(addr)}")
    if name_row and (name_row.get("role") or "").strip():
        print(f"    role: {name_row['role'].strip()}")
    unit = unit_for(addr)
    if unit:
        status = unit_status(unit)
        print(f"    unit: {unit}" + (f" - {status}" if status else " (no ledger entry)"))
    cites = doc_cites(addr)
    for path, lineno, line in cites[:6]:
        print(f"    docs: {path}:{lineno}  {line[:110]}")
    print(
        f"    extent: none - no .pdata entry covers this address, so the "
        f"client's body is not in the unwind table"
    )
    print(
        f"    frames: exe {addr:#x}  v2b {V2B_BASE + (addr - BASE):#x}  "
        "(the verify harness runs the oracle in the v2b frame)"
    )
    print(f"    window: {BASE + lo:#x}..{BASE + hi:#x} (to the next known function)")
    if not no_asm:
        print()
        print(
            corpus.disassemble(
                BASE + lo, BASE + hi, note="linear disassembly (no .pdata extent)"
            ).rstrip()
        )
    if decompile:
        text, why = decompile(addr)
        print()
        if why:
            print(f"// no decompile: {why}")
        else:
            print("// ===== the client's C (Ghidra) =====")
            print(text.rstrip())
    return 0


def callers_of(addr: int) -> list[tuple[int, int, str]]:
    """Who calls this function, from the whole-image index.

    A function the unwind table does not cover still has call sites, so the
    exact target is queried as well as the union span: the index records the
    raw target of every transfer, and a call into a body with no `.pdata`
    extent is exactly such a record. The old behaviour (refusing, because
    there is no union row to span) hid those callers.

    A call site inside the function itself is a branch, not a caller: the
    index records every control transfer, so the function's own back edges
    are dropped.
    """
    c = con()
    if c is None:
        return []
    rva = addr - BASE
    row = containing(rva)
    rows: list[tuple[int, int, str]] = []
    if row is not None:
        rows = list(
            c.execute(
                "SELECT site,src,form FROM edges WHERE target>=? AND target<?",
                (row[0], row[1]),
            ).fetchall()
        )
    exact = list(
        c.execute("SELECT site,src,form FROM edges WHERE target=?", (rva,)).fetchall()
    )
    seen = {(site, form) for site, _t, form in rows}
    for site, src, form in exact:
        if (site, form) in seen:
            continue
        rows.append((site, src, form))
        seen.add((site, form))
    return [
        (BASE + site, BASE + src, form)
        for site, src, form in rows
        if src != rva
    ]


def callees_of(addr: int) -> list[tuple[int, int, str]]:
    """What this function calls, with each target resolved to its function.

    The function's own branches are not callees."""
    c = con()
    if c is None:
        return []
    row = containing(addr - BASE)
    if row is None:
        return []
    out = []
    for site, target, form in c.execute(
        "SELECT site,target,form FROM edges WHERE src=?", (row[0],)
    ).fetchall():
        fn = containing(target)
        tgt = fn[0] if fn else target
        if tgt == row[0]:
            continue
        out.append((BASE + site, BASE + tgt, form))
    return out


def function_extent(addr: int) -> tuple[int, int]:
    """The function's own extent: the union when the unwind table has one,
    otherwise the body's last `ret` before the next known function."""
    import capstone

    row = containing(addr - BASE)
    if row is not None:
        return BASE + row[0], BASE + row[1]
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.skipdata = True
    rows = functions()
    i = bisect.bisect_right(rows, (addr - BASE, 1 << 62, 1 << 62, 1 << 62))
    hi = BASE + rows[i][0] if i < len(rows) else addr + 0x200
    end = addr
    for ins in md.disasm(image()[addr - BASE : hi - BASE], addr):
        if ins.mnemonic == "ret":
            end = ins.address + ins.size
        elif ins.mnemonic == "int3" and end != addr:
            break
    return addr, (end if end != addr else hi)


def decompile(addr: int) -> tuple[str, str]:
    """The client's C for one function, and the reason when there is none.

    The decompiler lives in Ghidra, so this runs the project's own headless
    script (`McpDecompile.java`) against the merged working copy and caches
    the result under `dev/data/decompiles`. The paths are this machine's
    defaults and can be overridden in the environment:

        GHIDRA_HEADLESS   the analyzeHeadless.bat to run
        GHIDRA_SCRIPTS    the script directory
        GHIDRA_PROJECT    the project directory holding `owmod_clean`

    The project must not be locked by another Ghidra; the merged copy in
    `GHIDRA_PROJECT` is a separate project file, so the GUI's own project
    does not block it.
    """
    headless = os.environ.get(
        "GHIDRA_HEADLESS", r"E:\ghidra\ghidra_12.1.4_PUBLIC\support\analyzeHeadless.bat"
    )
    scripts = os.environ.get("GHIDRA_SCRIPTS", r"C:\Users\Happy\ghidra_scripts")
    project = os.environ.get("GHIDRA_PROJECT", r"C:\Temp\owread")
    cache = DECOMPILE_DIR / f"{addr:x}.c"
    if cache.is_file():
        return cache.read_text(encoding="utf-8", errors="replace"), ""
    if not pathlib.Path(headless).is_file():
        return "", f"no Ghidra at {headless} (set GHIDRA_HEADLESS)"
    if not pathlib.Path(scripts, "McpDecompile.java").is_file():
        return "", f"no McpDecompile.java in {scripts} (set GHIDRA_SCRIPTS)"
    if not pathlib.Path(project).is_dir():
        return "", f"no project directory at {project} (set GHIDRA_PROJECT)"
    begin, end = function_extent(addr)
    tmp = DECOMPILE_DIR / "_run"
    tmp.mkdir(parents=True, exist_ok=True)
    args_file = tmp / "args.txt"
    out_file = tmp / "out.c"
    args_file.write_text(f"0x{begin:x} 0x{end:x}\n", encoding="ascii")
    cmd = [
        headless,
        project,
        "owmod_clean",
        "-process",
        "Overwatch.exe",
        "-noanalysis",
        "-readOnly",
        "-scriptPath",
        scripts,
        "-postScript",
        "McpDecompile.java",
        str(args_file),
        str(out_file),
    ]
    try:
        proc = subprocess.run(
            cmd, capture_output=True, text=True, timeout=900, check=False
        )
    except (OSError, subprocess.SubprocessError) as exc:
        return "", f"the headless run failed: {exc}"
    if not out_file.is_file():
        tail = (proc.stdout or "")[-400:]
        return "", f"the headless run produced no output ({tail.strip()})"
    text = out_file.read_text(encoding="utf-8", errors="replace")
    if not text.strip():
        return "", "Ghidra has no function at that address"
    DECOMPILE_DIR.mkdir(parents=True, exist_ok=True)
    cache.write_text(text, encoding="utf-8")
    return text, ""


def cmd_callers(args) -> int:
    addr = parse_addr(args.addr)
    row = containing(addr - BASE)
    hits = callers_of(addr)
    owner = f"{BASE + row[0]:#x} ({label(BASE + row[0])})" if row else f"{addr:#x}"
    print(f"callers of {owner}: {len(hits)}")
    for site, src, form in hits:
        print(f"  {src:#x}  {label(src)}  (call site {site:#x}, {form})")
    return 0


def cmd_callees(args) -> int:
    addr = parse_addr(args.addr)
    row = containing(addr - BASE)
    if row is None:
        # A body the unwind table does not cover has no union row to walk
        # from, so the edges that leave it are read by their raw source
        # address instead.
        c = con()
        if c is None:
            print(f"{addr:#x}: no corpus index")
            return 1
        hits = [
            (BASE + site, BASE + target, form)
            for site, target, form in c.execute(
                "SELECT site,target,form FROM edges WHERE src=?", (addr - BASE,)
            ).fetchall()
        ]
        print(f"callees of {addr:#x} (no .pdata extent): {len(hits)}")
        for site, target, form in hits:
            fn = containing(target - BASE)
            tgt = BASE + (fn[0] if fn else target - BASE)
            print(f"  {tgt:#x}  {label(tgt)}  (at {site:#x}, {form})")
        return 0
    hits = callees_of(addr)
    print(f"callees of {BASE + row[0]:#x} ({label(BASE + row[0])}): {len(hits)}")
    for site, target, form in hits:
        print(f"  {target:#x}  {label(target)}  (at {site:#x}, {form})")
    return 0


def cmd_scope(args) -> int:
    """The transitive closure of the callees: the cluster around one function."""
    addr = parse_addr(args.addr)
    row = containing(addr - BASE)
    if row is None:
        print(f"{addr:#x} is not inside any function")
        return 1
    c = need_index()
    root = row[0]
    seen = {root: 0}
    order = [root]
    frontier = [root]
    depth = 0
    while frontier and depth < args.depth:
        depth += 1
        nxt = []
        for src in frontier:
            for _site, target, _form in c.execute(
                "SELECT site,target,form FROM edges WHERE src=?", (src,)
            ).fetchall():
                fn = containing(target)
                tgt = fn[0] if fn else target
                if tgt not in seen:
                    seen[tgt] = depth
                    order.append(tgt)
                    nxt.append(tgt)
        frontier = nxt
    print(
        f"scope of {BASE + root:#x} ({label(BASE + root)}): "
        f"{len(seen)} function(s) within {depth} level(s)"
    )
    for rva in sorted(seen, key=lambda r: (seen[r], r)):
        mark = "*" if rva == root else " "
        print(f" {mark} {BASE + rva:#x}  d{seen[rva]}  {label(BASE + rva)}")
    return 0


def cmd_slots(args) -> int:
    """Who dispatches through a vtable slot: `call [reg + disp]`.

    The displacement size matters: a slot below 0x80 is invisible to a
    disp32-only scan, so most slots are 1-byte displacements."""
    c = need_index()
    disp = args.disp
    if args.width:
        rows = c.execute(
            "SELECT site,src,kind,base,width FROM slot_sites WHERE disp=? AND width=?",
            (disp, args.width),
        ).fetchall()
    else:
        rows = c.execute(
            "SELECT site,src,kind,base,width FROM slot_sites WHERE disp=?", (disp,)
        ).fetchall()
    print(f"slot +{disp:#x}: {len(rows)} site(s)")
    by_fn: dict[int, list[tuple[int, str, str, int]]] = {}
    for site, src, kind, base, width in rows:
        by_fn.setdefault(src, []).append((site, kind, base, width))
    for src in sorted(by_fn):
        head = f"  {BASE + src:#x}  {label(BASE + src)}" if src else "  (no .pdata covers the site)"
        print(head)
        for site, kind, base, width in by_fn[src][:8]:
            print(f"      {BASE + site:#x}  {kind} [{base}] (+{disp:#x}, {width}-byte disp)")
    return 0


# --------------------------------------------------------------------------
# the find path
# --------------------------------------------------------------------------


def cmd_surface(args) -> int:
    """The client functions around what a crate cites, and the ones it does not.

    The groups are the **crate's own files**, because a crate file is the
    format's vocabulary: `crates/ow-tank/src/model.rs` is the model's
    cluster. For each file the report lists the client functions its cited
    addresses reach (the callees, two levels) that the crate does **not**
    anchor to an address.

    An unanchored function is either behaviour the crate does not implement,
    or behaviour it implements without citing the client's function, which
    is the same thing for the 1-to-1 rule: neither is proven.
    """
    import crate_coverage

    cited = crate_coverage.cited(args.crate)
    if not cited:
        print(f"{args.crate} cites no client address")
        return 1
    c = need_index()

    def closure(root: int, depth: int) -> set[int]:
        seen = {root}
        frontier = [root]
        for _ in range(depth):
            nxt = []
            for src in frontier:
                for _site, target, _form in c.execute(
                    "SELECT site,target,form FROM edges WHERE src=?", (src,)
                ).fetchall():
                    fn = containing(target)
                    tgt = fn[0] if fn else target
                    if tgt not in seen:
                        seen.add(tgt)
                        nxt.append(tgt)
            frontier = nxt
        return seen

    by_file: dict[str, list[int]] = {}
    for addr, files in cited.items():
        for f in files:
            by_file.setdefault(f, []).append(addr)

    total_unknown: set[int] = set()
    print(
        f"{args.crate} cites {len(cited)} client address(es) in "
        f"{len(by_file)} file(s)"
    )
    print()
    for path in sorted(by_file):
        cluster: set[int] = set()
        for addr in by_file[path]:
            row = containing(addr - BASE)
            if row is None:
                cluster.add(addr - BASE)
                continue
            cluster |= closure(row[0], args.depth)
        unknown = sorted(
            r
            for r in cluster
            if BASE + r not in cited and names().get(BASE + r)
        )
        total_unknown.update(unknown)
        print(f"  {path}  ({len(by_file[path])} cited address(es))")
        for r in unknown:
            print(f"      {BASE + r:#x}  {label(BASE + r)}")
    print()
    print(
        f"  {len(total_unknown)} named client function(s) in the clusters are "
        "not anchored by the crate"
    )
    print(
        "  (a shared helper appears in every file whose cluster reaches it)"
    )
    return 0


def cmd_tags(args) -> int:
    """The tags of the name table, and how many functions each holds."""
    table = names()
    counts: dict[str, int] = {}
    for row in table.values():
        tag = (row.get("tag") or "").strip() or "(none)"
        counts[tag] = counts.get(tag, 0) + 1
    print(f"{len(table)} named function(s) in {len(counts)} tag(s):")
    for tag, n in sorted(counts.items(), key=lambda kv: (-kv[1], kv[0])):
        print(f"  {tag:<16} {n:>4}")
    print()
    print("list one tag with: python dev/fn.py tag <tag>")
    return 0


def cmd_tag(args) -> int:
    """The named functions of one tag."""
    want = args.tag.lower()
    rows = [
        (addr, row)
        for addr, row in names().items()
        if (row.get("tag") or "").strip().lower() == want
    ]
    if not rows:
        print(f"no function carries the tag {args.tag!r}")
        return 1
    print(f"{len(rows)} function(s) tagged {args.tag!r}:")
    for addr, row in sorted(rows):
        role = (row.get("role") or "").strip()
        print(f"  {addr:#x}  {row['name']}")
        if role:
            print(f"      {role[:150]}")
    return 0


# The client's job registry. `JobForType` (`0x141fd8300`) reads the type
# table and then the job table; `0x141fd89f0` is the call that *fills* a job
# slot, so its callers are the registration functions, and the descriptor
# each one passes holds the job id at `+8`, the load callback at `+0x48` and
# the release callback at `+0x78`.
JOB_FOR_TYPE = 0x141FD8300
JOB_REGISTER = 0x141FD89F0
JOB_TABLE = 0x1441B3CB0
TYPE_REGISTRY = 0x143A7C3A0
TYPE_MAX = 0x192

_SCAN: dict | None = None


def scan_rip() -> dict:
    """Every rip-relative `lea` and `mov` in `.text`, found by byte pattern.

    The linear decode of a body can desync (915 opaque functions), so the
    descriptor and the stores that fill it are found by their encodings
    instead: `lea reg, [rip+d]`, `mov dword ptr [rip+d], imm` and
    `mov qword ptr [rip+d], reg`.  Every REX prefix is handled, because
    `r12`-`r15` use `REX.R`/`REX.B` and a scan for `48 89` alone misses them.
    """
    global _SCAN
    if _SCAN is not None:
        return _SCAN
    data = image()
    lo, hi = 0x1000, TEXT_END
    leas: list[tuple[int, int, int]] = []
    imm_stores: list[tuple[int, int, int]] = []
    ptr_stores: list[tuple[int, int, int]] = []
    mov_imm: list[tuple[int, int, int]] = []

    def rip_disp(pos: int) -> int:
        return int.from_bytes(data[pos : pos + 4], "little", signed=True)

    pos = lo
    while True:
        pos = data.find(b"\xc7\x05", pos, hi)
        if pos < 0:
            break
        imm = int.from_bytes(data[pos + 6 : pos + 10], "little")
        imm_stores.append((pos, BASE + pos + 10 + rip_disp(pos + 2), imm))
        pos += 1
    for rex in range(0x40, 0x50):
        pos = lo
        while True:
            pos = data.find(bytes([rex, 0x8D]), pos, hi)
            if pos < 0:
                break
            modrm = data[pos + 2]
            if (modrm & 0xC7) == 0x05:
                reg = ((modrm >> 3) & 7) | (8 if rex & 1 else 0)
                leas.append((pos, reg, BASE + pos + 7 + rip_disp(pos + 3)))
            pos += 1
        pos = lo
        while True:
            pos = data.find(bytes([rex, 0x89]), pos, hi)
            if pos < 0:
                break
            modrm = data[pos + 2]
            if (modrm & 0xC7) == 0x05:
                reg = ((modrm >> 3) & 7) | (8 if rex & 1 else 0)
                ptr_stores.append((pos, BASE + pos + 7 + rip_disp(pos + 3), reg))
            pos += 1
        pos = lo
        while True:
            pos = data.find(bytes([rex, 0xC7]), pos, hi)
            if pos < 0:
                break
            modrm = data[pos + 2]
            if (modrm & 0xC0) == 0xC0:
                reg = ((modrm >> 3) & 7) | (8 if rex & 1 else 0)
                val = int.from_bytes(data[pos + 3 : pos + 7], "little", signed=True)
                mov_imm.append((pos, reg, val & 0xFFFFFFFFFFFFFFFF))
            pos += 1
    for r in range(8):
        pos = lo
        while True:
            pos = data.find(bytes([0x48, 0xB8 + r]), pos, hi)
            if pos < 0:
                break
            mov_imm.append((pos, r, int.from_bytes(data[pos + 2 : pos + 10], "little")))
            pos += 1
    # Base-relative descriptors are handled per function (see
    # `_registration_by_disassembly`): scanning the whole image for every
    # `c7` byte is far too slow.
    _SCAN = {
        "leas": sorted(leas),
        "imm_stores": sorted(imm_stores),
        "ptr_stores": sorted(ptr_stores),
        "mov_imm": sorted(mov_imm),
    }
    return _SCAN


def _last_before(items, pos: int, reg: int | None = None):
    best = None
    for it in items:
        if it[0] >= pos:
            break
        if reg is not None and it[1] != reg:
            continue
        best = it
    return best


def _reg_value(scan: dict, reg: int, before: int) -> int | None:
    """The value a register holds at `before`, from the image's own code."""
    best = None
    for items in (scan["leas"], scan["mov_imm"]):
        it = _last_before(items, before, reg)
        if it and (best is None or it[0] > best[0]):
            best = it
    return best[2] if best else None


def _registration_by_disassembly(begin: int, end: int, site: int) -> dict | None:
    """Read one registration function the way the compiler wrote it.

    Some registrations build the descriptor in a register
    (`lea rcx, [rdi + 0x20]`) and fill it through that base
    (`mov dword ptr [rdi + 0x28], 0x27`).  The base is often a *parameter*,
    so the descriptor is tracked symbolically - "base register plus offset" -
    and the stores are matched against that shape.  The whole-image byte
    scan cannot see this idiom cheaply, so it is read here, per function.
    """
    import capstone

    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.skipdata = True
    body = image()[begin:end]
    ins = list(md.disasm(body, BASE + begin))
    vals: dict[str, int] = {}
    shape: dict[str, tuple[str, int]] = {}
    desc: tuple[str, int] | None = None
    job = None
    load = release = None

    for x in ins:
        if x.address > site:
            break
        ops = x.op_str.split(", ")
        if x.mnemonic == "lea" and len(ops) == 2:
            m = re.fullmatch(r"([a-z0-9]+), \[rip ([+-] 0x[0-9a-f]+)\]", x.op_str)
            if m:
                vals[m.group(1)] = x.address + x.size + _hex(m.group(2))
                shape.pop(m.group(1), None)
                continue
            m = re.fullmatch(r"([a-z0-9]+), \[([a-z0-9]+) ([+-] 0x[0-9a-f]+)\]",
                             x.op_str)
            if m:
                dst, base, disp = m.group(1), m.group(2), _hex(m.group(3))
                if base in vals:
                    vals[dst] = vals[base] + disp
                    shape.pop(dst, None)
                else:
                    shape[dst] = (base, disp)
                    vals.pop(dst, None)
                if dst == "rcx":
                    desc = ("const", vals[dst]) if base in vals else (base, disp)
                continue
        elif x.mnemonic == "mov" and len(ops) == 2:
            dst, src = ops
            m = re.fullmatch(
                r"(?:qword|dword) ptr \[([a-z0-9]+) ([+-] 0x[0-9a-f]+)\]", dst
            )
            if m:
                base, disp = m.group(1), _hex(m.group(2))
                if desc is None:
                    continue
                dbase, ddisp = desc
                hit = None
                if dbase == "const" and base in vals:
                    offset = vals[base] + disp - ddisp
                elif dbase == base:
                    offset = disp - ddisp
                else:
                    offset = None
                if offset is not None:
                    if offset == 8 and re.fullmatch(r"0x[0-9a-f]+", src):
                        job = int(src, 16)
                    elif offset == 0x48:
                        load = vals.get(src)
                    elif offset == 0x78:
                        release = vals.get(src)
                continue
            if dst in REG_NUM and src in REG_NUM:
                if src in vals:
                    vals[dst] = vals[src]
                else:
                    vals.pop(dst, None)
                if src in shape:
                    shape[dst] = shape[src]
                else:
                    shape.pop(dst, None)
            elif dst in REG_NUM and re.fullmatch(r"0x[0-9a-f]+", src):
                vals[dst] = int(src, 16)
                shape.pop(dst, None)
    if job is None:
        return None
    return {"descriptor": desc, "job": job, "load": load, "release": release}


def job_registry() -> dict[int, dict]:
    """job id -> the registration that fills it and the callbacks it holds.

    Every fact here is read from the image: the job id is the immediate the
    registration stores at `descriptor + 8`, and the callbacks are the
    pointers it stores at `descriptor + 0x48` (load) and `+ 0x78` (release).
    """
    scan = scan_rip()
    c = need_index()
    sites = c.execute(
        "SELECT site, src FROM edges WHERE target=? AND form IN ('call','jmp')",
        (JOB_REGISTER - BASE,),
    ).fetchall()

    def job_at(desc: int, before: int) -> int | None:
        """The job id stored at `desc + 8`."""
        for _pos, target, imm in scan["imm_stores"]:
            if target == desc + 8:
                return imm
        return None

    def callback_at(desc: int, off: int, before: int) -> int | None:
        """The pointer stored at `desc + off`."""
        for pos, target, reg in scan["ptr_stores"]:
            if target == desc + off:
                return _reg_value(scan, reg, pos)
        return None

    out: dict[int, dict] = {}
    for site, src in sorted(sites):
        desc = None
        job = None
        load = release = None
        # the descriptor is the first argument: a rip-relative `lea rcx`, or
        # a base-relative one (`lea rcx, [rbx + 0x1810]`).
        lea = _last_before(scan["leas"], site, 1)
        if lea is not None:
            job = job_at(lea[2], site)
            if job is not None:
                desc = lea[2]
                load = callback_at(desc, 0x48, site)
                release = callback_at(desc, 0x78, site)
        if desc is None:
            row = containing(site)
            if row is None:
                continue
            found = _registration_by_disassembly(row[0], row[1], site + BASE)
            if found is None:
                continue
            desc, job = found["descriptor"], found["job"]
            load, release = found["load"], found["release"]
        # `src` is 0 for code no `.pdata` entry describes: the call site
        # still names the function, so resolve it from there.
        owner = BASE + src if src else None
        if owner is None:
            row = containing(site)
            owner = BASE + row[0] if row else site
        out.setdefault(job, {
            "registration": owner,
            "descriptor": desc,
            "load": load,
            "release": release,
        })
    return out


def _registration_names(begin: int, end: int) -> list[dict]:
    """The asset names a registration function decodes, with their type ids.

    The client hides each asset's name with the same XOR-with-a-key loop
    `formats/chunk-kinds.md` documents: byte `i` of the blob is XORed with
    `key[i & 7]`, the key sits 12 bytes before the blob, and the length is a
    u32 masked with `0xffffff` in the header at `blob - 4`.  The name
    pointer and the asset's **type id** are then stored next to each other,
    and the type id is what the type table indexes.
    """
    import capstone

    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.skipdata = True
    ins = list(md.disasm(image()[begin:end], BASE + begin))
    vals: dict[str, int] = {}
    loops: list[dict] = []      # the deobfuscation loops, in order
    ptr_stores: list[tuple[int, int, str]] = []   # at, target, source register
    imm_stores: list[tuple[int, int, int]] = []    # at, target, value
    for i, x in enumerate(ins):
        ops = x.op_str.split(", ")
        if x.mnemonic == "lea" and len(ops) == 2:
            m = re.fullmatch(r"([a-z0-9]+), \[rip ([+-] 0x[0-9a-f]+)\]", x.op_str)
            if m:
                vals[m.group(1)] = x.address + x.size + _hex(m.group(2))
        elif x.mnemonic == "mov" and len(ops) == 2:
            dst, src = ops
            m = re.fullmatch(r"([a-z0-9]+), 0x([0-9a-f]+)", x.op_str)
            if m:
                vals[m.group(1)] = int(m.group(2), 16)
            m = re.fullmatch(r"(?:qword|dword) ptr \[rip ([+-] 0x[0-9a-f]+)\]",
                             dst)
            if m:
                target = x.address + x.size + _hex(m.group(1))
                if dst.startswith("qword"):
                    ptr_stores.append((x.address, target, src))
                elif re.fullmatch(r"0x[0-9a-f]+", src):
                    imm_stores.append((x.address, target, int(src, 16)))
        if x.mnemonic != "xor":
            continue
        m = re.fullmatch(r"byte ptr \[([a-z0-9]+) \+ ([a-z0-9]+)\], al", x.op_str)
        if not m:
            continue
        blob_reg, base = m.group(1), m.group(2)
        key_off = blob_off = None
        for y in reversed(ins[max(0, i - 6):i]):
            m2 = re.fullmatch(
                r"eax, byte ptr \[rax \+ " + base + r" ([+-] 0x[0-9a-f]+)\]", y.op_str
            )
            if m2 and key_off is None:
                key_off = _hex(m2.group(1))
            m3 = re.fullmatch(blob_reg + r", \[rax ([+-] 0x[0-9a-f]+)\]", y.op_str)
            if m3 and blob_off is None:
                blob_off = _hex(m3.group(1))
        if key_off is None or blob_off is None:
            continue
        base_value = vals.get(base)
        if base_value is None:
            continue
        blob = base_value + blob_off
        key = base_value + key_off
        header = struct.unpack_from("<I", image(), blob - BASE - 4)[0]
        length = header & 0xFFFFFF
        raw = bytearray(
            image()[blob - BASE + k] ^ image()[key - BASE + (k & 7)]
            for k in range(length)
        )
        loops.append({
            "at": x.address,
            "blob": blob,
            "length": length,
            "name": raw.decode("ascii", "replace"),
        })

    # The name pointer is the helper's return value, so it cannot be tracked
    # through the registers.  What *can* be tracked is the shape: each
    # registration stores the name pointer and the asset's **type id** in a
    # qword store and a dword store eight bytes after it, just before the
    # deobfuscation loop.  The two stores are near each other but not always
    # adjacent, and their order varies.
    for index, loop in enumerate(loops):
        limit = loops[index - 1]["at"] if index else 0
        at = loop["at"]
        quads: list[tuple[int, int]] = []   # at, target
        dwords: list[tuple[int, int, int]] = []  # at, target, value
        base_quads: list[tuple[int, str, int]] = []   # at, base reg, disp
        base_dwords: list[tuple[int, str, int, int]] = []  # at, base, disp, value
        for x in ins:
            if not (limit < x.address < at):
                continue
            if x.mnemonic != "mov":
                continue
            ops = x.op_str.split(", ")
            if len(ops) != 2:
                continue
            dst, src = ops
            m = re.fullmatch(r"qword ptr \[rip ([+-] 0x[0-9a-f]+)\]", dst)
            if m:
                quads.append((x.address, x.address + x.size + _hex(m.group(1))))
                continue
            m = re.fullmatch(r"dword ptr \[rip ([+-] 0x[0-9a-f]+)\]", dst)
            if m:
                value = src
                if re.fullmatch(r"0x[0-9a-f]+", value):
                    number = int(value, 16)
                elif re.fullmatch(r"[0-9]+", value):
                    number = int(value)
                else:
                    continue
                if number > TYPE_MAX:
                    continue
                dwords.append(
                    (x.address, x.address + x.size + _hex(m.group(1)), number)
                )
                continue
            m = re.fullmatch(r"qword ptr \[([a-z0-9]+) ([+-] 0x[0-9a-f]+)\]", dst)
            if m:
                base_quads.append((x.address, m.group(1), _hex(m.group(2))))
                continue
            m = re.fullmatch(r"dword ptr \[([a-z0-9]+) ([+-] 0x[0-9a-f]+)\]", dst)
            if m:
                value = src
                if re.fullmatch(r"0x[0-9a-f]+", value):
                    number = int(value, 16)
                elif re.fullmatch(r"[0-9]+", value):
                    number = int(value)
                else:
                    continue
                if number > TYPE_MAX:
                    continue
                base_dwords.append(
                    (x.address, m.group(1), _hex(m.group(2)), number)
                )
        best = None
        for d_at, d_target, value in dwords:
            for q_at, q_target in quads:
                if d_target != q_target + 8:
                    continue
                distance = at - d_at
                if best is None or distance < best[0]:
                    best = (distance, value)
        for d_at, d_base, d_disp, value in base_dwords:
            for q_at, q_base, q_disp in base_quads:
                if d_base != q_base or d_disp != q_disp + 8:
                    continue
                distance = at - d_at
                if best is None or distance < best[0]:
                    best = (distance, value)
        if best is not None:
            loop["type"] = best[1]
    return loops


def cmd_names(args) -> int:
    """The client's own names for its asset types.

    Each registration function deobfuscates the name of the asset it
    registers, and stores it next to the asset's type id.  The type id is
    what the type table indexes, so this is the client's own vocabulary for
    its assets - read from the image, not from a reference library.
    """
    registry = job_registry()
    c = need_index()
    sites = c.execute(
        "SELECT DISTINCT src FROM edges WHERE target=? AND form IN ('call','jmp')",
        (JOB_REGISTER - BASE,),
    ).fetchall()
    data = image()
    types_by_job: dict[int, list[int]] = {}
    for t in range(TYPE_MAX + 1):
        job = struct.unpack_from("<I", data, TYPE_REGISTRY - BASE + 4 * t)[0]
        types_by_job.setdefault(job, []).append(t)

    rows = []
    seen: set[int] = set()
    for (src,) in sorted(sites):
        row = containing(src) if src else None
        if row is None:
            continue
        va = BASE + row[0]
        if va in seen:
            continue
        seen.add(va)
        for entry in _registration_names(row[0], row[1]):
            t = entry.get("type")
            job = None
            if t is not None:
                job = struct.unpack_from(
                    "<I", data, TYPE_REGISTRY - BASE + 4 * t
                )[0]
            info = registry.get(job) if job is not None else None
            rows.append((entry, job, info, va))

    print(f"{len(rows)} asset name(s) the client registers\n")
    for entry, job, info, va in rows:
        mark = "  " if entry.get("type") is not None else "**"
        print(
            f" {mark} {entry['name']:<28} type "
            + (f"{entry['type']:#05x}" if entry.get("type") is not None else "unknown")
            + (f"  job {job}" if job is not None else "")
        )
        if info:
            if info.get("load"):
                print(f"      load    {info['load']:#x}  {label(info['load'])}")
            if info.get("release"):
                print(f"      release {info['release']:#x}  {label(info['release'])}")
    return 0


def cmd_jobs(args) -> int:
    """The client's asset-type registry: type -> loader job, and the loader.

    Two tables, both read from the image:

    * the **type table** at `0x143a7c3a0`: 403 `u32` entries, one per asset
      type `0x00..0x192`, holding the job id;
    * the **job table** at `0x1441b3cb0`: a `u64` array of job objects. It is
      zero-fill - the job objects exist only at runtime - so the loader
      behind a job is read from the registration that fills it.

    `JobForType` (`0x141fd8300`) joins the two, and `0x141fd89f0` is the
    call that fills a job slot: its callers are the registration functions,
    and the descriptor each passes holds the job id at `+8`, the **load**
    callback at `+0x48` and the **release** callback at `+0x78`.
    """
    data = image()
    types_by_job: dict[int, list[int]] = {}
    for t in range(TYPE_MAX + 1):
        job = struct.unpack_from("<I", data, TYPE_REGISTRY - BASE + 4 * t)[0]
        types_by_job.setdefault(job, []).append(t)

    registry = job_registry()
    cited: dict[int, set[str]] = {}
    if args.crate:
        import crate_coverage

        cited = crate_coverage.cited(args.crate)
    print(f"asset-type registry at {TYPE_REGISTRY:#x} ({TYPE_MAX + 1} types)")
    print(f"  job table at {JOB_TABLE:#x} is zero-fill: the job objects exist")
    print("  only at runtime, so the loader is read from its registration")
    if args.crate:
        print(f"  the crate cited below: {args.crate}")
    print()
    for job in sorted(types_by_job):
        types = types_by_job[job]
        if job == 0:
            print(f"  job  {job:>3}: {len(types):>3} type(s) - no loader")
            continue
        info = registry.get(job)
        shown = ", ".join(f"{t:#04x}" for t in types[:6])
        more = f" (+{len(types) - 6} more)" if len(types) > 6 else ""
        print(f"  job  {job:>3}: {len(types):>3} type(s) {shown}{more}")
        if info:
            print(f"        registration {info['registration']:#x}")
            for key, what in (("load", "load   "), ("release", "release")):
                cb = info.get(key)
                if not cb:
                    continue
                mark = ""
                if cited:
                    mark = (
                        "  <- the crate cites it"
                        if cb in cited
                        else "  <- NOT cited by the crate"
                    )
                print(f"        {what} {cb:#x}  {label(cb)}{mark}")
        else:
            print("        registration: not resolved (the descriptor's job id")
            print("        is not an immediate in the image)")
    return 0


def _immediates(value: int) -> list[tuple[int, int]]:
    """Every place in `.text` where `value` appears as a 32-bit immediate.

    The scan finds the raw bytes, then every candidate is confirmed by
    decoding the function that holds it: a 4-byte value in code is usually
    a displacement or a jump target, not an immediate."""
    data = image()
    needle = struct.pack("<I", value)
    out: list[tuple[int, int]] = []
    pos = 0x1000
    while True:
        pos = data.find(needle, pos, TEXT_END)
        if pos < 0:
            break
        out.append((BASE + pos, pos))
        pos += 1
    return out


def _confirm_immediates(value: int) -> list[tuple[int, int, str]]:
    """(function_va, instruction_address, instruction_text) for each hit."""
    hits: list[tuple[int, int, str]] = []
    seen_fn: set[int] = set()
    needles = (f"0x{value:x}", f"0x{value:08x}")
    for _hit_va, rva in _immediates(value):
        fn = containing(rva)
        if fn is None:
            # Code the unwind table does not cover: read a window from the hit.
            lo, hi = body_window(rva)
            body = corpus.disassemble(
                BASE + lo, BASE + hi, note="linear disassembly (no .pdata extent)"
            )
            fn_begin = None
        else:
            begin, end = fn[0], fn[1]
            if begin in seen_fn:
                continue
            seen_fn.add(begin)
            body = corpus.disassemble(BASE + begin, BASE + end)
            fn_begin = begin
        for line in body.splitlines():
            toks = line.split()
            # address, bytes, mnemonic, operands...
            if len(toks) < 3 or toks[2].startswith("."):
                continue
            mnemonic = toks[2]
            operands = " ".join(toks[3:])
            if mnemonic not in ("cmp", "mov", "test", "add", "sub", "and", "or", "xor"):
                continue
            if not any(n in operands for n in needles):
                continue
            try:
                ins_va = int(toks[0], 16)
            except ValueError:
                continue
            owner = BASE + fn_begin if fn_begin is not None else ins_va
            hits.append((owner, ins_va, f"{mnemonic} {operands}"))
    return hits


def cmd_find(args) -> int:
    query = args.query
    low = query.lower()

    if low.startswith("name:"):
        return _find_name(query[5:])
    if low.startswith("type:"):
        return _find_type(query[5:])
    if low.startswith("str:"):
        return _find_string(query[4:])
    if low.startswith("imm:"):
        return _find_imm(query[4:])
    if low.startswith("tag:"):
        return _find_imm(query[4:], tag=True)

    # A 4CC tag is four printable characters: look for it as an immediate.
    if len(query) == 4 and query.isprintable() and query.isalpha():
        if _find_imm(query, tag=True, quiet=True) == 0:
            return 0
        if _find_name(query, quiet=True) == 0:
            return 0
        return _find_string(query)
    # Otherwise try the name table, then an immediate, then a string.
    if _find_name(query, quiet=True) == 0:
        return 0
    if _find_imm(query, quiet=True) == 0:
        return 0
    return _find_string(query)


def _find_name(text: str, quiet: bool = False) -> int:
    needle = text.lower()
    rows = [(a, r) for a, r in names().items() if needle in (r.get("name") or "").lower()]
    if not rows:
        if not quiet:
            print(f"no name matches {text!r}")
        return 1
    print(f"{len(rows)} name(s) matching {text!r}:")
    for addr, row in sorted(rows):
        tag = (row.get("tag") or "").strip()
        role = (row.get("role") or "").strip()
        print(f"  {addr:#x}  {row['name']}" + (f" [{tag}]" if tag else ""))
        if role:
            print(f"      {role}")
    return 0


def _find_type(text: str) -> int:
    try:
        t = int(text, 0)
    except ValueError:
        print(f"not an asset type: {text}")
        return 1
    data = image()
    if not 0 <= t <= TYPE_MAX:
        print(f"asset type {t:#x} is outside the registry (0..{TYPE_MAX:#x})")
        return 1
    job = struct.unpack_from("<I", data, TYPE_REGISTRY - BASE + 4 * t)[0]
    same = [
        x
        for x in range(TYPE_MAX + 1)
        if struct.unpack_from("<I", data, TYPE_REGISTRY - BASE + 4 * x)[0] == job
    ]
    print(f"asset type {t:#04x} -> job {job}")
    print(f"  the job object lives in the runtime table at {JOB_TABLE:#x}")
    print(f"  types on the same job: {', '.join(f'{x:#04x}' for x in same[:16])}")
    if len(same) > 16:
        print(f"  ... and {len(same) - 16} more")
    return 0


def _find_imm(text: str, tag: bool = False, quiet: bool = False) -> int:
    token = text.lower()
    if tag:
        try:
            raw = text.encode("ascii")
        except UnicodeEncodeError:
            if not quiet:
                print(f"not a 4CC tag: {text}")
            return 1
        # The client compares a chunk tag as a big-endian u32: the bytes
        # "MRNM" in the file are the value 0x4d524e4d, and the comparison
        # at 0x141b5e02e carries that immediate.
        value = struct.unpack(">I", raw)[0]
        other = struct.unpack("<I", raw)[0]
        desc = f"tag {text!r} ({value:#010x} big-endian)"
    else:
        try:
            value = int(token, 0)
        except ValueError:
            if not quiet:
                print(f"not a number: {text}")
            return 1
        other = None
        desc = f"immediate {value:#x}"
    hits = _confirm_immediates(value)
    if not hits and other is not None:
        hits = _confirm_immediates(other)
        if hits:
            desc += f" (little-endian {other:#010x})"
    if not hits:
        if not quiet:
            print(f"{desc}: no instruction in .text carries it")
        return 1
    print(f"{desc}: {len(hits)} instruction(s)")
    for fn_va, ins_va, text_line in hits:
        print(f"  {fn_va:#x}  {label(fn_va)}")
        print(f"      {ins_va:#x}  {text_line}")
    return 0


def _find_string(text: str, quiet: bool = False) -> int:
    """Strings in the image's data, and the code that refers to them."""
    data = image()
    needle = text.encode("utf-8", "replace")
    hits = []
    pos = TEXT_END
    while True:
        pos = data.find(needle, pos)
        if pos < 0:
            break
        # A string starts at its first printable byte.
        start = pos
        while start > 0 and 0x20 <= data[start - 1] < 0x7F:
            start -= 1
        end = pos + len(needle)
        while end < len(data) and 0x20 <= data[end] < 0x7F:
            end += 1
        hits.append((BASE + start, data[start:end].decode("ascii", "replace")))
        pos += 1
    if not hits:
        if not quiet:
            print(f"no string matching {text!r} in the image's data")
        return 1
    print(f"{len(hits)} string(s) matching {text!r}:")
    for va, text_str in hits[:40]:
        print(f"  {va:#x}  {text_str[:90]}")
    return 0


# --------------------------------------------------------------------------


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("read", help="everything known about one function")
    p.add_argument("addr")
    p.add_argument("--no-asm", action="store_true", help="skip the disassembly")
    p.add_argument(
        "--decompile",
        action="store_true",
        help="also run the client's decompiler (Ghidra) and print the C",
    )

    p = sub.add_parser("find", help="find functions by name, tag, string or immediate")
    p.add_argument("query")

    p = sub.add_parser("callers", help="who calls this function")
    p.add_argument("addr")

    p = sub.add_parser("callees", help="what this function calls")
    p.add_argument("addr")

    p = sub.add_parser("scope", help="the transitive closure of a function")
    p.add_argument("addr")
    p.add_argument("--depth", type=int, default=3)

    p = sub.add_parser("jobs", help="the asset-type -> job registry")
    p.add_argument(
        "--crate", default=None, help="mark the loaders this crate cites"
    )

    p = sub.add_parser("names", help="the client's own names for its asset types")

    p = sub.add_parser("tags", help="the name table's tags")
    p = sub.add_parser("tag", help="the named functions of one tag")
    p.add_argument("tag")

    p = sub.add_parser(
        "surface", help="the client functions around what a crate cites"
    )
    p.add_argument("crate")
    p.add_argument("--depth", type=int, default=2)

    p = sub.add_parser("slots", help="who dispatches through a vtable slot")
    p.add_argument("disp", type=lambda s: int(s, 0))
    p.add_argument(
        "--width", type=int, default=0, choices=(0, 1, 4), help="0 = any (default)"
    )

    args = ap.parse_args()
    if args.cmd == "read":
        return cmd_read(args)
    if args.cmd == "find":
        return cmd_find(args)
    if args.cmd == "callers":
        return cmd_callers(args)
    if args.cmd == "callees":
        return cmd_callees(args)
    if args.cmd == "scope":
        return cmd_scope(args)
    if args.cmd == "jobs":
        return cmd_jobs(args)
    if args.cmd == "names":
        return cmd_names(args)
    if args.cmd == "tags":
        return cmd_tags(args)
    if args.cmd == "tag":
        return cmd_tag(args)
    if args.cmd == "surface":
        return cmd_surface(args)
    if args.cmd == "slots":
        return cmd_slots(args)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
