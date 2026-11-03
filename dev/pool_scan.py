"""Verified store/load census for the Domino world's pool-table fields.

    python dev/pool_scan.py writes          # stores to [reg+off] base/count fields
    python dev/pool_scan.py reads  OFFS...  # loads of the same fields
    python dev/pool_scan.py at     0xADDR.. # decode: what does this site do
    python dev/pool_scan.py func   0xSTART  # every pool-field access in a function
    python dev/pool_scan.py calls  0xTARGET # .pdata census of who calls a target
    python dev/pool_scan.py sentinel        # the exclusive pool-record writer census

Why a scan and not a Ghidra xref list: this project's Ghidra caller metadata is
incomplete (docs/ghidra-project.md), and a raw disp32 byte scan is useless for
these offsets because 0x2b8 etc. are ordinary stack displacements. So every
site here comes from a *linear decode of every function*, where a function is
the `.pdata` entry with `UNW_FLAG_CHAININFO` clear plus the cold fragments MSVC
places directly after it (119,802 groups, the count docs/ghidra-project.md
reports). A site is an instruction on that decoded path whose memory operand has
a register base that is not rsp/rbp, no index, and one of the pool-table
displacements. Bytes that do not decode are skipped in capstone's `skipdata`
mode and recorded in `TRUNCATED`, so a "no writer" answer cannot come from a
silently short walk.

The displacements alone are not a discriminator: unrelated classes sit at the
same offsets (see `sentinel`, and docs/formats/shape-ingest.md section 2).

The world record is `dmWorld`. `DmWorldConstruct` (`0x142cbeb70`) writes the
pool table at `record+0x288 + 0x18*i`; the per-pool record is
`{ptr @+0x00, u32 used @+0x08, u32 cap @+0x0c, u32 high @+0x10, i32 free @+0x14}`.
Read-only against the offline image; writes nothing.
"""
from __future__ import annotations

import bisect
import struct
import sys

import capstone

sys.path.insert(0, r"E:\overwatch-client-rs\dev")

IMAGE = r"E:\overwatch-client-rs\dev\data\offline_recover\module_offline.bin"
BASE = 0x140000000

MD = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
MD.detail = True

# dmWorld pool-table base offsets (element pointer) and their stride.
POOLS = {
    0x288: ("bodies", 0x1C0),
    0x2A0: ("owners", 0x50),
    0x2B8: ("shape_entries", 0xC0),
    0x2D0: ("type0", 0x20),
    0x2E8: ("type1", 0x30),
    0x300: ("type2", 0x50),
    0x318: ("type3", 0x50),
    0x330: ("type4", 0x50),
}
# The other three dwords/qwords of each pool record.
FIELDS = {}
for _b in POOLS:
    FIELDS[_b + 0x08] = ("used", _b)
    FIELDS[_b + 0x0C] = ("cap", _b)
    FIELDS[_b + 0x10] = ("high", _b)
    FIELDS[_b + 0x14] = ("free", _b)

STACK = {"rsp", "esp", "sp", "rbp", "ebp", "bp"}

DATA: bytes = b""
RUNS: list[tuple[int, int]] = []


UNW_FLAG_CHAININFO = 0x04


def real_ranges(image: bytes) -> list[tuple[int, int]]:
    """The `.pdata` entries that are real function starts, not chained pieces.

    A chained entry's UNWIND_INFO carries `UNW_FLAG_CHAININFO`; decoding one as
    a function start produces a plausible but mid-body decode, which is what
    inflated the first version of this census (201 246 ranges instead of the
    119 802 real functions docs/ghidra-project.md counts).
    """
    from frame_check import pe_sections  # noqa: E402

    sections = pe_sections(image)
    pdr = sections[".pdata"]
    text = sections[".text"]
    text_lo = text["va"]
    text_hi = text["va"] + text["vsz"]
    best: list[tuple[int, int]] | None = None
    for offset in (pdr["raw"], pdr["va"]):
        if offset + min(pdr["rsz"], pdr["vsz"]) > len(image):
            continue
        raw = image[offset : offset + min(pdr["rsz"], pdr["vsz"])]
        out = []
        for i in range(len(raw) // 12):
            beg, end, unwind = struct.unpack_from("<III", raw, 12 * i)
            if not beg or end <= beg:
                continue
            if not (text_lo <= beg < text_hi and text_lo < end <= text_hi):
                continue
            info = unwind & ~1
            if info + 4 > len(image):
                continue
            if (image[info] >> 3) & UNW_FLAG_CHAININFO:
                continue
            out.append((beg, end))
        if best is None or len(out) > len(best):
            best = out
    # A chained entry is a cold fragment placed immediately after its parent, so
    # a function's real extent runs from its own start to the next real start.
    starts = sorted({beg for beg, _end in (best or [])})
    return [(starts[i], starts[i + 1] if i + 1 < len(starts) else starts[i] + 0x400)
            for i in range(len(starts))]


def load() -> None:
    global DATA, RUNS
    with open(IMAGE, "rb") as handle:
        DATA = handle.read()
    RUNS = real_ranges(DATA)


def function_of(rva: int):
    starts = [b for b, _e in RUNS]
    i = bisect.bisect_right(starts, rva) - 1
    if i >= 0 and RUNS[i][0] <= rva < RUNS[i][1]:
        return RUNS[i]
    return None


TRUNCATED: list[tuple[int, int]] = []


def decode_function(start_rva: int, end_rva: int):
    """Linear decode over a whole function group, recovering from bad bytes.

    A bare `MD.disasm` stops at the first byte it cannot decode, which silently
    truncated whole functions in the first version of this census (a probe that
    never reaches a store must not be read as "there is no store"). So when the
    strict decode stops short, the rest is decoded with `skipdata` on, which
    emits one pseudo-instruction per bad byte and keeps going. The truncation is
    recorded in `TRUNCATED` so a caller can print it rather than trust a
    silently short walk.
    """
    code = DATA[start_rva:end_rva]
    at = BASE + start_rva
    decoded = 0
    for ins in MD.disasm(code, at):
        yield ins
        decoded = ins.address + ins.size - at
    if decoded < end_rva - start_rva:
        TRUNCATED.append((at, at + decoded))
        MD.skipdata = True
        try:
            rest = DATA[start_rva + decoded : end_rva]
            for ins in MD.disasm(rest, at + decoded):
                yield ins
        finally:
            MD.skipdata = False


def mem_operand(ins):
    if ins.id == 0:
        return None  # a SKIPDATA pseudo-instruction carries no operands
    try:
        for op in ins.operands:
            if op.type == capstone.x86.X86_OP_MEM:
                return op
    except capstone.CsError:
        return None
    return None


def mem_base_name(op) -> str:
    if op.mem.base == 0:
        return ""
    return ins_reg_name(op.mem.base)


def ins_reg_name(reg_id: int) -> str:
    return MD.reg_name(reg_id) or ""


def writes_dest(ins) -> bool:
    """True when the instruction stores into its memory operand."""
    if ins.id == 0:
        return False
    if ins.id in (capstone.x86.X86_INS_MOV, capstone.x86.X86_INS_MOVSD,
                  capstone.x86.X86_INS_MOVSS, capstone.x86.X86_INS_MOVUPS,
                  capstone.x86.X86_INS_MOVAPS, capstone.x86.X86_INS_MOVD,
                  capstone.x86.X86_INS_MOVQ, capstone.x86.X86_INS_MOVDQU,
                  capstone.x86.X86_INS_MOVDQA, capstone.x86.X86_INS_MOVZX,
                  capstone.x86.X86_INS_MOVSX, capstone.x86.X86_INS_MOVSXD,
                  capstone.x86.X86_INS_LEA, capstone.x86.X86_INS_CMOVA,
                  capstone.x86.X86_INS_CMOVB, capstone.x86.X86_INS_CMOVE,
                  capstone.x86.X86_INS_CMOVNE, capstone.x86.X86_INS_CMOVBE,
                  capstone.x86.X86_INS_CMOVAE, capstone.x86.X86_INS_CMOVG,
                  capstone.x86.X86_INS_CMOVL, capstone.x86.X86_INS_CMOVGE,
                  capstone.x86.X86_INS_CMOVLE, capstone.x86.X86_INS_SETNE):
        return ins.operands[0].type == capstone.x86.X86_OP_MEM
    # Arithmetic / logic / xadd / cmpxchg / inc / dec store into a memory first
    # operand.
    if ins.operands and ins.operands[0].type == capstone.x86.X86_OP_MEM:
        return ins.mnemonic.split()[0] in {
            "add", "sub", "and", "or", "xor", "inc", "dec", "neg", "not",
            "shl", "shr", "sar", "xadd", "xchg", "vmovups", "vmovaps",
            "vmovdqu", "movdqu", "movdqa", "movsd", "movss", "movupd",
            "movapd", "push",
        }
    return False


def pool_sites():
    """Every decodable instruction whose memory operand is a pool field."""
    wanted = set(POOLS) | set(FIELDS)
    for start, end in RUNS:
        try:
            for ins in decode_function(start, end):
                op = mem_operand(ins)
                if op is None or op.mem.index != 0:
                    continue
                name = mem_base_name(op)
                if not name or name in STACK:
                    continue
                if op.mem.disp not in wanted:
                    continue
                yield start, ins, op, name
        except capstone.CapstoneError:
            continue


def cmd_writes(_args: list[str]) -> int:
    rows = []
    for start, ins, op, name in pool_sites():
        if not writes_dest(ins):
            continue
        rows.append((ins.address, BASE + start, ins.mnemonic + " " + ins.op_str))
    print(f"{len(rows)} store(s) with a non-stack base into a pool-table field")
    for address, start, text in rows:
        field = FIELDS.get(disp_of(text), None)
        print(f"  {address:#x}  in {start:#x}  disp={disp_of(text):#x}"
              f"  {field[0] if field else 'pool-base'}   {text}")
    return 0


def disp_of(text: str) -> int:
    # The displacement is the last +0xNN token before the closing bracket.
    body = text.split("[", 1)[1].split("]", 1)[0] if "[" in text else text
    for token in reversed(body.replace("-", "+-").split("+")):
        token = token.strip()
        if token.startswith("0x"):
            return int(token, 16) & 0xFFFFFFFF
    return 0


def cmd_reads(args: list[str]) -> int:
    wanted = {int(a, 0) for a in args} if args else set(POOLS) | set(FIELDS)
    rows = []
    for start, ins, op, name in pool_sites():
        if op.mem.disp not in wanted:
            continue
        if writes_dest(ins) and ins.operands[0].type == capstone.x86.X86_OP_MEM:
            continue
        rows.append((ins.address, BASE + start, ins.mnemonic + " " + ins.op_str))
    print(f"{len(rows)} load(s) from pool fields {sorted(hex(w) for w in wanted)}")
    for address, start, text in rows:
        print(f"  {address:#x}  in {start:#x}   {text}")
    return 0


def cmd_func(args: list[str]) -> int:
    start = int(args[0], 0)
    rng = function_of(start - BASE)
    if rng is None:
        print(f"{start:#x} is not inside a .pdata range")
        return 1
    print(f"function {BASE + rng[0]:#x}..{BASE + rng[1]:#x} ({rng[1] - rng[0]} bytes)")
    for ins in decode_function(rng[0], rng[1]):
        op = mem_operand(ins)
        if op is None:
            continue
        if op.mem.index != 0 or op.mem.disp not in (set(POOLS) | set(FIELDS)):
            continue
        name = mem_base_name(op)
        if not name or name in STACK:
            continue
        kind = "W" if writes_dest(ins) else "r"
        field = FIELDS.get(op.mem.disp)
        label = f"{field[1]:#x}.{field[0]}" if field else "pool-base"
        print(f"  {ins.address:#x} {kind} {op.mem.disp:#06x} {label:18} {ins.mnemonic} {ins.op_str}")
    return 0


def cmd_calls(args: list[str]) -> int:
    target = int(args[0], 0)
    sites = []
    needle = b"\xe8"
    i = 0
    while True:
        i = DATA.find(needle, i)
        if i < 0:
            break
        rel = struct.unpack_from("<i", DATA, i + 1)[0]
        if BASE + i + 5 + rel == target:
            sites.append(BASE + i)
        i += 1
    funcs: dict[int, list[int]] = {}
    for site in sites:
        rng = function_of(site - BASE)
        key = BASE + rng[0] if rng else 0
        funcs.setdefault(key, []).append(site)
    print(f"{len(sites)} E8 site(s) to {target:#x} in {len(funcs)} function(s)")
    for start in sorted(funcs):
        rng = function_of(start - BASE)
        size = (rng[1] - rng[0]) if rng else 0
        print(f"  {start:#x} ({size} bytes) sites={[hex(s) for s in funcs[start]]}")
    return 0


def cmd_sentinel(_args: list[str]) -> int:
    """The exclusive writer census: `-1` into all eight pool free heads."""
    free = sorted({base + 0x14 for base in POOLS})
    rows = []
    for start, end in RUNS:
        hits = []
        try:
            for ins in decode_function(start, end):
                if ins.id == 0 or not ins.mnemonic.startswith("mov"):
                    continue
                op = mem_operand(ins)
                if op is None or op.mem.index != 0 or op.mem.disp not in free:
                    continue
                name = mem_base_name(op)
                if not name or name in STACK:
                    continue
                if ins.operands[0].type != capstone.x86.X86_OP_MEM:
                    continue
                if len(ins.operands) > 1 and ins.operands[1].imm in (-1, 0xFFFFFFFF):
                    hits.append((ins.address, op.mem.disp))
        except capstone.CapstoneError:
            continue
        if hits:
            rows.append((BASE + start, hits))
    full = [r for r in rows if len(r[1]) == len(free)]
    print(f"{len(rows)} function(s) store -1 into a pool free-head field; "
          f"{len(full)} write all {len(free)} of them")
    for address, hits in rows:
        mark = "ALL" if len(hits) == len(free) else "   "
        print(f"  {mark} {address:#x}  {[(hex(a), hex(d)) for a, d in hits]}")
    return 0


def cmd_at(args: list[str]) -> int:
    for arg in args:
        va = int(arg, 0)
        rng = function_of(va - BASE)
        print(f"{va:#x} in {BASE + rng[0]:#x}..{BASE + rng[1]:#x}" if rng else f"{va:#x} outside .pdata")
        if rng is None:
            continue
        window = range(max(rng[0], va - BASE - 0x20), min(rng[1], va - BASE + 0x20))
        code = DATA[window.start:window.stop]
        for ins in MD.disasm(code, BASE + window.start):
            mark = ">>" if ins.address == va else "  "
            print(f"  {mark} {ins.address:#x}  {ins.mnemonic} {ins.op_str}")
    return 0


def main(argv: list[str]) -> int:
    load()
    if not argv:
        print(__doc__)
        return 2
    cmd, args = argv[0], argv[1:]
    table = {"writes": cmd_writes, "reads": cmd_reads, "func": cmd_func,
             "calls": cmd_calls, "at": cmd_at, "sentinel": cmd_sentinel}
    if cmd not in table:
        print(__doc__)
        return 2
    return table[cmd](args)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
