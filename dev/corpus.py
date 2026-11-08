"""Build and query a standing corpus for the current image.

    python dev/corpus.py build <start> <end> [--tag mover] [--force] [--asm-only]
    python dev/corpus.py make-args <start> <end> <out.txt>   # headless args file
    python dev/corpus.py show <address|name> [--asm] [--window 0x40]
    python dev/corpus.py grep <pattern> [-i] [--tag mover] [--asm-only]
    python dev/corpus.py list
    python dev/corpus.py index [--only fn|edges|slots]
    python dev/corpus.py verify
    python dev/corpus.py purge <tag>

The corpus lives inside the project at `dev/data/corpus` (ignored by the
whitelist `.gitignore`; decompiled client code is never tracked).

**The unit is a function, not a `.pdata` entry.** The client splits a function
into a hot part and one or more cold parts linked by `UNW_FLAG_CHAININFO`, and
`dev/fn_extent.py` walks that chain. Grouping the 201,246 entries by their
transitive root gives **119,802 functions** and 81,444 cold halves; chains reach
8 deep. In this image every union is contiguous and ascending (measured - see
`verify`), so one `begin end` pair expresses a function exactly. Schema 2 of
this tool indexed each entry as a function, which made 81,444 cold halves look
like functions and left 25,649 bodies short of their real end; `verify` pins the
counts on every run and `db()` migrates a schema-2 index from the image.

Each function gets an authoritative linear disassembly decoded from the image
itself, `asm/<hex>_<name>.asm`, whether or not Ghidra knows the function.
Ghidra's decompile is added as `c/<hex>_<name>.c` when it exists. Rows are
indexed in `corpus.db`: address, union extent, pieces, name, first comment,
file paths, entry point, `c_cut`. Tag membership is a separate table, so one
range can belong to several tags and rebuilding or purging a tag never moves
another tag's files.

The disassembly is the point: it survives a truncated or flattened decompile and
covers bodies Ghidra never defined. It is a *linear* decode, not a flow decode:
a body whose own branch lands inside its instruction (1.2% of them, `verify`
counts them) desyncs and must be read in the decompiler. `show --window`
disassembles around any interior address, including a cold half, which resolves
through its function's union row.

`build` uses the bridge only for decompiles (fast for hot ranges, works while
the GUI is open). For whole-image sweeps use `make-args` plus the canonical
headless batch (docs/ghidra-project.md); it needs the GUI project closed.
`make-args` emits chain-root unions only and refuses a chained start: a headless
run creates a function at any entry Ghidra lacks, and creating one at a cold
half trims the parent - the documented cause of the 251 multi-range bodies in
`docs/ghidra-image-audit.md` section 4. The tool is read-only against the
install and the Ghidra project.
"""

from __future__ import annotations

import argparse
import bisect
import concurrent.futures as cf
import hashlib
import pathlib
import re
import shutil
import sqlite3
import sys
import time
from datetime import datetime, timezone

import capstone

HEAD = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HEAD))
import fn_extent  # noqa: E402
import gh  # noqa: E402

ROOT = HEAD.parent
IMG = ROOT / "dev" / "data" / "offline_recover" / "module_offline.bin"
# The table itself is decoded in dev/fn_extent.py (`PDATA_RVA`).
# .pdata is 2,415,104 bytes = 201,246 entries of 12 (docs/ghidra-image-audit.md
# section 2). A short read would silently halve a sweep, so it is asserted.
PDATA_ENTRIES = 201_246
REAL_ENTRIES = 119_802
CHAINED_ENTRIES = 81_444
CHAIN_ROOTS = 25_649
IMAGE_BASE = 0x140000000
# .text is VA 0x140001000 + 49,460,224 bytes (audit section 1), not the image size.
TEXT_END = 0x2F2C400
SCHEMA = 3
UNW_FLAG_CHAININFO = 4
# Behaviour comes from this program only; never the active program, which may
# be a different image after a Ghidra session opens something else.
PROGRAM = "Overwatch.exe"
DEFAULT_OUT = ROOT / "dev" / "data" / "corpus"

_IMAGE: bytes | None = None
_MD = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
_MD.skipdata = True


def image() -> bytes:
    global _IMAGE
    if _IMAGE is None:
        _IMAGE = IMG.read_bytes()
    return _IMAGE


def pdata_entries() -> list[tuple[int, int, int]]:
    """Every `.pdata` entry as (begin_rva, stop_rva, unwind_rva).

    `dev/fn_extent.py` is the one decoder of the table; this wrapper exists to
    assert that it read all of it, because a short read looks like a smaller
    image rather than an error."""
    ents = fn_extent.entries(fn_extent.load())
    if len(ents) != PDATA_ENTRIES:
        raise SystemExit(
            f".pdata read {len(ents)} entries, expected {PDATA_ENTRIES}: "
            "the image or the .pdata offset in dev/fn_extent.py is wrong"
        )
    return ents


def unions() -> tuple[dict[int, tuple[int, int, list[tuple[int, int]]]], dict[int, int]]:
    """Group `.pdata` entries by their transitive chain root.

    Returns (functions, fragments). `functions[root_rva]` is
    `(begin_rva, end_rva, pieces)` with `pieces` the entries that make the
    function, ascending; `fragments[piece_rva]` is that piece's root. The walk
    is memoised and path-compressed because chains reach 8 deep here; grouping
    by the immediate parent instead leaves 10,972 gapped, overlapping bodies.

    This does not use `fn_extent --group`: that groups on shared `UNWIND_INFO`,
    which the linker deduplicates across unrelated functions."""
    data = fn_extent.load()
    ents = pdata_entries()
    index = {b: i for i, (b, _e, _u) in enumerate(ents)}
    parent: dict[int, int] = {}
    for i, (_b, _e, unwind) in enumerate(ents):
        info = fn_extent.unwind(data, unwind)
        if not info or not info[1] & UNW_FLAG_CHAININFO:
            continue
        up = index.get(info[5][0])
        if up is None:
            raise SystemExit(
                f"chained entry {ents[i][0] + IMAGE_BASE:#x} names a parent "
                f"{info[5][0] + IMAGE_BASE:#x} that is not an extent"
            )
        parent[i] = up

    def root(i: int) -> int:
        path = []
        while i in parent:
            path.append(i)
            i = parent[i]
        for j in path:
            parent[j] = i
        return i

    members: dict[int, list[int]] = {}
    for i in parent:
        members.setdefault(root(i), []).append(i)

    functions = {
        i: (begin, stop, [(begin, stop)]) for i, (begin, stop, _u) in enumerate(ents)
    }
    for r, group in members.items():
        pieces = sorted([functions[r][2][0]] + [functions[m][2][0] for m in group])
        functions[r] = (pieces[0][0], pieces[-1][1], pieces)
    # A cold half is a piece of its root, not a function of its own. Chains
    # reach 8 deep, so an intermediate piece is a member of the same root.
    for group in members.values():
        for m in group:
            functions.pop(m, None)
    fragments = {ents[m][0]: r for r, group in members.items() for m in group}
    return functions, fragments


def pdata_bodies(start: int, end: int) -> list[tuple[int, int, int, list[tuple[int, int]]]]:
    """Functions whose chain-root begin lies in [start, end] (absolute
    addresses), as (begin_abs, end_abs, begin_rva, pieces). `pieces` are the
    entry extents that make the function, ascending, absolute; a cold half is a
    piece of a function, never a function of its own."""
    functions, _fragments = unions()
    out = [
        (
            IMAGE_BASE + begin,
            IMAGE_BASE + stop,
            begin,
            [(IMAGE_BASE + b, IMAGE_BASE + e) for b, e in pieces],
        )
        for begin, stop, pieces in functions.values()
        if start <= IMAGE_BASE + begin <= end
    ]
    out.sort()
    return out


def disassemble(
    begin: int,
    stop: int,
    lo: int | None = None,
    hi: int | None = None,
    note: str = "linear .pdata disassembly",
) -> str:
    """Linear disassembly of [begin, stop) straight from the image.

    Decoding always starts at `begin`: starting at an interior address can land
    mid-instruction and desync the whole window. `lo`/`hi` only filter the
    output.
    """
    # When filtering to a window, decode a few bytes past its end so the last
    # instruction is complete instead of being emitted as `.byte`.
    data_stop = stop if hi is None else min(stop + 15, IMAGE_BASE + len(image()))
    data = image()[begin - IMAGE_BASE : data_stop - IMAGE_BASE]
    window = f" window {lo:#x}..{hi:#x}" if lo is not None else ""
    lines = [f"; {begin:#x}..{stop:#x} {note}{window}"]
    for ins in _MD.disasm(data, begin):
        if lo is not None and ins.address < lo:
            continue
        if hi is not None and ins.address >= hi:
            break
        lines.append(
            f"{ins.address:016x}  {ins.bytes.hex():<20} {ins.mnemonic:<8} {ins.op_str}"
        )
    return "\n".join(lines) + "\n"


def _create(con: sqlite3.Connection) -> None:
    con.execute(
        """CREATE TABLE IF NOT EXISTS functions(
               address TEXT PRIMARY KEY,
               begin INTEGER, size INTEGER, body_end INTEGER,
               fragments INTEGER,
               name TEXT, first_comment TEXT,
               file TEXT, asm_file TEXT, entry TEXT, kind TEXT,
               decompile_checked INTEGER DEFAULT 0, c_cut INTEGER DEFAULT 0)"""
    )
    con.execute(
        """CREATE TABLE IF NOT EXISTS fragments(
               begin INTEGER PRIMARY KEY, end INTEGER, root INTEGER)"""
    )
    # A function can belong to more than one range tag. Membership lives in its
    # own table so rebuilding or purging one tag never removes another's rows.
    con.execute(
        """CREATE TABLE IF NOT EXISTS tagged(
               tag TEXT NOT NULL, address TEXT NOT NULL,
               PRIMARY KEY(tag, address))"""
    )
    con.execute("CREATE INDEX IF NOT EXISTS functions_begin ON functions(begin)")
    # Whole-image relationship index (RVAs), built by `index` from the image
    # bytes alone. `fn` is the function register: every chain-root union, not
    # just the ranges that have been built.
    con.execute(
        """CREATE TABLE IF NOT EXISTS fn(
               begin INTEGER PRIMARY KEY, end INTEGER, pieces INTEGER,
               opaque INTEGER DEFAULT 0)"""
    )
    # One row per control transfer instruction, keyed by its own address so a
    # site cannot be recorded twice. `src` is the containing function root or 0
    # for code no `.pdata` entry describes; `target` is the raw address (it may
    # be a cold half, or code with no function object - the 33,684 targets).
    con.execute(
        """CREATE TABLE IF NOT EXISTS edges(
               site INTEGER PRIMARY KEY, src INTEGER, target INTEGER,
               form TEXT, evidence TEXT)"""
    )
    con.execute("CREATE INDEX IF NOT EXISTS edges_target ON edges(target)")
    con.execute("CREATE INDEX IF NOT EXISTS edges_src ON edges(src)")
    # Memory-indirect dispatch: the target depends on a runtime register, so the
    # only static fact is the slot offset. `width` is the displacement size
    # (1 or 4) and matters: a slot below 0x80 is invisible to a disp32-only scan.
    con.execute(
        """CREATE TABLE IF NOT EXISTS slot_sites(
               site INTEGER PRIMARY KEY, src INTEGER, kind TEXT,
               disp INTEGER, width INTEGER, base TEXT)"""
    )
    con.execute("CREATE INDEX IF NOT EXISTS slot_disp ON slot_sites(disp, width)")


def _ensure_columns(con: sqlite3.Connection) -> None:
    """Additive table changes. A column added after an index was built appears
    on the next open instead of failing a three-minute decode at the last step."""
    for table, columns in (
        ("fn", (("opaque", "INTEGER DEFAULT 0"),)),
        ("slot_sites", (("base", "TEXT"),)),
    ):
        have = {row[1] for row in con.execute(f"PRAGMA table_info({table})")}
        for name, decl in columns:
            if name not in have:
                con.execute(f"ALTER TABLE {table} ADD COLUMN {name} {decl}")


def _migrate_v2(con: sqlite3.Connection, root: pathlib.Path) -> None:
    """Repair a schema-2 index (one row per `.pdata` entry) in place.

    A fragment row is dropped: its `.asm` is a cold half and its `.c` is a
    funclet decompile, not a function read. A root whose Ghidra body stopped at
    its extent keeps its `.c` but is marked `c_cut` and its `.asm` is rewritten
    over the union. Everything else is untouched, so the stored decompiles are
    not lost."""
    functions, fragments = unions()
    by_begin = {begin: (begin, stop, pieces) for begin, stop, pieces in functions.values()}
    cols = {row[1] for row in con.execute("PRAGMA table_info(functions)")}
    for name, decl in (
        ("fragments", "INTEGER"),
        ("c_cut", "INTEGER DEFAULT 0"),
        ("body_end", "INTEGER"),
    ):
        if name not in cols:
            con.execute(f"ALTER TABLE functions ADD COLUMN {name} {decl}")
    con.execute(
        """CREATE TABLE IF NOT EXISTS fragments(
               begin INTEGER PRIMARY KEY, end INTEGER, root INTEGER)"""
    )

    dropped = rewritten = cut = kept = 0
    for address, rva, size, file, asm_file in con.execute(
        "SELECT address,begin,size,file,asm_file FROM functions"
    ).fetchall():
        if rva in fragments:
            for rel in (file, asm_file):
                if not rel:
                    continue
                other = con.execute(
                    "SELECT 1 FROM functions WHERE (file=? OR asm_file=?)"
                    " AND address<>? LIMIT 1",
                    (rel, rel, address),
                ).fetchone()
                if other is None:
                    (root / rel).unlink(missing_ok=True)
            con.execute("DELETE FROM tagged WHERE address=?", (address,))
            con.execute("DELETE FROM functions WHERE address=?", (address,))
            dropped += 1
            continue
        union = by_begin.get(rva)
        if union is None:
            continue
        begin, stop, pieces = union
        if stop - begin != size:
            if asm_file:
                (root / asm_file).write_text(
                    disassemble(IMAGE_BASE + begin, IMAGE_BASE + stop),
                    encoding="utf-8",
                    newline="",
                )
            con.execute(
                "UPDATE functions SET size=?, body_end=?, fragments=?, c_cut=?"
                " WHERE address=?",
                (stop - begin, stop, len(pieces), 1 if file else 0, address),
            )
            rewritten += 1
            cut += 1 if file else 0
        else:
            con.execute(
                "UPDATE functions SET fragments=? WHERE address=?",
                (len(pieces), address),
            )
            kept += 1
    con.execute("DELETE FROM fragments")
    for address, rva in con.execute("SELECT address,begin FROM functions").fetchall():
        union = by_begin.get(rva)
        if union is None:
            continue
        for begin, stop in union[2][1:]:
            con.execute(
                "INSERT OR REPLACE INTO fragments(begin,end,root) VALUES(?,?,?)",
                (begin, stop, union[0]),
            )
    try:
        con.execute("ALTER TABLE functions DROP COLUMN tag")
    except sqlite3.OperationalError:
        pass  # column already gone
    con.execute("DELETE FROM meta WHERE key='schema'")
    con.commit()
    print(
        f"migrated {root}/corpus.db to schema {SCHEMA}: {dropped} cold halves "
        f"dropped, {rewritten} bodies widened ({cut} decompiles marked c_cut), "
        f"{kept} exact"
    )


def db(root: pathlib.Path) -> sqlite3.Connection:
    root.mkdir(parents=True, exist_ok=True)
    con = sqlite3.connect(root / "corpus.db")
    con.execute("CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT)")
    have = con.execute("SELECT value FROM meta WHERE key='schema'").fetchone()
    if have is None:
        _create(con)
    elif have[0] != str(SCHEMA):
        if have[0] == "2":
            _migrate_v2(con, root)
        else:
            raise SystemExit(
                f"{root}/corpus.db is schema {have[0]}, this tool writes {SCHEMA}: "
                "move the folder aside and rebuild the ranges you need"
            )
    # Every table is CREATE ... IF NOT EXISTS, so a table added after a build
    # appears on the next open without invalidating the index.
    _create(con)
    _ensure_columns(con)
    con.execute(
        "INSERT OR REPLACE INTO meta(key,value) VALUES('schema',?)", (str(SCHEMA),)
    )
    con.commit()
    return con


def check_image(con: sqlite3.Connection) -> str:
    sha = hashlib.sha256(image()).hexdigest()
    row = con.execute("SELECT value FROM meta WHERE key='image_sha256'").fetchone()
    if row is not None and row[0] != sha:
        raise SystemExit(
            "corpus index belongs to a different image "
            f"({row[0][:16]} != {sha[:16]}); purge the tags first"
        )
    con.execute(
        "INSERT OR REPLACE INTO meta(key,value) VALUES('image_sha256',?)", (sha,)
    )
    con.execute(
        "INSERT OR REPLACE INTO meta(key,value) VALUES('image_bytes',?)",
        (str(IMG.stat().st_size),),
    )
    con.execute(
        "INSERT OR REPLACE INTO meta(key,value) VALUES('schema',?)", (str(SCHEMA),)
    )
    con.execute(
        "INSERT OR REPLACE INTO meta(key,value) VALUES('program',?)", (PROGRAM,)
    )
    con.execute(
        "INSERT OR REPLACE INTO meta(key,value) VALUES('built',?)",
        (datetime.now(timezone.utc).isoformat(timespec="seconds"),),
    )
    return sha


def safe(name: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.-]+", "_", name).strip("_") or "unnamed"


def _first_comment(code: str) -> str:
    for line in code.splitlines():
        line = line.strip()
        if line.startswith("//") and "WARNING" not in line:
            return line[3:120].strip()
        if line and not line.startswith("//"):
            break
    return ""


def _bridge_addr(value: object) -> int | None:
    """An address from the bridge: hex with or without `0x`, never decimal."""
    if value is None:
        return None
    try:
        n = int(str(value).strip(), 16)
    except ValueError:
        return None
    return n if IMAGE_BASE <= n else None


def _stored_comment(root: pathlib.Path, rel: str | None) -> str:
    """First comment line of a stored decompile, empty when it is gone."""
    if not rel:
        return ""
    path = root / rel
    if not path.is_file():
        return ""
    return _first_comment(path.read_text(encoding="utf-8", errors="replace"))


def cmd_build(
    start: int,
    end: int,
    root: pathlib.Path,
    tag: str,
    force: bool = False,
    asm_only: bool = False,
    batch: int = 16,
) -> None:
    tag = safe(tag)
    lo = IMAGE_BASE + start if start < IMAGE_BASE else start
    hi = IMAGE_BASE + end if end < IMAGE_BASE else end
    bodies = pdata_bodies(lo, hi)
    print(f"build {tag}: {len(bodies)} functions in [{lo:#x}, {hi:#x}]")
    con = db(root)
    check_image(con)
    # Storage is shared across tags: a function belongs to ranges, not to a
    # folder. Tag membership lives in `tagged`, so rebuilding or purging one
    # range never moves another range's files.
    asm_dir = root / "asm"
    c_dir = root / "c"
    asm_dir.mkdir(parents=True, exist_ok=True)
    c_dir.mkdir(parents=True, exist_ok=True)

    existing = {}
    for row in con.execute(
        "SELECT address, name, file, entry, decompile_checked, c_cut FROM functions"
    ):
        existing[int(row[0], 16)] = row[1:]

    asm_written = decompiled = misses = skipped = errors = 0
    checked_now: set[int] = set()

    # 1. Disassembly over the chain-root union, including bodies Ghidra does
    #    not define. A cold half is part of its root and is not written alone.
    for i, (begin, stop, _rva, _pieces) in enumerate(bodies, 1):
        name = existing.get(begin, (None,))[0] or f"FUN_{begin:x}"
        asm_name = f"{begin:010x}_{safe(name)}.asm"
        (asm_dir / asm_name).write_text(
            disassemble(begin, stop), encoding="utf-8", newline=""
        )
        asm_written += 1
        if i % 100 == 0:
            print(f"  {i}/{len(bodies)} disassembled", end="\r")
    if bodies:
        print()

    # 2. Decompiles for bodies Ghidra has, skipping what is already stored.
    if not asm_only:
        need: list[tuple[int, int, int]] = []
        for begin, stop, _rva, _pieces in bodies:
            old = existing.get(begin)
            if not force and old is not None:
                # (name, file, entry, decompile_checked)
                file_ok = not old[1] or (root / old[1]).is_file()
                # A stored decompile or a recorded Ghidra miss both mean the
                # bridge already answered for this body.
                if file_ok and (old[1] or old[3]):
                    skipped += 1
                    continue
            need.append((begin, stop, begin))
        addrs = [f"{b:#x}" for b, _, _rva, _pieces in need]

        def fetch(a: str):
            # A bridge error (timeout, restart, wrong program) is not the
            # same answer as "Ghidra has no function here". Retry transport
            # failures and never turn one into a recorded miss.
            for attempt in range(3):
                r = gh.call("/get_functions", {"functions": a, "program": PROGRAM})
                if "functions" in r:
                    return r["functions"].get(a), True
                time.sleep(0.5 * (attempt + 1))
            return None, False

        results: dict[int, tuple[object, bool]] = {}
        for i in range(0, len(need), batch):
            chunk = need[i : i + batch]
            with cf.ThreadPoolExecutor(4) as ex:
                for (begin, _s, _r, _p), result in zip(
                    chunk, ex.map(fetch, addrs[i : i + batch])
                ):
                    results[begin] = result
            done = min(i + batch, len(need))
            print(f"  {done}/{len(need)} decompiled", end="\r")
        if need:
            print()

        for begin, stop, _rva, _pieces in need:
            fn, answered = results.get(begin, (None, False))
            if not answered:
                errors += 1
                continue
            checked_now.add(begin)
            code = (fn or {}).get("decompiled_code") or ""
            if not fn or not code.strip():
                misses += 1
                # Never keep a stale path from an earlier build.
                old = existing.get(begin) or (
                    f"FUN_{begin:x}",
                    None,
                    f"{begin:#x}",
                    0,
                )
                existing[begin] = (old[0], None, old[2] or f"{begin:#x}", 1)
                continue
            decompiled += 1
            entry = int(str(fn.get("entry_point") or f"{begin:x}"), 16)
            name = fn.get("name") or f"FUN_{begin:x}"
            fname = f"{entry:010x}_{safe(name)}.c"
            (c_dir / fname).write_text(code, encoding="utf-8", newline="")
            # Ghidra decompiled the body it owns, which until the chain merge
            # (audit section 8.2) stops at the hot part's extent.
            body_end = _bridge_addr(fn.get("body_end"))
            cut = 1 if body_end is not None and body_end < stop else 0
            existing[begin] = (name, f"c/{fname}", f"{entry:#x}", 1, cut)

    # 3. Index every function, preserving a stored decompile when this pass
    #    skipped it, and recording the cold halves as pieces of the root.
    for begin, stop, _rva, pieces in bodies:
        old = existing.get(begin)
        if old is not None:
            name, file, entry, old_checked = old[:4]
            c_cut = old[4] if len(old) > 4 else 0
            entry = entry or f"{begin:#x}"
            old_checked = old_checked or 0
        else:
            name, file, entry, old_checked = f"FUN_{begin:x}", None, f"{begin:#x}", 0
            c_cut = 0
        checked = 1 if (file or begin in checked_now) else old_checked
        kind = "ghidra" if file else "pdata-chunk"
        asm_name = f"{begin:010x}_{safe(name)}.asm"
        con.execute(
            """INSERT INTO functions(address,begin,size,body_end,fragments,name,
                                     first_comment,file,asm_file,entry,kind,
                                     decompile_checked,c_cut)
               VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)
               ON CONFLICT(address) DO UPDATE SET
                 begin=excluded.begin, size=excluded.size,
                 body_end=excluded.body_end, fragments=excluded.fragments,
                 name=excluded.name,
                 first_comment=excluded.first_comment, file=excluded.file,
                 asm_file=excluded.asm_file, entry=excluded.entry,
                 kind=excluded.kind,
                 decompile_checked=excluded.decompile_checked,
                 c_cut=excluded.c_cut""",
            (
                f"{begin:#x}",
                begin - IMAGE_BASE,
                stop - begin,
                stop - IMAGE_BASE,
                len(pieces),
                name,
                _stored_comment(root, file),
                file,
                f"asm/{asm_name}",
                entry,
                kind,
                1 if checked else 0,
                c_cut,
            ),
        )
        con.execute("DELETE FROM fragments WHERE root=?", (begin - IMAGE_BASE,))
        for piece_begin, piece_end in pieces[1:]:
            con.execute(
                "INSERT OR REPLACE INTO fragments(begin,end,root) VALUES(?,?,?)",
                (piece_begin - IMAGE_BASE, piece_end - IMAGE_BASE, begin - IMAGE_BASE),
            )
        con.execute(
            "INSERT OR REPLACE INTO tagged(tag,address) VALUES(?,?)",
            (tag, f"{begin:#x}"),
        )
    con.commit()
    if asm_only:
        print(
            f"built tag '{tag}': {asm_written} disassemblies "
            f"(bridge not called) -> {root}"
        )
    else:
        print(
            f"built tag '{tag}': {asm_written} disassemblies, {decompiled} "
            f"decompiles, {skipped} skipped, {misses} Ghidra misses, "
            f"{errors} bridge errors -> {root}"
        )


def cmd_make_args(start: int, end: int, out: pathlib.Path) -> None:
    """The headless args file (whitespace-separated start end pairs).

    Pairs are chain-root unions, one per function. A chained start is refused:
    `McpDecompile.java` creates a function at any entry Ghidra lacks, and
    creating one at a cold half trims its parent (audit section 4)."""
    lo = IMAGE_BASE + start if start < IMAGE_BASE else start
    hi = IMAGE_BASE + end if end < IMAGE_BASE else end
    bodies = pdata_bodies(lo, hi)
    _functions, fragments = unions()
    for begin, _stop, _rva, _pieces in bodies:
        if begin - IMAGE_BASE in fragments:
            raise SystemExit(
                f"refusing to emit chained start {begin:#x}: it is a cold half, "
                "not a function; sweeping it would trim its root"
            )
    whole = lo <= IMAGE_BASE + 0x1000 and hi >= IMAGE_BASE + TEXT_END
    if whole and len(bodies) != REAL_ENTRIES:
        raise SystemExit(
            f"whole-image sweep produced {len(bodies)} pairs, expected "
            f"{REAL_ENTRIES}: the .pdata read is wrong"
        )
    with out.open("w", encoding="utf-8") as f:
        f.write(f"# chain-root unions for [{lo:#x}, {hi:#x}]\n")
        f.write(
            f"# {len(bodies)} functions; {PDATA_ENTRIES} .pdata entries = "
            f"{REAL_ENTRIES} roots + {CHAINED_ENTRIES} chained halves\n"
        )
        for begin, stop, _rva, _pieces in bodies:
            f.write(f"{begin:#x} {stop:#x}\n")
    print(f"args file {out} : {len(bodies)} pairs; run McpDecompile.java with the GUI closed")


# Conditional jumps. `call`, `jmp` and these are the only mnemonics that
# transfer control to a static address.
JCC = {
    "ja", "jae", "jb", "jbe", "je", "jne", "jg", "jge", "jl", "jle",
    "jno", "jnp", "jns", "jo", "jp", "js", "jz", "jnz", "jpe", "jpo",
    "jcxz", "jecxz", "jrcxz",
}
# Documented facts the index must reproduce, as (what, expected):
# `docs/systems-coverage.md` row 12 fixes the chunk assembler's caller, and
# `docs/open-questions.md` line 1685 fixes the disp8/disp32 slot split.
GOLDEN_EDGE = (0x1420251F5 - IMAGE_BASE, 0x14209FC90 - IMAGE_BASE)
# `docs/formats/shape-ingest.md`: 27 `ShapeAddToBody` sites in 12 function
# groups (an earlier count said 13 because a cold half was counted as a
# function). The pair is the test of the function model, not just of the edges.
GOLDEN_CENSUS = (0x142CB3940 - IMAGE_BASE, 27, 12)
# Register-slot calls (base not rsp/r12) at +0x40, disp8, from the linear
# decode: 485. `dev/call_slot_scan.py` counts 505 raw byte patterns / 492
# confirmed in a byte-scan context; the reconciliation (38 SIB stack forms, 12
# decode-only, 19 byte-scan-only) is in docs/open-questions.md.
GOLDEN_SLOT40 = 485


def _disp_width(ins: capstone.CsInsn, disp: int) -> int:
    """Displacement size of a memory operand: 1 (disp8), 4 (disp32) or 0.

    The distinction is load-bearing: a vtable slot below 0x80 is only
    encodable as `FF 5x 40` (disp8), so a disp32-only scan reports zero callers
    for every early slot (`dev/call_slot_scan.py`)."""
    raw = ins.bytes
    if raw:
        last = raw[-1] - 256 if raw[-1] >= 128 else raw[-1]
        if last == disp:
            return 1
    if len(raw) >= 4 and int.from_bytes(raw[-4:], "little", signed=True) == disp:
        return 4
    return 0


def index_faults(con: sqlite3.Connection) -> list[str]:
    """The documented facts the edge index has to reproduce."""
    faults: list[str] = []
    got = con.execute("SELECT target FROM edges WHERE site=?", (GOLDEN_EDGE[0],)).fetchone()
    if got is None:
        faults.append(
            f"no edge at site {GOLDEN_EDGE[0] + IMAGE_BASE:#x} "
            f"(docs/systems-coverage.md row 12)"
        )
    elif got[0] != GOLDEN_EDGE[1]:
        faults.append(
            f"edge {GOLDEN_EDGE[0] + IMAGE_BASE:#x} -> {got[0] + IMAGE_BASE:#x}, "
            f"docs say {GOLDEN_EDGE[1] + IMAGE_BASE:#x}"
        )
    slot = con.execute(
        "SELECT COUNT(*) FROM slot_sites WHERE kind='call' AND disp=0x40 "
        "AND width=1 AND base NOT IN ('rsp','r12')"
    ).fetchone()[0]
    if slot != GOLDEN_SLOT40:
        faults.append(
            f"register-slot call +0x40 (disp8) sites {slot}, expected "
            f"{GOLDEN_SLOT40} (docs/open-questions.md line 1685)"
        )
    sites, groups = con.execute(
        "SELECT COUNT(*), COUNT(DISTINCT src) FROM edges WHERE target=? AND form='call'",
        (GOLDEN_CENSUS[0],),
    ).fetchone()
    if (sites, groups) != GOLDEN_CENSUS[1:]:
        faults.append(
            f"ShapeAddToBody {GOLDEN_CENSUS[0] + IMAGE_BASE:#x}: {sites} sites in "
            f"{groups} functions, docs say {GOLDEN_CENSUS[1]} in {GOLDEN_CENSUS[2]} "
            f"(docs/formats/shape-ingest.md)"
        )
    bad = con.execute(
        "SELECT COUNT(*) FROM edges WHERE form IN ('call','jmp','jcc') "
        "AND (target < 0x1000 OR target >= ?)",
        (TEXT_END,),
    ).fetchone()[0]
    if bad:
        faults.append(f"{bad} code edges point outside .text")
    api = con.execute(
        "SELECT COUNT(*) FROM edges WHERE form LIKE 'api-%' AND (target < 0 OR target >= ?)",
        (len(image()),),
    ).fetchone()[0]
    if api:
        faults.append(f"{api} api edges point outside the image")
    return faults


def build_index(root: pathlib.Path, only: str | None = None) -> int:
    """Build the whole-image relationship index from the image bytes.

    Everything is an RVA (add 0x140000000). One linear decode anchored on
    instruction boundaries - not a byte pattern scan, which is 54% false
    positives on a 2 MB measurement - fills `edges` (call/jmp/jcc, and the
    rip-relative `FF 15`/`FF 25` Windows-API form) and `slot_sites`
    (`call`/`jmp [reg+disp]`, the vtable dispatch no `E8` reaches, with the base
    register so stack-local call-throughs can be told apart). `fn` is the
    function register; `opaque` marks a function whose linear listing desyncs
    (a branch lands inside its own instruction, or a code transfer points
    outside `.text`), where the disassembly must be read in the decompiler.

    Attribution is by union span, so a call into a cold half or into
    `.pdata`-less code is still recorded, with the raw target address."""
    started = time.time()
    functions, _fragments = unions()
    con = db(root)
    spans = sorted((b, e, b) for b, e, _p in functions.values())
    begins = [span[0] for span in spans]

    if only in (None, "fn"):
        con.execute("DELETE FROM fn")
        con.executemany(
            "INSERT INTO fn(begin,end,pieces) VALUES(?,?,?)",
            [(b, e, len(p)) for b, e, p in functions.values()],
        )
        con.commit()
        print(f"fn: {len(functions)} functions")

    if only in (None, "edges", "slots"):
        md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
        md.detail = True
        md.skipdata = True
        data = image()
        lo = IMAGE_BASE + 0x1000
        hi = IMAGE_BASE + TEXT_END
        edges: list[tuple[int, int, int, str, str]] = []
        slots: list[tuple[int, int, str, int, int, str]] = []
        opaque: set[int] = set()
        reg_indirect = dropped = 0
        for ins in md.disasm(data[lo - IMAGE_BASE : hi - IMAGE_BASE], lo):
            mnemonic = ins.mnemonic
            if mnemonic not in JCC and mnemonic not in ("call", "jmp"):
                continue
            if not ins.operands:
                continue
            site = ins.address - IMAGE_BASE
            i = bisect.bisect_right(begins, site) - 1
            src = spans[i][2] if i >= 0 and site < spans[i][1] else 0
            kind = "call" if mnemonic == "call" else ("jmp" if mnemonic == "jmp" else "jcc")
            op = ins.operands[0]
            if op.type == capstone.x86.X86_OP_IMM:
                target = op.imm - IMAGE_BASE if op.imm >= IMAGE_BASE else op.imm
                if not 0x1000 <= target < TEXT_END:
                    # Not a code transfer. Two causes, both worth marking: the
                    # linear listing desynced, or the target is data.
                    opaque.add(src)
                    dropped += 1
                    continue
                if kind != "call" and site < target < site + ins.size:
                    opaque.add(src)  # a branch inside its own instruction
                edges.append((site, src, target, kind, "decoded"))
            elif op.type == capstone.x86.X86_OP_MEM:
                mem = op.mem
                if mem.base == capstone.x86.X86_REG_RIP:
                    # Windows API and global dispatch tables: `FF 15` / `FF 25`.
                    target = ins.address + ins.size + mem.disp - IMAGE_BASE
                    form = "api-call" if kind == "call" else "api-jmp"
                    edges.append((site, src, target, form, "decoded"))
                    continue
                width = _disp_width(ins, mem.disp)
                if width == 0:
                    reg_indirect += 1  # `call rax`: no static fact at all
                    continue
                slots.append(
                    (site, src, kind, mem.disp, width, ins.reg_name(mem.base))
                )
        con.execute("DELETE FROM edges")
        con.execute("DELETE FROM slot_sites")
        con.execute("UPDATE fn SET opaque=0")
        con.executemany(
            "INSERT OR REPLACE INTO edges(site,src,target,form,evidence) VALUES(?,?,?,?,?)",
            edges,
        )
        con.executemany(
            "INSERT OR REPLACE INTO slot_sites(site,src,kind,disp,width,base) "
            "VALUES(?,?,?,?,?,?)",
            slots,
        )
        if opaque:
            con.executemany(
                "UPDATE fn SET opaque=1 WHERE begin=?", [(s,) for s in opaque]
            )
        con.commit()
        print(
            f"edges: {len(edges)} ({dropped} code transfers dropped as "
            f"not-code, {reg_indirect} register-indirect sites have no target); "
            f"slot sites: {len(slots)}; opaque functions: {len(opaque)}"
        )

    if only is None:
        con.execute(
            "INSERT OR REPLACE INTO meta(key,value) VALUES('indexed',?)",
            (datetime.now(timezone.utc).isoformat(timespec="seconds"),),
        )
        con.commit()
    for form, n in con.execute(
        "SELECT form, COUNT(*) FROM edges GROUP BY form ORDER BY 2 DESC"
    ):
        print(f"  {form:<10} {n:>7}")
    faults = index_faults(con)
    for fault in faults:
        print(f"FAULT {fault}")
    print(
        f"index built in {time.time() - started:.0f}s"
        if not faults
        else "index has faults"
    )
    return 1 if faults else 0


def cmd_verify(root: pathlib.Path | None = None) -> int:
    """Pin the function model. Any mismatch is a fault, not a warning.

    Every number here was measured from the image; the Ghidra-side counts in
    `docs/ghidra-image-audit.md` differ by exactly the 11 chain roots that have
    no function object (11,366 bytes of continuation), which is why this one is
    the image-side pin."""
    ents = pdata_entries()
    functions, fragments = unions()
    roots = [f for f in functions.values() if len(f[2]) > 1]
    faults: list[str] = []

    if len(fragments) != CHAINED_ENTRIES:
        faults.append(f"chained entries {len(fragments)} != {CHAINED_ENTRIES}")
    if len(functions) != REAL_ENTRIES:
        faults.append(f"functions {len(functions)} != {REAL_ENTRIES}")
    if len(roots) != CHAIN_ROOTS:
        faults.append(f"roots with a cold part {len(roots)} != {CHAIN_ROOTS}")

    claimed: dict[int, int] = {}
    for _r, (begin, stop, pieces) in functions.items():
        if len(pieces) > 1:
            for (_ab, ae), (bb, _be) in zip(pieces, pieces[1:]):
                if ae != bb:
                    faults.append(
                        f"gap or overlap in {begin + IMAGE_BASE:#x}: "
                        f"{ae + IMAGE_BASE:#x} -> {bb + IMAGE_BASE:#x}"
                    )
            if pieces[-1][1] != stop:
                faults.append(f"union end wrong for {begin + IMAGE_BASE:#x}")
    for frag in fragments:
        claimed[frag] = claimed.get(frag, 0) + 1
    doubles = [f for f, n in claimed.items() if n > 1]
    if doubles:
        faults.append(f"{len(doubles)} pieces claimed by more than one function")

    single = sum(1 for f in functions.values() if len(f[2]) == 1)
    print(
        f".pdata entries          {len(ents):>7}\n"
        f"functions (chain roots) {len(functions):>7}  "
        f"({single} single-part, {len(roots)} with a cold part)\n"
        f"chained halves          {len(fragments):>7}\n"
        f"body bytes              {sum(f[1] - f[0] for f in functions.values()):>7}  "
        f"(root + cold parts)"
    )

    if (root / "corpus.db").is_file():
        con = db(root)
        known = {begin for begin, _stop, _pieces in functions.values()}
        rows = con.execute("SELECT address,begin,fragments FROM functions").fetchall()
        stray = [a for a, begin, _n in rows if begin not in known]
        cold_as_row = [a for a, _begin, n in rows if n == 1 and a in {f"{f:#x}" for f in fragments}]
        pieces = con.execute("SELECT COUNT(*) FROM fragments").fetchone()[0]
        print(
            f"index rows              {len(rows):>7}; cold halves indexed as "
            f"rows: {len(cold_as_row)}; stored piece rows: {pieces}"
        )
        if stray:
            faults.append(f"{len(stray)} index rows are not chain roots")
        if cold_as_row:
            faults.append(f"{len(cold_as_row)} cold halves are still indexed as functions")
        indexed = con.execute("SELECT COUNT(*) FROM edges").fetchone()[0]
        if indexed:
            print(
                f"edges                   {indexed:>7}; slot sites "
                f"{con.execute('SELECT COUNT(*) FROM slot_sites').fetchone()[0]}"
            )
            faults.extend(index_faults(con))

    if faults:
        for fault in faults[:20]:
            print(f"FAULT {fault}")
        return 1
    print("verify ok")
    return 0


def _parse_addr(what: str) -> int | None:
    token = str(what).lower().removeprefix("0x")
    if not token or set(token) - set("0123456789abcdef"):
        return None
    v = int(token, 16)
    if IMAGE_BASE <= v < IMAGE_BASE + TEXT_END:
        return v
    if v < TEXT_END:
        return IMAGE_BASE + v
    return None


def resolve(con: sqlite3.Connection, what: str, window: int | None = None):
    """Return rows for an address (exact, then containing) or an exact name.

    An address inside a cold half resolves through the function's union row,
    because a cold half is no longer a row of its own."""
    cols = "address,name,file,asm_file,begin,body_end,fragments,c_cut,entry"
    addr = _parse_addr(what)
    if addr is not None:
        rva = addr - IMAGE_BASE
        rows = con.execute(
            f"SELECT {cols} FROM functions WHERE address=?", (f"{addr:#x}",)
        ).fetchall()
        if rows:
            return rows
        return con.execute(
            f"SELECT {cols} FROM functions "
            "WHERE begin<=? AND body_end>? ORDER BY begin DESC LIMIT 1",
            (rva, rva),
        ).fetchall()
    return con.execute(
        f"SELECT {cols} FROM functions WHERE name=? COLLATE NOCASE",
        (what,),
    ).fetchall()


def cmd_show(
    what: str, root: pathlib.Path, want_asm: bool, window: int | None
) -> int:
    con = db(root)
    rows = resolve(con, str(what), window)
    if not rows:
        addr = _parse_addr(str(what))
        if addr is not None and window is not None:
            # Code outside every stored .pdata chunk still exists (the ramp
            # leaves live in such a gap); decode from the requested address.
            hi = min(addr + 2 * window, IMAGE_BASE + TEXT_END)
            print(f"// {addr:#x} is not inside a stored .pdata chunk")
            print(
                disassemble(
                    addr,
                    hi,
                    lo=addr,
                    hi=hi,
                    note="linear from the requested address; not a .pdata chunk",
                )
            )
            return 0
        print(f"not in corpus: {what} (build the range first)")
        return 1
    if len(rows) > 1:
        print(f"several matches for {what}:")
        for addr, name, *_ in rows:
            print(f"  {addr} {name}")
        return 2
    addr, name, file, asm_file, begin, body_end, pieces, c_cut, entry = rows[0]
    tags = con.execute(
        "SELECT group_concat(tag) FROM tagged WHERE address=?", (addr,)
    ).fetchone()[0]
    print(
        f"// {addr} {name} [{tags or '-'}] function {begin + IMAGE_BASE:#x}.."
        f"{body_end + IMAGE_BASE:#x}" + (f" ({pieces} pieces)" if pieces != 1 else "")
        + f" entry {entry or addr}"
    )
    if c_cut:
        print(
            "// c_cut: this decompile came from a Ghidra body shorter than the "
            "function (chain merge not applied); read the disassembly too"
        )

    if window is not None:
        at = _parse_addr(str(what)) or (begin + IMAGE_BASE)
        lo = max(begin + IMAGE_BASE, at - window)
        hi = min(body_end + IMAGE_BASE, at + window)
        print(f"// window {lo:#x}..{hi:#x}")
        print(disassemble(begin + IMAGE_BASE, body_end + IMAGE_BASE, lo, hi))
        return 0

    if file:
        path = root / file
        if path.is_file():
            print(path.read_text(encoding="utf-8", errors="replace"))
        else:
            print(f"// decompile file missing: {path}")
    if want_asm or not file:
        path = root / asm_file if asm_file else None
        if path is None or not path.is_file():
            print("// no disassembly stored")
        else:
            if file:
                print("\n// ---- disassembly ----\n")
            print(path.read_text(encoding="utf-8", errors="replace"))
    return 0


def cmd_grep(
    pattern: str,
    root: pathlib.Path,
    flags: int,
    tag: str | None,
    asm_only: bool,
    c_only: bool,
) -> int:
    con = db(root)
    rx = re.compile(pattern, flags)
    if tag:
        c_sql = (
            "SELECT f.file,f.address,f.name FROM tagged t "
            "JOIN functions f ON f.address=t.address WHERE t.tag=? ORDER BY f.file"
        )
        a_sql = (
            "SELECT f.asm_file,f.address,f.name FROM tagged t "
            "JOIN functions f ON f.address=t.address WHERE t.tag=? ORDER BY f.asm_file"
        )
        params = (tag,)
    else:
        c_sql = "SELECT file,address,name FROM functions ORDER BY file"
        a_sql = "SELECT asm_file,address,name FROM functions ORDER BY asm_file"
        params = ()
    hits = 0

    if not asm_only:
        seen = set()
        for file_rel, addr, name in con.execute(c_sql, params):
            if not file_rel or file_rel in seen:
                continue
            seen.add(file_rel)
            path = root / file_rel
            if not path.is_file():
                continue
            for lineno, line in enumerate(
                path.read_text(encoding="utf-8", errors="replace").splitlines(), 1
            ):
                if rx.search(line):
                    hits += 1
                    print(f"{file_rel}:{lineno}  [{addr} {name}]  {line.strip()[:160]}")

    if not c_only:
        seen = set()
        for asm_rel, addr, name in con.execute(a_sql, params):
            if not asm_rel or asm_rel in seen:
                continue
            seen.add(asm_rel)
            path = root / asm_rel
            if not path.is_file():
                continue
            for lineno, line in enumerate(
                path.read_text(encoding="utf-8", errors="replace").splitlines(), 1
            ):
                if rx.search(line):
                    hits += 1
                    print(f"{asm_rel}:{lineno}  [{addr} {name}]  {line.strip()[:160]}")

    print(f"{hits} hit(s) for {pattern!r}")
    return 0 if hits else 1


def cmd_list(root: pathlib.Path) -> None:
    con = db(root)
    sha = con.execute("SELECT value FROM meta WHERE key='image_sha256'").fetchone()
    print(f"index image: {sha[0][:16] if sha else 'unknown'}")
    for tag, n, asm, dec, cut, lo, hi in con.execute(
        "SELECT t.tag, COUNT(*), SUM(f.asm_file IS NOT NULL), "
        "SUM(f.file IS NOT NULL), SUM(f.c_cut), MIN(f.begin), MAX(f.begin) "
        "FROM tagged t JOIN functions f ON f.address=t.address "
        "GROUP BY t.tag ORDER BY t.tag"
    ):
        print(
            f"  {tag:<14} {n:>5} functions  {asm or 0:>5} asm  {dec or 0:>5} c"
            f"  {cut or 0:>4} cut  {lo + IMAGE_BASE:#x} .. {hi + IMAGE_BASE:#x}"
        )


def cmd_purge(tag: str, root: pathlib.Path) -> None:
    tag = safe(tag)
    con = db(root)
    files = [
        row[0]
        for row in con.execute(
            "SELECT f.file FROM tagged t JOIN functions f ON f.address=t.address "
            "WHERE t.tag=? AND f.file IS NOT NULL "
            "UNION SELECT f.asm_file FROM tagged t "
            "JOIN functions f ON f.address=t.address "
            "WHERE t.tag=? AND f.asm_file IS NOT NULL",
            (tag, tag),
        )
    ]
    con.execute("DELETE FROM tagged WHERE tag=?", (tag,))
    # Rows with no remaining tag membership are orphans; drop them.
    con.execute(
        "DELETE FROM functions WHERE address NOT IN (SELECT address FROM tagged)"
    )
    con.commit()
    for rel in files:
        used = con.execute(
            "SELECT 1 FROM functions WHERE file=? OR asm_file=? LIMIT 1", (rel, rel)
        ).fetchone()
        if used is None:
            (root / rel).unlink(missing_ok=True)
    # Legacy per-tag folders may still hold files; the shared asm/ and c/
    # directories are pruned file by file above.
    folder = root / tag
    if folder.is_dir():
        shutil.rmtree(folder, ignore_errors=True)
    print(f"purged tag {tag}")


def main() -> int:
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("build")
    p.add_argument("start")
    p.add_argument("end")
    p.add_argument("--tag", default=None)
    p.add_argument("--out", type=pathlib.Path, default=DEFAULT_OUT)
    p.add_argument("--force", action="store_true", help="re-fetch every decompile")
    p.add_argument(
        "--asm-only",
        action="store_true",
        help="write/refresh disassembly without calling the bridge",
    )

    p = sub.add_parser("make-args")
    p.add_argument("start")
    p.add_argument("end")
    p.add_argument("out", type=pathlib.Path)

    p = sub.add_parser("show")
    p.add_argument("what")
    p.add_argument("--asm", action="store_true")
    p.add_argument("--window", type=lambda s: int(s, 0), default=None)
    p.add_argument("--out", type=pathlib.Path, default=DEFAULT_OUT)

    p = sub.add_parser("grep")
    p.add_argument("pattern")
    p.add_argument("-i", action="store_true")
    p.add_argument("--tag", default=None)
    p.add_argument("--asm-only", action="store_true")
    p.add_argument("--c-only", action="store_true")
    p.add_argument("--out", type=pathlib.Path, default=DEFAULT_OUT)

    p = sub.add_parser("list")
    p.add_argument("--out", type=pathlib.Path, default=DEFAULT_OUT)

    p = sub.add_parser("index")
    p.add_argument("--only", choices=("fn", "edges", "slots"), default=None)
    p.add_argument("--out", type=pathlib.Path, default=DEFAULT_OUT)

    p = sub.add_parser("verify")
    p.add_argument("--out", type=pathlib.Path, default=DEFAULT_OUT)

    p = sub.add_parser("purge")
    p.add_argument("tag")
    p.add_argument("--out", type=pathlib.Path, default=DEFAULT_OUT)

    args = ap.parse_args()
    hexv = lambda s: int(s, 16)  # noqa: E731
    if args.cmd == "build":
        cmd_build(
            hexv(args.start),
            hexv(args.end),
            args.out,
            args.tag or f"range_{args.start}",
            args.force,
            args.asm_only,
        )
    elif args.cmd == "make-args":
        cmd_make_args(hexv(args.start), hexv(args.end), args.out)
    elif args.cmd == "show":
        return cmd_show(args.what, args.out, args.asm, args.window)
    elif args.cmd == "grep":
        return cmd_grep(
            args.pattern,
            args.out,
            re.IGNORECASE if args.i else 0,
            args.tag,
            args.asm_only,
            args.c_only,
        )
    elif args.cmd == "list":
        cmd_list(args.out)
    elif args.cmd == "index":
        return build_index(args.out, args.only)
    elif args.cmd == "verify":
        return cmd_verify(args.out)
    elif args.cmd == "purge":
        cmd_purge(args.tag, args.out)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except BrokenPipeError:  # `show | head` is a normal way to read this
        raise SystemExit(0)
