"""Deterministic (kind, found, chosen, entry, existing events) cases for
`fn/DynamicIndexEntrySink`.

The client's `DynamicIndexEntrySink` (`0x142520590`) takes the key-state
object in RCX, the type byte in DL and a pointer to a 16-byte entry record
`{u64 location, u32 size, u32 trailing}` in R8. It removes 7 from the type
byte (`dec eax; cmp eax, 6`) and jumps through the table at `0x1425206a8`:

- 1: found = 1, copy all 16 entry bytes to the chosen record at `+0x18`,
  clear the count at `+0x130`, then append the entry resident;
- 2: found = 1, copy all 16 bytes, clear the count;
- 3: found = 0, zero the chosen location and size only (the trailing dword
  at `+0x24` is preserved), clear the count;
- 4: call the log function with the logger at `[obj+0x08]`, clear the
  count, append the *current* chosen entry resident;
- 5: call the log function, clear the count;
- 6: append the entry resident (shares type 1's append tail, without the
  found/copy/clear prefix);
- 7: append the entry non-resident (its append normalizes immediately);
- anything else: return, changing nothing.

The appends are `DynamicIndexAppendInterval` (`0x142520700`), already
verified: a matrix with the resident flag produces `(start, +1)` /
`(start + (u32)len, -1)` or the two `0x40000000` magnitudes, the latter
tail-jumping to the normalize (`0x1425208d0`).

Surface: three positional args (RCX obj, DL kind as a u64, R8 entry).
The obj is one 0x4000-byte blob like the sibling units: the key-state
header at 0 (disabled logger at 0x400 whose level byte at +8 is 0, so
the type-4/5 log call returns after its cookie checks), the chosen
record at +0x18, the event buffer at 0x1000 (256 records), slots past
the count carrying a per-slot sentinel the client never writes. The
header's logger pointer (+0x08) and event base (+0x128) are relative
offsets rebased by `relocs=(0x08, 0x128)`. The entry is a separate
16-byte pointee; its trailing dword is varied so the `movups` copy on
types 1/2 and the type-3 preserve are pinned.

Restrictions (the sibling units' declared gaps): capacity is pinned at
256, past every append a case can do (`capacity > count + 1`, so the
grow path with its log + allocator vtable never runs); every appended
interval start/end and every pre-existing event start is distinct,
except where a resident append or a same-start non-resident pair makes
the outcome sort-free (a resident append never sorts, and a same-start
pair's normalize group sums commutatively); non-resident (type-7)
appends sit strictly after every pre-existing start, with the appended
pair itself strictly after, so the immediate normalize's sort is a
no-op - the same restriction `fn/DynamicIndexNormalizeIntervals`
declares. Counts stay tied: the declared count is exactly the number
of pre-existing event records the fixture writes.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ArgSpec

BLOB_LEN = 0x4000
LOGGER_OFFSET = 0x400
EVENTS_OFFSET = 0x1000
EVENTS_LEN = 0x1000
CAPACITY = 256

# (kind, found, chosen (loc, size, trailing), entry (loc, size, trailing),
#  existing events [(start, delta)])
Case = Tuple[int, bool, Tuple[int, int, int], Tuple[int, int, int], List[Tuple[int, int]]]

SENTINEL_CHOSEN = (0x9000000000000000, 0x30, 0x11111111)
JUNK_ENTRY = (0x9999999999999999, 0x77, 0x66666666)


def _cases() -> List[Case]:
    cases: List[Case] = []

    def add(kind, found, chosen, entry, existing):
        cases.append((kind, found, chosen, entry, list(existing)))

    pair = [(0x800, 1), (0x840, -1)]

    # Type 1: found set from false, the full 16-byte copy (the trailing
    # dword 0x22222222 replaces the chosen 0x11111111), the pre-existing
    # pair cleared, the resident append at slots 0/1.
    add(1, False, SENTINEL_CHOSEN, (0x1000, 0x40, 0x22222222), pair)
    # Type 1 with an empty buffer and a zero-size entry: the appended
    # pair shares a start with opposite deltas and is never sorted.
    add(1, True, SENTINEL_CHOSEN, (0x1000, 0, 0x33333333), [])
    # Type 1 over an empty buffer, maximum trailing dword.
    add(1, False, (0, 0, 0), (0x1000, 0x40, 0xFFFFFFFF), [])
    # Type 2: found set, 16-byte copy, count cleared, nothing appended -
    # the pre-existing slot 0 keeps its bytes past the new count of 0.
    add(2, False, SENTINEL_CHOSEN, (0x1000, 0x40, 0x44444444), pair)
    # Type 2 with an empty buffer.
    add(2, True, SENTINEL_CHOSEN, (0x1000, 0x40, 0x44444444), [])
    # Type 3: found cleared, location and size zeroed, the trailing
    # dword 0x55555555 preserved (the client writes only +0x18 and
    # +0x20), the pre-existing pair cleared, the entry arg ignored.
    add(3, True, (0x9000000000000000, 0x30, 0x55555555), (0x1000, 0x40, 0x66666666), pair)
    # Type 3 on an already-unfound state with an empty buffer: the
    # trailing dword is still preserved, the entry arg still ignored.
    add(3, False, (0x9000000000000000, 0x30, 0x77777777), JUNK_ENTRY, [])
    # Type 4: log call (disabled logger), count cleared, the *current*
    # chosen entry appended resident; found/chosen (with trailing
    # 0x88888888) preserved; the entry arg is ignored.
    add(4, True, (0x2000, 0x40, 0x88888888), JUNK_ENTRY, pair)
    # Type 4 on an unfound state appends the zero chosen entry and
    # leaves found clear; the zero pair shares a start, never sorted.
    add(4, False, (0, 0, 0x99999999), JUNK_ENTRY, [])
    # Type 4 with a zero-size chosen entry, found set.
    add(4, True, (0x2000, 0, 0x88888888), JUNK_ENTRY, [])
    # Type 5: log call, count cleared, everything else preserved.
    add(5, True, (0x2000, 0x40, 0xAAAAAAAA), JUNK_ENTRY, pair)
    # Type 5 with an empty buffer, found clear.
    add(5, False, SENTINEL_CHOSEN, JUNK_ENTRY, [])
    # Type 6: resident append at the incoming count; found and the
    # chosen record (trailing 0xBBBBBBBB) preserved; the entry's
    # trailing dword is irrelevant to the append.
    add(6, False, (0x9000000000000000, 0x30, 0xBBBBBBBB), (0x1000, 0x40, 0xCCCCCCCC), pair)
    # Type 6 with an empty buffer.
    add(6, True, SENTINEL_CHOSEN, (0x1000, 0x40, 0), [])
    # Type 6 with a wrapping interval end: start near the u64 top.
    add(6, False, SENTINEL_CHOSEN, (0xFFFFFFFFFFFFFF00, 0x100, 0), [])
    # Type 7: non-resident append after a resident pair; the immediate
    # normalize drops the non-resident pair (coverage goes negative and
    # back) and keeps the resident pair; found/chosen preserved.
    add(7, False, (0x9000000000000000, 0x30, 0xDDDDDDDD), (0x1000, 0x40, 0), pair)
    # Type 7 alone: the non-resident pair is dropped, count back to 0,
    # its sorted records stay as the tail.
    add(7, False, SENTINEL_CHOSEN, (0x1000, 0x40, 0), [])
    # Type 7 overlapping resident coverage: the normalize emits the
    # outer pair and the hole edges around the dropped coverage.
    add(7, True, SENTINEL_CHOSEN, (0x1100, 0x40, 0), [(0x1000, 1)])
    # Type 7 with a zero-size entry: the same-start pair sums to zero
    # in one normalize group and is dropped; order never matters.
    add(7, False, SENTINEL_CHOSEN, (0x1000, 0, 0), pair)
    # Default exit: types 0, 8 and 255 change nothing at all - found,
    # chosen (trailing included) and the buffer are untouched.
    add(0, True, (0x2000, 0x40, 0xEEEEEEEE), JUNK_ENTRY, pair)
    add(8, False, SENTINEL_CHOSEN, JUNK_ENTRY, pair)
    add(255, True, SENTINEL_CHOSEN, JUNK_ENTRY, [])
    # The type byte is read with `movzx eax, dl`: only the low byte is
    # the type. 0x107 dispatches as 7, 0x1ff as the default.
    add(0x107, False, SENTINEL_CHOSEN, (0x1000, 0x40, 0), [])
    add(0x1FF, True, (0x2000, 0x40, 0xEEEEEEEE), JUNK_ENTRY, pair)
    # Type 1 reaches the same append tail as type 6 (jump-table entry
    # 0x1425205d3 is inside the type-1 block); the two cases above pin
    # the shared tail's resident flag from both entries.
    return cases


SPECIALS = _cases()


def _random(r: int) -> Case:
    kind = r % 10
    # Pre-existing events: a normalized resident pair (or two), starts
    # well below every appended start.
    count = r % 3
    existing = []
    for j in range(count):
        existing.append((0x800 + j * 0x100, 1))
        existing.append((0x840 + j * 0x100, -1))
    chosen_loc = 0x9000 + (r % 0x100) * 0x100
    chosen = (chosen_loc, 0x30, 0x10000000 + r)
    entry_loc = 0x20000 + r * 0x200
    entry = (entry_loc, 0x40, 0x20000000 + r)
    return (kind, (r % 2) == 0, chosen, entry, existing)


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _random(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    kind, found, chosen, entry, existing = make_case(i)
    blob = bytearray(b"\xA5" * BLOB_LEN)
    # Key-state header: logger (reloc 0x08) and events base (reloc 0x128)
    # are relative offsets; the disabled logger's level at 0x408 is 0.
    blob[LOGGER_OFFSET : LOGGER_OFFSET + 32] = bytes(32)
    blob[0x08:0x10] = LOGGER_OFFSET.to_bytes(8, "little")
    blob[0x10] = 1 if found else 0
    blob[0x18:0x20] = chosen[0].to_bytes(8, "little")
    blob[0x20:0x24] = chosen[1].to_bytes(4, "little")
    blob[0x24:0x28] = chosen[2].to_bytes(4, "little")
    blob[0x128:0x130] = EVENTS_OFFSET.to_bytes(8, "little")
    blob[0x130:0x134] = len(existing).to_bytes(4, "little")
    blob[0x134:0x138] = CAPACITY.to_bytes(4, "little")
    # Events: a per-slot sentinel record (distinct start, uniform
    # padding) so an untouched slot is recognisable and a moved pad is
    # not; the declared pre-existing events overwrite the front slots.
    for slot in range(EVENTS_LEN // 16):
        at = EVENTS_OFFSET + slot * 16
        blob[at : at + 8] = (0xA5A5A5A5A5A5A5A5 ^ slot).to_bytes(8, "little")
        blob[at + 8 : at + 12] = (0xA5A5A5A5).to_bytes(4, "little")
        blob[at + 12 : at + 16] = b"\xCC" * 4
    for index, (start, delta) in enumerate(existing):
        at = EVENTS_OFFSET + index * 16
        blob[at : at + 8] = start.to_bytes(8, "little")
        blob[at + 8 : at + 12] = (delta & 0xFFFFFFFF).to_bytes(4, "little")
    entry_bytes = (
        entry[0].to_bytes(8, "little")
        + entry[1].to_bytes(4, "little")
        + entry[2].to_bytes(4, "little")
    )
    return [
        ArgSpec(kind="ptr", mem=bytes(blob), arena_offset=0, relocs=(0x08, 0x128)),
        ArgSpec(kind="u64", bits=kind),
        ArgSpec(kind="ptr", mem=entry_bytes, arena_offset=0x4000),
    ]
