"""Field-access index over the complete client image.

The old discovery sweeps matched raw displacements against a capture's code and
used a prologue heuristic for function boundaries. That is wrong in two ways:
the character body and the mover component share offsets like `+0xc0`, and a
function may reach a field through a biased base (`rbx = rcx + 0xa8`, then
`[rbx + 0x18]` is body `+0xc0`). This tool fixes both:

* function boundaries come from the install's **plaintext `.pdata`** (or the
  offline image's copy at its RVA), not a prologue guess;
* a small alias analysis follows `mov`/`lea`/`add`/`sub` so a store is reported
  at the field's offset from the function's parameter, not the raw displacement;
* the character body, the mover component and the dmBody are separate
  profiles, because `+0xc0` is a different field in each.

It is a *finder*, not a verifier: a hit names a function to read. Nothing here
makes a unit verified.

The code bytes come from the offline image (`module_offline.bin`, flat, file
offset == RVA) because the install's `.text` is encrypted on disk. The
`.pdata` table comes from the install `Overwatch.exe` directly, which is the
authoritative plaintext table; the offline image carries a byte-identical copy
at RVA `0x450b000`.

For a whole-image scan, export Ghidra's function ranges with
`docs/ghidra/DumpFunctionRanges.java` and pass the CSV as
`--ghidra-functions`. Ghidra's entry points resolve `.pdata` chains; the
`.pdata` begin alone is not always the real function entry (a chained fragment
can start mid-function), so the two sources are not interchangeable.

Usage:
    python dev/field_index.py --profile mover-body --start 0x140c00000 --end 0x140e00000
    python dev/field_index.py --profile mover-body --start 0x140c00000 --end 0x140e00000 \
        --ghidra-functions C:/Temp/mover_functions.csv
    python dev/field_index.py --profile dm-body --start 0x140c00000 --end 0x140e00000 --json out.json
"""

from __future__ import annotations

import argparse
import json
import struct
import sys
from dataclasses import dataclass, field
from typing import Dict, Iterable, List, Optional, Sequence, Tuple

import capstone

INSTALL = r"E:\Overwatch\_retail_\Overwatch.exe"
OFFLINE = r"E:\overwatch-client-rs\dev\data\offline_recover\module_offline.bin"
BASE = 0x140000000


@dataclass
class Section:
    name: str
    va: int
    vsize: int
    raw: int
    raw_size: int


def sections(image: bytes) -> List[Section]:
    pe = struct.unpack_from("<I", image, 0x3C)[0]
    nsec = struct.unpack_from("<H", image, pe + 6)[0]
    opt = struct.unpack_from("<H", image, pe + 20)[0]
    s0 = pe + 24 + opt
    out = []
    for i in range(nsec):
        o = s0 + 40 * i
        name = image[o : o + 8].split(b"\x00")[0].decode("latin-1")
        vsz, va, rsz, ro = struct.unpack_from("<IIII", image, o + 8)
        out.append(Section(name, va, vsz, ro, rsz))
    return out


def read_file(path: str) -> bytes:
    with open(path, "rb") as f:
        return f.read()


def pdata_entries(install: bytes, offline: bytes) -> List[Tuple[int, int, int]]:
    """RUNTIME_FUNCTION (begin, end, unwind) from the install's `.pdata`.

    The install's section header is authoritative and its `.pdata` is
    plaintext. The offline image is flat, so its copy is at the section's RVA,
    not at the raw offset the header still carries.
    """
    sec = next((s for s in sections(install) if s.name == ".pdata"), None)
    if sec is not None:
        size = min(sec.raw_size, sec.vsize)
        raw = install[sec.raw : sec.raw + size]
        return _parse_pdata(raw)
    sec = next((s for s in sections(offline) if s.name == ".pdata"), None)
    if sec is None:
        raise RuntimeError("no .pdata in the install or the offline image")
    raw = offline[sec.va : sec.va + min(sec.vsize, sec.raw_size)]
    return _parse_pdata(raw)


def _parse_pdata(raw: bytes) -> List[Tuple[int, int, int]]:
    out = []
    for i in range(len(raw) // 12):
        beg, end, unwind = struct.unpack_from("<III", raw, 12 * i)
        if beg and end > beg:
            out.append((beg, end, unwind))
    return out


# Profiles are byte offsets from the object's base. The mover component
# (0x130) and the character body (0x1c00) deliberately do not share a set:
# +0xc0 is the component's crouch slot and the body's velocity-like quad.
PROFILES: Dict[str, Dict[int, str]] = {
    "mover-body": {
        0xC0: "vel.x", 0xC4: "vel.y", 0xC8: "vel.z", 0xCC: "vel.w",
        0x1140: "wish.x", 0x1144: "wish.y", 0x1148: "wish.z", 0x114C: "wish.w",
        0x1A90: "transform.m00", 0x1AD8: "world", 0x1B80: "committed.pos",
        0x1B84: "committed.y", 0x1B88: "committed.z", 0x1B8C: "committed.w",
        0x35B: "body/session", 0x10D0: "flags", 0x10E4: "state.id",
        0x21A: "flags2", 0x1A77: "state.byte", 0x1A79: "state.mode",
        0x1A7B: "state.flag", 0x652: "grounded", 0x664: "ground.time",
        0x1A60: "state.quad", 0x23C: "state.short",
        0x870: "modifier.table", 0x878: "modifier.count",
    },
    "dm-body": {
        0x30: "linVel.x", 0x34: "linVel.y", 0x38: "linVel.z",
        0x40: "angVel.x", 0x130: "force.x", 0x134: "force.y", 0x138: "force.z",
        0x164: "mass", 0x168: "invMass", 0x16C: "gravityScale",
        0x170: "linearDamping", 0x174: "angularDamping", 0x1B6: "characterMover",
    },
    "mover-component": {
        0x08: "inst", 0x48: "jump.struct", 0x4C: "jump.0", 0x50: "jump.1",
        0x54: "jump.2", 0x58: "jump.3", 0x5C: "jump.4", 0x60: "jump.5",
        0x64: "jump.6", 0x7C: "1972c687", 0xAC: "back", 0xB0: "forward",
        0xB4: "side", 0xC0: "crouch", 0xE0: "speed", 0xF0: "fall_terminal",
    },
}
BODY_MARKERS = {0x1140, 0x1A90, 0x1AD8, 0x1B80, 0x35B, 0x10D0, 0x10E4}
VEL = {0xC0, 0xC4, 0xC8, 0xCC}
FORCE = {0x130, 0x134, 0x138}

CALLEE_SAVED = {
    capstone.x86.X86_REG_RBX, capstone.x86.X86_REG_RBP,
    capstone.x86.X86_REG_RSI, capstone.x86.X86_REG_RDI,
    capstone.x86.X86_REG_R12, capstone.x86.X86_REG_R13,
    capstone.x86.X86_REG_R14, capstone.x86.X86_REG_R15,
}

STORE_MNEMONICS = {
    "mov", "movss", "movsd", "movups", "movaps", "movdqu", "movdqa",
    "vmovss", "vmovsd", "vmovups", "vmovaps", "vmovdqu", "vmovdqa",
    "and", "or", "xor", "add", "sub", "inc", "dec",
}


@dataclass
class Hit:
    address: int
    offset: int
    label: str
    direction: str
    mnemonic: str
    operand: str


@dataclass
class FunctionHits:
    start: int
    end: int
    hits: List[Hit] = field(default_factory=list)

    def count(self, offsets: Iterable[int], direction: Optional[str] = None) -> int:
        offsets = set(offsets)
        return sum(
            1
            for h in self.hits
            if h.offset in offsets and (direction is None or h.direction == direction)
        )

    def score(self) -> int:
        markers = len({h.offset for h in self.hits if h.offset in BODY_MARKERS})
        vel = self.count(VEL, "write")
        force = self.count(FORCE, "write")
        # A body candidate reads several body markers and writes the velocity
        # or force; a bulk copy or a constructor scores lower because it lacks
        # the co-occurring markers.
        return 3 * markers + 6 * vel + 5 * force


def load_ghidra_functions(path: str) -> List[Tuple[int, int, int]]:
    """Read `begin,end,name` rows written by the DumpFunctionRanges script.

    Ghidra's body end is the last address of the body, so the half-open end is
    one past it. The unwind word is unused here; 0 marks a Ghidra-only range
    (no `.pdata` entry), which the report prints.
    """
    import csv

    out = []
    with open(path, newline="", encoding="utf-8") as f:
        for row in csv.reader(f):
            if len(row) < 2:
                continue
            try:
                beg = int(row[0], 16)
                end = int(row[1], 16)
            except ValueError:
                continue
            if end >= beg:
                out.append((beg - BASE, end - BASE + 1, 0))
    return out


def scan(
    code: bytes,
    entries: Sequence[Tuple[int, int, int]],
    profile: Dict[int, str],
    start: int,
    end: int,
    seeds: Sequence[int] = (capstone.x86.X86_REG_RCX,),
) -> List[FunctionHits]:
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True
    md.skipdata = True
    out: List[FunctionHits] = []
    for beg, fin, _unwind in entries:
        if fin <= start - BASE or beg >= end - BASE or fin <= beg:
            continue
        data = code[beg:fin]
        alias: Dict[int, int] = {r: 0 for r in seeds}
        fh = FunctionHits(BASE + beg, BASE + fin)
        for ins in md.disasm(data, BASE + beg):
            try:
                ops = ins.operands
            except capstone.CsError:
                continue
            m = ins.mnemonic
            if m == "mov" and len(ops) == 2 and ops[0].type == capstone.x86.X86_OP_REG:
                dst = ops[0].reg
                if ops[1].type == capstone.x86.X86_OP_REG and ops[1].reg in alias:
                    alias[dst] = alias[ops[1].reg]
                elif ops[1].type == capstone.x86.X86_OP_IMM and ops[1].imm == 0:
                    alias.pop(dst, None)
            elif m == "lea" and len(ops) == 2 and ops[0].type == capstone.x86.X86_OP_REG:
                dst = ops[0].reg
                mem = ops[1].mem
                if mem.base in alias:
                    alias[dst] = alias[mem.base] + mem.disp
            elif m in ("add", "sub") and len(ops) == 2 and ops[0].type == capstone.x86.X86_OP_REG and ops[1].type == capstone.x86.X86_OP_IMM:
                dst = ops[0].reg
                if dst in alias:
                    alias[dst] += ops[1].imm if m == "add" else -ops[1].imm
            elif m == "call":
                for r in list(alias):
                    if r not in CALLEE_SAVED:
                        alias.pop(r, None)
            for i, op in enumerate(ops):
                if op.type != capstone.x86.X86_OP_MEM:
                    continue
                if op.mem.base not in alias:
                    continue
                total = alias[op.mem.base] + op.mem.disp
                if total not in profile:
                    continue
                direction = "write" if (i == 0 and m in STORE_MNEMONICS) else "read"
                fh.hits.append(
                    Hit(ins.address, total, profile[total], direction, m, ins.op_str)
                )
        if fh.hits:
            out.append(fh)
    out.sort(key=lambda f: (-f.score(), f.start))
    return out


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--profile", choices=sorted(PROFILES), default="mover-body")
    ap.add_argument("--start", default="0x140c00000")
    ap.add_argument("--end", default="0x140e00000")
    ap.add_argument("--code-image", default=OFFLINE)
    ap.add_argument("--install", default=INSTALL)
    ap.add_argument(
        "--ghidra-functions",
        default="",
        help="CSV begin,end,name from the DumpFunctionRanges Ghidra script; "
        "use instead of .pdata so functions without unwind entries are included",
    )
    ap.add_argument("--json", default="")
    ap.add_argument("--top", type=int, default=40)
    ap.add_argument(
        "--seed",
        default="rcx",
        help="comma-separated parameter registers to treat as the object base (default rcx)",
    )
    ap.add_argument("--min-markers", type=int, default=0)
    ap.add_argument("--min-vel-writes", type=int, default=0)
    args = ap.parse_args(argv)

    seed_map = {
        "rcx": capstone.x86.X86_REG_RCX,
        "rdx": capstone.x86.X86_REG_RDX,
        "r8": capstone.x86.X86_REG_R8,
        "r9": capstone.x86.X86_REG_R9,
        "rbx": capstone.x86.X86_REG_RBX,
        "r12": capstone.x86.X86_REG_R12,
        "r13": capstone.x86.X86_REG_R13,
        "rdi": capstone.x86.X86_REG_RDI,
        "rsi": capstone.x86.X86_REG_RSI,
    }
    seeds = []
    for name in args.seed.split(","):
        name = name.strip().lower()
        if name not in seed_map:
            raise SystemExit(f"unknown seed register {name!r}")
        seeds.append(seed_map[name])

    code = read_file(args.code_image)
    install = read_file(args.install)
    if args.ghidra_functions:
        entries = load_ghidra_functions(args.ghidra_functions)
        source = f"Ghidra CSV {args.ghidra_functions}"
    else:
        entries = pdata_entries(install, code)
        source = "install .pdata"
    profile = PROFILES[args.profile]
    hits = scan(
        code,
        entries,
        profile,
        int(args.start, 0),
        int(args.end, 0),
        seeds=seeds,
    )
    hits = [
        f
        for f in hits
        if len({h.offset for h in f.hits if h.offset in BODY_MARKERS}) >= args.min_markers
        and f.count(VEL, "write") >= args.min_vel_writes
    ]

    print(
        f"profile {args.profile}: {len(entries)} {source} entries, "
        f"{len(hits)} functions with a tracked field in "
        f"{args.start}..{args.end}"
    )
    for f in hits[: args.top]:
        markers = sorted({h.offset for h in f.hits if h.offset in BODY_MARKERS})
        velw = [h for h in f.hits if h.offset in VEL and h.direction == "write"]
        forcew = [h for h in f.hits if h.offset in FORCE and h.direction == "write"]
        print(
            f"  {f.start:#x}..{f.end:#x} score {f.score():>3} "
            f"markers {[hex(x) for x in markers]} "
            f"velw {len(velw)} forcew {len(forcew)} hits {len(f.hits)}"
        )
        for h in (velw + forcew + [h for h in f.hits if h.offset in BODY_MARKERS])[:8]:
            print(
                f"    {h.address:#x} {h.direction:<5} +0x{h.offset:x} "
                f"{h.label:<14} {h.mnemonic} {h.operand}"
            )
    if args.json:
        payload = [
            {
                "start": f"0x{f.start:x}",
                "end": f"0x{f.end:x}",
                "score": f.score(),
                "hits": [
                    {
                        "address": f"0x{h.address:x}",
                        "offset": f"0x{h.offset:x}",
                        "label": h.label,
                        "direction": h.direction,
                        "mnemonic": h.mnemonic,
                        "operand": h.operand,
                    }
                    for h in f.hits
                ],
            }
            for f in hits
        ]
        with open(args.json, "w", encoding="utf-8") as fh:
            json.dump(payload, fh, indent=1)
        print(f"wrote {args.json}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
