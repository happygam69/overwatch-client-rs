"""Pick and screen a cluster for §9: leaves first, then the head.

`docs/port-playbook.md` §9 says to port a cluster bottom-up and §1.5 says to
screen the callees before reading the body. This does both in one command: it
finds a head whose in-scope callees are unported, then classifies **every**
function in the cluster by what stops it running in the emulator - an import
thunk, a virtual slot, a kernel transition, ambient CPU state - and prints the
port order with the verdict. Reading a cluster that cannot verify costs a
session; screening it costs a second.

    python dev/cluster.py                     # pick: portable clusters, smallest first

Measured 2026-11-20: 135 of the 2,814 unported heads with a callee are portable
(4.8%); the rest reach an import thunk (1,694), a virtual call (1,769), a call
target with no index entry (2,276), an indirect call (1,031), `rdtsc` (662) or
`cpuid` (598).

The no-index targets are followed as far as static data flow allows: a
straight-line `jmp` forwarder, and the hot-patch guard `mov r10,[g]; test;
jmp r10` whose slot is installed by an initializer with `lea` operands (see
`docs/ghidra-project.md`). Following them does **not** raise the portable count:
the guard resolves to `0x141bb6660` -> `0x141bb00e0`, a virtual call. It makes
the *message* true, which is what a reader needs.
    python dev/cluster.py --limit 12          # more candidates
    python dev/cluster.py --head 0x142cfd1e0  # screen one cluster in full
    python dev/cluster.py --all               # include the blocked ones, with reasons

Read-only: the image, the corpus index, the units. Writes nothing.
"""
from __future__ import annotations

import bisect
import glob
import json
import sqlite3
import struct
import sys
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple

sys.path.insert(0, str(Path(__file__).resolve().parent))

import pe_layout  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
CORPUS = ROOT / "dev" / "data" / "corpus" / "corpus.db"
IMAGE = ROOT / "dev" / "data" / "offline_recover" / "module_offline.bin"
BASE = 0x140000000
V2B = 0x7FF7C4D20000

# Genuinely outside the code: a clock, the host CPU's identity, a kernel
# transition. Nothing the client's own bytes determine.
AMBIENT = {
    "rdtsc", "rdtscp", "cpuid", "xgetbv", "xsetbv", "rdfsbase", "rdgsbase",
    "wrfsbase", "int3", "int", "syscall", "sysenter", "hlt",
}

# Executes in the emulator, but the *value* it produces is the emulator's, not a
# CPU's - checked by running it (measured 2026-11-20): `stmxcsr` stores 0 where a
# real CPU starts at 0x1F80, so a test of the exception-mask bits reads the
# opposite of the truth. Not a blocker, a fidelity note on the unit.
MODELED = {"stmxcsr": "MXCSR reads 0, a CPU starts at 0x1f80", "ldmxcsr": "sets the emulator's MXCSR"}


class Index:
    def __init__(self) -> None:
        self.c = sqlite3.connect(f"file:{CORPUS}?mode=ro", uri=True)
        rows = list(self.c.execute("SELECT begin,end FROM fn"))
        self.begins = [r[0] for r in rows]
        self.ends = [r[1] if r[1] is not None else r[0] for r in rows]
        self.state: Dict[int, str] = dict(self.c.execute("SELECT begin,state FROM triage"))
        self.size: Dict[int, int] = dict(self.c.execute("SELECT begin,size FROM triage"))
        self.name: Dict[int, str] = dict(self.c.execute("SELECT begin,name FROM triage"))
        self.system: Dict[int, str] = dict(self.c.execute("SELECT begin,systems FROM triage"))
        self.img = IMAGE.read_bytes()
        self.secs = pe_layout.sections(self.img)
        self._callees: Dict[int, Set[int]] = {}

    def root(self, rva: int) -> Optional[int]:
        i = bisect.bisect_right(self.begins, rva) - 1
        if i >= 0 and rva < self.ends[i]:
            return self.begins[i]
        return None

    def callees(self, fn: int) -> Set[int]:
        """Direct call targets, **inside the triage scope or not**.

        Following only in-scope callees is how a cluster looks portable and is
        not: `0x142cf7c10`'s leaf calls `0x142cadf00`, a guard-dispatch stub two
        hops from an import, and it is outside the scope, so it never appeared in
        the plan.
        """
        if fn not in self._callees:
            out: Set[int] = set()
            for (t,) in self.c.execute("SELECT target FROM edges WHERE src=?", (fn,)):
                r = self.root(t)
                if r is not None and r != fn:
                    out.add(r)
            self._callees[fn] = out
        return self._callees[fn]

    def tail_target(self, rva: int, limit: int = 8) -> Optional[int]:
        """If `rva` opens with a straight-line `jmp <root>`, that root.

        A tail-call forwarder is a few instructions that only shuffle arguments
        before a `jmp`; it carries no `.pdata` of its own. Anything else (a real
        body, a conditional branch, a `jmp` through a pointer) returns `None`.
        """
        from capstone import Cs as _Cs
        from capstone import CS_ARCH_X86, CS_MODE_64
        from capstone.x86 import X86_OP_IMM

        md = _Cs(CS_ARCH_X86, CS_MODE_64)
        md.detail = True
        for count, ins in enumerate(md.disasm(self.img[rva : rva + 64], BASE + rva)):
            if count >= limit:
                return None
            if ins.mnemonic == "jmp":
                ops = ins.operands
                if len(ops) == 1 and ops[0].type == X86_OP_IMM:
                    r = self.root(ops[0].imm - BASE)
                    return r
                return None
            if ins.mnemonic in ("ret", "call", "int3") or ins.mnemonic.startswith("j"):
                return None
        return None

    def classify(self, fn: int, depth: int = 3, seen: Optional[Set[int]] = None) -> Tuple[str, str]:
        """(verdict, reason) for one function, by the §1.5 rules.

        The calls are read from the body, not from the edge table, because the
        thing that blocks a body is often invisible to the table: `0x142cee300`
        calls `0x142cadf00`, a guard-dispatch stub with **no `.pdata` extent**, so
        it has no function root, no edge row and no triage state - and it is two
        hops from an import.
        """
        from capstone import CS_ARCH_X86, CS_MODE_64, Cs
        from capstone.x86 import X86_OP_IMM, X86_OP_MEM

        size = self.size.get(fn) or 0
        if not size:
            end = self.ends[bisect.bisect_right(self.begins, fn) - 1]
            if fn >= end:
                return "unmapped", f"{BASE + fn:#x} has no .pdata extent (thunk or padding)"
            size = end - fn
        md = Cs(CS_ARCH_X86, CS_MODE_64)
        md.detail = True
        code = self.img[fn : fn + size]
        seen = set(seen or ()) | {fn}
        for ins in md.disasm(code, BASE + fn):
            mn = ins.mnemonic
            if mn == "int3":
                # 0xCC between functions is padding, not a trap: stop decoding.
                break
            if mn in AMBIENT:
                return "blocked", f"ambient CPU state: {mn} at {ins.address:#x}"
            if mn in MODELED:
                return "note", f"{mn}: {MODELED[mn]} at {ins.address:#x}"
            if mn != "call" or not ins.operands:
                continue
            op = ins.operands[0]
            if op.type == X86_OP_IMM:
                target = op.imm
                r = self.root(target - BASE)
                if r is None:
                    # A call target with no function root is usually a
                    # **forwarder**: an argument shuffle and a `jmp` into a real
                    # function (measured: 113 of 186 single-stub blockers). Follow
                    # the tail call instead of calling the address opaque.
                    reached, trail = self.chain_to_root(target)
                    if reached is not None and reached not in seen:
                        verdict, why = self.classify(reached, depth - 1, seen)
                        if verdict == "ok":
                            continue
                        hop = " ".join(str(t) for t in trail)
                        return verdict, f"via {target:#x} ({hop}) -> {BASE + reached:#x}: {why}"
                    if self.import_thunk(target - BASE):
                        return "blocked", f"import thunk via {target:#x} at {ins.address:#x}"
                    return "blocked", (
                        f"call target {target:#x} has no .pdata extent at {ins.address:#x} "
                        f"(unindexed body)"
                    )
                if depth > 0 and r not in seen:
                    verdict, why = self.classify(r, depth - 1, seen)
                    if verdict == "ok":
                        continue
                    return verdict, f"via {BASE + r:#x}: {why}"
                continue
            if op.type == X86_OP_MEM and not op.mem.base:
                return "blocked", f"register call at {ins.address:#x}"
            if op.type == X86_OP_MEM:
                base = ins.reg_name(op.mem.base)
                if base == "rip":
                    return "blocked", f"import thunk at {ins.address:#x}"
                return "blocked", f"virtual slot [{base}+{op.mem.disp:#x}] at {ins.address:#x}"
            return "blocked", f"unclassified call at {ins.address:#x}"
        return "ok", ""


def ported() -> Set[int]:
    out: Set[int] = set()
    for p in glob.glob(str(ROOT / "dev" / "verify_harness" / "units" / "*.json")):
        d = json.loads(Path(p).read_text(encoding="utf-8"))
        a = (d.get("address") or {}).get("v2b")
        if a:
            out.add(int(str(a), 16) - V2B)
    return out


def plan(idx: Index, head: int, have: Set[int]) -> Tuple[List[int], List[Tuple[int, str]]]:
    """Bottom-up order (leaves first) plus every blocked function with why."""
    order: List[int] = []
    blocked: List[Tuple[int, str]] = []
    seen = set()

    def walk(fn: int) -> None:
        if fn in seen:
            return
        seen.add(fn)
        verdict, why = idx.classify(fn)
        if verdict != "ok" and verdict != "note":
            blocked.append((fn, why))
        if fn != head and fn not in have:
            order.append(fn)
        for k in sorted(idx.callees(fn)):
            if k in idx.state:
                walk(k)

    walk(head)
    if head not in have:
        order.append(head)
    return order, blocked


def show(idx: Index, head: int, have: Set[int], verbose: bool) -> bool:
    order, blocked = plan(idx, head, have)
    name = idx.name.get(head) or ""
    sysname = (idx.system.get(head) or "")[:28]
    total = sum(idx.size.get(f, 0) for f in order)
    tag = "PORTABLE" if not blocked else f"BLOCKED ({len(blocked)})"
    print(
        f"{BASE + head:#x}  {name:20} {idx.size.get(head, 0):5}B  [{sysname:28}] "
        f"{len(order):2} unit(s), {total:5}B  {tag}"
    )
    if not verbose:
        for fn, why in blocked:
            print(f"    blocked: {BASE + fn:#x} {idx.name.get(fn) or '':16} {why}")
        return not blocked
    print(f"  order (leaves first):")
    for i, fn in enumerate(order, 1):
        verdict, why = idx.classify(fn)
        mark = "ok " if verdict == "ok" else "XX "
        print(
            f"   {i}. {mark}{BASE + fn:#x}  {idx.size.get(fn, 0):5}B  "
            f"{idx.name.get(fn) or '':18} {idx.state.get(fn)}  {why}"
        )
    for fn, why in blocked:
        if fn not in order:
            print(f"       blocked outside the order: {BASE + fn:#x} {why}")
    return not blocked


def main(argv: List[str]) -> int:
    limit = 8
    head = None
    want_all = False
    i = 1
    while i < len(argv):
        a = argv[i]
        if a == "--head":
            head = int(argv[i + 1], 16)
            i += 2
        elif a == "--limit":
            limit = int(argv[i + 1])
            i += 2
        elif a == "--all":
            want_all = True
            i += 1
        else:
            print(__doc__)
            return 2
    idx = Index()
    have = ported()
    if head is not None:
        return 0 if show(idx, head - BASE, have, True) else 1

    cands: List[Tuple[int, int]] = []
    for h in idx.state:
        if h in have:
            continue
        kids = idx.callees(h)
        if not 2 <= len(kids) <= 5:
            continue
        if any(k in have for k in kids):
            continue
        total = idx.size.get(h, 0) + sum(idx.size.get(k, 0) for k in kids)
        if total > 4000:
            continue
        cands.append((total, h))
    cands.sort()
    found = 0
    for _total, h in cands:
        if found >= limit:
            break
        ok = show(idx, h, have, False)
        if ok or want_all:
            found += 1
    print()
    print("  python dev/cluster.py --head 0x<addr>   # the full order for one cluster")
    return 0




# --- Guard dispatch through a runtime-initialized slot -----------------------
#
# `mov r10,[rip+G]; test r10,r10; je fallback; jmp r10` is the client's hot-patch
# guard: it dispatches through a slot in the `.data` zero-fill tail, so the
# target looks unknowable. It usually is not - the slot has an *installer* whose
# operands are `lea` constants (measured: `0x141ff3190` fills three slots). This
# resolves the slot instead of declaring the call opaque.
#
# The chain it follows: store site -> the unindexed region the store lives in
# -> that region's call sites -> the last `lea <stored-reg>` before each call.
# Anything it cannot follow returns None, and the caller stays blocked.

GUARD_SLOTS = 0


def _add_slot_resolution(cls) -> None:
    def _sec(self, name: str) -> dict:
        for s in self.secs:
            if s["name"] == name:
                return s
        raise KeyError(name)

    def text_slice(self) -> bytes:
        sec = self._sec(".text")
        return self.img[sec["va"] : sec["va"] + sec["vsz"]]

    def rel_targets(self) -> dict:
        """target RVA -> [(site RVA, 'call' | 'jmp')], one pass over .text."""
        if getattr(self, "_rel", None) is not None:
            return self._rel
        import numpy as np

        sec = self._sec(".text")
        d = np.frombuffer(self.img, dtype=np.uint8, count=sec["vsz"], offset=sec["va"])
        out: dict = {}
        for op, kind in ((0xE8, "call"), (0xE9, "jmp")):
            hits = np.nonzero(d[:-5] == op)[0]
            if not len(hits):
                continue
            disp = d[hits + 1].astype(np.int64)
            disp |= d[hits + 2].astype(np.int64) << 8
            disp |= d[hits + 3].astype(np.int64) << 16
            disp |= d[hits + 4].astype(np.int64) << 24
            disp = (disp << 32) >> 32  # sign-extend the rel32
            targets = sec['va'] + hits + 5 + disp
            for site, t in zip(hits.tolist(), targets.tolist()):
                out.setdefault(t, []).append((site, kind))
        self._rel = out
        return out

    def store_sites(self, global_rva: int) -> list:
        """Every `mov [rip+global], reg` site, as (site RVA, source register)."""
        import numpy as np
        from capstone import Cs as _Cs
        from capstone import CS_ARCH_X86, CS_MODE_64
        from capstone.x86 import X86_OP_MEM, X86_OP_REG, X86_REG_RIP

        sec = self._sec(".text")
        d = np.frombuffer(self.img, dtype=np.uint8, count=sec["vsz"], offset=sec["va"])
        # A rip-relative disp field at offset p encodes a target of p + 4 + disp.
        disp = d[:-4].astype(np.int64)
        disp |= d[1:-3].astype(np.int64) << 8
        disp |= d[2:-2].astype(np.int64) << 16
        disp |= d[3:-1].astype(np.int64) << 24
        disp = (disp << 32) >> 32
        want = global_rva - (sec['va'] + np.arange(len(disp)) + 4)
        where = np.nonzero(disp == want)[0]
        md = _Cs(CS_ARCH_X86, CS_MODE_64)
        md.detail = True
        out = []
        for p in where.tolist():
            for start in (p - 3, p - 4, p - 2, p - 5, p - 6, p - 7):
                if start < 0:
                    continue
                ins = list(md.disasm(self.img[sec["va"] + start : sec["va"] + start + 16], BASE + sec["va"] + start))
                if not ins:
                    continue
                ins = ins[0]
                if ins.address + ins.size != BASE + sec["va"] + p + 4:
                    continue
                ops = ins.operands
                if not ops or ops[0].type != X86_OP_MEM or ops[0].mem.base != X86_REG_RIP:
                    continue
                if ins.mnemonic not in ("mov", "and", "or", "xor", "add", "sub", "inc", "dec"):
                    continue
                if len(ops) == 2 and ops[1].type == X86_OP_REG:
                    out.append((sec["va"] + start, ins.reg_name(ops[1].reg)))
                break
        return out

    def enclosing_region(self, rva: int) -> tuple:
        """(entry, callers) for the unindexed region a site sits in.

        The setters have no `.pdata` of their own, so their entry is the nearest
        preceding rel32 target that no function root covers, and their callers
        are that entry's call sites.
        """
        rel = self.rel_targets()
        best = None
        for target, sites in rel.items():
            if rva - 0x400 <= target <= rva and self.root(target) is None:
                if best is None or target > best:
                    best = target
        if best is None:
            return None, []
        return best, [s for s, kind in rel.get(best, []) if kind == "call"]

    def lea_before(self, fn_rva: int, site_rva: int, reg: str):
        """The last `lea reg, [rip+x]` in fn .. site, as an absolute address."""
        from capstone import Cs as _Cs
        from capstone import CS_ARCH_X86, CS_MODE_64
        from capstone.x86 import X86_OP_MEM, X86_REG_RIP

        md = _Cs(CS_ARCH_X86, CS_MODE_64)
        md.detail = True
        sec = self._sec(".text")
        start = sec["va"] + fn_rva
        end = sec["va"] + site_rva
        value = None
        for ins in md.disasm(self.img[start:end], BASE + sec["va"] + fn_rva):
            ops = ins.operands
            if ins.mnemonic in ("lea", "mov") and len(ops) == 2 and ops[1].type == X86_OP_MEM:
                if ops[1].mem.base == X86_REG_RIP and ins.reg_name(ops[0].reg) == reg:
                    value = ins.address + ins.size + ops[1].mem.disp
            elif ins.mnemonic == "mov" and len(ops) == 2 and ops[1].type == 2:
                if ins.reg_name(ops[0].reg) == reg and ops[1].imm:
                    value = ops[1].imm
        return value

    def import_thunk(self, rva: int) -> bool:
        """Does this address open with `jmp qword ptr [rip+x]` - an IAT thunk?"""
        from capstone import Cs as _Cs
        from capstone import CS_ARCH_X86, CS_MODE_64
        from capstone.x86 import X86_OP_MEM, X86_REG_RIP

        sec = self._sec(".text")
        md = _Cs(CS_ARCH_X86, CS_MODE_64)
        md.detail = True
        ins = list(md.disasm(self.img[sec["va"] + rva : sec["va"] + rva + 16], BASE + sec["va"] + rva))[:1]
        if not ins or ins[0].mnemonic != "jmp":
            return False
        ops = ins[0].operands
        return len(ops) == 1 and ops[0].type == X86_OP_MEM and ops[0].mem.base == X86_REG_RIP

    def chain_to_root(self, target: int, hops: int = 6):
        """Follow forwarders and hot-patch guards to a function root.

        Returns `(root_rva_or_None, trail)`; `trail` is the addresses it walked,
        so a message can say *how* it got there.
        """
        trail = []
        rva = target - BASE
        for _ in range(hops):
            tail = self.tail_target(rva)
            if tail is not None:
                trail.append(f'{BASE + rva:#x}') 
                rva = tail
                continue
            how, guard = self.guard_target(rva)
            if guard is None:
                return (rva if self.root(rva) is not None else None), trail
            trail.append(f"{BASE + rva}[{how}]")
            rva = guard - BASE
        return None, trail

    def guard_target(self, rva: int):
        """(how, target) for a hot-patch dispatch stub, else (None, None).

        The shape is `mov rX,[rip+G]; test rX,rX; je fallback; jmp rX`. `how` is
        `"slot"` when the installer's static operand resolves `G`, and
        `"fallback"` when **no instruction stores to `G`** - then the slot is BSS
        and stays zero, so the `je` edge is the one taken.
        """
        from capstone import Cs as _Cs
        from capstone import CS_ARCH_X86, CS_MODE_64
        from capstone.x86 import X86_OP_IMM, X86_OP_MEM, X86_OP_REG, X86_REG_RIP

        sec = self._sec(".text")
        md = _Cs(CS_ARCH_X86, CS_MODE_64)
        md.detail = True
        ins = list(md.disasm(self.img[sec["va"] + rva : sec["va"] + rva + 40], BASE + sec["va"] + rva))[:6]
        if len(ins) < 4:
            return None, None
        load, test, je, jmp = ins[0], ins[1], ins[2], ins[3]
        ops = load.operands
        if not (load.mnemonic == "mov" and len(ops) == 2 and ops[0].type == X86_OP_REG
                and ops[1].type == X86_OP_MEM and ops[1].mem.base == X86_REG_RIP):
            return None, None
        reg = load.reg_name(ops[0].reg)
        slot = load.address + load.size + ops[1].mem.disp
        if not (test.mnemonic == "test" and reg in test.op_str):
            return None, None
        if not (je.mnemonic in ("je", "jz") and je.operands and je.operands[0].type == X86_OP_IMM):
            return None, None
        if not (jmp.mnemonic == "jmp" and reg in jmp.op_str):
            return None, None
        value = self.slot_value(slot - BASE)
        if value is not None:
            return "slot", value
        if not self.store_sites(slot - BASE):
            # Nothing writes the slot: it is BSS, so the `je` edge is taken.
            return "fallback", je.operands[0].imm
        return None, None

    def slot_value(self, global_rva: int):
        """What a runtime-initialized slot holds, or None if not statically read."""
        if getattr(self, "_slot_cache", None) is None:
            self._slot_cache = {}
        if global_rva in self._slot_cache:
            return self._slot_cache[global_rva]
        self._slot_cache[global_rva] = None
        for site, reg in self.store_sites(global_rva):
            entry, callers = self.enclosing_region(site)
            if entry is None:
                continue
            for caller in callers:
                root = self.root(caller)
                if root is None:
                    continue
                v = self.lea_before(root, caller, reg)
                if v:
                    self._slot_cache[global_rva] = v
                    return v
        return None

    cls._sec = _sec
    cls.text_slice = text_slice
    cls.rel_targets = rel_targets
    cls.store_sites = store_sites
    cls.enclosing_region = enclosing_region
    cls.lea_before = lea_before
    cls.slot_value = slot_value
    cls.guard_target = guard_target
    cls.chain_to_root = chain_to_root
    cls.import_thunk = import_thunk


_add_slot_resolution(Index)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
