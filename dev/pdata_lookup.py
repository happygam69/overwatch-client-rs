"""Lookup .pdata function ranges containing given rvas.

Usage: python dev/pdata_lookup.py <rva> [rva ...]
Reads .pdata from the on-disk Overwatch.exe and reports for each rva:
the packed run_info, the function's begin/end, its length, and the
bytes-as-code flag from the run_info unpack (begin:
first 4 bytes BE).  This is the address book used to turn raw pointer
targets into callable decompilation targets.
"""
import struct
import sys
import subprocess

EXE = r"E:\Overwatch\_retail_\Overwatch.exe"
SBASE = 0x7FF7C4D20000  # standing v2b base for reporting session addrs


def sections(exe):
    pe = struct.unpack_from("<I", exe, 0x3C)[0]
    nsec = struct.unpack_from("<H", exe, pe + 6)[0]
    opt = struct.unpack_from("<H", exe, pe + 20)[0]
    s0 = pe + 24 + opt
    out = []
    for i in range(nsec):
        o = s0 + 40 * i
        name = exe[o:o + 8].split(b"\x00")[0].decode("latin-1")
        vsz, va, rsz, ro = struct.unpack_from("<IIII", exe, o + 8)
        out.append((name, va, vsz, ro, rsz))
    return out


def main() -> int:
    exe = open(EXE, "rb").read()
    secs = sections(exe)
    pdata = None
    for name, va, vsz, ro, rsz in secs:
        if name == ".pdata":
            pdata = (ro + (0x4635000 - va) if False else ro)
            pdata_len = (vsz if rsz > vsz else vsz)
    raw = exe[pdata:pdata + 0x24dd1c]
    n_ranges = len(raw) // 12

    rvas = [int(a, 16) for a in sys.argv[1:]]
    entries = [(struct.unpack_from("<II", raw, 12 * i)) for i in
               range(n_ranges)]
    for rva in rvas:
        best = None
        for j, (beg, end) in enumerate(entries):
            if beg <= rva < end:
                best = (j, beg, end)
                break
        if best:
            j, beg, end = best
            ri = struct.unpack_from("<I", raw, 12 * j + 8)[0]
            print(f"rva {rva:#x}: fn {beg:#x}-{end:#x} "
                  f"len {end - beg:#x} run_info {ri:#x}")
        else:
            print(f"rva {rva:#x}: no pdata range")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

