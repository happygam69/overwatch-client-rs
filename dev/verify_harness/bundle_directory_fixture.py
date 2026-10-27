"""Deterministic synthetic bundle directories for the `fn/BundleDirectoryLookup`
unit.

The client's `BundleDirectoryLookup` (`0x141fd5160`) walks the content
database's per-type directory:

```text
directory           +0x18 u64  type-entry array (absolute guest pointer)
                    +0x20 i32  type count
type entry (0x20)   +0x00 i32  type key (the asset type id)
                    +0x08 u64  type block
                    +0x10 u64  bundle handle array
type block          +0x00 u64  group array
                    +0x08 u32  group stride (not read by the lookup)
                    +0x0c u16  group count
                    +0x0e u16  type id (not read by the lookup)
group (0x28)        +0x00 u64  listed GUID array (not read by the lookup)
                    +0x08 u64  sorted GUID array
                    +0x10 u64  index array, one u16 per sorted GUID
                    +0x18 u64  the bundle's own GUID
                    +0x20 i32  sorted GUID count
                    +0x24 i16  listed GUID count
```

Every pointer is an absolute guest address. The fixture builds the directory
object at its real image address (`DAT_1441b3b88`, v2b frame) because the
client also reads the type-entry pointer through that fixed global: the
unit's `state` block covers the object and its structure pool, and the
directory's absolute pointers are written into that state by guest pointer
arguments (the oracle rebases the relative values the fixture stores; the
probe does the same). No byte comes from the install. The only global the
function reads outside the blob is the loaded-bundle set via `FUN_141fcbf40`;
every group here names bundle GUID `0`, which that function answers without
touching the set.

Case order matters: the first block is the pinned corpus that reaches every
reachable branch the client has, and the generator after it keeps the edge
mix broad.
"""

from __future__ import annotations

import random
import struct
from typing import Dict, List, Optional, Sequence, Tuple

from emu_oracle import V2B_BASE, ArgSpec

# The directory object's image address in the v2b frame. The client reads
# `[directory+0x18]` and also loads it through the fixed global at that exact
# address, so the directory must sit here for the two reads to name one array.
DIR_GUEST = V2B_BASE + (0x1441_B3B88 - 0x1400_00000)
POOL_OFF = 0x28
OUT_OFF = 0x1F00
STATE_LEN = 0x2000
POOL_LIMIT = OUT_OFF - POOL_OFF
M64 = (1 << 64) - 1


def guid_type_key(guid: int) -> int:
    """The client's GUID-to-type key (`BundleDirectoryLookup` prologue).

    It is the asset type id: the bit-reversed 12-bit field at GUID bits
    48..59, plus one, sign-extended. `guid::type_of` is the same value except
    for field 0xFFF, where the client's 12-bit sign extension wraps to 0.
    """
    a = ((guid & 0xFFFF_0000_0000_0000) << 4) & M64
    s = a >> 1
    x = (((a << 1) & M64) ^ s) & 0xAAAA_AAAA_AAAA_AAAA
    x ^= s
    s = x >> 2
    y = (((x << 2) & M64) ^ s) & 0xCCCC_CCCC_CCCC_CCCC
    y ^= s
    s = y >> 4
    z = (((y << 4) & M64) ^ s) & 0xF0F0_F0F0_F0F0_F0F0
    z ^= s
    w = (int.from_bytes(z.to_bytes(8, "little"), "big") + 1) & 0xFFFF_FFFF
    w = (w << 20) & 0xFFFF_FFFF
    if w & 0x8000_0000:
        w -= 1 << 32
    return w >> 20


class Pool:
    """The structure region after the directory object.

    A pointer value stored at a reloc slot is an offset from the pool's base;
    `make_args` declares those slots, and both loaders add the pool's guest
    address.
    """

    def __init__(self) -> None:
        self.base = DIR_GUEST + POOL_OFF
        self.buf = bytearray()
        self.relocs: List[int] = []

    def _align(self, align: int) -> None:
        while (self.base + len(self.buf)) % align:
            self.buf.append(0)

    def alloc(self, size: int, align: int = 8) -> int:
        self._align(align)
        off = len(self.buf)
        self.buf += b"\x00" * size
        return off

    def w64(self, at: int, value: int) -> None:
        struct.pack_into("<Q", self.buf, at, value & M64)

    def w32(self, at: int, value: int) -> None:
        struct.pack_into("<I", self.buf, at, value & 0xFFFF_FFFF)

    def w16(self, at: int, value: int) -> None:
        struct.pack_into("<H", self.buf, at, value & 0xFFFF)

    def w8(self, at: int, value: int) -> None:
        self.buf[at] = value & 0xFF

    def ptr(self, at: int, target_off: int) -> None:
        self.relocs.append(at)
        self.w64(at, target_off)

    def addr(self, off: int) -> int:
        return self.base + off


def _handle_row(pool: Pool, spec: Dict) -> int:
    """One handle-array value, building its object if the spec has one.

    A spec is `{"tag": 0..0x3f, "loaded": bool, "bits": status bits,
    "obj_tag": override}`; `{"handle": value}` writes a raw handle instead
    (`-1`, a tag-only value, or anything else the client must classify).
    `loaded_byte` overrides `obj+0xa` and `dword` overrides the whole `obj+8`
    dword, for the bits the client does not read but a mutant might.
    """
    if "handle" in spec:
        return spec["handle"] & M64
    obj = pool.alloc(0x40, 0x40)
    tag = spec.get("tag", 0x21) & 0x3F
    obj_tag = spec.get("obj_tag", tag) & 0x3F
    dword = spec.get("dword")
    if dword is None:
        dword = obj_tag | spec.get("bits", 0)
        loaded_byte = spec.get("loaded_byte")
        if loaded_byte is None:
            loaded_byte = 1 if spec.get("loaded", True) else 0
        dword = (dword & ~0xFF_0000) | ((loaded_byte & 0xFF) << 16)
    pool.w32(obj + 8, dword)
    return pool.addr(obj) | tag


def _build(case: Dict) -> Tuple[bytes, bytes, Tuple[int, ...]]:
    """Build the 0x28-byte directory object and the pool for one case.

    Two sentinel structures sit just past the arrays the client may read: one
    extra type entry after the type array and one extra group after each type's
    group array. Both name the target and resolve to a status-4 handle, and the
    client's own count checks stop before them. A port that runs one entry past
    a count therefore reads a match it cannot justify, which is how the count
    bounds become observable.
    """
    pool = Pool()
    types = case["types"]
    t_off = pool.alloc((len(types) + 1) * 0x20, 0x10)
    type_sentinel = t_off + len(types) * 0x20
    # (block, group array, handle array, rows before the group sentinel)
    layout = []
    for t in types:
        block = pool.alloc(0x18)
        groups = t["groups"]
        group_off = pool.alloc((len(groups) + 1) * 0x28)
        running = 0
        rows = 0
        for g in groups:
            index = g.get("index")
            if index is None:
                index = list(range(len(g["sorted"])))
            rows = max(rows, running + max(index, default=0) + 1)
            running += g.get("listed_count", len(g["sorted"]))
        # The group sentinel's handle slot is one past every real row; the
        # array also has to hold the case's own handle specs.
        slot = max(rows, running, len(t.get("handles", [])))
        handles_off = pool.alloc((slot + 1) * 8)
        layout.append((block, group_off, handles_off, slot, running))
    group_arrays = []
    group_sentinels = []
    for t in types:
        arrays = []
        for g in t["groups"]:
            listed = pool.alloc(max(len(g.get("listed", g["sorted"])), 1) * 8)
            sorted_off = pool.alloc(max(len(g["sorted"]), 1) * 8)
            index = g.get("index")
            if index is None:
                index = list(range(len(g["sorted"])))
            index_off = pool.alloc(max(len(index), 1) * 2, 2)
            arrays.append((listed, sorted_off, index_off))
        group_arrays.append(arrays)
        group_sentinels.append((pool.alloc(8), pool.alloc(2, 2)))
    handle_values: List[List[int]] = []
    for t, (block, group_off, handles_off, slot, running) in zip(types, layout):
        specs = list(t.get("handles", []))
        while len(specs) < slot:
            specs.append({"tag": 0x21, "loaded": True})
        handle_values.append([_handle_row(pool, spec) for spec in specs])
        handle_values[-1].append(_handle_row(pool, {"tag": 0x3E, "loaded": True}))

    # Fill the type entries, blocks, groups and arrays.
    for ti, (t, (block, group_off, handles_off, slot, running)) in enumerate(
        zip(types, layout)
    ):
        entry = t_off + ti * 0x20
        key = t.get("key", guid_type_key(case["target"]))
        pool.w32(entry, key & 0xFFFF_FFFF)
        pool.ptr(entry + 8, block)
        pool.ptr(entry + 0x10, handles_off)
        pool.ptr(block, group_off)
        pool.w32(block + 8, t.get("stride", 0x28))
        pool.w16(block + 0x0c, len(t["groups"]))
        pool.w16(block + 0x0e, t.get("type", 0x9C))
        for gi, g in enumerate(t["groups"]):
            g_at = group_off + gi * 0x28
            listed, sorted_off, index_off = group_arrays[ti][gi]
            pool.ptr(g_at, listed)
            pool.ptr(g_at + 8, sorted_off)
            pool.ptr(g_at + 0x10, index_off)
            pool.w64(g_at + 0x18, g.get("bundle", 0))
            pool.w32(g_at + 0x20, g.get("count", len(g["sorted"])) & 0xFFFF_FFFF)
            pool.w16(g_at + 0x24, g.get("listed_count", len(g["sorted"])) & 0xFFFF)
            for i, guid in enumerate(g["sorted"]):
                pool.w64(sorted_off + i * 8, guid)
            index = g.get("index")
            if index is None:
                index = list(range(len(g["sorted"])))
            for i, value in enumerate(index):
                pool.w16(index_off + i * 2, value)
        # Group sentinel: a loaded group holding the target, one past the
        # declared count. Its listed count is 0, so its handle slot is the
        # running total plus its own index.
        sentinel_group = group_off + len(t["groups"]) * 0x28
        sentinel_sorted, sentinel_index = group_sentinels[ti]
        pool.w64(sentinel_group, 0)
        pool.ptr(sentinel_group + 8, sentinel_sorted)
        pool.ptr(sentinel_group + 0x10, sentinel_index)
        pool.w64(sentinel_group + 0x18, 0)
        pool.w32(sentinel_group + 0x20, 1)
        pool.w16(sentinel_group + 0x24, 0)
        pool.w64(sentinel_sorted, case["target"] & M64)
        pool.w16(sentinel_index, slot - running)
        for i, value in enumerate(handle_values[ti]):
            pool.w64(handles_off + i * 8, value)

    if len(pool.buf) > POOL_LIMIT:
        raise AssertionError(
            f"case pool {len(pool.buf):#x} exceeds {POOL_LIMIT:#x}; shrink the case"
        )
    # The type sentinel: the target's key, pointing at the first type's block
    # and handle array when there is one. The client stops at the type count
    # and never reads it; a scan that runs one entry past the count finds a
    # match that resolves to a different answer.
    pool.w32(type_sentinel, guid_type_key(case["target"]) & 0xFFFF_FFFF)
    if layout:
        block0, _, handles0, _, _ = layout[0]
        pool.ptr(type_sentinel + 8, block0)
        pool.ptr(type_sentinel + 0x10, handles0)
    else:
        pool.w64(type_sentinel + 8, 0)
        pool.w64(type_sentinel + 0x10, 0)
    directory = bytearray(0x28)
    struct.pack_into("<i", directory, 0x20, len(types))
    # D+0x18 is the type-entry array, relative to the directory's own base.
    struct.pack_into("<Q", directory, 0x18, (POOL_OFF + t_off) & M64)
    return bytes(directory), bytes(pool.buf), tuple(pool.relocs)


# ---------------------------------------------------------------------------
# Pinned corpus
# ---------------------------------------------------------------------------

G = 0x0D90_847B_FC92_C67C  # a type-0x09C GUID; guid_type_key(G) == 0x09C
G1 = G + 0x10
G2 = G + 0x20
G3 = G + 0x30
G8 = G + 0x80
K = guid_type_key(G)


def _one(
    target: int,
    sorted_guids: Sequence[int],
    index: Optional[Sequence[int]] = None,
    *,
    key: Optional[int] = None,
    listed_count: Optional[int] = None,
    handle: Optional[Dict] = None,
    bundle: int = 0,
    stride: int = 0x28,
    type_id: int = 0x9C,
) -> Dict:
    """One type, one group, one handle array."""
    group: Dict = {"bundle": bundle, "sorted": list(sorted_guids)}
    if index is not None:
        group["index"] = list(index)
    if listed_count is not None:
        group["listed_count"] = listed_count
    return {
        "target": target,
        "types": [
            {
                "key": K if key is None else key,
                "type": type_id,
                "stride": stride,
                "groups": [group],
                "handles": [handle or {"tag": 0x21, "loaded": True}],
            }
        ],
    }


def _two_groups(
    target: int,
    first: Dict,
    second: Dict,
    handles: Sequence[Dict],
    *,
    key: Optional[int] = None,
) -> Dict:
    return {
        "target": target,
        "types": [
            {
                "key": K if key is None else key,
                "groups": [first, second],
                "handles": list(handles),
            }
        ],
    }


_PINNED: List[Dict] = [
    # --- one type, one group, one row: the loop-skipped shape ---
    _one(G, [G]),
    _one(G ^ 1, [G]),  # key unchanged, absent below
    _one(G1, [G]),  # absent above
    _one(G, [G], handle={"handle": 0xFFFFFFFFFFFFFFFF}),  # the array's -1
    _one(G, [G], handle={"handle": 0x21}),  # handle & ~0x3f == 0
    _one(G, [G], handle={"tag": 0x21, "obj_tag": 0x22}),  # tag mismatch
    _one(G, [G], handle={"tag": 0x21, "loaded": False}),  # status 2
    _one(G, [G], handle={"tag": 0x21, "bits": 1 << 15}),  # status 3
    _one(G, [G], handle={"tag": 0x21, "bits": 1 << 23}),  # status 4: not a bit
    _one(G, [G], handle={"tag": 0x21, "bits": 1 << 31}),  # status 4: not a bit
    # obj+0xa is read as one byte and tested for bit 0 only: 2 is *not*
    # loaded (a wider mask would call it loaded).
    _one(G, [G], handle={"tag": 0x21, "loaded_byte": 2}),
    _one(G, [G], handle={"tag": 0x21, "loaded_byte": 0xFF}),
    # obj+8 is tested for its low *six* bits, not seven: bit 6 is not a tag
    # bit, so a 0x7f mask would reject this handle.
    _one(G, [G], handle={"tag": 0x21, "dword": (0x21 | 0x40) | 0x1_0000}),
    # --- count 2: every post-loop shape ---
    _one(G, [G, G1]),
    _one(G1, [G, G1]),
    _one(G + 8, [G, G1]),  # between: lo == last, g > target
    _one(G - 8, [G, G1]),  # below: lo < last, g > target
    _one(G1 + 8, [G, G1]),  # above: lo == last, g < target
    # --- count 3/4/8: repeated search iterations, first/middle/last ---
    _one(G, [G, G1, G2], [2, 1, 0]),
    _one(G1, [G, G1, G2], [2, 1, 0]),
    _one(G2, [G, G1, G2], [2, 1, 0]),
    _one(G + 1, [G, G1, G2], [2, 1, 0]),
    _one(
        G3,
        [G, G1, G2, G3, G + 0x40, G + 0x50, G + 0x60, G8],
        [7, 6, 5, 4, 3, 2, 1, 0],
    ),
    _one(
        G8,
        [G, G1, G2, G3, G + 0x40, G + 0x50, G + 0x60, G8],
        [7, 6, 5, 4, 3, 2, 1, 0],
    ),
    _one(
        G + 0x45,
        [G, G1, G2, G3, G + 0x40, G + 0x50, G + 0x60, G8],
    ),
    # --- duplicate rows: the lower bound is the first equal row ---
    _one(G, [G, G, G1], [1, 0, 2]),
    _one(G, [G, G, G1], [2, 1, 0]),
    # --- the client's walk on an unsorted array ---
    _one(G1, [G1, G]),
    _one(G, [G1, G]),
    # --- type table shape ---
    _one(G, [G], key=K + 1),  # the key is not listed
    _one(G, [G], key=K, stride=0),  # the block stride is not read
    _one(G, [G], key=K, stride=0xFFFFFFFF),
    _one(G, [G], key=K, type_id=0xBEEF),  # the block type is not read
    _one(G, [G], bundle=0),  # GUID 0 is the loaded answer
    # --- type scan: the key at the third entry ---
    {
        "target": G,
        "types": [
            {"key": K + 1, "groups": [], "handles": []},
            {"key": K + 2, "groups": [], "handles": []},
            {"key": K, "groups": [{"bundle": 0, "sorted": [G]}], "handles": []},
        ],
    },
    # --- type scan: the key at the second (odd) entry ---
    {
        "target": G,
        "types": [
            {"key": K + 1, "groups": [], "handles": []},
            {"key": K, "groups": [{"bundle": 0, "sorted": [G]}], "handles": []},
        ],
    },
    # --- type scan: the key at the fourth (odd) entry ---
    {
        "target": G,
        "types": [
            {"key": K + 1, "groups": [], "handles": []},
            {"key": K + 2, "groups": [], "handles": []},
            {"key": K + 3, "groups": [], "handles": []},
            {"key": K, "groups": [{"bundle": 0, "sorted": [G]}], "handles": []},
        ],
    },
    {
        "target": G,
        "types": [
            {"key": K + 1, "groups": [], "handles": []},
            {"key": K + 2, "groups": [], "handles": []},
        ],
    },
    # --- no types / a type with no groups ---
    {"target": G, "types": []},
    _one(G, [G]),
    {
        "target": G,
        "types": [{"key": K, "groups": [], "handles": []}],
    },
    # --- a loaded group with no rows: skipped, even when the word after the
    #     empty array happens to equal the target (GUID 0)
    _one(0, [], key=guid_type_key(0)),
    # --- an empty sorted array before the hit in a later group ---
    _two_groups(
        G,
        {"bundle": 0, "sorted": []},
        {"bundle": 0, "sorted": [G]},
        [{"tag": 0x21, "loaded": True}],
    ),
    # --- two groups: a remembered unready handle, then a loaded one ---
    _two_groups(
        G,
        {"bundle": 0, "sorted": [G]},
        {"bundle": 0, "sorted": [G]},
        [
            {"tag": 0x21, "loaded": False},
            {"tag": 0x22, "loaded": True},
        ],
    ),
    # --- two groups: unready, then another unready duplicate ---
    _two_groups(
        G,
        {"bundle": 0, "sorted": [G]},
        {"bundle": 0, "sorted": [G]},
        [
            {"tag": 0x21, "loaded": False},
            {"tag": 0x22, "loaded": False},
        ],
    ),
    # --- two groups: loaded first returns before the second is read ---
    _two_groups(
        G,
        {"bundle": 0, "sorted": [G]},
        {"bundle": 0, "sorted": [G]},
        [
            {"tag": 0x21, "loaded": True},
            {"tag": 0x22, "loaded": True},
        ],
    ),
    # --- running listed count: group 0 misses after two listed rows, group
    #     1's index 0 names handle row 2 ---
    _two_groups(
        G1,
        {"bundle": 0, "sorted": [G], "listed_count": 2},
        {"bundle": 0, "sorted": [G1], "index": [0], "listed_count": 1},
        [
            {"tag": 0x21, "loaded": True},
            {"tag": 0x22, "loaded": True},
            {"tag": 0x23, "loaded": True},
        ],
    ),
    # --- the last found row ends at the pool's final byte ---
    _one(G, [G], listed_count=0),
    # --- GUID 0 and u64::MAX are ordinary targets ---
    _one(0, [0], key=guid_type_key(0)),
    _one(M64, [M64], key=guid_type_key(M64)),
    _one(M64 - 1, [M64], key=guid_type_key(M64 - 1)),
]


def _generated(i: int) -> Dict:
    """A deterministic case for index `i` beyond the pinned list."""
    rnd = random.Random(0x4449_5245 ^ i)  # "DIRE"
    target = rnd.getrandbits(64)
    key = guid_type_key(target)
    ntypes = rnd.randrange(1, 4)
    hit_type = rnd.randrange(ntypes)
    types = []
    for ti in range(ntypes):
        tkey = key if ti == hit_type else (key + 1 + ti) & 0xFFFF_FFFF
        ngroups = rnd.randrange(1, 3)
        groups = []
        for gi in range(ngroups):
            if gi == rnd.randrange(ngroups):
                count = rnd.choice([0, 1, 2, 3, 4, 6])
            else:
                count = rnd.choice([0, 1, 2, 3])
            listed_count = rnd.randrange(0, 5)
            candidates = [target]
            for k in range(max(count, 1) + 1):
                candidates.append((target + 1 + k * 0x10) & M64)
            if count:
                sorted_rows = sorted(candidates[:count])
                if rnd.randrange(4) == 0:
                    rnd.shuffle(sorted_rows)
                index = [rnd.randrange(max(listed_count, 1)) for _ in sorted_rows]
            else:
                sorted_rows = []
                index = []
            groups.append(
                {
                    "bundle": 0,
                    "sorted": sorted_rows,
                    "index": index,
                    "listed_count": listed_count,
                }
            )
        handles = []
        for _ in range(rnd.randrange(1, 9)):
            mode = rnd.randrange(10)
            if mode == 0:
                handles.append({"handle": 0xFFFFFFFFFFFFFFFF})
            elif mode == 1:
                handles.append({"handle": rnd.randrange(0x40)})
            elif mode == 2:
                handles.append({"tag": 0x21, "obj_tag": 0x22})
            else:
                handles.append(
                    {
                        "tag": rnd.randrange(0x40),
                        "loaded": bool(rnd.randrange(2)),
                        "bits": rnd.choice([0, 0, 0, 1 << 15, 1 << 23, 1 << 31]),
                    }
                )
        types.append({"key": tkey, "groups": groups, "handles": handles})
    return {"target": target, "types": types}


def make_args(i: int) -> List[ArgSpec]:
    """The i-th input: directory object, out slot, target GUID, pool."""
    case = _PINNED[i] if i < len(_PINNED) else _generated(i)
    directory, pool, relocs = _build(case)
    return [
        ArgSpec(
            kind="ptr",
            mem=directory,
            guest=DIR_GUEST,
            relocs=(0x18,),
        ),
        ArgSpec(kind="ptr", mem=b"\x00" * 8, guest=DIR_GUEST + OUT_OFF),
        ArgSpec(kind="u64", bits=case["target"] & M64),
        ArgSpec(
            kind="ptr",
            mem=pool,
            guest=DIR_GUEST + POOL_OFF,
            relocs=relocs,
        ),
    ]
