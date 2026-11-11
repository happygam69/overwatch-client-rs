"""Structured input factory for fn/CurveKeySampleDual.

The callee reads a struct of two pointers and two counts, then walks two
sorted time/key tracks. The generic byte generator cannot build a valid
linked structure, so this fixture lays the regions out inside one pinned
arena blob and stores RELATIVE pointers; both loaders rebase them (oracle
to the arena base, C candidate to its local copy). The function under test
never sees the relative form.

Layout inside the arena blob (offsets are dynamic, computed per shape):
  +0x00  param_1 struct: values_ptr(+0x00), times_ptr(+0x08),
         count1(+0x10 u16), count2(+0x12 u16)
  then   key values: (count1 + count2) vec4s, 16 bytes each
  then   times: count1 pairs then count2 pairs, 8 bytes each
         (f32 time, u32 index-as-bits)
Separately pinned:
  out1   param_2 result vec4
  out2   param_3 result vec4

Times must be sorted ascending - the search is a linear/binary walk that
assumes order. Track2's stored key index is relative to track2; the callee
adds count1. The large-track shape (> 0x3f keys) reaches the callee's
binary-search branch.
"""

from __future__ import annotations

import struct
from typing import List

from emu_oracle import ARENA_BASE, ArgSpec

# Fixed out slots (well past any blob the shapes below produce).
OUT1_OFF = 0x3000
OUT2_OFF = 0x3040

# Deterministic (track1, track2) time shapes. Sorted ascending each.
# Shapes chosen to reach every early-out in both halves of the callee:
#   count == 1 fast paths      (track2 length-1 shape)
#   t before / on / after keys (all shapes, by sample choice)
#   count > 0x3f binary search (80-key shape, BOTH tracks >= 64 so each
#                               half's search runs)
_T_BASES = [
    ([-10.0, 0.0, 1.0, 10.0], [0.0, 1.0]),
    ([0.0, 0.5, 1.0], [0.5]),           # track2 count == 1 fast path
    ([-1.0, 0.0, 1.0, 2.0, 3.0], [0.0, 1.0, 2.0]),
    ([0.0], [0.0]),                      # both counts == 1
    ([0.0, 1.0], [0.0, 0.5, 1.0]),       # track1 count == 2 boundary
    ([float(j) * 0.5 for j in range(80)],
     [float(j) * 0.5 for j in range(80)]),  # both halves binary-search
    ([float(j) * 0.5 for j in range(63)],
     [float(j) * 0.5 for j in range(63)]),  # count == 63: just below 0x3f gate
    ([float(j) * 0.5 for j in range(80)],
     [float(j) * 0.5 for j in range(80)]),  # track2 key-0 sample (i%7 picks it)
    # 128 keys: the search's window is [0, count-1] and it halves it once per
    # pass, so the loop repeats only while (hi - lo) >= 0x40. After the first
    # pass lo is the middle key (63) and hi is 127, so (hi - lo) == 64 - the
    # loop repeats exactly once. 80 keys halve to 40 and never repeat, which
    # is why the search's back edge (0x140b90a80 / 0x140b90b70) needs this
    # shape. The sample has to sit above the middle key for lo to advance.
    ([float(j) * 0.5 for j in range(128)],
     [float(j) * 0.5 for j in range(128)]),
]


def _full_blob(i: int) -> bytes:
    """Arena image with RELATIVE pointers (offset from blob base 0)."""
    shape = i % len(_T_BASES)
    times1, times2 = _T_BASES[shape]
    times1 = list(times1)
    times2 = list(times2)
    count1 = len(times1)
    count2 = len(times2)

    values = b""
    for k in range(count1 + count2):
        values += struct.pack(
            "<4f", float(k), float(k) + 0.25, float(k) + 0.5, float(k) + 0.75
        )

    def pack_track(times, nkeys, is_track2=False, total_keys=0):
        # Index validity after the callee's own arithmetic:
        #   track1: index used as-is, must land in [0, total_keys).
        #   track2: callee adds count1, stored index must land in
        #           [0, count2) so the final index is in the values array.
        # For a single-key track store a NON-ZERO index when one is valid,
        # so the fast path's pointer arithmetic is observable; a stored 0
        # would make +0/-0 and *0x10//0x10 invisible to the differential.
        out = b""
        for j, t in enumerate(times):
            tt = t + (i % 7) * 1e-4
            if nkeys == 1:
                if is_track2:
                    idx = 0  # final = count1, the only track2 key; +count1 still observable
                else:
                    idx = 1 if total_keys > 1 else 0
            else:
                idx = j % nkeys
            out += struct.pack("<fI", tt, idx)
        return out

    times_blob = pack_track(times1, count1, False, count1 + count2) + pack_track(
        times2, count2, True, count1 + count2
    )
    # Dynamic offsets: struct at 0, values right after, times after values.
    struct_size = 24
    values_off = (struct_size + 15) & ~15
    times_off = (values_off + len(values) + 15) & ~15
    blob = bytearray(times_off + len(times_blob))
    blob[0:8] = struct.pack("<Q", values_off)  # relative
    blob[8:16] = struct.pack("<Q", times_off)  # relative
    blob[16:18] = struct.pack("<H", count1)
    blob[18:20] = struct.pack("<H", count2)
    blob[values_off : values_off + len(values)] = values
    blob[times_off : times_off + len(times_blob)] = times_blob
    return bytes(blob)


def rebase_blob(blob: bytes, base: int) -> bytes:
    """Add `base` to the two relative pointer slots (offsets 0 and 8)."""
    b = bytearray(blob)
    for off in (0, 8):
        rel = struct.unpack_from("<Q", b, off)[0]
        struct.pack_into("<Q", b, off, base + rel)
    return bytes(b)


def make_args(i: int) -> List[ArgSpec]:
    shape = i % len(_T_BASES)
    times1, times2 = _T_BASES[shape]
    times1 = list(times1)
    times2 = list(times2)
    count1 = len(times1)
    # Stored times are jittered by (i%7)*1e-4; the "on first key" sample
    # must match the jittered value or it falls into the before-all fast
    # path and never exercises the search.
    jitter = (i % 7) * 1e-4
    mode = (i // len(_T_BASES)) % 6
    if mode == 0:
        sample = -100.0
    elif mode == 1:
        sample = 100.0
    elif mode == 2:
        sample = times1[0] + jitter          # on track1's first key
    elif mode == 3:
        sample = times1[count1 // 2] + 0.123  # between keys, mid track
    elif mode == 4 and times2:
        sample = times2[0] + jitter          # on track2's first key (uVar12 probe)
    else:
        sample = times1[min(1, count1 - 1)] - 0.05  # just before second key

    return [
        ArgSpec(kind="ptr", mem=_full_blob(i), arena_offset=0, relocs=(0, 8)),
        ArgSpec(kind="ptr", mem=b"\x00" * 16, arena_offset=OUT1_OFF),
        ArgSpec(kind="ptr", mem=b"\x00" * 16, arena_offset=OUT2_OFF),
        ArgSpec(kind="f32", value=sample),
    ]
