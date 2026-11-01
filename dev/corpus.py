"""Build and query a standing corpus for the current image.

    python dev/corpus.py build <start> <end> [--tag mover] [--force] [--asm-only]
    python dev/corpus.py make-args <start> <end> <out.txt>   # headless args file
    python dev/corpus.py show <address|name> [--asm] [--window 0x40]
    python dev/corpus.py grep <pattern> [-i] [--tag mover] [--asm-only]
    python dev/corpus.py list
    python dev/corpus.py purge <tag>

The corpus lives inside the project at `dev/data/corpus` (ignored by the
whitelist `.gitignore`; decompiled client code is never tracked). One build
covers every function whose `.pdata` body lies in `[start, end]` of the current
image (flat: file offset == RVA). Each body gets an authoritative linear
disassembly from the image itself, `asm/<hex>_<name>.asm`, whether or not
Ghidra knows the function. Ghidra's decompile is added as
`c/<hex>_<name>.c` when it exists. Rows are indexed in `corpus.db`: address,
body extent, name, first comment, file paths, entry point. Tag membership is
a separate table, so one chunk can belong to several ranges and rebuilding or
purging a tag never moves another tag's files.

The disassembly is the point: it survives a truncated or flattened decompile,
covers `.pdata` chunks that Ghidra never defined, and is the form a 1-to-1 read
actually needs. `show --window` disassembles around any interior address.

`build` uses the bridge only for decompiles (fast for hot ranges, works while
the GUI is open). For whole-image sweeps use `make-args` plus the canonical
headless batch (docs/ghidra-project.md); it needs the GUI project closed. The
tool is read-only against the install and the Ghidra project.
"""

from __future__ import annotations

import argparse
import concurrent.futures as cf
import hashlib
import pathlib
import re
import shutil
import sqlite3
import struct
import sys
import time
from datetime import datetime, timezone

import capstone

HEAD = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HEAD))
import gh  # noqa: E402

ROOT = HEAD.parent
IMG = ROOT / "dev" / "data" / "offline_recover" / "module_offline.bin"
PDATA_RVA = 0x450B000
IMAGE_BASE = 0x140000000
TEXT_END = 0x4838000
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


def pdata_bodies(start: int, end: int) -> list[tuple[int, int, int]]:
    """Authoritative bodies whose absolute address lies in [start, end],
    returned as (begin_abs, end_abs, begin_rva). Flat image: file offset ==
    RVA, so the extents come from the image itself, not the bridge."""
    data = image()
    n = (len(data) - PDATA_RVA) // 12
    out = []
    prev = 0
    for i in range(n):
        begin, stop, unwind = struct.unpack_from("<III", data, PDATA_RVA + i * 12)
        if begin >= stop or stop >= TEXT_END or unwind >= TEXT_END:
            break
        if i and begin < prev:
            break
        prev = begin
        if start <= IMAGE_BASE + begin <= end:
            out.append((IMAGE_BASE + begin, IMAGE_BASE + stop, begin))
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


def db(root: pathlib.Path) -> sqlite3.Connection:
    root.mkdir(parents=True, exist_ok=True)
    con = sqlite3.connect(root / "corpus.db")
    con.execute(
        """CREATE TABLE IF NOT EXISTS functions(
               address TEXT PRIMARY KEY,
               begin INTEGER, size INTEGER, name TEXT, first_comment TEXT,
               file TEXT, tag TEXT)"""
    )
    cols = {row[1] for row in con.execute("PRAGMA table_info(functions)")}
    for name, decl in (
        ("body_end", "INTEGER"),
        ("asm_file", "TEXT"),
        ("entry", "TEXT"),
        ("kind", "TEXT"),
        ("decompile_checked", "INTEGER DEFAULT 0"),
    ):
        if name not in cols:
            con.execute(f"ALTER TABLE functions ADD COLUMN {name} {decl}")
    con.execute(
        """CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT)"""
    )
    # A chunk can belong to more than one range tag. Membership lives in its
    # own table so rebuilding or purging one tag never removes another's row.
    con.execute(
        """CREATE TABLE IF NOT EXISTS tagged(
               tag TEXT NOT NULL, address TEXT NOT NULL,
               PRIMARY KEY(tag, address))"""
    )
    con.execute(
        "INSERT OR IGNORE INTO tagged(tag,address) "
        "SELECT tag,address FROM functions WHERE tag IS NOT NULL"
    )
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
        "INSERT OR REPLACE INTO meta(key,value) VALUES('schema',?)", ("2",)
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
    print(f"build {tag}: {len(bodies)} .pdata bodies in [{lo:#x}, {hi:#x}]")
    con = db(root)
    check_image(con)
    # Storage is shared across tags: a chunk belongs to ranges, not to a
    # folder. Tag membership lives in `tagged`, so rebuilding or purging one
    # range never moves another range's files.
    asm_dir = root / "asm"
    c_dir = root / "c"
    asm_dir.mkdir(parents=True, exist_ok=True)
    c_dir.mkdir(parents=True, exist_ok=True)

    existing = {}
    for row in con.execute(
        "SELECT address, name, file, entry, decompile_checked FROM functions"
    ):
        existing[int(row[0], 16)] = row[1:]

    asm_written = decompiled = misses = skipped = errors = 0
    checked_now: set[int] = set()

    # 1. Disassembly for every body, including bodies Ghidra does not define.
    for i, (begin, stop, _) in enumerate(bodies, 1):
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
        for begin, stop, _ in bodies:
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
        addrs = [f"{b:#x}" for b, _, _ in need]

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
                for (begin, _, _), result in zip(
                    chunk, ex.map(fetch, addrs[i : i + batch])
                ):
                    results[begin] = result
            done = min(i + batch, len(need))
            print(f"  {done}/{len(need)} decompiled", end="\r")
        if need:
            print()

        for begin, stop, _ in need:
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
            existing[begin] = (name, f"c/{fname}", f"{entry:#x}", 1)

    # 3. Index every body, preserving a stored decompile when this pass skipped it.
    for begin, stop, _ in bodies:
        old = existing.get(begin)
        if old is not None:
            name, file, entry, old_checked = old
            entry = entry or f"{begin:#x}"
            old_checked = old_checked or 0
        else:
            name, file, entry, old_checked = f"FUN_{begin:x}", None, f"{begin:#x}", 0
        checked = 1 if (file or begin in checked_now) else old_checked
        kind = "ghidra" if file else "pdata-chunk"
        asm_name = f"{begin:010x}_{safe(name)}.asm"
        con.execute(
            """INSERT INTO functions(address,begin,size,body_end,name,first_comment,
                                     file,asm_file,entry,kind,decompile_checked)
               VALUES(?,?,?,?,?,?,?,?,?,?,?)
               ON CONFLICT(address) DO UPDATE SET
                 begin=excluded.begin, size=excluded.size,
                 body_end=excluded.body_end, name=excluded.name,
                 first_comment=excluded.first_comment, file=excluded.file,
                 asm_file=excluded.asm_file, entry=excluded.entry,
                 kind=excluded.kind,
                 decompile_checked=excluded.decompile_checked""",
            (
                f"{begin:#x}",
                begin - IMAGE_BASE,
                stop - begin,
                stop - IMAGE_BASE,
                name,
                _stored_comment(root, file),
                file,
                f"asm/{asm_name}",
                entry,
                kind,
                1 if checked else 0,
            ),
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
    """The headless args file (whitespace-separated start end pairs)."""
    lo = IMAGE_BASE + start if start < IMAGE_BASE else start
    hi = IMAGE_BASE + end if end < IMAGE_BASE else end
    bodies = pdata_bodies(lo, hi)
    with out.open("w", encoding="utf-8") as f:
        f.write(f"# .pdata extents for [{lo:#x}, {hi:#x}]\n")
        for begin, stop, _ in bodies:
            f.write(f"{begin:#x} {stop:#x}\n")
    print(f"args file {out} : {len(bodies)} pairs; run McpDecompile.java with the GUI closed")


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
    """Return rows for an address (exact or containing) or an exact name."""
    addr = _parse_addr(what)
    if addr is not None:
        rva = addr - IMAGE_BASE
        rows = con.execute(
            "SELECT address,name,file,asm_file,begin,body_end,entry FROM functions "
            "WHERE address=?",
            (f"{addr:#x}",),
        ).fetchall()
        if rows:
            return rows
        return con.execute(
            "SELECT address,name,file,asm_file,begin,body_end,entry FROM functions "
            "WHERE begin<=? AND body_end>? ORDER BY begin DESC LIMIT 1",
            (rva, rva),
        ).fetchall()
    return con.execute(
        "SELECT address,name,file,asm_file,begin,body_end,entry FROM functions "
        "WHERE name=? COLLATE NOCASE",
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
    addr, name, file, asm_file, begin, body_end, entry = rows[0]
    tags = con.execute(
        "SELECT group_concat(tag) FROM tagged WHERE address=?", (addr,)
    ).fetchone()[0]
    print(
        f"// {addr} {name} [{tags or '-'}] chunk {begin + IMAGE_BASE:#x}.."
        f"{body_end + IMAGE_BASE:#x} entry {entry or addr}"
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
    for tag, n, asm, dec, lo, hi in con.execute(
        "SELECT t.tag, COUNT(*), SUM(f.asm_file IS NOT NULL), "
        "SUM(f.file IS NOT NULL), MIN(f.begin), MAX(f.begin) "
        "FROM tagged t JOIN functions f ON f.address=t.address "
        "GROUP BY t.tag ORDER BY t.tag"
    ):
        print(
            f"  {tag:<14} {n:>5} chunks  {asm or 0:>5} asm  {dec or 0:>5} c  "
            f"{lo + IMAGE_BASE:#x} .. {hi + IMAGE_BASE:#x}"
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
    elif args.cmd == "purge":
        cmd_purge(args.tag, args.out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
