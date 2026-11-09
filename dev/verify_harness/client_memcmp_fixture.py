"""Deterministic (a, b, count) cases for `fn/Memcmp`.

The client's `memcmp` (`0x142efecf0`) is `(const void *a, const void *b,
size_t count) -> int`: `0` when the `count` bytes are equal, `-1` when the
first differing byte of `a` is below `b`'s, `1` when it is above (the client
writes `eax` through `sbb eax, eax` / `sbb eax, -1`, so RAX carries
`0x0000_0000_ffff_ffff` for `-1`). It never writes either buffer.

Surface: two 0x100-byte arena buffers, `a` at 0x1000 + `bias` (0..7) and `b`
at 0x2000, plus the length as a plain u64. The bias selects the client's
alignment walk: `bias = 0` starts on the 32-byte block path, `bias = 1..7`
walks the first bytes one at a time until `a` is 8-byte aligned. `count` is
always strictly below 0x100 so the byte one past the compared region exists
and differs between the buffers - a boundary mutant that widens the loop
reads that byte and answers differently instead of faulting.

Every declared case is a hand-picked boundary of one of the client's branch
sites: `count` below/at 8, exact multiples of 8 and of 0x20, leftovers of
1..7, 0x20-blocks with a difference in each of the four qwords, a difference
inside the alignment head, inside the 8-byte loop, inside the 0..7 tail, and
a difference only past `count` (equal answer). The random cases sweep the
remaining combinations of bias, length and differing position.
"""

from __future__ import annotations

from typing import List, Optional, Tuple

from emu_oracle import ArgSpec

A_OFF = 0x1000
B_OFF = 0x2000
LEN = 0x100
# Padding past the compared region: distinct in the two buffers, so a loop
# that runs one byte too long returns a different sign.
PAD_A = 0x10
PAD_B = 0xE0

# (bias, a, b, count)
Case = Tuple[int, bytes, bytes, int]


def _case(
    count: int,
    diff: Optional[int] = None,
    bias: int = 0,
    below: bool = True,
    seedn: int = 0,
) -> Case:
    """`count` equal bytes, an optional difference at `diff`, padded buffers."""
    body = bytes((0x41 + ((seedn * 13 + i * 3) & 0x3F)) for i in range(count))
    a = bytearray(body) + bytes([PAD_A]) * (LEN - count)
    b = bytearray(body) + bytes([PAD_B]) * (LEN - count)
    if diff is not None:
        a[diff] = 0x00 if below else 0xFF
        b[diff] = 0xFF if below else 0x00
    return (bias, bytes(a), bytes(b), count)


def _cases() -> List[Case]:
    return [
        # Zero length: the `test r8, r8; je` zero exit, nothing compared.
        _case(0),
        # The byte tail (count < 8): equal, and a difference at the first and
        # the last byte of the run, in both directions.
        _case(1),
        _case(1, 0),
        _case(1, 0, 0, False),
        _case(2),
        _case(3),
        _case(3, 2),
        _case(5),
        _case(5, 4, 0, False),
        _case(7),
        _case(7, 6),
        # count == 8 aligned: the 8-byte compare, a difference in its first and
        # last byte, both signs.
        _case(8),
        _case(8, 0),
        _case(8, 7),
        _case(8, 0, 0, False),
        # count == 8 unaligned: the head byte walk (its back edge), then a
        # single leftover byte in the tail (the `r9 == 0` path after
        # alignment), and differences in the head and in that last byte.
        _case(8, None, 1),
        _case(8, 0, 1),
        _case(8, 7, 1),
        _case(8, None, 5),
        # 9..15 unaligned: head walk, then fewer than 8 bytes left -> tail.
        _case(9, None, 1),
        _case(15, None, 7),
        # 16 aligned: two qwords in the 8-byte loop (its back edge on and off).
        _case(16),
        _case(16, 8),
        # 24 aligned: three qwords in the 8-byte loop.
        _case(24),
        # 32 aligned: one 32-byte block, no leftover (the `r8 & 0x1f == 0`
        # tail exit), a difference in each of the four qwords.
        _case(32),
        _case(32, 0),
        _case(32, 8, 0, False),
        _case(32, 0x10),
        _case(32, 0x18),
        # 32 unaligned: head walk (5 bytes), then 27 bytes -> three qwords in
        # the 8-byte loop and a 3-byte tail; and the same with a 1-byte head.
        _case(32, None, 3),
        _case(32, None, 7),
        # 40 aligned: one 32-byte block, then one leftover qword (the leftover
        # loop taken once), no tail.
        _case(40),
        _case(40, 32),
        # 48 aligned: leftover 16 -> the leftover 8-byte loop taken twice.
        _case(48),
        # 39 aligned: one 32-byte block, leftover 7 -> the tail from the
        # leftover path.
        _case(39),
        _case(39, 36),
        # 64 aligned: two 32-byte blocks (the block back edge taken), and a
        # difference in the second block.
        _case(64),
        _case(64, 40),
        # 128 aligned: four 32-byte blocks, difference at the last byte.
        _case(0x80),
        _case(0x80, 0x7F),
        # The largest count (0xFF): seven 32-byte blocks, leftover 24, no tail.
        _case(0xFF),
        _case(0xFF, 200, 0, False),
        # Alignment 4 and 6 with a long equal run (head walk length 4 and 2).
        _case(0x20, None, 4),
        _case(0x30, 44, 2),
        _case(0x40, 8, 6),
        # Equal over the whole count, differing only past it.
        _case(0xF8),
    ]


SPECIALS = _cases()


def _random(r: int) -> Case:
    """A generated case: random length, alignment and differing position."""
    count = (r * 37 + 11) % LEN
    slope = 1 + r % 5
    body = bytes((r * 5 + i * slope + (r >> 2)) & 0xFF for i in range(count))
    a = bytearray(body) + bytes([PAD_A]) * (LEN - count)
    b = bytearray(body) + bytes([PAD_B]) * (LEN - count)
    shape = r % 6
    if count:
        if shape in (0, 1, 4):
            # A difference inside the compared region, at a random position.
            at = (r * 7) % count
            if shape == 1:
                a[at] = (a[at] + 1 + r % 0x7F) & 0xFF
            else:
                b[at] = (b[at] + 1 + r % 0x7F) & 0xFF
        elif shape == 2:
            # Equal over the region; the padding past it differs anyway.
            pass
        elif shape == 3:
            # A long equal prefix, then a difference at the last byte.
            at = count - 1
            b[at] ^= 0xFF
        else:
            # An 8-byte boundary difference.
            at = min((r % (count // 8 + 1)) * 8, count - 1)
            a[at] = (a[at] + 0x80) & 0xFF
    bias = r % 8
    return (bias, bytes(a), bytes(b), count)


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _random(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    bias, a, b, count = make_case(i)
    return [
        ArgSpec(kind="ptr", mem=a, arena_offset=A_OFF + bias),
        ArgSpec(kind="ptr", mem=b, arena_offset=B_OFF),
        ArgSpec(kind="u64", bits=count),
    ]
