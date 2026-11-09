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

The process surface, not just the image: besides the image at `base`, the
oracle maps the emulator stack, the argument arena, a `ret` sentinel page, the
shadow-space page and a one-page fake TEB with `gs:` pointed at it. The TEB is
there for the CRT: `__chkstk` reads the stack limit at `gs:[0x10]`, and with
zero there it returns without probing - the behaviour of a process whose stack
is committed - where an unmapped `gs:` faults (oracle fault `unmapped read at
0x10`). Every mapped page except the image is reset before each call, so
nothing carries between cases.
"""

from __future__ import annotations

import os
import struct
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Sequence, Tuple

from unicorn import Uc, UcError, UC_ARCH_X86, UC_MODE_64
from unicorn import x86_const as x
from unicorn import UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
from unicorn import UC_HOOK_MEM_READ_UNMAPPED, UC_HOOK_MEM_WRITE_UNMAPPED
from unicorn import UC_HOOK_MEM_FETCH_UNMAPPED

V2B_BASE = 0x7FF7C4D20000
IMAGE_SIZE = 0x4838000  # offline_recover/module_offline.bin
# The image the units are authored against: `dev/data/offline_recover/
# module_offline.bin`, sha256 pinned by `docs/ghidra-image-audit.md` section 2.
# A unit proven against different bytes is not proven at all, so a mismatch is
# fatal rather than a warning - the v2b frame and the current-image frame differ
# by a fixed offset, which is exactly what makes a wrong image easy to miss.
IMAGE_SHA256 = "12acfd1126c2fe2464f5e29892523bd258412fac8b11449dc9bb3fed22eb6300"

STACK_BASE = 0x0000_7FFF_0000_0000
STACK_SIZE = 0x1_0000
STACK_TOP = STACK_BASE + STACK_SIZE - 0x1000

ARENA_BASE = 0x0000_0002_0000_0000
ARENA_SIZE = 0x1_0000

SENTINEL = 0x0000_0003_0000_0000  # page of `ret` - return trap
SHADOW_BASE = 0x0000_0003_1000_0000  # MS x64 shadow space for the call

# A fake TEB, so the CRT helpers that read `gs:` can run. `__chkstk`
# (`0x1426c3ce0`) is the reason this exists: it reads the stack limit at
# `gs:[0x10]` and probes one page at a time down to the requested frame. With
# zero there, every stack address is "above the limit", so it returns without
# probing - which is what a process with a committed stack does. The page is
# zeroed before every call, so a callee that wrote a TEB field cannot make one
# case depend on the previous one.
TEB_BASE = 0x0000_0004_0000_0000
TEB_SIZE = 0x1000

MAX_STEPS = 2_000_000

# Images are large (tens of MB) and immutable during a session; several
# consumers (selftest, runner, coverage) build more than one EmuOracle over
# the same path. Read each file once per process.
_IMAGE_CACHE: Dict[str, bytes] = {}


def _read_image(path: str) -> bytes:
    key = os.path.realpath(path)
    img = _IMAGE_CACHE.get(key)
    if img is None:
        with open(path, "rb") as f:
            img = f.read()
        import hashlib

        sha = hashlib.sha256(img).hexdigest()
        if sha != IMAGE_SHA256:
            raise SystemExit(
                f"oracle image {path} is not the pinned image: sha256 {sha[:16]} "
                f"!= {IMAGE_SHA256[:16]}; a unit proven against different bytes "
                "is not proven (docs/ghidra-image-audit.md section 2)"
            )
        _IMAGE_CACHE[key] = img
    return img


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
    # Absolute guest address for a pointer into a preloaded state snapshot.
    # When set, `mem` is written at this address (after the snapshot load) and
    # the pointer register is set to it; the arena layout is not used. The
    # probe translates the same guest address into its state buffer, so both
    # sides see one controlled struct.
    guest: Optional[int] = None
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
    # State-snapshot channel: the whole declared region before and after the
    # call. None when the unit has no state.
    state_before: Optional[bytes] = None
    state_after: Optional[bytes] = None


class EmuOracle:
    def __init__(self, image_path: str, base: int = V2B_BASE):
        self.base = base
        self.image = _read_image(image_path)
        self.uc = self._build()
        # (base, bytes) of the controlled state region, if one is declared.
        self.state: Optional[Tuple[int, bytes]] = None
        self._state_restore: Optional[Tuple[int, bytes]] = None
        self._extra_maps: List[Tuple[int, int]] = []

    def set_state(self, base: int, data: bytes) -> None:
        """Preload a controlled memory snapshot for a stateful unit.

        The region may live inside the image (a data address) or in a fresh
        mapped block. Each ``call`` writes the snapshot, runs the function,
        captures the whole region and restores the pre-snapshot bytes, so the
        oracle stays stateless between calls just like the arena.
        """
        if not data:
            raise ValueError("state snapshot is empty")
        self.state = (base, bytes(data))
        page = base & ~0xFFF
        end = base + len(data)
        if self._overlaps_known(page, end):
            self._state_restore = (base, self._read_any(base, len(data)))
        else:
            size = ((end - page) + 0xFFF) & ~0xFFF
            self.uc.mem_map(page, size)
            self._extra_maps.append((page, size))
            self._state_restore = None

    def _overlaps_known(self, lo: int, hi: int) -> bool:
        img_end = self.base + ((len(self.image) + 0xFFF) & ~0xFFF)
        ranges = [
            (self.base, img_end),
            (STACK_BASE, STACK_BASE + STACK_SIZE),
            (ARENA_BASE, ARENA_BASE + ARENA_SIZE),
            (SENTINEL, SENTINEL + 0x1000),
            (SHADOW_BASE, SHADOW_BASE + 0x1000),
            (TEB_BASE, TEB_BASE + TEB_SIZE),
            *self._extra_maps,
        ]
        return any(lo < b and a < hi for a, b in ranges)

    def _read_any(self, addr: int, size: int) -> bytes:
        try:
            return bytes(self.uc.mem_read(addr, size))
        except UcError:
            return b"\x00" * size

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
        u.mem_map(TEB_BASE, TEB_SIZE)
        u.reg_write(x.UC_X86_REG_GS_BASE, TEB_BASE)
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
        start_regs: Optional[Dict[str, object]] = None,
    ) -> CallResult:
        u = self.uc
        # Reset arena, stack and registers each call - determinism by
        # construction, and the oracle is stateless between calls (which is
        # what lets the runner cache and reuse results for identical args).
        u.mem_write(ARENA_BASE, b"\x00" * ARENA_SIZE)
        u.mem_write(STACK_BASE, b"\x00" * STACK_SIZE)
        # The fake TEB is part of the reset, not of the mapping: a callee that
        # wrote a TEB field must not change the next case's answer.
        u.mem_write(TEB_BASE, b"\x00" * TEB_SIZE)
        u.reg_write(x.UC_X86_REG_GS_BASE, TEB_BASE)
        # The controlled state region is re-seeded from its snapshot on every
        # call; the caller's per-case pointee writes below then specialise it.
        state_lo = state_hi = None
        if self.state is not None:
            state_lo, state_data = self.state
            state_hi = state_lo + len(state_data)
            u.mem_write(state_lo, state_data)
        for reg in (
            x.UC_X86_REG_RAX, x.UC_X86_REG_RBX, x.UC_X86_REG_RCX,
            x.UC_X86_REG_RDX, x.UC_X86_REG_RSI, x.UC_X86_REG_RDI,
            x.UC_X86_REG_RBP, x.UC_X86_REG_R8, x.UC_X86_REG_R9,
            x.UC_X86_REG_R10, x.UC_X86_REG_R11, x.UC_X86_REG_R12,
            x.UC_X86_REG_R13, x.UC_X86_REG_R14, x.UC_X86_REG_R15,
            x.UC_X86_REG_XMM0, x.UC_X86_REG_XMM1, x.UC_X86_REG_XMM2,
            x.UC_X86_REG_XMM3, x.UC_X86_REG_XMM4, x.UC_X86_REG_XMM5,
            x.UC_X86_REG_XMM6, x.UC_X86_REG_XMM7, x.UC_X86_REG_EFLAGS,
        ):
            u.reg_write(reg, 0)

        # Lay out pointees in the arena; record the layout for read/write filters.
        ptrs: List[int] = []
        cursor = ARENA_BASE
        for a in args:
            if a.kind == "ptr":
                mem = a.mem
                if a.guest is not None:
                    addr = a.guest
                    if self.state is None:
                        raise ValueError("guest pointer requires a preloaded state")
                    if addr < state_lo or addr + len(mem) > state_hi:
                        raise ValueError(
                            f"guest pointee {addr:#x}+{len(mem)} outside state "
                            f"{state_lo:#x}..{state_hi:#x}"
                        )
                elif a.arena_offset is not None:
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
                continue  # stack args are written after the final RSP is set
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

        # MS x64: the caller's RSP is 16-byte aligned at the CALL, so after
        # the call pushes the 8-byte return address, RSP % 16 == 8 at entry.
        # STACK_TOP is 16-aligned, so STACK_TOP - 8 is exactly that. (Do NOT
        # round: masking with ~0xF would undo the offset and emulate a
        # misaligned caller.) The old code emulated RSP % 16 == 0 at entry,
        # which faults every `movaps [rsp+X]` spill a real compiler emits
        # after one push - a divergence the oracle itself manufactured.
        rsp = STACK_TOP - 8
        u.reg_write(x.UC_X86_REG_RSP, rsp)
        u.mem_write(rsp, struct.pack("<Q", SENTINEL))

        # Interior-start leaves take their pointer in a non-argument register
        # (e.g. RBX) or need a flag/stack register set. Seed those after the
        # normal argument placement and the canonical RSP; "argN" resolves to
        # that argument's arena pointer, and a string is parsed as an integer
        # literal (hex allowed). A startRegs rsp can select a smaller frame so
        # the block's epilogue lands on the sentinel.
        if start_regs:
            reg_map = {
                "rax": x.UC_X86_REG_RAX, "rbx": x.UC_X86_REG_RBX,
                "rcx": x.UC_X86_REG_RCX, "rdx": x.UC_X86_REG_RDX,
                "rsi": x.UC_X86_REG_RSI, "rdi": x.UC_X86_REG_RDI,
                "rbp": x.UC_X86_REG_RBP, "rsp": x.UC_X86_REG_RSP,
                "r8": x.UC_X86_REG_R8, "r9": x.UC_X86_REG_R9,
                "r10": x.UC_X86_REG_R10, "r11": x.UC_X86_REG_R11,
                "r12": x.UC_X86_REG_R12, "r13": x.UC_X86_REG_R13,
                "r14": x.UC_X86_REG_R14, "r15": x.UC_X86_REG_R15,
            }
            for name, source in start_regs.items():
                key = name.lower()
                if key not in reg_map:
                    raise ValueError(f"unknown startRegs register {name!r}")
                if isinstance(source, str) and source.startswith("arg"):
                    value = ptrs[int(source[3:])]
                elif isinstance(source, str):
                    value = int(source, 0)
                else:
                    value = int(source)
                u.reg_write(reg_map[key], value)

        # MS x64 stack arguments. At callee entry RSP points at the return
        # address; the caller's shadow space is [rsp+8, rsp+0x27] and the
        # first stack argument is [rsp+0x28]. Only integer-class slots are
        # used here (pointer/u64); f32/f64 are stored in the low bits.
        rsp_now = u.reg_read(x.UC_X86_REG_RSP)
        for pos in range(4, len(args)):
            a = args[pos]
            if a.kind == "ptr":
                value = ptrs[pos]
            elif a.kind == "u64":
                value = a.bits
            elif a.kind == "f32":
                value = struct.unpack("<I", struct.pack("<f", a.value))[0]
            elif a.kind == "f64":
                value = struct.unpack("<Q", struct.pack("<d", a.value))[0]
            else:
                raise ValueError(f"unknown stack arg kind {a.kind!r}")
            u.mem_write(rsp_now + 0x28 + 8 * (pos - 4), struct.pack("<Q", value))

        writes: List[Tuple[int, bytes]] = []
        reads_outside: List[Tuple[int, int]] = []
        oow: List[Tuple[int, int]] = []
        arena_lo, arena_hi = ARENA_BASE, ARENA_BASE + ARENA_SIZE
        fault: List[str] = []

        def hook_arena_write(uu, access, address, size, value, ud):
            blob = uu.mem_read(address, size)
            writes.append((address, bytes(blob)))

        def hook_image_write(uu, access, address, size, value, ud):
            # A write into the image itself: the function patched shipped
            # code/data. Undeclared by definition - unless it is a write into
            # the controlled state region, which is the unit's declared state
            # (the state channel compares it).
            if state_lo is not None and state_lo <= address < state_hi:
                return
            oow.append((address, size))

        def hook_image_read(uu, access, address, size, value, ud):
            # Reads of the image = global/constant reads outside declared
            # args. Arena/stack/sentinel/shadow are allowed by construction
            # and never reach this callback (range-limited hook).
            reads_outside.append((address, size))

        def hook_unmapped(uu, access, address, size, value, ud):
            kinds = {16: "read", 17: "write", 18: "fetch",
                     19: "read-unmapped", 20: "write-unmapped", 21: "fetch-unmapped"}
            fault.append(f"unmapped {kinds.get(access, access)} at {address:#x} size {size}")
            return False

        # Range-limited hooks: the Python callbacks below fire only for
        # accesses inside the image (the only mapped region where "outside
        # the declared surface" is decidable). Arena/stack traffic - the
        # overwhelming majority - stays native, which is several times
        # faster than a whole-memory hook.
        img_end = (len(self.image) + 0xFFF) & ~0xFFF
        h1 = u.hook_add(UC_HOOK_MEM_WRITE, hook_arena_write,
                        begin=arena_lo, end=arena_hi - 1)
        h2 = u.hook_add(UC_HOOK_MEM_WRITE, hook_image_write,
                        begin=self.base, end=self.base + img_end - 1)
        h3 = u.hook_add(UC_HOOK_MEM_READ, hook_image_read,
                        begin=self.base, end=self.base + img_end - 1)
        h4 = u.hook_add(
            UC_HOOK_MEM_READ_UNMAPPED | UC_HOOK_MEM_WRITE_UNMAPPED | UC_HOOK_MEM_FETCH_UNMAPPED,
            hook_unmapped,
        )

        ret_bits = 0
        ok = False
        state_before = None
        if self.state is not None:
            # The per-case pointee writes above have been applied; this is the
            # exact state the callee starts from.
            state_before = bytes(u.mem_read(state_lo, state_hi - state_lo))
        try:
            # `count` is the native instruction budget; no per-instruction
            # Python hook is needed to enforce it.
            u.emu_start(v2b_addr, SENTINEL, count=steps)
            rip = u.reg_read(x.UC_X86_REG_RIP)
            if rip != SENTINEL:
                # Stopped mid-function: the native step budget ran out (an
                # infinite/runaway loop) or emulation died. Either way this
                # is NOT an ok result - the old code counted steps in a
                # Python hook that could never fire past `count`, so a
                # runaway returned ok=True with garbage registers.
                fault.append(
                    f"no clean return (rip={rip:#x}); step budget {steps} "
                    "likely exhausted"
                )
            if ret_kind == "f32":
                ret_bits = u.reg_read(x.UC_X86_REG_XMM0) & 0xFFFF_FFFF
            elif ret_kind == "f64":
                ret_bits = u.reg_read(x.UC_X86_REG_XMM0) & 0xFFFF_FFFF_FFFF_FFFF
            elif ret_kind == "vec4":
                # The full 128-bit XMM0 value: four little-endian f32 lanes.
                ret_bits = u.reg_read(x.UC_X86_REG_XMM0) & (
                    (1 << 128) - 1
                )
            else:  # integer in RAX
                ret_bits = u.reg_read(x.UC_X86_REG_RAX)
        except UcError as e:
            fault.append(f"UcError: {e}")
        ok = not fault

        u.hook_del(h1)
        u.hook_del(h2)
        u.hook_del(h3)
        u.hook_del(h4)

        after = b""
        for a, p in zip(args, ptrs):
            if a.kind == "ptr":
                after += u.mem_read(p, len(a.mem))

        state_after = None
        if self.state is not None:
            state_after = bytes(u.mem_read(state_lo, state_hi - state_lo))
            # Restore the pre-snapshot bytes so the next call starts from the
            # same state irrespective of what this one wrote.
            if self._state_restore is not None:
                rb, rb_data = self._state_restore
                u.mem_write(rb, rb_data)

        return CallResult(
            ok=ok,
            ret_bits=ret_bits,
            ret_kind=ret_kind,
            writes=writes,
            reads_outside=reads_outside,
            out_of_arena_writes=oow,
            steps=steps if not ok else 0,  # nonzero only when the budget was hit
            fault=fault[0] if fault else None,
            pointee_before=before,
            pointee_after=after,
            state_before=state_before,
            state_after=state_after,
        )


def f32_bits(f: float) -> int:
    return struct.unpack("<I", struct.pack("<f", f))[0]


def bits_f32(b: int) -> float:
    return struct.unpack("<f", struct.pack("<I", b & 0xFFFF_FFFF))[0]
