"""Deterministic (text, len, out, flag) cases for `fn/ParseHexKeyId`.

The client's `ParseHexKeyId` (`0x14248f040`) is `ParseHexKey`'s body with an
8-byte target: it zeroes the 8-byte output and a flag byte, skips leading
whitespace (space, tab, LF, VT, FF, CR), decodes exactly 8 bytes through
`0x14269ab80` and returns `whitespace + 16`, or 0 when fewer than 8 bytes
decoded. The `out` and `flag` buffers start as `0xA5` so the zeroing, the
partial writes and the flag write are all compared.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ArgSpec

OUT_OFFSET = 0xB000
FLAG_OFFSET = 0xC000
SENTINEL = 0xA5

KEY = b"0011223344556677"
NUL = bytes([0x00])


def _out(n: int) -> bytes:
    return bytes([SENTINEL]) * n


Case = Tuple[bytes, int, bytes, bytes]


def _cases8() -> List[Case]:
    o, f = _out(8), _out(1)
    return [
        # Determinism case: 16 characters, no whitespace.
        (KEY, 16, o, f),
        (b"00112233445566FF", 16, o, f),
        # Leading whitespace of every skipped kind.
        (b" \t\r\n\x0b\x0c" + KEY, 22, o, f),
        (b"   " + KEY, 19, o, f),
        # 0x08 and 0x0e are outside the skipped range.
        (b"\x08" + KEY, 17, o, f),
        (b"\x0e" + KEY, 17, o, f),
        # A tail past the 16 characters is not consumed.
        (KEY + b" tail", 21, o, f),
        # More than 8 bytes' worth of hex: the decoder stops at 8.
        (b"00112233445566778899aabb", 24, o, f),
        (b"001122334455667788", 18, o, f),
        # One character short, one character only, nothing at all.
        (KEY[:-1], 15, o, f),
        (b"0", 1, o, f),
        (b"", 0, o, f),
        # A non-hex character in either position of a pair.
        (b"g011223344556677", 16, o, f),
        (b"00112233445566g7", 16, o, f),
        # A NUL at the start and in the middle of a pair.
        (NUL + KEY, 17, o, f),
        (b"0011" + NUL + b"223344556677", 17, o, f),
        # Whitespace only.
        (b" ", 1, o, f),
        # Every byte different from the sentinel.
        (b"ffeeddccbbaa9988", 16, o, f),
        # A high-bit byte is not a hex digit.
        (bytes([0x80]) + KEY[1:], 16, o, f),
    ]


SPECIALS = _cases8()


def _random(r: int) -> Case:
    bits = bytes((r * 41 + i * 13 + (r >> 2)) & 0xFF for i in range(8))
    text = "".join("%02x" % b for b in bits).encode()
    prefix = bytes([0x20, 0x09, 0x0A, 0x0B, 0x0C, 0x0D][r % 6]) * (r % 4)
    body = bytearray(text)
    length = len(body)
    shape = r % 7
    if shape == 1:
        length -= 1 + (r % 2)
    elif shape == 2 and length > 1:
        body[r % length] = 0x47  # 'G'
    elif shape == 3 and length > 2:
        body[r % length] = 0x00
    elif shape == 4:
        body = bytearray(text.upper())
    elif shape == 5:
        body += b"tail"
        length = len(body)
    return (prefix + bytes(body), length, _out(8), _out(1))


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
