"""Find what points at an address in a live process (read-only).

The mover component is per-hero *config* — 173 instances exist in the practice
range and none of them changes while the player walks (measured: `live_trace.py
mover --watch` reported zero changed slots). So the per-tick movement state is
another object, and the shortest route to it is: find every pointer to the
player's mover instance, then watch those referrers while the player moves.

Usage:
    python find_referrers.py 0xADDR [0xADDR ...] [--pid N] [--max-mb 12288]

Prints, per target, the referrer addresses (where a qword equal to the target
is stored).  Vectorized with numpy: each 64 MB chunk is checked with `np.isin`
against the target set, which is a couple of minutes over the full address
space.

READ-ONLY: OpenProcess with VM_READ|QUERY, ReadProcessMemory only; the target
is never written.
"""

from __future__ import annotations

import sys

import numpy as np

import dump_process as DP

CHUNK = 64 << 20


def main() -> int:
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return 2
    targets = []
    for a in args:
        if a.startswith("0x") or a.startswith("0X"):
            try:
                targets.append(int(a, 16))
            except ValueError:
                pass
    if not targets:
        print("no hex addresses given")
        return 2
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else 0
    max_mb = int(args[args.index("--max-mb") + 1]) if "--max-mb" in args else 12288

    if not pid:
        # Same discovery as live_trace.py: by image name.
        import ctypes
        from ctypes import wintypes as wt

        class PROCESSENTRY32W(ctypes.Structure):
            _fields_ = [
                ("dwSize", wt.DWORD), ("cntUsage", wt.DWORD),
                ("th32ProcessID", wt.DWORD), ("th32DefaultHeapID", ctypes.POINTER(ctypes.c_ulong)),
                ("th32ModuleID", wt.DWORD), ("cntThreads", wt.DWORD),
                ("th32ParentProcessID", wt.DWORD), ("pcPriClassBase", ctypes.c_long),
                ("dwFlags", wt.DWORD), ("szExeFile", ctypes.c_wchar * 260),
            ]

        k32 = ctypes.WinDLL("kernel32", use_last_error=True)
        k32.CreateToolhelp32Snapshot.restype = wt.HANDLE
        snap = k32.CreateToolhelp32Snapshot(0x2, 0)
        entry = PROCESSENTRY32W()
        entry.dwSize = ctypes.sizeof(entry)
        ok = k32.Process32FirstW(snap, ctypes.byref(entry))
        while ok:
            if "overwatch" in entry.szExeFile.lower():
                pid = entry.th32ProcessID
                break
            ok = k32.Process32NextW(snap, ctypes.byref(entry))
        k32.CloseHandle(snap)
    if not pid:
        print("no Overwatch process found")
        return 1

    handle = DP.open_target(pid)
    tset = np.array(sorted(set(targets)), dtype="<u8")
    print(f"pid {pid}: {len(tset)} target(s); scanning (cap {max_mb} MB)")

    hits: dict[int, list[int]] = {int(t): [] for t in tset}
    scanned = 0
    chunks = 0
    for base, size, protect, kind, _alloc in DP.regions(handle):
        if protect & (DP.PAGE_NOACCESS | DP.PAGE_GUARD) or kind == DP.MEM_IMAGE:
            continue
        if scanned >= max_mb * 1048576:
            print(f"  (stopping at the {max_mb} MB cap)")
            break
        offset = 0
        while offset < size:
            origin = base + offset
            take = min(CHUNK, size - offset)
            if take < 8:
                break
            data = DP.read_region(handle, origin, take)
            if data and len(data) >= 8:
                scanned += len(data)
                chunks += 1
                u = np.frombuffer(data[: len(data) // 8 * 8], dtype="<u8")
                mask = np.isin(u, tset)
                if mask.any():
                    for i in np.nonzero(mask)[0]:
                        v = int(u[i])
                        hits[v].append(origin + int(i) * 8)
            offset += take
        if chunks % 64 == 0 and chunks:
            print(f"  {scanned >> 20} MB scanned, {sum(len(v) for v in hits.values())} referrer(s)")

    total = 0
    for t in sorted(hits):
        refs = hits[t]
        total += len(refs)
        if refs:
            print(f"\n0x{t:x}: {len(refs)} referrer(s)")
            for r in refs[:40]:
                print(f"   {r:#014x}")
    print(f"\n{total} referrer(s) over {scanned >> 20} MB")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
