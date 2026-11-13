"""One command that prepares a reading session: the evidence pack for a function.

A read has four costs: finding the function, **reconstructing its record layout**
from register arithmetic, **classifying its globals** (raw bytes or the BSS tail
where a zero read is not a value), and **recording the conclusion** in the right
place. `dev/fn.py packet` handles the first; this handles the other three and
puts the whole thing in one file, so a session starts from one artifact instead
of four commands and a mental join.

    python dev/read.py 0x142cd4bc0
    python dev/read.py 0x142cd4bc0 --width 8      # more caller window
    python dev/read.py --queue                    # verified units with no pack yet, biggest
                                                  # dispatch fan-out first

Writes `dev/reads/<addr>_<name>.md` (gitignored, like `dev/decompiles/`) and
prints a short summary. The pack contains:

* identity, extent and chain pieces, callers, callees, the verified unit, docs hits;
* **the field map**: every memory access, folded through a conservative register
  dataflow so `[rcx+0x15c]` reads as `arg0+0x15c`, grouped by argument with the
  access direction and the site address;
* **globals classified** as `raw` / `zero-fill` / `unmapped` by the image's own
  section table, with a value preview - a `zero-fill` line is the BSS trap that
  has already produced one wrong conclusion in this project;
* switch tables inside the body, immediates that act as bounds or tags;
* the **argument setup at each caller's call site**, which is where the real
  types come from;
* an evidence skeleton to fill in `docs/formats/<system>.md`.

Nothing here is written into the repo: the pack lives under `dev/`, and the doc
gets addresses and conclusions, never disassembly.
"""
from __future__ import annotations

import struct
import sys
from pathlib import Path
from typing import Dict, List, Optional, Tuple

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "dev"))

import fn as FN  # noqa: E402
import pe_layout  # noqa: E402

OUT = ROOT / "dev" / "reads"
BASE = 0x140000000

# System V / Win64 integer argument registers, in order. XMM0-3 are the float
# side; a body that reads `[xmm0]` is reading a pointer, so they are tracked
# only as "argN (float side)".
ARG_REGS = ["rcx", "rdx", "r8", "r9"]
VOLATILE = {"rax", "rcx", "rdx", "r8", "r9", "r10", "r11"}
# Register name -> the 64-bit parent, so the map survives `mov ecx, ...`.
PARENT = {
    "al": "rax", "ah": "rax", "ax": "rax", "eax": "rax",
    "cl": "rcx", "cx": "rcx", "ecx": "rcx",
    "dl": "rdx", "dx": "rdx", "edx": "rdx",
    "bl": "rbx", "bx": "rbx", "ebx": "rbx",
    "sil": "rsi", "si": "rsi", "esi": "rsi",
    "dil": "rdi", "di": "rdi", "edi": "rdi",
    "spl": "rsp", "sp": "rsp", "esp": "rsp",
    "bpl": "rbp", "bp": "rbp", "ebp": "rbp",
}
for _i in range(8, 16):
    for suf in ("b", "w", "d"):
        PARENT[f"r{_i}{suf}"] = f"r{_i}"


def parent(reg: str) -> str:
    return PARENT.get(reg, reg)


def image() -> bytes:
    return FN.image()


def sections() -> List[dict]:
    return pe_layout.sections(image())


def classify(secs: List[dict], va: int) -> str:
    kind, _sec = pe_layout.classify(secs, va)
    return kind


def preview(data: bytes, va: int, count: int = 16) -> str:
    """The first bytes at `va` as u32 / f32 / ascii, so a table reads inline."""
    off = va - BASE
    if not 0 <= off < len(data):
        return "outside the image"
    raw = data[off : off + count]
    if not raw:
        return "empty"
    words = [struct.unpack_from("<I", raw, i)[0] for i in range(0, len(raw) - 3, 4)]
    floats = [struct.unpack_from("<f", raw, i)[0] for i in range(0, len(raw) - 3, 4)]
    text = "".join(chr(c) if 0x20 <= c < 0x7F else "." for c in raw)
    out = f"u32 {[hex(w) for w in words[:4]]}"
    if any(abs(f) > 1e-6 for f in floats):
        out += f"  f32 {[round(f, 4) for f in floats[:4]]}"
    out += f'  ascii "{text}"'
    return out


# ---------------------------------------------------------------- field map


def field_map(begin_va: int, end_va: int, data: bytes) -> Dict[str, List[Tuple[int, str, int, int]]]:
    """Memory accesses folded to their source argument.

    Returns {owner: [(displacement, direction, size, site)]} where owner is
    `arg0`..`arg3`, `ret`, or a register/stack name the dataflow could not
    resolve. The dataflow is deliberately shallow and conservative: it follows
    `mov reg, reg`, `lea reg, [reg+disp]` and `add/sub reg, imm` within a
    straight line, and drops a register's origin at any call (volatile) or any
    other write. A wrong origin is worse than `?`, so anything ambiguous is
    reported under the register's own name.
    """
    from capstone import CS_ARCH_X86, CS_MODE_64, Cs
    from capstone.x86 import X86_OP_MEM, X86_OP_REG

    md = Cs(CS_ARCH_X86, CS_MODE_64)
    md.detail = True
    code = data[begin_va - BASE : end_va - BASE]
    # register/stack slot -> (label, offset)
    origin: Dict[str, Tuple[str, int]] = {r: (f"arg{i}", 0) for i, r in enumerate(ARG_REGS)}
    origin["rsp"] = ("stack", 0)
    origin["rbp"] = ("stack?rbp", 0)
    out: Dict[str, List[Tuple[int, str, int, int]]] = {}

    def note(owner: str, disp: int, direction: str, size: int, site: int) -> None:
        out.setdefault(owner, []).append((disp, direction, size, site))

    for ins in md.disasm(code, begin_va):
        mn = ins.mnemonic
        ops = ins.operands
        # --- record the accesses in this instruction, before mutating origins.
        if mn in ("push", "pop"):
            continue  # stack discipline, not a field of any argument
        # `lea` never dereferences, but its result is usually an argument or a
        # member address (`lea rcx, [rsi+0x1b0]`), so the row is kept as
        # `addr` instead of `read`.
        for i, op in enumerate(ops):
            if op.type != X86_OP_MEM:
                continue
            base = ins.reg_name(op.mem.base) if op.mem.base else ""
            index = ins.reg_name(op.mem.index) if op.mem.index else ""
            size = op.size or 0
            # `[reg]`, `[reg+disp]`, `[rip+disp]` (a global) and stack slots.
            if base == "rip":
                continue
            if not base:
                note(f"[{index}]", op.mem.disp, "read", size, ins.address)
                continue
            owner, off = origin.get(parent(base), (parent(base), 0))
            disp = off + op.mem.disp
            if mn == "lea":
                direction = "addr"
            elif i == 0 and mn not in ("cmp", "test"):
                direction = "write"
            else:
                direction = "read"
            if index:
                direction += "+idx"  # base + i*element: the member of one element
            note(owner, disp, direction, size, ins.address)

        # --- update the origins.
        if mn == "call":
            for r in VOLATILE:
                origin.pop(r, None)
            continue
        if mn == "ret":
            origin = {r: (f"arg{j}", 0) for j, r in enumerate(ARG_REGS)}
            origin["rsp"] = ("stack", 0)
            continue
        if mn.startswith("j"):
            # A conditional branch falls through with its registers intact, and
            # this is a linear scan, so the usual `test`/`je` chain keeps the
            # argument mapping (`mov rsi, rdx` then a run of tests is the common
            # shape). An unconditional `jmp` is a merge the scan cannot follow,
            # so it resets to the argument-only map.
            if mn == "jmp":
                origin = {r: (f"arg{j}", 0) for j, r in enumerate(ARG_REGS)}
                origin["rsp"] = ("stack", 0)
            continue
        if not ops:
            continue
        dst = ops[0]
        if dst.type != X86_OP_REG:
            # A store through memory leaves no register mapping to update.
            continue
        d = parent(ins.reg_name(dst.reg))
        if mn in ("mov", "movzx", "movsx", "movsxd", "movups", "movaps", "movd", "movq") and len(ops) > 1:
            src = ops[1]
            if src.type == X86_OP_REG:
                s = parent(ins.reg_name(src.reg))
                origin[d] = origin.get(s, (s, 0))
            elif src.type == X86_OP_MEM:
                # A load through a mapped base: keep the ancestry visible, so
                # `[rax+0x54]` reads as `arg1+0x2b8 -> +0x54` instead of `rax`.
                b = parent(ins.reg_name(src.mem.base)) if src.mem.base else ""
                if b in origin:
                    own, off = origin[b]
                    origin[d] = (f"{own}+{off + src.mem.disp:#x}->", 0)
                else:
                    origin.pop(d, None)
            else:
                origin.pop(d, None)
        elif mn == "lea" and len(ops) > 1 and ops[1].type == X86_OP_MEM:
            src = ops[1]
            b = parent(ins.reg_name(src.mem.base)) if src.mem.base else ""
            if src.mem.index:
                # `lea rax, [rax+rax*2]` is an arithmetic expression, not a
                # member offset; following it would invent a displacement.
                origin.pop(d, None)
                continue
            if b in origin:
                own, off = origin[b]
                origin[d] = (own, off + src.mem.disp)
            else:
                origin[d] = (f"&{b}", src.mem.disp) if b else (d, 0)
        elif mn in ("add", "sub") and len(ops) > 1:
            from capstone.x86 import X86_OP_IMM

            if ops[1].type == X86_OP_IMM and d in origin:
                own, off = origin[d]
                delta = ops[1].imm if mn == "add" else -ops[1].imm
                origin[d] = (own, off + delta)
            else:
                origin.pop(d, None)
        elif mn.startswith("cmp") or mn.startswith("test"):
            continue
        else:
            origin.pop(d, None)
    return out


def render_fields(fields: Dict, limit: int = 40) -> List[str]:
    order = ["arg0", "arg1", "arg2", "arg3"]
    keys = [k for k in order if k in fields]
    keys += sorted(k for k in fields if k not in order)
    lines: List[str] = []
    for owner in keys:
        rows = sorted(fields[owner], key=lambda r: (r[0], r[3]))
        # Collapse a repeated access to the same offset: the first site and a count.
        merged: Dict[Tuple[int, str, int], List[int]] = {}
        for disp, direction, size, site in rows:
            merged.setdefault((disp, direction, size), []).append(site)
        lines.append(f"### {owner}  ({len(merged)} distinct member(s))")
        for (disp, direction, size), sites in sorted(merged.items())[:limit]:
            span = " ".join(f"@{s:#x}" for s in sites[:3])
            more = f" +{len(sites) - 3}" if len(sites) > 3 else ""
            sign = "+" if disp >= 0 else "-"
            lines.append(
                f"  {owner}{sign}0x{abs(disp):03x}  {direction:5} {size}B  {span}{more}"
            )
        if len(merged) > limit:
            lines.append(f"  ... and {len(merged) - limit} more")
        lines.append("")
    return lines


def global_accesses(
    begin_va: int, end_va: int, data: bytes
) -> Dict[int, Dict[str, int]]:
    """Rip-relative global references, split into read / write / addr.

    A function that *writes* a zero-fill global is that global's writer - the
    answer the AGENTS note sends you looking for when a read returns zeros.
    `addr` is a `lea`: the address is taken and passed on, not dereferenced.
    """
    from capstone import CS_ARCH_X86, CS_MODE_64, Cs
    from capstone.x86 import X86_OP_MEM

    md = Cs(CS_ARCH_X86, CS_MODE_64)
    md.detail = True
    out: Dict[int, Dict[str, int]] = {}
    code = data[begin_va - BASE : end_va - BASE]
    for ins in md.disasm(code, begin_va):
        tail = ins.mnemonic.startswith("j") or ins.mnemonic in ("cmp", "test", "nop")
        for i, op in enumerate(ins.operands):
            if op.type != X86_OP_MEM or not op.mem.base:
                continue
            if ins.reg_name(op.mem.base) != "rip":
                continue
            target = ins.address + ins.size + op.mem.disp
            row = out.setdefault(target, {"read": 0, "write": 0, "addr": 0})
            if ins.mnemonic == "lea":
                row["addr"] += 1
            elif i == 0 and not tail:
                row["write"] += 1
            else:
                row["read"] += 1
    return out


# ---------------------------------------------------------------- callers


def caller_window(site: int, width: int = 12) -> List[str]:
    """The last `width` instructions before a call site, instruction-aligned.

    Decoding from `site - 4*width` starts mid-instruction and produces nonsense;
    the window is decoded from the caller's own chain root instead and then
    trimmed to the instructions that precede the call.
    """
    from capstone import CS_ARCH_X86, CS_MODE_64, Cs

    md = Cs(CS_ARCH_X86, CS_MODE_64)
    md.detail = True
    row = FN.containing(site - BASE)
    if row is None:
        return [f"    (no function covers the call site {site:#x})"]
    begin = BASE + row[0]
    data = image()
    code = data[begin - BASE : site - BASE]
    rows = [
        f"    {i.address:#x}  {i.mnemonic} {i.op_str}"
        for i in md.disasm(code, begin)
        if i.address < site
    ]
    return rows[-width:]


def main(argv: List[str]) -> int:
    if len(argv) > 1 and argv[1] == "--queue":
        return queue()
    if len(argv) < 2:
        print(__doc__)
        return 2
    addr = FN.parse_addr(argv[1])
    width = 12
    if "--width" in argv:
        width = int(argv[argv.index("--width") + 1])
    row = FN.containing(addr - BASE)
    if row is None:
        print(f"{addr:#x} is not inside a function")
        return 1
    begin, end, pieces, opaque = row
    begin_va, end_va = BASE + begin, BASE + end
    data = image()
    secs = sections()
    name = FN.label(begin_va)

    L: List[str] = []
    L.append(f"# read pack: {name} ({begin_va:#x})")
    L.append("")
    L.append("Evidence pack for one reading session. Addresses and conclusions go")
    L.append("into `docs/formats/<system>.md`; this file stays in `dev/`.")
    L.append("")

    # --- identity
    L.append("## identity")
    L.append("")
    L.append(f"- address   `{begin_va:#x}`  ({end - begin} bytes, {pieces} piece(s), opaque={opaque})")
    L.append(f"- extent    `{begin_va:#x}..{end_va:#x}`")
    unit = FN.unit_for(begin_va)
    if unit:
        status = FN.unit_status(unit)
        L.append(f"- unit      `{unit}`" + (f" - **{status}**" if status else ""))
    else:
        L.append("- unit      none")
    cites = FN.doc_cites(begin_va)
    for path, lineno, line in cites[:6]:
        safe = line.encode("ascii", "replace").decode("ascii")
        L.append(f"- docs      `{path}:{lineno}` {safe[:120]}")
    if len(cites) > 6:
        L.append(f"- docs      ... and {len(cites) - 6} more")
    try:
        import sqlite3

        conn = sqlite3.connect(
            f"file:{ROOT / 'dev' / 'data' / 'corpus' / 'corpus.db'}?mode=ro", uri=True
        )
        slots = [
            (n, d)
            for n, d in conn.execute(
                "SELECT COUNT(*), COUNT(DISTINCT disp) FROM slot_sites WHERE src=?",
                (begin - 0,),
            )
        ]
        conn.close()
        if slots and slots[0][0]:
            L.append(
                f"- dispatch  {slots[0][0]} memory-indirect site(s), {slots[0][1]} distinct "
                f"slot(s) - the callee list below is only the direct half"
            )
    except Exception:
        pass
    cached = sorted((ROOT / "dev" / "decompiles").glob(f"*0x{begin_va:x}.c"))
    if cached:
        L.append(f"- client C  `{cached[0].relative_to(ROOT).as_posix()}` (cached)")
    else:
        L.append(
            f"- client C  not cached - `python dev/gh_decompile.py {begin_va:#x}` before reading"
        )
    L.append("")

    # --- callers and callees
    callers = FN.callers_of(begin_va)
    callees = FN.callees_of(begin_va)
    L.append(f"## callers ({len(callers)})")
    L.append("")
    for site, src, form in callers[:20]:
        L.append(f"- `{src:#x}` {FN.label(src)}  (`{form}` at `{site:#x}`)")
    if len(callers) > 20:
        L.append(f"- ... and {len(callers) - 20} more")
    by_target: Dict[int, List[int]] = {}
    for site, target, _form in callees:
        by_target.setdefault(target, []).append(site)
    L.append("")
    L.append(f"## callees ({len(by_target)} function(s), {len(callees)} site(s))")
    L.append("")
    for target, sites in sorted(by_target.items(), key=lambda kv: (-len(kv[1]), kv[0])):
        L.append(f"- `{target:#x}` x{len(sites)} {FN.label(target)}")
    L.append("")

    # --- field map
    L.append("## field map (register dataflow: `mov`/`lea`/`add`, reset at calls and branches)")
    L.append("")
    fields = field_map(begin_va, end_va, data)
    L.extend(render_fields(fields))
    if not fields:
        L.append("no memory access with a resolvable base")
        L.append("")

    # --- globals
    gacc = global_accesses(begin_va, end_va, data)
    refs = sorted(
        gacc.items(), key=lambda kv: (-sum(kv[1].values()), kv[0])
    )
    L.append(f"## globals ({len(refs)})")
    L.append("")
    if not refs:
        L.append("none (the body is self-contained)")
    for target, acc in refs[:20]:
        count = sum(acc.values())
        kind = classify(secs, target)
        if target == BASE:
            kind = "image base (not data)"
        flag = ""
        if kind == "zero-fill":
            flag = "  **ZERO-FILL: a read here is not a value - find the writer**"
        elif kind == "unmapped":
            flag = "  (computed address, not a readable global)"
        s = FN._packet_string(target, data)
        split = ", ".join(f"{n} {k}" for k, n in acc.items() if n)
        L.append(f"- `{target:#x}` x{count} ({split})  **{kind}**{flag}")
        if kind == "raw":
            L.append(f"    {preview(data, target)}")
    L.append("")

    # --- tables and immediates
    tables = FN._packet_switch_tables(begin_va, end_va, data)
    if tables:
        L.append(f"## switch tables ({len(tables)})")
        L.append("")
        for start, count, distinct, low, high in tables:
            L.append(
                f"- `{start:#x}`  {count} entries, {distinct} distinct  -> `{low:#x}..{high:#x}`"
            )
        L.append("")
    imm = FN._packet_immediates(begin_va, end_va, data)
    ranked = sorted(imm["counts"].items(), key=lambda kv: (-kv[1], kv[0]))
    L.append(f"## immediates ({imm['shape']['instructions']} insn, {imm['shape']['calls']} calls)")
    L.append("")
    L.append("top: " + ", ".join(f"0x{v:x} x{n}" for v, n in ranked[:12]))
    small = sorted(v for v in imm["counts"] if 2 <= v <= 0x200)
    if small:
        L.append("bounds/tags in 2..0x200: " + " ".join(f"0x{v:x}" for v in small[:24]))
    L.append("")

    # --- caller argument setup
    L.append("## argument setup at each call site (the real types come from here)")
    L.append("")
    for site, src, _form in callers[:6]:
        L.append(f"### caller `{src:#x}` {FN.label(src)} -> call at `{site:#x}`")
        L.append("```")
        L.extend(caller_window(site, width))
        L.append("```")
        L.append("")

    # --- evidence skeleton
    L.append("## evidence skeleton (fill into `docs/formats/<system>.md`)")
    L.append("")
    L.append(f"- function `{begin_va:#x}` ({name}) does ...")
    L.append("- arguments: arg0 = ... (from the field map and the caller setup)")
    L.append("- state it writes: ... (the `write` rows above)")
    L.append("- constants: ... (the immediates/tables above; a `zero-fill` global is not one)")
    L.append("- open: ... (what the body needs that cannot be read here)")
    L.append("- unit: `" + (unit or "fn/<Name>") + "`; evidence class per `docs/provenance.md`")
    L.append("")

    OUT.mkdir(parents=True, exist_ok=True)
    safe = "".join(c if c.isalnum() else "_" for c in name)[:40]
    path = OUT / f"{begin_va:x}_{safe}.md"
    path.write_text("\n".join(L) + "\n", encoding="utf-8")

    zero = [t for t, _a in refs if classify(secs, t) == "zero-fill"]
    print(f"{begin_va:#x}  {name}")
    print(f"  pack       {path.relative_to(ROOT).as_posix()}  ({len(L)} lines, {path.stat().st_size} bytes)")
    print(f"  fields     {sum(len(v) for v in fields.values())} access(es) over {len(fields)} base(s)")
    print(f"  globals    {len(refs)}" + (f"  **{len(zero)} zero-fill**" if zero else ""))
    print(f"  tables     {len(tables)} switch table(s), {imm['shape']['instructions']} instructions")
    print(f"  callers    {len(callers)};  callees {len(by_target)} function(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
