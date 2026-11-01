"""Trace one call of an obfuscated function, to map its executed blocks.

    python dev/probe_evaluator.py [address] [--steps N] [--world-size BYTES]

This is a **diagnostic**, not a read: it runs the client's bytes under the
same Unicorn oracle the verification harness uses and logs which instructions
actually execute and which memory the function touches. It is how an
obfuscated function (flattened control flow, opaque predicates, overlapping
instructions) is turned into a block graph that can then be read. Nothing
here is a gameplay claim; a finding only counts once the executed blocks are
read.

The default entry is the fan's candidate evaluator `0x1420328c0`, called as
`(world, out, block, record)`; the world and the three pointees are zeroed
arena pages, so the trace shows the first reads a real caller would satisfy.

Output, in order:
  1. the status, step count and return value;
  2. every executed run, as `start..end` (consecutive addresses merged);
  3. the image reads/writes outside the executed code;
  4. the world reads/writes with sizes, so the world's field offsets are
     visible.
"""

from __future__ import annotations

import argparse
import os
import struct
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "verify_harness"))

from emu_oracle import (  # noqa: E402
    SENTINEL,
    STACK_TOP,
    V2B_BASE,
    EmuOracle,
)

from unicorn import (  # noqa: E402
    UC_HOOK_CODE,
    UC_HOOK_MEM_READ,
    UC_HOOK_MEM_WRITE,
    UcError,
)
from unicorn import x86_const as x  # noqa: E402

DEFAULT_IMAGE = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "dev",
    "data",
    "offline_recover",
    "module_offline.bin",
)

WORLD_BASE = 0x0000_0004_0000_0000
WORLD_SIZE = 0x10_0000
IMAGE_BASE = 0x140000000


def runs(addresses):
    """Merge a sorted address list into `[start, end)` runs."""
    out = []
    for a in addresses:
        if out and a == out[-1][1]:
            out[-1][1] = a + 1
        else:
            out.append([a, a + 1])
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("address", nargs="?", default="0x1420328c0")
    ap.add_argument("--steps", type=int, default=200_000)
    ap.add_argument("--world-size", type=lambda s: int(s, 0), default=WORLD_SIZE)
    ap.add_argument("--image", default=DEFAULT_IMAGE)
    args = ap.parse_args()

    entry = int(args.address, 0) - IMAGE_BASE + V2B_BASE
    oracle = EmuOracle(args.image)
    u = oracle.uc
    u.mem_map(WORLD_BASE, args.world_size)
    image_end = V2B_BASE + len(oracle.image)

    executed = []
    image_reads = []
    image_writes = []
    world = []

    def on_code(uc, address, size, _ud):
        executed.append(address)

    def on_read(uc, access, address, size, value, _ud):
        if WORLD_BASE <= address < WORLD_BASE + args.world_size:
            world.append(("r", address, size))
        elif V2B_BASE <= address < image_end:
            image_reads.append((address, size))

    def on_write(uc, access, address, size, value, _ud):
        if WORLD_BASE <= address < WORLD_BASE + args.world_size:
            world.append(("w", address, size))
        elif V2B_BASE <= address < image_end:
            image_writes.append((address, size))

    u.hook_add(UC_HOOK_CODE, on_code)
    u.hook_add(UC_HOOK_MEM_READ, on_read)
    u.hook_add(UC_HOOK_MEM_WRITE, on_write)

    # (world, out, block, record), all inside the world arena.
    for reg, value in (
        (x.UC_X86_REG_RCX, WORLD_BASE),
        (x.UC_X86_REG_RDX, WORLD_BASE + 0x100),
        (x.UC_X86_REG_R8, WORLD_BASE + 0x200),
        (x.UC_X86_REG_R9, WORLD_BASE + 0x300),
    ):
        u.reg_write(reg, value)
    rsp = STACK_TOP - 8
    u.reg_write(x.UC_X86_REG_RSP, rsp)
    u.mem_write(rsp, struct.pack("<Q", SENTINEL))

    status = "returned"
    try:
        u.emu_start(entry, SENTINEL, count=args.steps)
    except UcError as exc:
        status = f"fault: {exc}"
    rip = u.reg_read(x.UC_X86_REG_RIP)
    if status == "returned" and rip != SENTINEL:
        status = f"stopped at {rip:#x} after {args.steps} steps"

    print(f"entry   {entry:#x}  ({args.address})")
    print(f"status  {status}")
    print(f"steps   {len(executed)}")
    print(f"rax     {u.reg_read(x.UC_X86_REG_RAX):#x}")
    print()

    unique = sorted(set(executed))
    print(f"executed runs ({len(runs(unique))} runs, {len(unique)} addresses):")
    for lo, hi in runs(unique):
        print(f"  {lo - V2B_BASE + IMAGE_BASE:#x}..{hi - V2B_BASE + IMAGE_BASE:#x}")
    print()

    def show(title, items):
        print(f"{title} ({len(items)}):")
        counts = {}
        for addr, size in items:
            counts[(addr, size)] = counts.get((addr, size), 0) + 1
        for (addr, size) in sorted(counts):
            print(
                f"  {addr - V2B_BASE + IMAGE_BASE:#x} size {size}"
                f" x{counts[(addr, size)]}"
            )
        print()

    show("image reads", image_reads)
    show("image writes", image_writes)

    print(f"world accesses ({len(world)}):")
    counts = {}
    for kind, addr, size in world:
        key = (kind, addr - WORLD_BASE, size)
        counts[key] = counts.get(key, 0) + 1
    for kind, off, size in sorted(counts, key=lambda t: (t[1], t[0], t[2])):
        print(f"  {kind} world+{off:#07x} size {size} x{counts[(kind, off, size)]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
