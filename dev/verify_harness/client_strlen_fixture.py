"""Deterministic NUL-terminated string cases for `fn/ClientStrlen`.

The client's `strlen` (`0x1426cce30`) takes one pointer in RCX and returns the
length of the NUL-terminated string in RAX: `neg rcx` turns the start into
`-s`, so the return is `end - s`, not the terminator's address. It never
writes the buffer.

Surface: one 0x100-byte arena buffer holding the string. The buffer is placed
at `ARENA_BASE + bias` (bias 0..7) so the client's alignment walk runs; the NUL
sits at `nul_at`, and every byte before it is non-zero. The candidate's own
pointer alignment is not the arena's, but the algorithm's result is the same
for every alignment, so the differential compares the length only.

The client's zero-byte test (`0x7efefefefefefeff + x`, `(~x) ^ that`,
`& 0x8101010101010100`) is not a perfect detector: some qwords with no zero
byte are flagged. The client's byte scan then finds no zero and loops to the
next qword (`test dh, dh` / `jne`), so the returned length is still the true
one. The declared cases include such a qword (`FP`) to drive that loop.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ArgSpec

LEN = 0x100

# A qword the client's test flags but that holds no zero byte (little-endian
# bytes of 0x803982c75afe2a0f). The byte scan finds nothing and loops.
FP = bytes.fromhex("0f2afe5ac7823980")

# (bias, nul_at, seed, prefix)
Case = Tuple[int, int, int, bytes]


def _cases() -> List[Case]:
    cases: List[Case] = []
    # Empty string at every alignment: the byte walk's NUL exit.
    for bias in range(8):
        cases.append((bias, 0, bias, b""))
    # NUL at each byte of the first qword, aligned start: the SWAR byte scan,
    # every `je` edge.
    for n in range(8):
        cases.append((0, n, 0x10 + n, b""))
    # Unaligned starts: the byte walk runs, then either finds the NUL or
    # aligns and enters the SWAR. Both edges of the walk's `jne`.
    for bias in range(1, 8):
        for n in range(0, 16):
            cases.append((bias, n, 0x20 + bias * 0x10 + n, b""))
    # Several qwords before the NUL: the SWAR loop back edge (`je` taken).
    for n in (8, 9, 15, 16, 17, 31, 32, 0x40, 0x7F, 0x80, 0xF8, 0xFF):
        cases.append((0, n, 0x30 + (n & 0xFF), b""))
    # Unaligned with several qwords before the NUL.
    for bias in (1, 3, 7):
        for n in (0x20, 0x40, 0xF8):
            cases.append((bias, n, 0x40 + bias, b""))
    # A flagged qword with no zero byte, then a NUL: the byte scan's `jne`
    # loops to the next qword.
    cases.append((0, 8, 0x55, FP))
    cases.append((0, 9, 0x56, FP))
    cases.append((3, 8, 0x57, FP))
    return cases


SPECIALS = _cases()


def _random(r: int) -> Case:
    bias = r % 8
    nul_at = (r * 37 + 11) % LEN
    seed = r * 97 + 5
    return (bias, nul_at, seed, b"")


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _random(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    bias, nul_at, seed, prefix = make_case(i)
    buf = bytearray(LEN)
    for j in range(LEN):
        x = (seed * 31 + j * 7 + (j >> 3) * 13) & 0xFF
        buf[j] = x if x != 0 else 0x41
    if prefix:
        buf[: len(prefix)] = prefix
    buf[nul_at] = 0
    return [ArgSpec(kind="ptr", mem=bytes(buf), arena_offset=bias)]
