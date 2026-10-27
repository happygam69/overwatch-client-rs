"""Deterministic inputs for the `fn/EncodingHeaderParse` unit.

The client's `EncodingHeaderParse` (`0x142498920`) takes a 40-byte output
struct, the header bytes and a byte count. The oracle is the client's own
bytes; this module only builds the inputs.

The first case is the install's real 22-byte encoding header
(`111,058,223` decoded bytes): `'EN'`, version 1, 16/16 keys, 4 KiB pages,
16025 + 10519 pages, byte 17 zero, and a 1,484,507-byte espec block. Then
every length 0..22 (each truncation of that header) and two longer lengths,
every gate byte, both key widths at their edges, both page sizes and page
counts at zero, one and the all-ones word, the espec block at zero, one and
all-ones, combinations, and a deterministic byte/offset/length sweep.

The output buffer starts at a nonzero `0xA5` pattern, so the differential
compares every byte the client leaves untouched as well as the seven fields
it writes. The header buffer is padded with `0x5A`, so the input-mutation
channel is real too; a reject must leave both buffers exactly as they came
in, and a value reject must still show the seven written fields.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ArgSpec

OUT_LEN = 40
DATA_LEN = 64
OUT_FILL = 0xA5
DATA_FILL = 0x5A

# The install's real encoding header: the first 22 bytes of the decoded
# encoding file named by the build config.
REAL_HEADER = bytes.fromhex("454e0110100004000400003e9900002917000016a6db")

assert len(REAL_HEADER) == 22


def _with(off: int, value: int) -> bytes:
    out = bytearray(REAL_HEADER)
    out[off] = value & 0xFF
    return bytes(out)


def _be16(off: int, value: int) -> bytes:
    out = bytearray(REAL_HEADER)
    out[off : off + 2] = value.to_bytes(2, "big")
    return bytes(out)


def _be32(off: int, value: int) -> bytes:
    out = bytearray(REAL_HEADER)
    out[off : off + 4] = value.to_bytes(4, "big")
    return bytes(out)


def _cases() -> List[Tuple[bytes, int]]:
    out: List[Tuple[bytes, int]] = []

    # Every short length rejects before reading a byte; 22 is the first
    # accepted header; 23 and 64 prove the extra bytes are ignored.
    for n in range(0, 23):
        out.append((REAL_HEADER, n))
    out.append((REAL_HEADER, 23))
    out.append((REAL_HEADER, DATA_LEN))

    # The four signature/version gates and the byte-17 gate.
    out.append((_with(0, ord("D")), 22))
    out.append((_with(1, ord("M")), 22))
    out.append((_with(2, 0), 22))
    out.append((_with(2, 2), 22))
    out.append((_with(0x11, 1), 22))

    # Key widths: 0 and 17 reject, 1 and 16 accept. Both sides of each gate,
    # and the width-1 case proves the header parser accepts it even though the
    # shipped page reader is fixed at 16.
    for w in (0, 1, 17):
        out.append((_with(3, w), 22))
    for w in (0, 1, 17):
        out.append((_with(4, w), 22))

    # The zero edge of each value check, in the client's validation order.
    out.append((_be16(5, 0), 22))
    out.append((_be16(7, 0), 22))
    out.append((_be32(9, 0), 22))
    out.append((_be32(13, 0), 22))
    out.append((_be32(18, 0), 22))

    # The all-ones word: every byte nonzero, so a wrong shift, width or byte
    # order cannot agree with the client by accident.
    out.append((_be16(5, 0xFFFF), 22))
    out.append((_be16(7, 0xFFFF), 22))
    out.append((_be32(9, 0xFFFFFFFF), 22))
    out.append((_be32(13, 0xFFFFFFFF), 22))
    out.append((_be32(18, 0xFFFFFFFF), 22))

    # The smallest accepted value of each field.
    out.append((_be16(5, 1), 22))
    out.append((_be16(7, 1), 22))
    out.append((_be32(9, 1), 22))
    out.append((_be32(13, 1), 22))
    out.append((_be32(18, 1), 22))

    # Combinations: the narrowest and widest headers the parser accepts, a
    # mixed-width header, an all-zero value header (all seven writes, then the
    # first value reject) and three late failures after every field is written.
    minimal = bytearray(REAL_HEADER)
    minimal[3] = 1
    minimal[4] = 1
    minimal[5:7] = (1).to_bytes(2, "big")
    minimal[7:9] = (1).to_bytes(2, "big")
    minimal[9:13] = (1).to_bytes(4, "big")
    minimal[13:17] = (1).to_bytes(4, "big")
    minimal[18:22] = (1).to_bytes(4, "big")
    out.append((bytes(minimal), 22))

    maximal = bytearray(REAL_HEADER)
    maximal[3] = 16
    maximal[4] = 16
    maximal[5:7] = (0xFFFF).to_bytes(2, "big")
    maximal[7:9] = (0xFFFF).to_bytes(2, "big")
    maximal[9:13] = (0xFFFFFFFF).to_bytes(4, "big")
    maximal[13:17] = (0xFFFFFFFF).to_bytes(4, "big")
    maximal[18:22] = (0xFFFFFFFF).to_bytes(4, "big")
    out.append((bytes(maximal), 22))

    mixed = bytearray(REAL_HEADER)
    mixed[3] = 1
    mixed[4] = 15
    mixed[5:7] = (0xFFFF).to_bytes(2, "big")
    mixed[7:9] = (1).to_bytes(2, "big")
    mixed[9:13] = (1).to_bytes(4, "big")
    mixed[13:17] = (0xFFFFFFFF).to_bytes(4, "big")
    mixed[18:22] = (1).to_bytes(4, "big")
    out.append((bytes(mixed), 22))

    # Every gate byte passes while every value check is zero: the client
    # writes all seven fields, then rejects at the first value check.
    zero_values = bytearray(REAL_HEADER)
    zero_values[3] = 0
    zero_values[4] = 0
    zero_values[5:9] = bytes(4)
    zero_values[9:17] = bytes(8)
    zero_values[18:22] = bytes(4)
    out.append((bytes(zero_values), 22))

    # One late failure with every other value at its maximum: the output must
    # show every write, including the field whose check fails.
    late = bytearray(maximal)
    late[18:22] = bytes(4)
    out.append((bytes(late), 22))
    late = bytearray(maximal)
    late[13:17] = bytes(4)
    out.append((bytes(late), 22))
    late = bytearray(maximal)
    late[7:9] = bytes(2)
    out.append((bytes(late), 22))
    return out


SPECIALS = _cases()

# Lengths for the deterministic sweep. 64 is the declared pointee length, so
# no case can ask the callee to read past the buffer.
_LENGTHS = list(range(0, 65)) + [0, 21, 22, 23, 40, 63, 64]


def _sweep(r: int) -> Tuple[bytes, int]:
    """A deterministic (data, len) after the pinned cases.

    Three runs in four start from the real header and change one to three
    bytes, so the accepted and per-value-reject paths recur; the fourth is
    noise, so the gate bytes vary. The length is independent of the content,
    which pins short-length rejects over both valid and invalid prefixes.
    """
    state = (0xE0000 + r) & 0xFFFFFFFF

    def next_byte() -> int:
        nonlocal state
        state = (state * 1664525 + 1013904223) & 0xFFFFFFFF
        return state >> 24

    data = bytearray(next_byte() for _ in range(DATA_LEN))
    if r % 4:
        data[: len(REAL_HEADER)] = REAL_HEADER
    for _ in range(1 + r % 3):
        data[next_byte() % len(REAL_HEADER)] = next_byte()
    return bytes(data), _LENGTHS[r % len(_LENGTHS)]


def make_case(i: int) -> Tuple[bytes, int]:
    """The i-th (data, len). Pure function of i."""
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _sweep(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    data, length = make_case(i)
    data = data + bytes([DATA_FILL]) * (DATA_LEN - len(data))
    return [
        ArgSpec(kind="ptr", mem=bytes([OUT_FILL]) * OUT_LEN, arena_offset=0),
        ArgSpec(kind="ptr", mem=data, arena_offset=0x100),
        ArgSpec(kind="u64", bits=length),
    ]
