"""Unicorn oracle: run one captured client function on one input.

The oracle is the real machine code from a merged session image, emulated
offline. It is never a re-encoding, a decompile, or a reimplementation: the
bytes at the function's v2b address are what executed in the client. That is
the same trust position as the Sandustry harness's mechanically extracted
JS slices, and for the same reason - a hand-written oracle would put a human
in the trust path.

Usage (library):
    o = EmuOracle(image_path, base=0x7ff7c4d20000)
    o.load_function(v2b_addr, max_size=0x2000)
    out = o.call(arg_specs, arg_values)   # see CallResult
"""

from __future__ import annotations

import struct
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Sequence, Tuple

from unicorn import Uc, UcError, UC_ARCH_X86, UC_MODE_64
from unicorn import x86_const as x
from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
from unicorn import UC_HOOK_MEM_READ_UNMAPPED, UC_HOOK_MEM_WRITE_UNMAPPED
from unicorn import UC_HOOK_MEM_FETCH_UNMAPPED

V2B_BASE = 0x7FF7C4D20000
IMAGE_SIZE = 0x4838000  # offline_recover/module_offline.bin

STACK_BASE = 0x0000_7FFF_0000_0000
STACK_SIZE = 0x1_0000
STACK_TOP = STACK_BASE + STACK_SIZE - 0x1000

ARENA_BASE = 0x0000_0002_0000_0000
ARENA_SIZE = 0x1_0000

SENTINEL = 0x0000_0003_0000_0000  # page of `ret` - return trap
SHADOW_BASE = 0x0000_0003_1000_0000  # MS x64 shadow space for the call

MAX_STEPS = 2_000_000


@dataclass
class ArgSpec:
    """One parameter. `kind` is the ABI slot; `mem` is bytes placed in the arena.

    kind:
      f32     - XMMn, one float
      f64     - XMMn, one double
      ptr     - RCX/RDX/... pointer into the arena; `mem` is the pointee bytes
      u64     - integer register

    For ptr args, `arena_offset` (when not None) pins the pointee to
    ARENA_BASE + offset instead of packing sequentially. A fixture that
    builds a struct with internal pointers uses this so it can compute
    absolute addresses (ARENA_BASE is fixed) and patch them into `mem`.
    """

    kind: str
    mem: bytes = b""
    value: float = 0.0
    bits: int = 0
    arena_offset: Optional[int] = None
    # Byte offsets within `mem` that hold a u64 relative pointer (offset
    # from the pointee's own base). The oracle rebases these to the real
    # arena address on write; the C candidate's wrapper rebases to its
    # local copy. Fixture plumbing only - the callee never sees the
    # relative form.
    relocs: Tuple[int, ...] = ()


@dataclass
class CallResult:
    ok: bool
    ret_bits: int = 0  # f32/f64 bits or integer return
    ret_kind: str = "f32"
    writes: List[Tuple[int, bytes]] = field(default_factory=list)
    reads_outside: List[Tuple[int, int]] = field(default_factory=list)
    out_of_arena_writes: List[Tuple[int, int]] = field(default_factory=list)
    steps: int = 0
    fault: Optional[str] = None
    pointee_before: bytes = b""
    pointee_after: bytes = b""


class EmuOracle:
    def __init__(self, image_path: str, base: int = V2B_BASE):
        self.base = base
        with open(image_path, "rb") as f:
            self.image = f.read()
        self.uc = self._build()

    def _build(self) -> Uc:
        u = Uc(UC_ARCH_X86, UC_MODE_64)
        img_size = (len(self.image) + 0xFFF) & ~0xFFF
        u.mem_map(self.base, img_size)
        u.mem_write(self.base, self.image)
        u.mem_map(STACK_BASE, STACK_SIZE)
        u.mem_map(ARENA_BASE, ARENA_SIZE)
        u.mem_map(SENTINEL, 0x1000)
        u.mem_write(SENTINEL, b"\xC3" * 0x1000)
        u.mem_map(SHADOW_BASE, 0x1000)
        return u

    def load_function(self, v2b_addr: int, max_size: int = 0x2000) -> bytes:
        off = v2b_addr - self.base
        if off < 0 or off >= len(self.image):
            raise ValueError(f"address {v2b_addr:#x} outside image")
        blob = self.image[off : off + max_size]
        if len(blob) < 16 or all(b == 0 for b in blob[:16]):
            raise ValueError(
                f"cold page at {v2b_addr:#x}: first 16 bytes are zero; "
                "capture a warm image before verifying this function"
            )
        return blob

    def call(
        self,
        v2b_addr: int,
        args: Sequence[ArgSpec],
        ret_kind: str = "f32",
        steps: int = MAX_STEPS,
    ) -> CallResult:
        u = self.uc
        # Reset arena and stack each call - determinism by construction.
        u.mem_write(ARENA_BASE, b"\x00" * ARENA_SIZE)
        u.mem_write(STACK_BASE, b"\x00" * STACK_SIZE)

        # Lay out pointees in the arena; record the layout for read/write filters.
        ptrs: List[int] = []
        cursor = ARENA_BASE
        for a in args:
            if a.kind == "ptr":
                mem = a.mem
                if a.arena_offset is not None:
                    addr = ARENA_BASE + a.arena_offset
                    if addr + len(mem) > ARENA_BASE + ARENA_SIZE:
                        raise ValueError("arena_offset pointee exceeds arena")
                else:
                    addr = cursor
                    if addr + len(mem) > ARENA_BASE + ARENA_SIZE:
                        raise ValueError("argument arena exhausted")
                    cursor = (addr + len(mem) + 15) & ~15
                if a.relocs:
                    mem = bytearray(mem)
                    for off in a.relocs:
                        rel = struct.unpack_from("<Q", mem, off)[0]
                        struct.pack_into("<Q", mem, off, addr + rel)
                    mem = bytes(mem)
                u.mem_write(addr, mem)
                ptrs.append(addr)
            else:
                ptrs.append(0)

        # Capture pointee bytes before (mutation channel).
        before = b""
        for a, p in zip(args, ptrs):
            if a.kind == "ptr":
                before += u.mem_read(p, len(a.mem))

        # MS x64: register slot is chosen by ARGUMENT POSITION, class by type.
        # Arg 0 -> RCX/XMM0, arg 1 -> RDX/XMM1, arg 2 -> R8/XMM2, arg 3 -> R9/XMM3.
        # Packing all ints into RC..R9 and all floats into XMM0..3 separately
        # is wrong for mixed signatures (the (float*, float) shape puts the
        # float in XMM1, not XMM0).
        int_regs = [x.UC_X86_REG_RCX, x.UC_X86_REG_RDX, x.UC_X86_REG_R8, x.UC_X86_REG_R9]
        xmm_regs = [x.UC_X86_REG_XMM0, x.UC_X86_REG_XMM1, x.UC_X86_REG_XMM2, x.UC_X86_REG_XMM3]
        for pos, (a, p) in enumerate(zip(args, ptrs)):
            if pos >= 4:
                raise ValueError("stack args not supported yet (first 4 only)")
            if a.kind == "ptr":
                u.reg_write(int_regs[pos], p)
            elif a.kind == "u64":
                u.reg_write(int_regs[pos], a.bits)
            elif a.kind == "f32":
                u.reg_write(xmm_regs[pos], struct.unpack("<I", struct.pack("<f", a.value))[0])
            elif a.kind == "f64":
                u.reg_write(xmm_regs[pos], struct.unpack("<Q", struct.pack("<d", a.value))[0])
            else:
                raise ValueError(f"unknown arg kind {a.kind!r}")

        rsp = STACK_TOP & ~0xF
        u.reg_write(x.UC_X86_REG_RSP, rsp)
        u.mem_write(rsp, struct.pack("<Q", SENTINEL))

        writes: List[Tuple[int, bytes]] = []
        reads_outside: List[Tuple[int, int]] = []
        oow: List[Tuple[int, int]] = []
        arena_lo, arena_hi = ARENA_BASE, ARENA_BASE + ARENA_SIZE
        stack_lo, stack_hi = STACK_BASE, STACK_BASE + STACK_SIZE
        sent_lo, sent_hi = SENTINEL, SENTINEL + 0x1000
        shadow_lo, shadow_hi = SHADOW_BASE, SHADOW_BASE + 0x1000
        steps_hit = {"n": 0}
        fault: List[str] = []

        def in_allowed(addr: int) -> bool:
            return (
                arena_lo <= addr < arena_hi
                or stack_lo <= addr < stack_hi
                or sent_lo <= addr < sent_hi
                or shadow_lo <= addr < shadow_hi
            )

        def hook_write(uu, access, address, size, value, ud):
            # Stack spills, shadow-space home slots and the sentinel page are
            # normal function behaviour; only writes outside all of those
            # (i.e. into the image or unknown memory) are undeclared.
            if arena_lo <= address < arena_hi:
                blob = uu.mem_read(address, size)
                writes.append((address, bytes(blob)))
            elif not (
                stack_lo <= address < stack_hi
                or shadow_lo <= address < shadow_hi
                or sent_lo <= address < sent_hi
            ):
                oow.append((address, size))

        def hook_read(uu, access, address, size, value, ud):
            if not in_allowed(address):
                reads_outside.append((address, size))

        def hook_unmapped(uu, access, address, size, value, ud):
            kinds = {1: "read", 2: "write", 3: "fetch? "}
            fault.append(f"unmapped {kinds.get(access, access)} at {address:#x} size {size}")
            return False

        def hook_code(uu, addr, size, ud):
            steps_hit["n"] += 1
            if steps_hit["n"] > steps:
                fault.append(f"step limit {steps} exceeded at {addr:#x}")
                uu.emu_stop()

        h1 = u.hook_add(UC_HOOK_MEM_WRITE, hook_write)
        h2 = u.hook_add(UC_HOOK_MEM_READ, hook_read)
        h3 = u.hook_add(
            UC_HOOK_MEM_READ_UNMAPPED | UC_HOOK_MEM_WRITE_UNMAPPED | UC_HOOK_MEM_FETCH_UNMAPPED,
            hook_unmapped,
        )
        h4 = u.hook_add(UC_HOOK_CODE, hook_code)

        ret_bits = 0
        ok = False
        try:
            u.emu_start(v2b_addr, SENTINEL, count=steps)
            ok = not fault
            if ret_kind == "f32":
                ret_bits = u.reg_read(x.UC_X86_REG_XMM0) & 0xFFFF_FFFF
            elif ret_kind == "f64":
                ret_bits = u.reg_read(x.UC_X86_REG_XMM0) & 0xFFFF_FFFF_FFFF_FFFF
            else:  # integer in RAX
                ret_bits = u.reg_read(x.UC_X86_REG_RAX)
        except UcError as e:
            fault.append(f"UcError: {e}")

        u.hook_del(h1)
        u.hook_del(h2)
        u.hook_del(h3)
        u.hook_del(h4)

        after = b""
        for a, p in zip(args, ptrs):
            if a.kind == "ptr":
                after += u.mem_read(p, len(a.mem))

        return CallResult(
            ok=ok,
            ret_bits=ret_bits,
            ret_kind=ret_kind,
            writes=writes,
            reads_outside=reads_outside,
            out_of_arena_writes=oow,
            steps=steps_hit["n"],
            fault=fault[0] if fault else None,
            pointee_before=before,
            pointee_after=after,
        )


def f32_bits(f: float) -> int:
    return struct.unpack("<I", struct.pack("<f", f))[0]


def bits_f32(b: int) -> float:
    return struct.unpack("<f", struct.pack("<I", b & 0xFFFF_FFFF))[0]
