"""Deterministic (slots, clock, query) cases for `fn/PageSlotFind`.

The client's `PageSlotFind` (`0x1424fa440`) takes the page-cache object in RCX
(count u32 at `+8`, last-use clock u64 at `+0x20`, slot base u64 at `+0x28`),
the archive ordinal in RDX, the page offset in R8, the minimum stored length
in R9, and two stack arguments: a byte pointer to store the "usable" answer
through, and the address of a slot to exclude from the eviction choice. It
returns a slot pointer or null, and bumps the clock and the chosen slot's
stamp.

Surface: one 0x3000-byte blob. The cache object sits at 0 with the slot base
pointing at 0x100, sixteen `0x70`-byte slot records start there, and the
`out_usable` byte sits at 0x2000. The archive/page/length/exclude arguments
are plain u64s. Every byte of the blob is a sentinel except the fields the
function reads, so a write anywhere else is visible; the returned slot address
is compared as the return value.

Slot records (offsets the function reads): archive qword at `+8`, page qword
at `+0x10`, stamp qword at `+0x20`, stored length u32 at `+0x28`, the
"invalidated while loading" byte at `+0x2d` and the "busy or failed" byte at
`+0x2e`. `+0x2c` is never read by this function and is left as sentinel.
"""

from __future__ import annotations

from typing import List, Optional, Tuple

from emu_oracle import ARENA_BASE, ArgSpec

BLOB_LEN = 0x3000
SLOTS = 0x100
SLOT_STRIDE = 0x70
SLOT_SLOTS = 16
OUT_USABLE = 0x2000

# (archive, page, stamp, stored, invalid, failed)
Slot = Tuple[int, int, int, int, int, int]
# (clock, slots, archive, page, len, exclude index or None)
Case = Tuple[int, List[Slot], int, int, int, Optional[int]]


def _slot(
    archive: int = 0,
    page: int = 0,
    stamp: int = 0,
    stored: int = 0,
    invalid: int = 0,
    failed: int = 0,
) -> Slot:
    return (archive, page, stamp, stored, invalid, failed)


def _cases() -> List[Case]:
    return [
        # Empty table: null, nothing written but out_usable (already 0).
        (0, [], 1, 0x1000, 0x40, None),
        (7, [], 1, 0x1000, 0x40, 0),
        # A single matching slot that covers the request: usable 1.
        (0, [_slot(1, 0x1000, 0, 0x40)], 1, 0x1000, 0x40, None),
        (5, [_slot(1, 0x1000, 3, 0x40)], 1, 0x1000, 0x10, None),
        (5, [_slot(1, 0x1000, 3, 0)], 1, 0x1000, 0, None),
        # A match that is busy: still returned, with usable 0.
        (5, [_slot(1, 0x1000, 3, 0x40, 0, 1)], 1, 0x1000, 0x40, None),
        # A match with a short stored length and busy: the stored test fails
        # and the busy test makes it a match anyway (usable 0).
        (5, [_slot(1, 0x1000, 3, 0x10, 0, 1)], 1, 0x1000, 0x40, None),
        # A match with a short stored length that is idle: not a match, but a
        # candidate - the only slot, so it is chosen for reload.
        (5, [_slot(1, 0x1000, 3, 0x10)], 1, 0x1000, 0x40, None),
        # The archive matches but the page does not: candidate.
        (5, [_slot(1, 0x2000, 3, 0x40)], 1, 0x1000, 0x40, None),
        # The page matches but the archive does not: candidate.
        (5, [_slot(2, 0x1000, 3, 0x40)], 1, 0x1000, 0x40, None),
        # A slot invalidated while loading skips the match test entirely and
        # becomes a candidate.
        (5, [_slot(1, 0x1000, 3, 0x40, 1, 0)], 1, 0x1000, 0x40, None),
        # Three idle candidates: the largest clock - stamp wins (the oldest).
        (
            10,
            [_slot(9, 0x9000, 9, 0x40), _slot(9, 0x9000, 2, 0x40), _slot(9, 0x9000, 5, 0x40)],
            1,
            0x1000,
            0x40,
            None,
        ),
        # A distance tie: the lower index wins (strict `ja`).
        (
            10,
            [_slot(9, 0x9000, 4, 0x40), _slot(9, 0x9000, 4, 0x40), _slot(9, 0x9000, 4, 0x40)],
            1,
            0x1000,
            0x40,
            None,
        ),
        # The first candidate sits exactly on the clock, so its distance is 0:
        # the strict `ja` falls through with no candidate yet, and the
        # existing-candidate test is then not taken.
        (5, [_slot(9, 0x9000, 5, 0x40), _slot(9, 0x9000, 6, 0x40)], 1, 0x1000, 0x40, None),
        (0, [_slot(9, 0x9000, 0, 0x40), _slot(9, 0x9000, 1, 0x40)], 1, 0x1000, 0x40, None),
        (5, [_slot(9, 0x9000, 5, 0x40)], 1, 0x1000, 0x40, None),
        # The oldest slot is excluded: the next oldest wins.
        (
            10,
            [_slot(9, 0x9000, 9, 0x40, 0, 1), _slot(9, 0x9000, 1, 0x40), _slot(9, 0x9000, 5, 0x40)],
            1,
            0x1000,
            0x40,
            None,
        ),
        (
            10,
            [_slot(9, 0x9000, 1, 0x40), _slot(9, 0x9000, 9, 0x40), _slot(9, 0x9000, 5, 0x40)],
            1,
            0x1000,
            0x40,
            0,
        ),
        # Every slot busy: no candidate, null answer.
        (
            10,
            [_slot(9, 0x9000, 1, 0x40, 0, 1), _slot(9, 0x9000, 9, 0x40, 0, 1)],
            1,
            0x1000,
            0x40,
            None,
        ),
        # A match after a candidate was already picked: the match wins.
        (
            10,
            [_slot(9, 0x9000, 1, 0x40), _slot(1, 0x1000, 7, 0x40)],
            1,
            0x1000,
            0x40,
            None,
        ),
        # The only candidate is excluded: null.
        (10, [_slot(9, 0x9000, 1, 0x40)], 1, 0x1000, 0x40, 0),
        # The clock wraps against a stamp near the top of the range.
        (
            0,
            [_slot(9, 0x9000, 0xFFFFFFFFFFFFFFFF, 0x40), _slot(9, 0x9000, 0xFFFFFFFFFFFFFFFE, 0x40)],
            1,
            0x1000,
            0x40,
            None,
        ),
        # A clock that is behind every stamp: every distance wraps high, and
        # the smallest stamp is still the largest distance.
        (
            3,
            [_slot(9, 0x9000, 10, 0x40), _slot(9, 0x9000, 8, 0x40), _slot(9, 0x9000, 9, 0x40)],
            1,
            0x1000,
            0x40,
            None,
        ),
        # Sixteen slots, a match at the last one.
        (
            20,
            [_slot(9, 0x9000, i, 0x40) for i in range(15)]
            + [_slot(1, 0x1000, 99, 0x200)],
            1,
            0x1000,
            0x100,
            None,
        ),
        # Sixteen idle candidates: the first slot has the smallest stamp by a
        # wide margin.
        (
            100,
            [_slot(9, 0x9000, 0, 0x40)] + [_slot(9, 0x9000, 50 + i, 0x40) for i in range(15)],
            1,
            0x1000,
            0x40,
            None,
        ),
        # An excluded slot that also does not match: the exclusion is only
        # consulted in the candidate branch.
        (10, [_slot(1, 0x1000, 1, 0x40), _slot(9, 0x9000, 5, 0x40)], 1, 0x1000, 0x40, 1),
    ]


SPECIALS = _cases()


def _random(r: int) -> Case:
    count = r % (SLOT_SLOTS + 1)
    clock = (r * 7919) % 1000
    archive = 1 + r % 3
    page = 0x1000 * (1 + (r // 3) % 3)
    length = [0, 1, 0x40, 0x100][(r // 5) % 4]
    slots: List[Slot] = []
    for i in range(count):
        # Every third slot carries the queried key so the match branch runs.
        same = (r + i) % 3 == 0
        slots.append(
            _slot(
                archive if same else archive + 1 + (r + i) % 2,
                page if same else page + 0x1000,
                (r * 13 + i * 5) % 600,
                [0, 0x10, 0x40, 0x200][(r + i) % 4],
                (r + i) % 5 == 0,
                (r + i) % 4 == 0,
            )
        )
    exclude = None
    if r % 4 == 0 and count:
        exclude = (r // 4) % count
    return (clock, slots, archive, page, length, exclude)


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _random(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    clock, slots, archive, page, length, exclude = make_case(i)
    if len(slots) > SLOT_SLOTS:
        raise ValueError("fixture case has more slots than the table holds")
    blob = bytearray(b"\xD7" * BLOB_LEN)
    # The cache object: count at +8, clock at +0x20, slot base at +0x28.
    blob[0x00:0x08] = b"\xD7" * 8
    blob[0x08:0x0C] = len(slots).to_bytes(4, "little")
    blob[0x0C:0x20] = b"\xD7" * (0x20 - 0x0C)
    blob[0x20:0x28] = clock.to_bytes(8, "little")
    blob[0x28:0x30] = (ARENA_BASE + SLOTS).to_bytes(8, "little")
    blob[0x30:0x40] = b"\xD7" * 16
    for index, (s_archive, s_page, stamp, stored, invalid, failed) in enumerate(slots):
        at = SLOTS + index * SLOT_STRIDE
        blob[at:at + SLOT_STRIDE] = bytes(((index * 31 + k) & 0xFF) for k in range(SLOT_STRIDE))
        blob[at + 0x08:at + 0x10] = s_archive.to_bytes(8, "little")
        blob[at + 0x10:at + 0x18] = s_page.to_bytes(8, "little")
        blob[at + 0x20:at + 0x28] = stamp.to_bytes(8, "little")
        blob[at + 0x28:at + 0x2C] = stored.to_bytes(4, "little")
        blob[at + 0x2D] = invalid & 0xFF
        blob[at + 0x2E] = failed & 0xFF
    blob[OUT_USABLE] = 0x5A
    return [
        ArgSpec(kind="ptr", mem=bytes(blob), arena_offset=0),
        ArgSpec(kind="u64", bits=archive),
        ArgSpec(kind="u64", bits=page),
        ArgSpec(kind="u64", bits=length),
        ArgSpec(kind="u64", bits=ARENA_BASE + OUT_USABLE),
        ArgSpec(
            kind="u64",
            bits=(ARENA_BASE + SLOTS + exclude * SLOT_STRIDE) if exclude is not None else 0,
        ),
    ]
