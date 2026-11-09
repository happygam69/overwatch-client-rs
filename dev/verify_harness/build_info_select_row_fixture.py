"""Deterministic (rows, flavor) cases for `fn/BuildInfoSelectRow`.

The client's `BuildInfoSelectRow` (`0x1424aec30`) takes the table in RCX
(rows pointer at `+0x20`, count `u32` at `+0x28`) and the flavor pointer
in RDX (null allowed), and returns a row pointer or null. Rows are
0xb0 bytes each; Product is the qword at row `+0`, the active byte is at
`+0x10`. A null Product pointer compares as the empty string (the
default at `0x142f7aa00` starts with a zero byte). A null flavor or a
flavor whose first byte is zero takes the no-flavor path: every row must
carry the first row's Product and the last active row wins. Any other
flavor returns the first active row whose Product equals it, byte for
byte, case-sensitively.

Surface: 2 register args (RCX table, RDX flavor as a u64 arena address,
0 for null - so a null flavor is representable). One 0x4000-byte blob
holds the 0x30-byte header at 0, the rows at 0x1000, the Product
strings at 0x2000 and the flavor string at 0x2800. The header's rows
field at `+0x20` holds the relative offset 0x1000; the ArgSpec
`relocs=(0x20,)` rebases it on the oracle side. Product pointers are
absolute arena addresses (`ARENA_BASE + offset`) or 0 for null. The
function writes nothing, so every other byte is sentinel and must come
back unchanged.

Lengths and counts stay tied: the count never exceeds the rows the
blob holds (at most 8 rows), and every non-null Product/flavor pointer
names a NUL-terminated string inside the blob.
"""

from __future__ import annotations

from typing import List, Optional, Tuple

from emu_oracle import ARENA_BASE, ArgSpec

ROWS_OFFSET = 0x1000
STRINGS_OFFSET = 0x2000
FLAVOR_OFFSET = 0x2800
STRIDE = 0xB0
BLOB_LEN = 0x4000

# (rows, flavor) where a row is (product or None for a null pointer, active)
# and flavor is None for a null pointer or a string ("" = empty flavor).
Case = Tuple[List[Tuple[Optional[str], bool]], Optional[str]]


def _cases() -> List[Case]:
    return [
        # No rows: both flavor paths return null.
        ([], "pro"),
        ([], None),
        ([], ""),
        # One active row, flavor match / no match / case mismatch.
        ([("pro", True)], "pro"),
        ([("pro", True)], "dev"),
        ([("Pro", True)], "pro"),
        ([("pro", True)], "Pro"),
        # Prefixes are not matches, in either direction.
        ([("pro", True)], "pr"),
        ([("pr", True)], "pro"),
        # First active match wins; an inactive match is skipped.
        ([("pro", False), ("pro", True), ("pro", True)], "pro"),
        ([("pro", True), ("pro", True)], "pro"),
        # Only an inactive match: null.
        ([("pro", False)], "pro"),
        ([("dev", True), ("pro", False)], "pro"),
        # No flavor, uniform products: the last active row wins.
        ([("pro", True), ("pro", True), ("pro", False)], None),
        ([("pro", False), ("pro", True)], None),
        ([("pro", False), ("pro", False)], None),
        ([("pro", True)], None),
        # No flavor, first char shared but a later byte differs: the
        # walk at 0x1424aed41..0x1424aed53 runs and exits on the
        # mismatch fall-through to 0x1424aed55 (the loop-back site
        # 0x1424aed53 needs this to see its fall-through edge; the
        # uniform cases below only drive its taken loop-back edge).
        ([("pro", True), ("pxo", True)], None),
        ([("pro", True), ("pr", True)], None),
        # No flavor, mixed products: null, even when the differing row
        # is inactive (the compare runs before the active test).
        ([("pro", True), ("dev", True)], None),
        ([("pro", True), ("dev", False)], None),
        ([("pro", True), ("pro", True), ("dev", False)], None),
        ([("pro", False), ("dev", False)], None),
        # Empty flavor is the no-flavor path.
        ([("pro", True), ("pro", True)], ""),
        ([("pro", True), ("dev", True)], ""),
        ([("pro", False), ("pro", False)], ""),
        # Null Product pointers compare as the empty string.
        ([(None, True)], "pro"),
        ([(None, True), ("pro", True)], "pro"),
        ([(None, True), (None, True)], None),
        ([(None, False), (None, True)], None),
        ([(None, True), ("", True)], None),
        ([(None, True), ("pro", True)], None),
        # Empty Product strings.
        ([("", True)], "pro"),
        ([("", True), ("", True)], None),
        ([("", True)], ""),
        # A late first-char-only difference, and a second-byte one.
        ([("xro", True), ("pro", True)], "pro"),
        ([("pxo", True)], "pro"),
        # Many rows, match in the middle and at the end.
        (
            [("dev", True), ("dev", True), ("pro", True), ("dev", True)],
            "pro",
        ),
        (
            [("dev", True)] * 7 + [("pro", True)],
            "pro",
        ),
    ]


SPECIALS = _cases()

_PRODUCT_POOL: List[Optional[str]] = ["pro", "dev", "", "Pro", None, "pr", "xro"]


def _random(r: int) -> Case:
    count = r % 9
    rows: List[Tuple[Optional[str], bool]] = []
    for i in range(count):
        # Half the cases use one product everywhere so the no-flavor
        # path can reach its last-active return.
        product = (
            "pro" if r % 2 == 0 else _PRODUCT_POOL[(r + i) % len(_PRODUCT_POOL)]
        )
        rows.append((product, (r + i) % 3 != 0))
    flavor: Optional[str] = [None, "", "pro", "dev", "Pro", "pr"][r % 6]
    return (rows, flavor)


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _random(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    rows, flavor = make_case(i)
    blob = bytearray(b"\xA5" * BLOB_LEN)
    # Header: rows pointer as a relative offset (reloc rebases it on the
    # oracle side), count at +0x28.
    blob[0x20:0x28] = ROWS_OFFSET.to_bytes(8, "little")
    blob[0x28:0x2C] = len(rows).to_bytes(4, "little")
    string_off = STRINGS_OFFSET
    string_offsets = {}
    for index, (product, active) in enumerate(rows):
        at = ROWS_OFFSET + index * STRIDE
        # Mark the row body so a write through the row is visible.
        blob[at:at + STRIDE] = b"\xCC" * STRIDE
        blob[at + 0x10] = 1 if active else 0
        if product is not None:
            if product not in string_offsets:
                encoded = product.encode() + b"\x00"
                blob[string_off:string_off + len(encoded)] = encoded
                string_offsets[product] = string_off
                string_off += len(encoded)
            blob[at:at + 8] = (ARENA_BASE + string_offsets[product]).to_bytes(
                8, "little"
            )
        else:
            blob[at:at + 8] = (0).to_bytes(8, "little")
    if flavor is not None:
        encoded = flavor.encode() + b"\x00"
        blob[FLAVOR_OFFSET:FLAVOR_OFFSET + len(encoded)] = encoded
        flavor_addr = ARENA_BASE + FLAVOR_OFFSET
    else:
        flavor_addr = 0
    return [
        ArgSpec(kind="ptr", mem=bytes(blob), arena_offset=0, relocs=(0x20,)),
        ArgSpec(kind="u64", bits=flavor_addr),
    ]
