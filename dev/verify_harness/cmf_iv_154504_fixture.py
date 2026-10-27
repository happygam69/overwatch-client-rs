"""Deterministic inputs for the `fn/CmfIv154504` unit.

The client's `CmfIv154504` (`0x142e48700`) reads the 512-byte table at
`0x1436100c0` (the unit's `state` block), takes the file-name string in the
client's 32-byte layout (inline buffer at `+0`, size dword at `+0x10`,
capacity dword at `+0x14`), reads `buildVersion` at header `+0` and `dataCount`
at header `+0x20`, and writes 16 IV bytes. The digest inside the client is
SHA-1 of the name; the probe computes the same SHA-1 with the crate's `sha1`
dependency and passes it to `ow_tank::manifest::iv_154504`.

The table is synthetic and not game data: `table[i] = i & 0xFF`, so
`table[0] = 0x00` and `table[255] = 0xFF`. The pinned cases include one whose
first two reads are exactly those two indices.

The cases pin:

* a manifest-shaped build/count and a short/long name around the 16-byte
  inline boundary;
* build `0`, `1`, `32`, `0x0800_0000` (cursor `0x8000_0000`, the signed
  `SignedMod` edge), `0x1000_0000` and `0x8000_0000` (cursor wraps to 0),
  `0x7FFF_FFFF` and `0xFFFF_FFFF` (cursor wraps below `2^32`);
* data count `0`, `19`, `20`, `21`, `39`, `40`, `41`, `0x7FFF_FFFF`,
  `0x8000_0000` (as i32 it is negative, cast to u32 for `% 20`) and
  `0xFFFF_FFFF`;
* a SHA-1 digest byte of `0x00` and `0xFF` at every digest index 0..19: the
  step uses index `dataCount % 20` for 0..3, and the XOR uses
  `(154504 + i) % 20`, which is 4..19 across the 16 output bytes;
* the 0x00/0xFF table bytes at the first two reads of `cmf_table_edge.cmf`.
"""

from __future__ import annotations

import hashlib
import random
import struct
from typing import List, Tuple

from emu_oracle import ArgSpec

TABLE_SIZE = 512
HEADER_LEN = 48
OUT_LEN = 16
BUILD = 154_504

CMF_STRING_INLINE = 16
CMF_STRING_OBJECT = 0x20


def iv_table() -> bytes:
    """The synthetic table: index 0 is 0x00 and index 255 is 0xFF."""
    return bytes(i & 0xFF for i in range(TABLE_SIZE))


TABLE = iv_table()

# `cmf_table_edge.cmf`: sha1[602 % 20] + 602 = 767; `767 & 511 == 255`, so
# with build 0 the first read is index 0 (0x00) and the second is index 255
# (0xFF). Found by scanning the deterministic data count, not copied.
_TABLE_EDGE_NAME = b"cmf_table_edge.cmf"
_TABLE_EDGE_COUNT = 602
_GRID_NAME = b"Synth_Region_RDEV_EExt.cmf"


def _sha1(name: bytes) -> bytes:
    return hashlib.sha1(name).digest()


def _name_with_digest_byte(index: int, value: int, salt: int) -> bytes:
    """A deterministic ASCII name whose SHA-1 byte at `index` is `value`."""
    n = 0
    while True:
        name = f"cmf_{salt}_{n}.cmf".encode("ascii")
        if _sha1(name)[index] == value:
            return name
        n += 1


def _cases() -> List[Tuple[bytes, int, int]]:
    cases: List[Tuple[bytes, int, int]] = [
        (b"Synth_Region_RDEV_EExt.cmf", BUILD, 1_234_567),
        (b"", 0, 0),
        (b"a", 0, 0),
        (b"0123456789abcde", 1, 19),  # 15 bytes: inline
        (b"0123456789abcdef", 32, 20),  # 16 bytes: heap
        (b"x" * 64, 0xFFFF_FFFF, 0xFFFF_FFFF),
        (b"y" * 200, 0x0800_0000, 0x7FFF_FFFF),  # cursor 0x8000_0000
        (b"z" * 17, 0x1000_0000, 0x8000_0000),  # cursor 0
        (b"w" * 16, 0x8000_0000, 0xFFFF_FFFF),  # cursor 0
        (b"v" * 23, 0xFFFF_FFFF, 1),  # cursor 0xFFFF_FFF0
        (_TABLE_EDGE_NAME, 0, _TABLE_EDGE_COUNT),
    ]
    for build in (
        0,
        1,
        32,
        0x0800_0000,
        0x1000_0000,
        0x7FFF_FFFF,
        0x8000_0000,
        0xFFFF_FFFF,
    ):
        for count in (
            0,
            19,
            20,
            21,
            39,
            40,
            41,
            0x7FFF_FFFF,
            0x8000_0000,
            0xFFFF_FFFF,
        ):
            cases.append((_GRID_NAME, build, count))
    for index in range(20):
        for value in (0x00, 0xFF):
            salt = index * 2 + (0 if value == 0 else 1)
            cases.append(
                (_name_with_digest_byte(index, value, salt), BUILD, index)
            )
    return cases


_PINNED = _cases()


def _string_object(name: bytes) -> Tuple[bytes, Tuple[int, ...]]:
    """The client's string object for `name`, plus its reloc slot.

    The capacity decides where the client reads the characters: inline at
    `+0` for the 16-byte buffer, or through the pointer at `+0`. The fixture
    writes the relative offset `0x20` in the pointer slot; the oracle rebases
    it into its arena and the probe reads the same relative value.
    """
    n = len(name)
    obj = bytearray(CMF_STRING_OBJECT + n)
    if n >= CMF_STRING_INLINE:
        struct.pack_into("<Q", obj, 0, CMF_STRING_OBJECT)
        struct.pack_into("<I", obj, 0x10, n)
        struct.pack_into("<I", obj, 0x14, n + 1)
        relocs: Tuple[int, ...] = (0,)
    else:
        obj[:n] = name
        struct.pack_into("<I", obj, 0x10, n)
        struct.pack_into("<I", obj, 0x14, CMF_STRING_INLINE)
        relocs = ()
    obj[CMF_STRING_OBJECT:] = name
    return bytes(obj), relocs


def _args(name: bytes, build: int, data_count: int, fill: int) -> List[ArgSpec]:
    obj, relocs = _string_object(name)
    header = bytearray(HEADER_LEN)
    struct.pack_into("<I", header, 0, build & 0xFFFF_FFFF)
    struct.pack_into("<I", header, 0x20, data_count & 0xFFFF_FFFF)
    return [
        ArgSpec(kind="ptr", mem=obj, relocs=relocs),
        ArgSpec(kind="ptr", mem=bytes(header)),
        ArgSpec(kind="ptr", mem=bytes([fill]) * OUT_LEN),
        ArgSpec(kind="u64", bits=OUT_LEN),
    ]


_LENGTHS = [0, 1, 5, 14, 15, 16, 17, 23, 40, 80, 200]
_BUILDS = [
    0,
    1,
    16,
    32,
    0x0800_0000,
    0x1000_0000,
    0x7FFF_FFFF,
    0x8000_0000,
    0xFFFF_FFFF,
]
_COUNTS = [
    0,
    1,
    19,
    20,
    21,
    39,
    40,
    41,
    0x7FFF_FFFF,
    0x8000_0000,
    0xFFFF_FFFF,
]
_ALPHABET = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_."


def make_args(i: int) -> List[ArgSpec]:
    """The i-th input: a client string object, a header, an out buffer, len."""
    if i < len(_PINNED):
        name, build, data_count = _PINNED[i]
    else:
        rng = random.Random(0x1545_04 ^ i)
        name = bytes(rng.choice(_ALPHABET) for _ in range(rng.choice(_LENGTHS)))
        build = rng.choice(_BUILDS) if i % 3 else rng.getrandbits(32)
        data_count = rng.choice(_COUNTS) if i % 2 else rng.getrandbits(32)
    return _args(name, build, data_count, (i * 17 + 3) & 0xFF)
