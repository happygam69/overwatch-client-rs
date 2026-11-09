"""Deterministic (out, count, text, len) cases for `fn/HexPairDecode`.

The client's hex-pair decoder (`0x14269ab80`) is
`(out, count, text, len) -> written`: it decodes up to `count` bytes,
two characters per byte, and returns the bytes written. It never zeroes
`out`: only the bytes it writes change, so the `out` buffer starts as
`0xA5` sentinels and every unwritten byte is compared. A pair needs
two characters (`offset >= len - 1` ends the run, so a trailing odd
character is never decoded), a zero byte as the first character ends
the run, any character outside `0-9a-fA-F` in either nibble ends it,
and the loop leaves as soon as `count` bytes are done. The client
does no whitespace skipping here: a space is just a non-hex character.

The declared cases sit exactly on the branches and constants: the
three hex ranges and their one-off neighbours in both nibble
positions (`/` below `0`, `:` above `9`, `` ` `` below `a`, `g` above
`f`, `@` below `A`, `G` above `F`), upper/lowercase, nibble order,
NUL in either position, high-bit bytes, the `len` truncation of a
longer buffer (exact-length vs short, odd trailing char), and the
`count` edges (0, exact capacity, capacity minus one, past the text).
Pointers are arena offsets: `text` at 0, `out` at 0x9000. Lengths stay
tied to the buffers: `len <= len(text)` and `count <= len(out)`,
because the client has no bounds checks.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ArgSpec

OUT_OFFSET = 0x9000
SENTINEL = 0xA5
OUT_LEN = 32
# The text buffer is never longer than this; the unit declares 64.
TEXT_MAX = 64

# (out, count, text, len)
Case = Tuple[bytes, int, bytes, int]


def _out() -> bytes:
    return bytes([SENTINEL]) * OUT_LEN


def _cases() -> List[Case]:
    o = _out()
    return [
        # Determinism: a plain pair, count and length both exact.
        (o, 1, b"0f", 2),
        # Nibble order: the same digits written the other way round.
        (o, 1, b"f0", 2),
        (o, 2, b"0123", 4),
        (o, 2, b"1023", 4),
        # The empty length (len == 0) and the zero count.
        (o, 4, b"", 0),
        (o, 0, b"0f", 2),
        (o, 0, b"", 0),
        (o, 0, b"0123456789abcdef", 16),
        # One character only: offset 0 >= len - 1, nothing decoded.
        (o, 4, b"a", 1),
        (o, 1, b"F", 1),
        # An odd trailing character is never decoded (needs a pair).
        (o, 10, b"abc", 3),
        (o, 10, b"0", 1),
        (o, 10, b"012", 3),
        # Exact length vs a len that truncates a longer buffer mid-pair.
        (o, 10, b"abcd", 4),
        (o, 10, b"abcd", 3),
        (o, 10, b"abcd", 2),
        (o, 10, b"abcd", 1),
        (o, 10, b"abcd", 0),
        # More text than count: the loop leaves at count, byte 2 untouched.
        (o, 1, b"abcd", 4),
        (o, 2, b"abcd", 4),
        (o, 2, b"0123456789abcdef", 16),
        # Count past the text: short result, rest of out stays sentinel.
        (o, 10, b"0123", 4),
        (o, OUT_LEN, b"0123", 4),
        # Count at the exact output capacity, and one below it.
        (o, OUT_LEN, b"0123456789abcdef" * 4, 64),
        (o, OUT_LEN - 1, b"0123456789abcdef" * 4, 64),
        (o, OUT_LEN, b"ff" * 32, 64),
        # Every hex range, mixed case: digits, a-f, A-F in both nibbles.
        (o, 11, b"0123456789abcdefABCDEF", 22),
        (o, 6, b"aAbBcC", 6),
        (o, 6, b"Ff09Aa", 6),
        (o, 4, b"9A9a", 4),
        (o, 4, b"gG@`", 4),  # (control: all rejected, see below)
        # The decoded byte may equal the sentinel; it still counts.
        (o, 1, b"a5", 2),
        (o, 2, b"a5A5", 4),
        # Boundary neighbours of '0'..'9' as the first character.
        (o, 4, b"/123", 4),  # '/' is one below '0'
        (o, 4, b":123", 4),  # ':' is one above '9'
        (o, 4, b" 123", 4),  # space: below '0', no whitespace skip here
        (o, 4, b"\t123", 4),
        (o, 4, b"0123", 4),  # the boundary itself decodes
        # Boundary neighbours as the second character.
        (o, 4, b"0/23", 4),
        (o, 4, b"0:23", 4),
        (o, 4, b"0 23", 4),
        (o, 4, b"09ab", 4),
        # Boundary neighbours of 'a'..'f' as the first character.
        (o, 4, b"`123", 4),  # '`' is one below 'a'
        (o, 4, b"g123", 4),  # 'g' is one above 'f'
        (o, 4, b"f123", 4),
        # ... and as the second character.
        (o, 4, b"0`23", 4),
        (o, 4, b"0g23", 4),
        (o, 4, b"0f23", 4),
        # Boundary neighbours of 'A'..'F' as the first character.
        (o, 4, b"@123", 4),  # '@' is one below 'A'
        (o, 4, b"G123", 4),  # 'G' is one above 'F'
        (o, 4, b"A123", 4),
        (o, 4, b"F123", 4),
        # ... and as the second character.
        (o, 4, b"0@23", 4),
        (o, 4, b"0G23", 4),
        (o, 4, b"0A23", 4),
        (o, 4, b"0F23", 4),
        # A NUL as the first character of a pair, and as the second.
        (o, 4, bytes([0x00]) + b"123", 4),
        (o, 4, b"0" + bytes([0x00]) + b"23", 4),
        # A NUL after one good pair: the partial write stays observable.
        (o, 4, b"ab" + bytes([0x00]) + b"123", 6),
        (o, 4, b"ab" + bytes([0x00][0:1]) + b"0" + bytes([0x00]) + b"2", 6),
        # Whitespace is not skipped by this function.
        (o, 4, b"ab cd", 5),
        (o, 4, b"ab\tcd", 5),
        (o, 4, b"ab\ncd", 5),
        (o, 4, b"ab\rcd", 5),
        (o, 4, b" abcd", 5),
        # High-bit bytes are not hex digits, in either nibble position.
        (o, 4, bytes([0x80]) + b"123", 4),
        (o, 4, bytes([0xFF]) + b"123", 4),
        (o, 4, bytes([0xE1]) + b"123", 4),
        (o, 4, b"0" + bytes([0x80]) + b"23", 4),
        (o, 4, b"0" + bytes([0xFF]) + b"23", 4),
        (o, 4, bytes([0xFF, 0xFF]), 2),
        # A non-hex after good pairs: partial writes, then the stop.
        (o, 8, b"0123g567", 8),
        (o, 8, b"0123" + bytes([0xFF]) + b"567", 8),
        # len stops before the buffer's end: the rest is never read.
        (o, 8, b"0123456789abcdef", 4),
        (o, 8, b"0123456789abcdef", 5),
        (o, 8, b"0123456789abcdef", 2),
        # Every byte different from the sentinel.
        (o, 8, b"ffeeddccbbaa9988", 16),
        (o, 32, bytes([0x30, 0x39] * 32), 64),
    ]


SPECIALS = _cases()


def _random(r: int) -> Case:
    """A generated case: hex text (mixed case), sometimes corrupted."""
    bits = bytes((r * 37 + i * 11 + (r >> 3)) & 0xFF for i in range(16))
    text = "".join("%02x" % b for b in bits).encode()
    if r % 3 == 0:
        text = text.upper()
    elif r % 3 == 1 and len(text) >= 2:
        mixed = bytearray(text)
        for i in range(0, len(mixed), 3):
            mixed[i] = mixed[i] - 0x20  # 'a' -> 'A'
        text = bytes(mixed)
    body = bytearray(text)
    length = len(body)
    shape = r % 11
    if shape == 1:
        length -= 1 + (r % 3)
    elif shape == 2 and length > 1:
        body[r % length] = 0x67  # 'g'
    elif shape == 3 and length > 2:
        body[r % length] = 0x00
    elif shape == 4 and length > 1:
        body[r % length] = 0xFF
    elif shape == 5 and length > 1:
        body[r % length] = 0x20  # space
    elif shape == 6:
        length = 0
    elif shape == 7:
        body += b"tail"
        length = len(body)
    elif shape == 8:
        # An odd trailing character.
        body += b"f"
        length = len(body)
    count = (r * 5 + 1) % (OUT_LEN + 1)
    if shape == 9:
        count = 0
    elif shape == 10:
        count = OUT_LEN
    if length > TEXT_MAX:
        body = body[:TEXT_MAX]
        length = min(length, TEXT_MAX)
    return (_out(), count, bytes(body), min(length, len(body)))


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _random(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    out, count, text, length = make_case(i)
    return [
        ArgSpec(kind="ptr", mem=out, arena_offset=OUT_OFFSET),
        ArgSpec(kind="u64", bits=count),
        ArgSpec(kind="ptr", mem=text, arena_offset=0),
        ArgSpec(kind="u64", bits=length),
    ]
