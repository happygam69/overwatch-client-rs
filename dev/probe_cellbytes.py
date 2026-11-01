"""Diagnostic: settle MapChunkCellBytesFixup's array/record indirection.

    python dev/probe_cellbytes.py

Runs the client's bytes at 0x141ff51a0 under the unicorn oracle with a
synthetic field header and a synthetic 17-qword record, for each of the three
header modes. Prints the record before/after and the addresses it touched, so
the encoding (which qword is the record pointer, where the count lives) is
observed rather than guessed. Nothing here is a gameplay claim.
"""

from __future__ import annotations

import argparse
import os
import struct
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "verify_harness"))

from emu_oracle import SENTINEL, STACK_TOP, V2B_BASE, EmuOracle  # noqa: E402

from unicorn import UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE, UcError  # noqa: E402
from unicorn import x86_const as x  # noqa: E402

DEFAULT_IMAGE = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "dev", "data", "offline_recover", "module_offline.bin",
)
IMAGE_BASE = 0x140000000
ENTRY = 0x141ff51A0
SCRATCH = 0x0000_0005_0000_0000
SCRATCH_SIZE = 0x4000


def q(buf, off):
    return struct.unpack_from("<Q", buf, off)[0]


def check_cells(path: str) -> int:
    """Validate the payload's cell container (docs/formats/maps.md)."""
    b = open(path, "rb").read()
    n = len(b)
    base = 0x20  # the STUD header the offsets are relative to
    count = struct.unpack_from("<I", b, base + 0x2428)[0]
    data = base + struct.unpack_from("<I", b, base + 0x2428 + 8)[0]
    cells = []
    for i in range(count):
        cell = base + q(b, data + 16 * i)
        size = q(b, cell)
        rec = base + q(b, cell + 8)
        offs = [q(b, rec + 8 * k) for k in range(17)]
        ok = 0 < rec < n and rec + size <= n
        ok = ok and all(o == 0 or rec + o < n for o in offs)
        # empty arrays are zero-length: boundaries may be equal
        ok = ok and offs[0] >= 0x88
        ok = ok and all(offs[k] <= offs[k + 1] for k in range(16))
        if not ok:
            print(f"  cell {i}: BAD cell {cell:#x} rec {rec:#x} size {size:#x}")
            return 1
        cells.append((cell, rec, size, offs))
    print(f"cells {count} ok; first {cells[0][0]:#x} rec {cells[0][1]:#x}"
          f" size {cells[0][2]:#x}; last cell {cells[-1][0]:#x}"
          f" end {cells[-1][1] + cells[-1][2]:#x} payload {n:#x}")
    print(f"entries end {data + 16 * count:#x}"
          f" (should be the first cell, {cells[0][0]:#x})")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--cells", default="")
    args = ap.parse_args()
    if args.cells:
        return check_cells(args.cells)
    oracle = EmuOracle(DEFAULT_IMAGE)
    u = oracle.uc
    u.mem_map(SCRATCH, SCRATCH_SIZE)
    entry = ENTRY - IMAGE_BASE + V2B_BASE
    execs = []

    def on_code(uc, address, size, _ud):
        execs.append(address)

    u.hook_add(UC_HOOK_CODE, on_code)

    # Cases: (label, mode, count_at, entries_at, entry_qword0, entry_qword1)
    cases = [
        ("mode1 count@ptr+8 entries@ptr     ", 1, 8, 0, "cell", 1),
        ("mode2 count@ptr   entries@ptr+8   ", 2, 0, 8, "cell", 2),
        ("mode3 count@ptr   entries@ptr+8   ", 3, 0, 8, "cell", 3),
    ]
    for label, mode, count_at, entries_at, _, flag in cases:
        u.mem_write(SCRATCH, b"\x00" * SCRATCH_SIZE)
        field = SCRATCH + 0x1000
        arr = SCRATCH + 0x1100          # header target (*field)
        cell = SCRATCH + 0x1200         # entry[0] target
        rec = SCRATCH + 0x1400          # candidate record
        # 17 small self-relative offsets; index 5 zero to see the null rule.
        recvals = [8 * (i + 1) for i in range(17)]
        recvals[5] = 0
        u.mem_write(rec, b"".join(struct.pack("<Q", v) for v in recvals))
        # header: count at arr+count_at, data pointer at arr+entries_at
        u.mem_write(arr + count_at, struct.pack("<I", 1))
        u.mem_write(arr + entries_at, struct.pack("<Q", arr + 0x80))
        # entry: {cell, flag}
        u.mem_write(arr + 0x80, struct.pack("<QQ", cell, flag))
        # cell: first qword = record, second qword = a marker
        u.mem_write(cell, struct.pack("<QQ", rec, 0xDEAD))
        u.mem_write(field, struct.pack("<QQ", arr, mode))

        u.reg_write(x.UC_X86_REG_RCX, field)
        rsp = STACK_TOP - 8
        u.reg_write(x.UC_X86_REG_RSP, rsp)
        u.mem_write(rsp, struct.pack("<Q", SENTINEL))
        del execs[:]
        status = "returned"
        try:
            u.emu_start(entry, SENTINEL, count=20000)
        except UcError as exc:
            status = f"fault: {exc}"
        after = bytes(u.mem_read(rec, 0x88))
        vals = [q(after, 8 * i) for i in range(17)]
        print(label, "->", status)
        print(f"  steps {len(execs)}  rax {u.reg_read(x.UC_X86_REG_RAX):#x}"
              f"  rip {u.reg_read(x.UC_X86_REG_RIP) - V2B_BASE + IMAGE_BASE:#x}")
        print(f"  rec+0x00 {q(after,0):#x}  rec+0x08 {q(after,8):#x}"
              f"  rec+0x28 {q(after,0x28):#x}  rec+0x80 {q(after,0x80):#x}")
        fixed = sum(1 for i, v in enumerate(vals)
                    if v == rec + 8 * (i + 1) or v == 0)
        print(f"  fixed {fixed}/17  expected {rec:#x}+off")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
