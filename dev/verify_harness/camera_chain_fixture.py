"""Inputs for `fn/CameraChainScale` (`0x141ffe590`), and its snapshot.

The client follows absolute pointers from a node to its parent, so the snapshot
holds several independent chains and each case selects one root by its **guest**
address (the fixed-snapshot rule of the playbook's 6.5).

| chain | at     | scales              | note                          |
|-------|--------|---------------------|-------------------------------|
| 0     | 0x0400 | 2.5                 | one node, no link             |
| 1     | 0x1000 | 2, 3, 0.5           | a product of 3                |
| 2     | 0x2000 | 2, 3 (stop), 10     | the stop bit on the middle    |
| 3     | 0x3000 | 7 (stop), 3         | the stop bit on the root      |
| 4     | 0x4000 | 0, 5                | a zero scale                  |
| 5     | 0x5000 | 1.5 x8              | a deep chain                  |

Nodes are `0x400` apart, so no chain can reach into another.
"""

from __future__ import annotations

import struct

from emu_oracle import ArgSpec

BASE = 0x300000010000
STATE_LEN = 0x7000  # chain 5 runs to 0x5000 + 7*0x400 + 0x2a0
FLAGS = 0x230
STOP = 0x4_0000_0000
SCALE = 0x270
LINK = 0x298
STRIDE = 0x400

CHAINS = [
    [0x0400, [2.5], None],
    [0x1000, [2.0, 3.0, 0.5], None],
    [0x2000, [2.0, 3.0, 10.0], 1],
    [0x3000, [7.0, 3.0], 0],
    [0x4000, [0.0, 5.0], None],
    [0x5000, [1.5] * 8, None],
]


def snapshot() -> bytes:
    state = bytearray(STATE_LEN)
    for start, scales, stop_at in CHAINS:
        for i, scale in enumerate(scales):
            at = start + i * STRIDE
            struct.pack_into("<f", state, at + SCALE, scale)
            link = BASE + at + STRIDE if i + 1 < len(scales) else 0
            struct.pack_into("<Q", state, at + LINK, link)
            if stop_at == i:
                struct.pack_into("<Q", state, at + FLAGS, STOP)
    return bytes(state)


def make_args(i: int):
    start, scales, _ = CHAINS[i % len(CHAINS)]
    depth = (i // len(CHAINS)) % len(scales)  # start at a later node of the chain
    # The client takes ONE argument - the node in `rcx`. The base rides in
    # `rdx`, which the body never reads, so the probe can translate the links
    # without changing what the client does.
    return [
        ArgSpec(kind="u64", bits=BASE + start + depth * STRIDE),
        ArgSpec(kind="u64", bits=BASE),
    ]


if __name__ == "__main__":
    print(snapshot().hex())
