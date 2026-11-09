"""Deterministic (text, len, out, flag) cases for `fn/ParseHexKey`.

The client's `ParseHexKey` (`0x14248efd0`) zeroes the 16-byte output and a flag
byte, skips leading whitespace (space, tab, LF, VT, FF, CR), then decodes
exactly 16 bytes through `0x14269ab80` and returns the characters consumed
(`whitespace + 32`), or 0 when fewer than 16 bytes decoded. The decoder needs
two characters per byte, stops at a zero byte or a non-hex character, and
leaves the bytes it already wrote in place - so the output is both an input
(the zeroing and the partial writes are visible) and an output.

The corpus pins every edge: the six whitespace bytes and the two near misses
(0x08 below and 0x0e above the range), a 31-character string, a 32-character
string with a tail, a non-hex character in either nibble position, an embedded
NUL, an all-whitespace string, the empty string and a one-character string.

The `out` and `flag` buffers start as `0xA5`, so a case that only checks the
return value cannot pass: the client's zeroing and its exact written prefix are
both compared.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ArgSpec

OUT_OFFSET = 0x9000
FLAG_OFFSET = 0xA000
SENTINEL = 0xA5

KEY = b"0123456789abcdef0123456789abcdef"
NUL = bytes([0x00])


def _out(n: int) -> bytes:
    return bytes([SENTINEL]) * n


# (text, len, out, flag)
Case = Tuple[bytes, int, bytes, bytes]


def _cases16() -> List[Case]:
    o, f = _out(16), _out(1)
    return [
        # Determinism case: 32 characters, no whitespace.
        (KEY, 32, o, f),
        # Uppercase and mixed case decode to the same bytes.
        (b"0123456789ABCDEF0123456789ABCDEF", 32, o, f),
        (b"0123456789AbCdEf0123456789aBcDeF", 32, o, f),
        # Every whitespace byte the client skips, in order.
        (b" \t\r\n\x0b\x0c" + KEY, 38, o, f),
        (b" " * 20 + KEY, 52, o, f),
        (b"\t\t" + KEY, 34, o, f),
        # 0x08 and 0x0e are one below and one above the skipped range: the
        # decode sees them as non-hex and reports 0.
        (b"\x08" + KEY, 33, o, f),
        (b"\x0e" + KEY, 33, o, f),
        # A tail past the 32 characters is not consumed.
        (KEY + b" tail", 37, o, f),
        (KEY + b"ffff", 36, o, f),
        # More than 16 bytes' worth of hex: the decoder stops at 16 and the
        # remaining characters are not consumed.
        (b"00112233445566778899aabbccddeeff00112233", 40, o, f),
        (b"00112233445566778899", 20, o, f),
        # One character short: 15 bytes decode, the count is 0 and the last
        # byte stays zero (the client zeroed the block first).
        (KEY[:-1], 31, o, f),
        (KEY[:-2], 30, o, f),
        # One character only.
        (b"0", 1, o, f),
        # A non-hex character first in the pair (position 0) and second in a
        # pair (position 17): both stop the run and report 0.
        (b"g123456789abcdef0123456789abcdef", 32, o, f),
        (b"0123456789abcdefg123456789abcdef", 32, o, f),
        (KEY[:16] + bytes([0x01]) + KEY[17:], 32, o, f),
        # A NUL at the start and in the middle of a pair.
        (NUL + KEY, 33, o, f),
        (b"0123" + NUL + b"56789abcdef0123456789abcdef", 32, o, f),
        # A space between nibbles is not a digit.
        (b"0123 56789abcdef0123456789abcdef", 32, o, f),
        # Whitespace only, and nothing at all.
        (b"   ", 3, o, f),
        (b"", 0, o, f),
        # The length may stop before the buffer's end: the rest is never read.
        (KEY + b"zzzzzzzz", 32, o, f),
        # High-bit bytes are not hex digits.
        (bytes([0x80]) + KEY[1:], 32, o, f),
        (bytes([0xFF]) * 32, 32, o, f),
        # Decoded bytes that differ from the sentinel in every position.
        (b"ffeeddccbbaa99887766554433221100", 32, o, f),
    ]


SPECIALS = _cases16()


def _random(r: int) -> Case:
    """A generated case: a valid key, a whitespace prefix and, sometimes, one
    corruption (a short length, a non-hex character or a NUL)."""
    bits = bytes((r * 37 + i * 11 + (r >> 3)) & 0xFF for i in range(16))
    text = "".join("%02x" % b for b in bits).encode()
    prefix = bytes([0x20, 0x09, 0x0A, 0x0B, 0x0C, 0x0D][r % 6]) * (r % 5)
    body = bytearray(text)
    length = len(body)
    shape = r % 7
    if shape == 1:
        length -= 1 + (r % 3)
    elif shape == 2 and length > 1:
        body[r % length] = 0x67  # 'g'
    elif shape == 3 and length > 2:
        body[r % length] = 0x00
    elif shape == 4:
        body = bytearray(text.upper())
    elif shape == 5:
        body += b"junk"
        length = len(body)
    return (prefix + bytes(body), length, _out(16), _out(1))


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _random(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    text, length, out, flag = make_case(i)
    return [
        ArgSpec(kind="ptr", mem=text, arena_offset=0),
        ArgSpec(kind="u64", bits=length),
        ArgSpec(kind="ptr", mem=out, arena_offset=OUT_OFFSET),
        ArgSpec(kind="ptr", mem=flag, arena_offset=FLAG_OFFSET),
    ]
