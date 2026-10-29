"""Deterministic byte/initial inputs for `fn/Crc32NameHash`.

The client's `FUN_141b52ca0` is `uint (byte *data, uint len, uint initial)`:
the reflected zlib CRC32 (polynomial `0xEDB88320`) with `~initial` on entry and
the final complement on return. `FUN_141bb9810` calls it with `initial = 0` over
the exact name bytes, so the specials pin the empty tail, the block/tail split
at lengths 0..24, the real `basecolor` name, and non-zero running seeds.

Every pointee is padded to the unit's declared 64 bytes; bytes past `len` must
not affect either side.
"""

from __future__ import annotations

from typing import List

from emu_oracle import ArgSpec

BUFFER_LEN = 64


def _case(data: bytes, initial: int = 0) -> List[ArgSpec]:
    assert len(data) <= BUFFER_LEN
    buf = data + bytes(BUFFER_LEN - len(data))
    return [
        ArgSpec(kind="ptr", mem=buf, arena_offset=0),
        ArgSpec(kind="u64", bits=len(data)),
        ArgSpec(kind="u64", bits=initial & 0xFFFF_FFFF),
    ]


def _lcg(seed: int, n: int) -> bytes:
    x = seed & 0xFFFF_FFFF
    out = bytearray(n)
    for i in range(n):
        x = (x * 1664525 + 1013904223) & 0xFFFF_FFFF
        out[i] = x >> 24
    return bytes(out)


_SPECIALS: List[List[ArgSpec]] = [
    _case(b""),
    _case(b"basecolor"),
    _case(b"BaseColor"),
    _case(b"123456789"),
    _case(b"basecolor", 0xFFFF_FFFF),
    # The block/tail split: 8 is the first full block, 7 is tail only.
    _case(b"12345678"),
    _case(b"1234567"),
    _case(b"1234567890abcdef"),
    # Realistic material input names.
    _case(b"normal"),
    _case(b"roughness"),
    _case(b"metallic"),
    _case(b"emissive"),
    # A non-zero running seed, then one with both a full block and a tail.
    _case(_lcg(0x1234_5678, 13), 0xDEAD_BEEF),
    _case(_lcg(0x9ABC_DEF0, 23), 0x89AB_CDEF),
    _case(_lcg(0x0BAD_F00D, 64), 0x1357_9BDF),
]
# Every length in the first three blocks, with a deterministic byte pattern.
for n in range(0, 25):
    _SPECIALS.append(_case(_lcg(0x4000 + n, n), (n * 0x0101_0101) & 0xFFFF_FFFF))


def make_args(i: int) -> List[ArgSpec]:
    """The i-th input: buffer bytes, the length the client reads, the seed."""
    if i < len(_SPECIALS):
        return _SPECIALS[i]
    r = i - len(_SPECIALS)
    n = r % (BUFFER_LEN + 1)
    seed = 0xC0DE_0000 ^ (r * 0x9E37_79B9 & 0xFFFF_FFFF)
    initial = 0 if r % 3 == 0 else (r * 0x0101_0101) & 0xFFFF_FFFF
    return _case(_lcg(seed, n), initial)
