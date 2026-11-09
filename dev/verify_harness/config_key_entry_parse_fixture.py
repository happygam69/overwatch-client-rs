"""Deterministic (key span, value span, vector) cases for
`fn/ConfigKeyEntryParse`.

The client's `ConfigKeyEntryParse` (`0x1424cebd0`) takes the context object in
RCX (a qword at `+8` points at the entry vector), the key span's begin in RDX
and end in R8, the value span's begin in R9 and end as the one stack argument.
It appends one 24-byte `{u64 id, [u8; 16] key}` entry to the vector and
returns the line disposition in AL: `0` handled, `1` not handled.

One 0x3000-byte blob holds the context at 0 (only `+8` is read), the vector
object at 0x40 (base at `+8`, count `u32` at `+0x10`, capacity `u32` at
`+0x14`), the entry array at 0x1000, the key text at 0x2000 and the value text
at 0x2200. All pointer fields are absolute arena addresses (`ARENA_BASE +
offset`) so both sides see identical bytes - no relocation is used and the
whole blob is one compared channel.

Lengths and counts stay tied: the spans always lie inside the text buffers,
`count <= capacity`, and the entry array carries 32 slots so an append at
`count` is always inside the blob. The grow path (`count == capacity`) is the
unit's declared gap - the client reallocates through an allocator vtable and
derives the new capacity from eight stack bytes it never initialises - so
every case keeps `count < capacity`.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ARENA_BASE, ArgSpec

CTX = 0x0000
VEC = 0x0040
ENTRIES = 0x1000
KEY_BUF = 0x2000
VAL_BUF = 0x2200
BLOB_LEN = 0x3000
ENTRY_SIZE = 24
ENTRY_SLOTS = 32

HEX16 = b"0011223344556677"
HEX32 = b"0123456789abcdef0123456789abcdef"
# 32 hex digits behind two spaces, for the value's whitespace skip.
PADDED = b"   0123456789abcdef0123456789abcdef"

# (key text, key span, value text, value span, count, capacity). A span is a
# (begin, end) pair of offsets inside its buffer.
Case = Tuple[bytes, Tuple[int, int], bytes, Tuple[int, int], int, int]


def _c(
    key: bytes,
    value: bytes = b"",
    count: int = 0,
    capacity: int = 8,
    key_span: Tuple[int, int] | None = None,
    val_span: Tuple[int, int] | None = None,
) -> Case:
    return (
        key,
        key_span if key_span is not None else (0, len(key)),
        value,
        val_span if val_span is not None else (0, len(value)),
        count,
        capacity,
    )


def _cases() -> List[Case]:
    good_key = b"key-" + HEX16
    return [
        # The first byte is not 'k'.
        _c(b"xey-0011223344556677", HEX32),
        _c(b"Key-0011223344556677", HEX32),
        _c(b" key-0011223344556677", HEX32),
        # Empty span: the client reads one byte anyway; the length test
        # rejects it either way.
        _c(b"key-0011223344556677", HEX32, key_span=(0, 0)),
        # Five bytes or fewer: "key" (3) and "key-" (4) are both too short.
        _c(b"key", HEX32),
        _c(b"key-", HEX32),
        # Long enough after the 'k' but the four-byte prefix is not "key-".
        _c(b"kez-0011223344556677", HEX32),
        _c(b"kex-0011223344556677", HEX32),
        # Prefix ok, token empty: the token scan stops on a space or a tab
        # immediately after "key-", so the id parse never runs and the entry
        # still grows by one zeroed record.
        _c(b"key- ", HEX32),
        _c(b"key-\t0011223344556677", HEX32),
        _c(b"key-\r0011223344556677", HEX32),
        # Prefix ok, token reaches the key span's end (no whitespace).
        _c(good_key, HEX32),
        _c(b"key-0011223344556677XYZ", HEX32, key_span=(0, 20)),
        # The token is cut short by the span end mid-id. The span end at 19
        # leaves 15 hex digits, one short of the eight bytes the id parser
        # wants, so the record stays zeroed - a scan that stepped one byte
        # past the span end would complete it and fill the record.
        _c(good_key, HEX32, key_span=(0, 19)),
        _c(good_key, HEX32, key_span=(0, 18)),
        _c(good_key, HEX32, key_span=(0, 10)),
        # The same cut, with a space (and a tab) just past the span end.
        _c(b"key-0011223344556677 xy", HEX32, key_span=(0, 19)),
        _c(b"key-0011223344556677\txy", HEX32, key_span=(0, 10)),
        # An inverted key span: the 32-bit pointer difference is huge, so the
        # length test passes on the four `key-` bytes at the begin and the
        # 64-bit `key_begin + 4 < key_end` test then fails - the line appends
        # one zeroed entry and returns 1.
        _c(b"xx" + good_key, HEX32, key_span=(2, 0)),
        _c(b"xxxxx" + good_key, HEX32, key_span=(5, 1)),
        # Leading whitespace inside the span before the id (space, tab, run).
        _c(b"key- 0011223344556677", HEX32),
        _c(b"key-\t\t0011223344556677", HEX32),
        _c(b"key-   0011223344556677", HEX32),
        # The id token is not hex at all, or too short for eight bytes.
        _c(b"key-xyz", HEX32),
        _c(b"key-0011", HEX32),
        _c(b"key-00112233445566778899aabbccddeeff", HEX32),
        # ---------------------------------------------------------------
        # A good id, then every value shape.
        # Empty value span, and an inverted one (both return 0).
        _c(good_key, b""),
        _c(good_key, HEX32, val_span=(0, 0)),
        _c(good_key, b"xx" + HEX32, val_span=(2, 0)),
        # Whitespace-only value: the skip runs off the end and returns 0.
        _c(good_key, b"   "),
        _c(good_key, b"\t\n\r "),
        # Leading whitespace, then 32 hex digits: the full parse, return 1.
        _c(good_key, PADDED),
        # 31 hex digits (too short) and 33 (the parser stops at 32).
        _c(good_key, HEX32[:-1]),
        _c(good_key, HEX32 + b"f"),
        _c(good_key, b"  " + HEX32[:-1]),
        # A non-hex byte inside the value, and one at the front.
        _c(good_key, b"0123456789abcdef0123456789abcdeg"),
        _c(good_key, b"x123456789abcdef0123456789abcdef"),
        # Upper case is hex too.
        _c(good_key, HEX32.upper()),
        # A value that is pure whitespace after a tab, and one that is a
        # single space then hex.
        _c(good_key, b"\t"),
        _c(good_key, b" " + HEX32),
        # ---------------------------------------------------------------
        # Vector positions: the append lands at count * 24.
        _c(good_key, HEX32, count=1),
        _c(good_key, HEX32, count=3),
        _c(good_key, HEX32, count=3, capacity=4),
        _c(good_key, HEX32, count=7, capacity=8),
        _c(good_key, HEX32, count=31, capacity=32),
        _c(good_key, b"   ", count=31, capacity=32),
        # A count ABOVE the capacity: the client never grows (the two compares
        # need equality) and appends at count * 24 anyway, which the 32 entry
        # slots of the blob can hold. The shape the client's invariant forbids
        # is reachable in memory, so it is part of the domain.
        _c(good_key, HEX32, count=5, capacity=3),
        _c(good_key, HEX32, count=9, capacity=1),
        _c(good_key, HEX32, count=5, capacity=4),
        # A failing line still appends.
        _c(b"xey-0011223344556677", HEX32, count=5, capacity=8),
        _c(b"key- ", HEX32, count=5, capacity=8),
        _c(b"key-0011", HEX32, count=5, capacity=8),
    ]


SPECIALS = _cases()

_KEY_POOL = [
    b"key-0011223344556677",
    b"key-" + b"deadbeefdeadbeef",
    b"key-0123456789abcdef",
    b"key- 0011223344556677",
    b"key-\t0011223344556677",
    b"key-\n0011223344556677",
    b"key-0011",
    b"key-00",
    b"key-",
    b"key",
    b"k",
    b"",
    b"xyz-0011223344556677",
    b"kez-0011223344556677",
    b"key-0011223344556677extra",
    b"key-00112233445566778899aabbccddeeff",
]
_VAL_POOL = [
    b"0123456789abcdef0123456789abcdef",
    HEX32.upper(),
    b"   " + HEX32,
    b"\t" + HEX32,
    HEX32[:-1],
    HEX32 + b"0",
    b"",
    b" ",
    b"\t\r\n\v\f",
    b"zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
    b"0123456789abcde",
    b"0123456789abcdef0123456789abcde0",
]


def _random(r: int) -> Case:
    key = _KEY_POOL[r % len(_KEY_POOL)]
    value = _VAL_POOL[(r // len(_KEY_POOL)) % len(_VAL_POOL)]
    capacity = 1 + r % 31
    count = r // 3 % capacity
    # Half the random cases cut the key span somewhere inside the text, which
    # is how the client's two pointers bound the token scan; every seventh
    # case inverts the span instead.
    key_span = None
    if r % 2 == 1 and key:
        cut = 1 + (r // 7) % len(key)
        key_span = (0, cut)
    if r % 7 == 3 and len(key) > 1:
        key_span = (1, 0)
    val_span = None
    if r % 5 == 0:
        cut = (r // 3) % (len(value) + 1)
        val_span = (cut, len(value))
    return (key, key_span or (0, len(key)), value, val_span or (0, len(value)), count, capacity)


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _random(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    key, (kb, ke), value, (vb, ve), count, capacity = make_case(i)
    if max(kb, ke) > len(key) or max(vb, ve) > len(value):
        raise ValueError("fixture case is inconsistent")
    if count >= ENTRY_SLOTS:
        raise ValueError("fixture case appends past the entry slots")
    if kb + 4 > BLOB_LEN - KEY_BUF:
        raise ValueError("fixture case has no room for the four key bytes")
    blob = bytearray(b"\xA5" * BLOB_LEN)
    # The context: one pointer at +8, the vector object.
    blob[CTX + 0x08:CTX + 0x10] = (ARENA_BASE + VEC).to_bytes(8, "little")
    # The vector object: allocator (never read on this path), base, count,
    # capacity. The tail stays sentinel so a write past +0x18 shows.
    vec = VEC
    blob[vec:vec + 8] = (0).to_bytes(8, "little")
    blob[vec + 0x08:vec + 0x10] = (ARENA_BASE + ENTRIES).to_bytes(8, "little")
    blob[vec + 0x10:vec + 0x14] = count.to_bytes(4, "little")
    blob[vec + 0x14:vec + 0x18] = capacity.to_bytes(4, "little")
    # Every entry slot is a distinct pattern, so a write lands somewhere
    # visible and a zeroed record is distinguishable from the fill.
    for slot in range(ENTRY_SLOTS):
        at = ENTRIES + slot * ENTRY_SIZE
        blob[at:at + ENTRY_SIZE] = bytes(((slot * 37 + k * 11) & 0xFF) for k in range(ENTRY_SIZE))
    blob[KEY_BUF:KEY_BUF + len(key)] = key
    blob[VAL_BUF:VAL_BUF + len(value)] = value
    return [
        ArgSpec(kind="ptr", mem=bytes(blob), arena_offset=0),
        ArgSpec(kind="u64", bits=ARENA_BASE + KEY_BUF + kb),
        ArgSpec(kind="u64", bits=ARENA_BASE + KEY_BUF + ke),
        ArgSpec(kind="u64", bits=ARENA_BASE + VAL_BUF + vb),
        ArgSpec(kind="u64", bits=ARENA_BASE + VAL_BUF + ve),
    ]
