"""Find memory that holds a position advancing at walking speed (shape-free).

The dmBody-shaped scans found no walking body in the practice range, and the
mover component's 173 instances plus their single referrers are all static
while the player walks (measured). So the live movement state has an unknown
shape, and this tool drops every shape assumption: it samples memory twice
`--window-ms` apart and reports every 3-float triple whose horizontal
component moved at a plausible walking speed in that window.

That is the behavioural locator `docs/formats/domino.md` argues for: shape and
value signatures flood, motion does not.

Usage:
    python find_moving_objects.py [--pid N] [--max-mb 12288] [--window-ms 1500]
                                  [--stride 4] [--speed-min 3] [--speed-max 8]

Run it while the player walks steadily (`inject_movement.ps1 -Mode walk`).

READ-ONLY.
"""

from __future__ import annotations

import ctypes
import sys
import time

import numpy as np

import dump_process as DP

CHUNK = 16 << 20


def find_pid() -> int:
    class PROCESSENTRY32W(ctypes.Structure):
        _fields_ = [
            ("dwSize", ctypes.c_ulong), ("cntUsage", ctypes.c_ulong),
            ("th32ProcessID", ctypes.c_ulong), ("th32DefaultHeapID", ctypes.POINTER(ctypes.c_ulong)),
            ("th32ModuleID", ctypes.c_ulong), ("cntThreads", ctypes.c_ulong),
            ("th32ParentProcessID", ctypes.c_ulong), ("pcPriClassBase", ctypes.c_long),
            ("dwFlags", ctypes.c_ulong), ("szExeFile", ctypes.c_wchar * 260),
        ]

    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.CreateToolhelp32Snapshot.restype = ctypes.c_void_p
    snap = k32.CreateToolhelp32Snapshot(0x2, 0)
    entry = PROCESSENTRY32W()
    entry.dwSize = ctypes.sizeof(entry)
    ok = k32.Process32FirstW(snap, ctypes.byref(entry))
    while ok:
        if "overwatch" in entry.szExeFile.lower():
            k32.CloseHandle(snap)
            return entry.th32ProcessID
        ok = k32.Process32NextW(snap, ctypes.byref(entry))
    k32.CloseHandle(snap)
    return 0


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else find_pid()
    if not pid:
        print("no Overwatch process found")
        return 1
    max_mb = int(args[args.index("--max-mb") + 1]) if "--max-mb" in args else 12288
    window_ms = int(args[args.index("--window-ms") + 1]) if "--window-ms" in args else 1500
    stride = int(args[args.index("--stride") + 1], 0) if "--stride" in args else 4
    vmin = float(args[args.index("--speed-min") + 1]) if "--speed-min" in args else 3.0
    vmax = float(args[args.index("--speed-max") + 1]) if "--speed-max" in args else 8.0
    window_s = window_ms / 1000.0

    handle = DP.open_target(pid)
    regions = [r for r in DP.regions(handle)
               if not (r[2] & (DP.PAGE_NOACCESS | DP.PAGE_GUARD)) and r[3] != DP.MEM_IMAGE]
    total = sum(r[1] for r in regions)
    print(f"pid {pid}: {total >> 20} MB present, cap {max_mb} MB, window {window_ms} ms, "
          f"speed {vmin}-{vmax} m/s — walk steadily now")

    hits = []
    scanned = 0
    lo, hi = vmin * window_s, vmax * window_s
    for base, size, protect, kind, _alloc in regions:
        if scanned >= max_mb * 1048576:
            print(f"  (stopping at the {max_mb} MB cap)")
            break
        offset = 0
        while offset < size:
            origin = base + offset
            take = min(CHUNK, size - offset)
            if take < 64:
                break
            a = DP.read_region(handle, origin, take)
            time.sleep(window_s)
            b = DP.read_region(handle, origin, take)
            scanned += take
            offset += take
            if not a or not b or len(a) != len(b) or len(a) < 64:
                continue
            nf = len(a) // 4          # float units
            stride_f = max(1, stride // 4)
            fa = np.frombuffer(a[: nf * 4], dtype="<f4")
            fb = np.frombuffer(b[: nf * 4], dtype="<f4")
            m = (nf - 3) // stride_f + 1
            if m <= 0:
                continue
            idx = np.arange(0, m * stride_f, stride_f, dtype=np.int64)
            # Arbitrary memory holds huge/invalid floats; the `finite` mask
            # rejects them, but the overflow/invalid warnings would flood the
            # output (this tool's own logs show them), so compute silently.
            with np.errstate(all="ignore"):
                ax, ay, az = fa[idx], fa[idx + 1], fa[idx + 2]
                bx, by, bz = fb[idx], fb[idx + 1], fb[idx + 2]
                finite = np.isfinite(ax) & np.isfinite(ay) & np.isfinite(az) \
                    & np.isfinite(bx) & np.isfinite(by) & np.isfinite(bz)
                dx = np.abs(bx - ax)
                dz = np.abs(bz - az)
                dy = np.abs(by - ay)
                ok = finite & (dx <= hi) & (dz <= hi) & (dy < 0.75) \
                    & (dx + dz >= lo) & (np.abs(ax) < 4000) & (np.abs(az) < 4000)
            for i in np.nonzero(ok)[0]:
                off = int(idx[i]) * 4
                hits.append((origin + off, float(ax[i]), float(ay[i]), float(az[i]),
                             float(bx[i]), float(by[i]), float(bz[i])))
        if (scanned >> 20) % 1024 < 20:
            print(f"  {scanned >> 20} MB scanned, {len(hits)} hit(s)", flush=True)

    print(f"\n{len(hits)} moving triple(s) over {scanned >> 20} MB")
    for addr, ax, ay, az, bx, by, bz in hits[:60]:
        d = ((bx - ax) ** 2 + (bz - az) ** 2) ** 0.5 / window_s
        print(f"  {addr:#014x}  ({ax:9.3f},{ay:8.3f},{az:9.3f}) -> "
              f"({bx:9.3f},{by:8.3f},{bz:9.3f})  {d:6.2f} m/s")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
