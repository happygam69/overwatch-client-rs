"""Synthetic `MRNM` payloads for the three model-mesh units.

The client's model chunk dispatcher rewrites an `MRNM` payload's offsets into
pointers in place, then `MrnmRuntimeSize` (`0x141b89390`) sizes the runtime
block and `MrnmHandler` (`0x141b88f10`) fills it. These fixtures build one
structurally valid payload per case:

    0x000  header: 3 offsets, 4 counts, 5 x 0x1c blocks
    0x100  vertex-buffer descriptors (0x60 each, at most 3)
    0x240  index-buffer descriptors  (0x10 each, at most 3)
    0x280  submesh descriptors       (0x80 each, at most 3)
    0x400  element arrays            (8 bytes per element, at most 4 each)
    0x500  vertex data streams
    0x600  index bytes

Every pointer is payload-relative, which is exactly the file form. The
`mrnm_fixup` fixture declares no relocations: the client's fixup needs the
relative form and adds the payload's own address to it. The `mrnm_load`
fixture declares the same slots as relocations, because the handler runs after
the fixup and must see absolute pointers; the oracle and the probe rebase them
to the same arena address.

The cases are chosen to exercise every branch of the handler: each of the
three counts zero and non-zero, an element count of zero, a separate-asset
flag (0x10) set and clear, a submesh byte 0x7b of zero (taking the model
default) and non-zero, a flags byte of 0x00, 0x10, 0x20 and 0x30 (so the
bit-0x10 test is pinned, not just "any flag"), and a submesh mask of 0, a bit
pattern and -1.
"""

from __future__ import annotations

import random
import struct

from emu_oracle import ARENA_BASE, ArgSpec

P_OFF = 0x1000
M_OFF = 0x2000
D_OFF = 0x3000

PAYLOAD_LEN = 0x700
MODEL_LEN = 0x100
DST_LEN = 0x600

VBO_AT = 0x100
VBO_STRIDE = 0x60
VBO_MAX = 3
IBO_AT = 0x240
IBO_STRIDE = 0x10
IBO_MAX = 3
SUB_AT = 0x280
SUB_STRIDE = 0x80
SUB_MAX = 3
ELEM_AT = 0x400
ELEM_SLOT = 0x20
STREAM_AT = 0x500
DATA_AT = 0x600

DEFAULT_BYTES = (0x00, 0x5A, 0xFF)


def _special(i: int):
    """(vbo, ibo, sub, element counts, flags, defaults, masks)."""
    table = [
        # vbo, ibo, sub, elements per vbo, flag bytes, sub byte 0x7b, masks
        (1, 1, 1, [2], [0x00], [0x11], [0x0000_0000_0000_0005]),
        (0, 0, 0, [], [], [], []),
        (3, 2, 3, [1, 2, 3], [0x00, 0x10, 0x00], [0x00, 0x22, 0x33],
         [0xFFFF_FFFF_FFFF_FFFF, 0x0F0F_0F0F_0F0F_0F0F, 0x0000_0000_0000_0001]),
        (1, 0, 1, [4], [0x20], [0x00], [0x0000_0000_0000_00FF]),
        (0, 2, 0, [], [], [], []),
        (2, 0, 0, [1, 0], [0x10, 0x00], [], []),
        (1, 1, 1, [0], [0x00], [0x00], [0x8000_0000_0000_0000]),
        (3, 1, 3, [1, 0, 4], [0x10, 0x10, 0x00], [0x00, 0x00, 0x7B],
         [0xFFFF_FFFF_FFFF_FFFF, 0xFFFF_FFFF_FFFF_FFFF, 0x0000_0000_0000_002A]),
    ]
    return table[i % len(table)]


def _case(i: int):
    if i < 8:
        return _special(i)
    rng = random.Random(0x4D52_4E4D ^ i)
    vbos = rng.randrange(VBO_MAX + 1)
    ibos = rng.randrange(IBO_MAX + 1)
    subs = rng.randrange(SUB_MAX + 1)
    elements = [rng.randrange(5) for _ in range(vbos)]
    flags = [rng.choice((0x00, 0x10, 0x20, 0x30)) for _ in range(vbos)]
    defaults = [rng.choice((0x00, 0x11, 0x22)) for _ in range(subs)]
    masks = [rng.choice((0, 1, 0xFFFF_FFFF_FFFF_FFFF, rng.getrandbits(64)))
             for _ in range(subs)]
    return vbos, ibos, subs, elements, flags, defaults, masks


def build(i: int):
    """Return (payload, reloc offsets, dst size)."""
    vbos, ibos, subs, elements, flags, defaults, masks = _case(i)
    rng = random.Random(0x5254_4D4D ^ i)
    p = bytearray(PAYLOAD_LEN)

    def q(at: int, value: int) -> None:
        struct.pack_into("<Q", p, at, value)

    def u32(at: int, value: int) -> None:
        struct.pack_into("<I", p, at, value)

    def u16(at: int, value: int) -> None:
        struct.pack_into("<H", p, at, value)

    q(0x00, VBO_AT)
    q(0x08, IBO_AT)
    q(0x10, SUB_AT)
    u32(0x18, rng.getrandbits(32))  # whole-model vertex count, copied only
    u32(0x1c, subs)
    u16(0x20, rng.getrandbits(16))  # material count, copied only
    p[0x22] = vbos
    p[0x23] = ibos
    u32(0x24, rng.getrandbits(32))
    for block in range(5):
        at = 0x28 + block * 0x1c
        for k in range(0x1c):
            p[at + k] = (0xA0 + block * 0x1C + k) & 0xFF

    relocs = [0x00, 0x08, 0x10]
    for v in range(vbos):
        at = VBO_AT + v * VBO_STRIDE
        u32(at + 0x00, rng.getrandbits(32))
        u32(at + 0x04, rng.getrandbits(32))
        p[at + 0x08] = rng.randrange(1, 0x80)  # stride 0
        p[at + 0x09] = rng.randrange(0, 0x80)  # stride 1
        p[at + 0x0a] = elements[v]
        p[at + 0x0b] = flags[v]
        u32(at + 0x0c, rng.getrandbits(32))
        elem_at = ELEM_AT + v * ELEM_SLOT
        q(at + 0x10, elem_at)
        relocs.append(at + 0x10)
        for k in range(7):
            q(at + 0x18 + k * 8, rng.getrandbits(64) if (i + k) % 3 else 0)
        stream0 = STREAM_AT + v * 0x10
        stream1 = STREAM_AT + 0x40 + v * 0x10
        if flags[v] & 0x10:
            # A vertex-data asset GUID; the fixup must not touch it.
            q(at + 0x50, 0x0D00_0000_0000_0800 + v)
            q(at + 0x58, 0x0D00_0000_0000_0900 + v)
        else:
            q(at + 0x50, stream0)
            q(at + 0x58, stream1)
            relocs.append(at + 0x50)
            relocs.append(at + 0x58)
        for e in range(elements[v]):
            src = elem_at + e * 8
            p[src] = rng.randrange(0, 17)  # semantic byte
            p[src + 1] = rng.randrange(0, 3)  # semantic index
            p[src + 2] = rng.choice((0x0, 0x2, 0x4, 0x6, 0x8, 0x9, 0xA, 0xC))
            p[src + 3] = rng.randrange(0, 2)  # stream
            u32(src + 4, rng.getrandbits(32))

    for b in range(ibos):
        at = IBO_AT + b * IBO_STRIDE
        u32(at + 0x00, rng.getrandbits(32))
        u32(at + 0x04, rng.choice((0, 1)))
        data = DATA_AT + b * 0x40
        q(at + 0x08, data)
        relocs.append(at + 0x08)
        for k in range(0x40):
            p[data + k] = rng.randrange(0, 0x100)

    for s in range(subs):
        at = SUB_AT + s * SUB_STRIDE
        for k in range(SUB_STRIDE):
            p[at + k] = rng.randrange(0, 0x100)
        # The used mask: the handler ORs this into the block's first qword.
        q(at + 0x28, masks[s])
        # The index-start block at +0x40..+0x60 is data, not pointers.
        for k in range(8):
            u32(at + 0x40 + k * 4, 0x1000 * (s + 1) + k)
        # Keep the descriptor's links and flags in a sane range.
        u32(at + 0x00, s)
        u32(at + 0x04, s + 1)
        u32(at + 0x08, s + 2)
        u32(at + 0x0c, s + 3)
        u32(at + 0x10, s + 4)
        u32(at + 0x38, s + 5)
        u32(at + 0x3c, s + 6)
        u32(at + 0x60, s + 7)
        u32(at + 0x64, s + 8)
        u16(at + 0x68, s + 9)
        u16(at + 0x6a, s + 10)
        u16(at + 0x6c, 0x0044 + s)
        for k in range(8):
            p[at + 0x6e + k] = s + k
        p[at + 0x76] = s
        p[at + 0x77] = s + 1
        p[at + 0x78] = 0xFF  # lod -1
        p[at + 0x79] = s + 2
        p[at + 0x7a] = s + 3
        p[at + 0x7b] = defaults[s] if s < len(defaults) else 0
        u16(at + 0x7c, 0x1000 + s)

    # The client formula: submeshes*0xa0 + 0xc0 + vbos*0xf8 + ibos*0x18.
    size = subs * 0xA0 + 0xC0 + vbos * 0xF8 + ibos * 0x18
    return bytes(p), relocs, size


def model_object(i: int) -> bytes:
    model = bytearray(MODEL_LEN)
    model[0xAA] = DEFAULT_BYTES[i % len(DEFAULT_BYTES)]
    for k in range(8, 0x18):
        model[k] = (0x30 + k) & 0xFF
    return bytes(model)


def fixup_args(i: int):
    payload, _relocs, _size = build(i)
    return [
        ArgSpec(kind="ptr", mem=payload, arena_offset=P_OFF),
        # Candidate-only: the payload's own address, which the client's fixup
        # uses as the base. The oracle reads it from RCX.
        ArgSpec(kind="u64", bits=ARENA_BASE + P_OFF),
    ]


def size_args(i: int):
    payload, _relocs, _size = build(i)
    return [ArgSpec(kind="ptr", mem=payload, arena_offset=P_OFF)]


def load_args(i: int):
    payload, relocs, _size = build(i)
    model = model_object(i)
    dst = bytes(DST_LEN)
    return [
        ArgSpec(kind="ptr", mem=payload, arena_offset=P_OFF, relocs=tuple(relocs)),
        ArgSpec(kind="ptr", mem=model, arena_offset=M_OFF),
        ArgSpec(kind="ptr", mem=dst, arena_offset=D_OFF),
        # Candidate-only: the two bases, so both sides write the same absolute
        # pointers. The client's 4th and 5th argument slots are dead.
        ArgSpec(kind="u64", bits=ARENA_BASE + P_OFF),
        ArgSpec(kind="u64", bits=ARENA_BASE + D_OFF),
    ]
