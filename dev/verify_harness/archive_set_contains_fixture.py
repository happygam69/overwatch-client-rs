"""Deterministic (reader, archive) cases for `fn/ArchiveSetContains`.

The client's `ArchiveSetContains` (`0x1424fcc80`) is
`(reader /*RCX*/, archive_number /*EDX*/) -> bool (AL)`: it reads the
open-archive set pointer at `reader+0x238`, folds the archive number with the
splitmix64 finalizer, takes the bucket `((h >> 33) ^ (u32)h) & (bucket_count -
1)` and walks that bucket's chain (`-1` ends it) comparing the 8-byte keys.
It writes nothing.

Surface: one 0x3000-byte blob. The reader object sits at 0 with the set
pointer at `+0x238`; the `0x28`-byte set object is at 0x1000 (heads at `+0`,
keys at `+8`, next at `+0x10`, bucket count u32 at `+0x20`); the bucket heads
(u32) are at 0x1100, the keys (u64) at 0x1800 and the next indices (u32) at
0x2000. Every pointer is an absolute `ARENA_BASE + offset` address baked into
the blob, exactly as `page_slot_find_fixture` does. The archive number is a
plain u64 (the probe truncates it to the client's `EDX`).

Unset keys hold `0x1_0000_0000 + index`, a value no `u32` archive number can
equal, so a chain that reaches an untouched entry cannot match by accident.
Unset heads and next entries hold `-1`, the client's chain terminator.

The declared cases drive every conditional branch to both edges (null set,
null heads, head `-1`, key match, next `-1`), the hash and the bucket mask,
and the two inverted-check shapes a mutant can produce: an empty bucket whose
entry 0 holds the queried archive (an inverted head check walks to entry 0 and
answers true), and a chain that ends at `-1` while entry 0 holds the queried
archive (an inverted next check does the same). The random cases sweep the
bucket counts, chain lengths, collisions and mismatches.
"""

from __future__ import annotations

from typing import List, Optional, Tuple

from emu_oracle import ARENA_BASE, ArgSpec

BLOB_LEN = 0x3000
SET_AT = 0x1000
HEADS_AT = 0x1100
KEYS_AT = 0x1800
NEXT_AT = 0x2000
HEADS_ENTRIES = 256
KEYS_ENTRIES = 128
NEXT_ENTRIES = 128

# A key no u32 archive number can equal.
KEY_FILL = 0x1_0000_0000

# (set_present, heads_present, bucket_count, archive, heads, keys, next)
Case = Tuple[bool, bool, int, int, List[int], List[int], List[int]]


def _hash(archive_number: int) -> int:
    """The client's splitmix64 finalizer over the zero-extended archive."""
    h = archive_number & 0xFFFF_FFFF
    h ^= h >> 33
    h = (h * 0xFF51_AFD7_ED55_8CCD) & 0xFFFF_FFFF_FFFF_FFFF
    h ^= h >> 33
    h = (h * 0xC4CE_B9FE_1A85_EC53) & 0xFFFF_FFFF_FFFF_FFFF
    return h


def _bucket(archive_number: int, bucket_count: int) -> int:
    h = _hash(archive_number)
    return ((h >> 33) ^ (h & 0xFFFF_FFFF)) & (bucket_count - 1)


def _table(
    archive: int,
    bucket_count: int,
    chains: List[Tuple[int, List[int]]],
    *,
    set_present: bool = True,
    heads_present: bool = True,
) -> Case:
    """Build a case. `chains` is a list of `(bucket_index, [key, ...])`; each
    chain is placed at the next free entry index and its bucket head points at
    it. An empty chain leaves the bucket at `-1`."""
    heads = [0xFFFF_FFFF] * bucket_count
    keys = [KEY_FILL + i for i in range(KEYS_ENTRIES)]
    nxt = [0xFFFF_FFFF] * NEXT_ENTRIES
    at = 0
    for bucket, chain in chains:
        if chain:
            heads[bucket] = at
            for i, key in enumerate(chain):
                keys[at] = key
                nxt[at] = at + 1 if i + 1 < len(chain) else 0xFFFF_FFFF
                at += 1
    return (set_present, heads_present, bucket_count, archive, heads, keys, nxt)


def _case(archive: int, bucket_count: int, chain: List[int], **kw) -> Case:
    """A single chain in the queried archive's own bucket."""
    return _table(archive, bucket_count, [(_bucket(archive, bucket_count), chain)], **kw)


def _raw(
    set_present: bool,
    heads_present: bool,
    bucket_count: int,
    archive: int,
    heads: List[int],
    keys: List[int],
    nxt: List[int],
) -> Case:
    return (set_present, heads_present, bucket_count, archive, heads, keys, nxt)


def _empty_heads(bucket_count: int) -> List[int]:
    return [0xFFFF_FFFF] * bucket_count


def _fill_keys() -> List[int]:
    return [KEY_FILL + i for i in range(KEYS_ENTRIES)]


def _fill_next() -> List[int]:
    return [0xFFFF_FFFF] * NEXT_ENTRIES


def _cases() -> List[Case]:
    cases: List[Case] = []

    # --- null set / null heads: the two early exits ----------------------
    cases.append(_case(1, 16, [], set_present=False))
    cases.append(_case(1, 16, [], heads_present=False))

    # --- empty bucket: head == -1 -> false -------------------------------
    cases.append(_case(1, 16, []))
    cases.append(_case(1, 1, []))
    cases.append(_case(1, 256, []))

    # --- single entry: match and mismatch --------------------------------
    cases.append(_case(1, 16, [1]))
    cases.append(_case(1, 16, [2]))
    cases.append(_case(0, 16, [0]))
    cases.append(_case(0x3FE, 16, [0x3FE]))
    cases.append(_case(0x3FE, 16, [0x3FD]))

    # --- mutation discriminators -----------------------------------------
    # Empty bucket, but entry 0 holds the queried archive: an inverted head
    # check (`entry != -1`) walks to entry 0 and answers true.
    keys = _fill_keys()
    keys[0] = 1
    cases.append(_raw(True, True, 16, 1, _empty_heads(16), keys, _fill_next()))
    # Chain that ends at -1 while entry 0 holds the queried archive: an
    # inverted next check (`entry == -1`) loops back to entry 0 and answers
    # true. The head is entry 1 so the client itself answers false.
    keys = _fill_keys()
    keys[0] = 1
    keys[1] = 2
    nxt = _fill_next()
    nxt[1] = 0xFFFF_FFFF
    heads = _empty_heads(16)
    heads[_bucket(1, 16)] = 1
    cases.append(_raw(True, True, 16, 1, heads, keys, nxt))

    # --- two-entry chains -------------------------------------------------
    cases.append(_case(1, 16, [2, 1]))
    cases.append(_case(1, 16, [2, 3]))
    cases.append(_case(1, 16, [1, 2]))
    cases.append(_case(1, 16, [2, 1, 3]))
    cases.append(_case(1, 16, [2, 3, 1]))
    cases.append(_case(1, 16, [2, 3, 4]))

    # --- collisions in one bucket ----------------------------------------
    # bc=16: 0, 30 and 79 all fold to bucket 0.
    cases.append(_case(0, 16, [0, 30, 79]))
    cases.append(_case(30, 16, [0, 30, 79]))
    cases.append(_case(79, 16, [0, 30, 79]))
    cases.append(_case(0, 16, [30, 79, 0]))
    cases.append(_case(30, 16, [79, 0, 30]))
    # bc=256: 0, 170 and 568 all fold to bucket 0.
    cases.append(_case(170, 256, [0, 170, 568]))
    cases.append(_case(568, 256, [0, 170, 568]))
    cases.append(_case(0, 256, [170, 568, 0]))

    # --- bucket counts ----------------------------------------------------
    for bc in (1, 2, 4, 8, 32, 64, 128, 256):
        cases.append(_case(1, bc, [1]))
        cases.append(_case(1, bc, [2]))

    # --- two buckets in one table ----------------------------------------
    cases.append(_table(1, 16, [(_bucket(1, 16), [1]), (_bucket(2, 16), [2])]))
    cases.append(_table(2, 16, [(_bucket(1, 16), [1]), (_bucket(2, 16), [2])]))
    cases.append(
        _table(
            0x3FE,
            256,
            [(_bucket(0x3FE, 256), [0x3FE, 0x3FD]), (_bucket(7, 256), [7])],
        )
    )
    # The queried bucket is empty while another bucket holds a chain.
    cases.append(_table(1, 16, [(_bucket(2, 16), [2])]))

    # --- archive numbers at the u32 edges --------------------------------
    cases.append(_case(0xFFFF_FFFF, 16, [0xFFFF_FFFF]))
    cases.append(_case(0xFFFF_FFFF, 16, [0xFFFF_FFFE]))
    cases.append(_case(0x8000_0000, 16, [0x8000_0000]))

    return cases


SPECIALS = _cases()


def _random(r: int) -> Case:
    bucket_count = [1, 2, 4, 8, 16, 32, 64, 128, 256][r % 9]
    archive = (r * 2654435761 + 0x9E37_79B9) & 0xFFFF_FFFF
    length = (r // 9) % 4
    chain: List[int] = []
    for i in range(length):
        # Every other entry carries the queried archive, so both the match
        # and the chain-end answers occur.
        chain.append(archive if (r + i) % 2 == 0 else (archive + 1 + i) & 0xFFFF_FFFF)
    return _case(archive, bucket_count, chain)


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _random(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    set_present, heads_present, bucket_count, archive, heads, keys, nxt = make_case(i)
    blob = bytearray(b"\xD7" * BLOB_LEN)
    # The reader object: only the set pointer at +0x238 is read.
    blob[0x238:0x240] = (
        (ARENA_BASE + SET_AT).to_bytes(8, "little") if set_present else b"\x00" * 8
    )
    # The 0x28-byte set object.
    blob[SET_AT : SET_AT + 0x28] = bytes(((0x11 + k) & 0xFF) for k in range(0x28))
    blob[SET_AT : SET_AT + 8] = (
        (ARENA_BASE + HEADS_AT).to_bytes(8, "little") if heads_present else b"\x00" * 8
    )
    blob[SET_AT + 8 : SET_AT + 0x10] = (ARENA_BASE + KEYS_AT).to_bytes(8, "little")
    blob[SET_AT + 0x10 : SET_AT + 0x18] = (ARENA_BASE + NEXT_AT).to_bytes(8, "little")
    blob[SET_AT + 0x20 : SET_AT + 0x24] = bucket_count.to_bytes(4, "little")
    # The bucket heads.
    for index, value in enumerate(heads):
        at = HEADS_AT + index * 4
        blob[at : at + 4] = value.to_bytes(4, "little")
    # The keys.
    for index, value in enumerate(keys):
        at = KEYS_AT + index * 8
        blob[at : at + 8] = value.to_bytes(8, "little")
    # The next indices.
    for index, value in enumerate(nxt):
        at = NEXT_AT + index * 4
        blob[at : at + 4] = value.to_bytes(4, "little")
    return [
        ArgSpec(kind="ptr", mem=bytes(blob), arena_offset=0),
        ArgSpec(kind="u64", bits=archive),
    ]
