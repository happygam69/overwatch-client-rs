"""Inputs for the `fn/ScopePop-steps` unit (`0x142cee300`), and its snapshot.

One pop removes the scope stack's last element: it subtracts the element's
`+0x08` from the container's two totals and decrements the count. Popping twice
therefore reads what the first pop wrote - the count - which is exactly the
feedback a steps unit exists to compare.

The state region holds four independent container/array pairs, so the snapshot
stays fixed per unit (rule 2 of the playbook's 6.5) and the cases vary *which*
pair they select:

| case | container | count | elements |
|---|---|---|---|
| 0 | 0 | 3 | 1, 2, 3 |
| 1 | 1 | 2 | 4, 5 (totals below the first value: the subtraction wraps) |
| 2 | 2 | 3 | 0, 0, 0 |
| 3 | 3 | 4 | 0xffffffff, 1, 2, 3 |

`make_steps(i)` picks the pair by `i % 4` and the chain length by `(i // 4) % 3`,
capped at the pair's count, so no case ever pops past the array (the call after
that would read outside the elements, which the client does not guard).

The container's `+0x18` carries the array's **guest address**, so the oracle and
the probe address the same bytes by construction.

    python dev/verify_harness/scope_pop_steps_fixture.py   # print the hex
"""

from __future__ import annotations

import struct

from emu_oracle import ArgSpec

BASE = 0x0000300000000C00  # outside the arena (0x200000000000..+0x10000)
STATE_LEN = 0x1000
CONTAINER_AT = 0x100
ARRAY_AT = 0x400
CONTAINER_STRIDE = 0x40   # a container is 0x28; 0x40 leaves room
ARRAY_STRIDE = 0x100      # an array is count*16 <= 0x40
PAIRS = 4
ELEMENT = 16

COUNTS = [3, 2, 3, 4]
TOTALS = [(100, 200), (1, 1), (0, 0), (7, 9)]
ELEMENTS = [
    [1, 2, 3, 0],
    [4, 5, 0, 0],
    [0, 0, 0, 0],
    [0xFFFF_FFFF, 1, 2, 3],
]


def snapshot() -> bytes:
    state = bytearray(STATE_LEN)
    # The regions must not overlap: a container at the array's stride lands on
    # the previous pair's elements, and the client then faithfully processes
    # corrupt inputs (this cost one debugging round).
    assert CONTAINER_AT + PAIRS * CONTAINER_STRIDE <= ARRAY_AT, "containers overlap the arrays"
    assert ARRAY_AT + PAIRS * ARRAY_STRIDE <= STATE_LEN, "arrays run past the state"
    for pair in range(PAIRS):
        c = CONTAINER_AT + pair * CONTAINER_STRIDE
        a = ARRAY_AT + pair * ARRAY_STRIDE
        low, high = TOTALS[pair]
        struct.pack_into("<I", state, c + 0x08, low)
        struct.pack_into("<I", state, c + 0x0C, high)
        struct.pack_into("<Q", state, c + 0x18, BASE + a)
        struct.pack_into("<I", state, c + 0x20, COUNTS[pair])
        for index, value in enumerate(ELEMENTS[pair]):
            struct.pack_into("<I", state, a + index * ELEMENT + 8, value)
            state[a + index * ELEMENT + 12] = 0  # flag: the readable path
    return bytes(state)


def make_steps(i: int):
    pair = i % PAIRS
    length = 1 + (i // PAIRS) % 3
    length = min(length, COUNTS[pair])
    container = BASE + CONTAINER_AT + pair * CONTAINER_STRIDE
    array = BASE + ARRAY_AT + pair * ARRAY_STRIDE
    return [
        [ArgSpec(kind="u64", bits=container), ArgSpec(kind="u64", bits=array)]
        for _ in range(length)
    ]


if __name__ == "__main__":
    print(snapshot().hex())
