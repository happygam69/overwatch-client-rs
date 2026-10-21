"""Watch every mover, its owner, and one pointer level out, during a real walk.

The mover component (STUCharacterMoverComponent, class 0x6EFDBD73) is config:
its 173 instances and their single referrer owners measured static while
walking. This tool discovers *all* of them fresh (vtable-pointer scan of the
heap band; owner = the object holding a pointer to the mover), expands one
pointer level out of each owner, then holds a strafe key (D — works even with
the player pinned against a wall) and reports:

  * position candidates: triples still before the walk whose value moved >2 m
    horizontally and stayed changed after the stop;
  * velocity candidates: triples still before the walk whose horizontal
    displacement over the walk samples is 3-8.5 m/s;
  * byte-changers: spans whose u32s change between the pre-walk idle and a
    mid-walk sample.

Usage:
    python hunt_actor.py [--pid N] [--key d] [--walk-ms 7000] [--idle-ms 700]

READ-ONLY with respect to memory; sends keys to the game window exactly like
inject_movement.ps1 does.
"""

from __future__ import annotations

import ctypes
import struct
import sys
import time

import numpy as np

import dump_process as DP

HEAP_LO = 0x020000000000
HEAP_HI = 0x030000000000
BAND_BASE = 0x024080000000
BAND_SIZE = 0x10000000          # 256 MB covers every mover seen so far
VTABLES = [0x7FF7117031F8, 0x7FF7117030C8, 0x7FF711703160]

u32 = ctypes.WinDLL("user32", use_last_error=True)
k32 = ctypes.WinDLL("kernel32", use_last_error=True)

SCANCODE = 0x0008
KEYUP = 0x0002
VK = {"w": 0x57, "a": 0x41, "s": 0x53, "d": 0x44, "space": 0x20, "ctrl": 0x11, "shift": 0x10, "e": 0x45, "q": 0x51, "r": 0x52, "h": 0x48}
SC = {"w": 0x11, "a": 0x1E, "s": 0x1F, "d": 0x20, "space": 0x39, "ctrl": 0x1D, "shift": 0x2A, "e": 0x12, "q": 0x10, "r": 0x13, "h": 0x23}


def find_pid() -> int:
    class PROCESSENTRY32W(ctypes.Structure):
        _fields_ = [
            ("dwSize", ctypes.c_ulong), ("cntUsage", ctypes.c_ulong),
            ("th32ProcessID", ctypes.c_ulong), ("th32DefaultHeapID", ctypes.POINTER(ctypes.c_ulong)),
            ("th32ModuleID", ctypes.c_ulong), ("cntThreads", ctypes.c_ulong),
            ("th32ParentProcessID", ctypes.c_ulong), ("pcPriClassBase", ctypes.c_long),
            ("dwFlags", ctypes.c_ulong), ("szExeFile", ctypes.c_wchar * 260),
        ]

    snap = k32.CreateToolhelp32Snapshot(0x2, 0)
    entry = PROCESSENTRY32W()
    entry.dwSize = ctypes.sizeof(entry)
    exact = 0
    fallback = 0
    ok = k32.Process32FirstW(snap, ctypes.byref(entry))
    while ok:
        name = entry.szExeFile.lower()
        # The launcher is "Overwatch Launcher.exe"; the game is exactly
        # "Overwatch.exe".  Prefer the game even if the launcher enumerates
        # first, and only fall back to any Overwatch-named process.
        if name == "overwatch.exe":
            exact = entry.th32ProcessID
        elif "overwatch" in name and not fallback:
            fallback = entry.th32ProcessID
        ok = k32.Process32NextW(snap, ctypes.byref(entry))
    k32.CloseHandle(snap)
    return exact or fallback


def focus_window(pid: int) -> bool:
    u32.SetForegroundWindow.restype = ctypes.c_bool
    u32.ShowWindow.restype = ctypes.c_bool
    u32.GetForegroundWindow.restype = ctypes.c_void_p
    u32.GetWindowThreadProcessId.restype = ctypes.c_ulong
    u32.AttachThreadInput.restype = ctypes.c_bool
    u32.EnumWindows.restype = ctypes.c_bool

    hwnds = []

    @ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)
    def cb(hwnd, _l):
        wpid = ctypes.c_ulong(0)
        u32.GetWindowThreadProcessId(ctypes.c_void_p(hwnd), ctypes.byref(wpid))
        if wpid.value == pid:
            buf = ctypes.create_unicode_buffer(512)
            u32.GetWindowTextW(ctypes.c_void_p(hwnd), buf, 512)
            if u32.IsWindowVisible(ctypes.c_void_p(hwnd)) and buf.value:
                hwnds.append(hwnd)
        return True

    u32.EnumWindows(cb, None)
    if not hwnds:
        return False
    h = hwnds[0]
    u32.ShowWindow(ctypes.c_void_p(h), 9)
    fg = u32.GetForegroundWindow()
    fg_pid = ctypes.c_ulong(0)
    their = u32.GetWindowThreadProcessId(ctypes.c_void_p(fg), ctypes.byref(fg_pid))
    me = k32.GetCurrentThreadId()
    u32.AttachThreadInput(me, their, True)
    u32.SetForegroundWindow(ctypes.c_void_p(h))
    u32.AttachThreadInput(me, their, False)
    time.sleep(0.4)
    fg2 = u32.GetForegroundWindow()
    fg2_pid = ctypes.c_ulong(0)
    u32.GetWindowThreadProcessId(ctypes.c_void_p(fg2), ctypes.byref(fg2_pid))
    return fg2_pid.value == pid


def send_key(name: str, down: bool):
    flags = SCANCODE | (0 if down else KEYUP)
    u32.keybd_event(VK[name], SC[name], flags, None)


def discover_movers(handle) -> list[int]:
    """All objects in the heap band whose first qword is a mover vtable."""
    vt = np.array(VTABLES, dtype="<u8")
    movers = []
    for base, size, protect, _kind, _alloc in DP.regions(handle):
        if protect & (DP.PAGE_NOACCESS | DP.PAGE_GUARD):
            continue
        lo = max(base, BAND_BASE)
        hi = min(base + size, BAND_BASE + BAND_SIZE)
        if lo >= hi:
            continue
        blob = DP.read_region(handle, lo, hi - lo)
        if blob:
            words = np.frombuffer(blob[: len(blob) // 8 * 8], dtype="<u8")
            idx = np.nonzero(np.isin(words, vt))[0]
            for i in idx:
                movers.append(lo + int(i) * 8)
    return movers


def discover_owners(handle, movers: list[int]) -> list[int]:
    owners = []
    for m in movers:
        blob = DP.read_region(handle, m, 0x1000)
        if not blob:
            continue
        words = np.frombuffer(blob[: len(blob) // 8 * 8], dtype="<u8")
        hits = np.nonzero(words == m)[0]
        for i in hits:
            owners.append(m + int(i) * 8)
    return owners


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else find_pid()
    key = args[args.index("--key") + 1] if "--key" in args else "d"
    walk_ms = int(args[args.index("--walk-ms") + 1]) if "--walk-ms" in args else 7000
    idle_ms = int(args[args.index("--idle-ms") + 1]) if "--idle-ms" in args else 700
    if not pid:
        print("no Overwatch process found")
        return 1
    handle = DP.open_target(pid)

    movers = discover_movers(handle)
    owners = discover_owners(handle, movers)
    print(f"pid {pid}: {len(movers)} mover(s), {len(owners)} owner pointer(s)")
    if not movers:
        print("no movers found; is the module base still 0x7ff70e520000?")
        return 1

    # watch spans: owners (0x1000), movers (0x140)
    spans: list[tuple[int, int]] = []
    for a in owners:
        spans.append((a - 0x100, 0x1000))
    for a in movers:
        spans.append((a, 0x140))

    # one pointer level out of the owners
    ptr_targets: set[int] = set()
    for a in owners:
        blob = DP.read_region(handle, a - 0x100, 0x1000)
        if not blob:
            continue
        words = np.frombuffer(blob[: len(blob) // 8 * 8], dtype="<u8")
        for v in words:
            v = int(v)
            if HEAP_LO <= v < HEAP_HI and (v & 7) == 0:
                ptr_targets.add(v)
    print(f"{len(ptr_targets)} pointer target(s) from owners (1 level)")
    for base in list(ptr_targets)[:1200]:
        spans.append((base, 0x800))

    spans.sort()
    merged: list[list[int]] = []
    for base, size in spans:
        if merged and base <= merged[-1][0] + merged[-1][1] + 0x10:
            end = max(merged[-1][0] + merged[-1][1], base + size)
            merged[-1][1] = end - merged[-1][0]
        else:
            merged.append([base, size])
    total = sum(s for _, s in merged)
    print(f"watching {len(merged)} span(s), {total} bytes")

    def snapshot() -> dict[int, bytes]:
        out = {}
        for base, size in merged:
            b = DP.read_region(handle, base, size)
            if b:
                out[base] = b
        return out

    if not focus_window(pid):
        print("WARNING: could not confirm game window is foreground; keys may not land")

    samples: list[tuple[float, str, dict[int, bytes]]] = []
    t0 = time.time()

    def take(tag: str):
        s = snapshot()
        samples.append((time.time() - t0, tag, s))

    print("pre-walk idle samples...")
    take("pre")
    time.sleep(idle_ms / 1000.0)
    take("pre")

    print(f"HOLD '{key}' NOW ({walk_ms} ms)")
    send_key(key, True)
    t_walk = time.time()
    while (time.time() - t_walk) * 1000 < walk_ms:
        take("walk")
        time.sleep(0.12)
    send_key(key, False)

    time.sleep(idle_ms / 1000.0)
    take("post")
    time.sleep(idle_ms / 1000.0)
    take("post")
    print(f"{len(samples)} sample(s)")

    pre = [s for s in samples if s[1] == "pre"]
    walk = [s for s in samples if s[1] == "walk"]
    post = [s for s in samples if s[1] == "post"]

    pos_hits: list[tuple[float, int, tuple]] = []
    vel_hits: list[tuple[float, int, tuple]] = []
    changers: list[tuple[int, int]] = []
    mid_walk = walk[len(walk) // 2]
    for base, size in merged:
        b_pre0 = pre[0][2].get(base)
        b_pre1 = pre[1][2].get(base)
        b_post = post[-1][2].get(base)
        b_mid = mid_walk[2].get(base)
        if b_pre0 is None or b_pre1 is None:
            continue
        n4 = min(len(b_pre0), len(b_pre1)) // 4 * 4
        if n4 < 16:
            continue
        f0 = np.frombuffer(b_pre0[:n4], dtype="<f4")
        f1 = np.frombuffer(b_pre1[:n4], dtype="<f4")
        still = np.abs(f0 - f1) < 1e-4
        if b_post is not None and b_mid is not None:
            a = np.frombuffer(b_pre0[:n4], dtype="<u4")
            b = np.frombuffer(b_mid[:n4], dtype="<u4")
            nch = int(np.count_nonzero(a != b))
            if nch:
                changers.append((nch, base))
        if b_post is None:
            continue
        fp = np.frombuffer(b_post[:n4], dtype="<f4")
        n = len(f0) - 2
        for i in range(n):
            if not (still[i] and still[i + 1] and still[i + 2]):
                continue
            x0, y0, z0 = float(f0[i]), float(f0[i + 1]), float(f0[i + 2])
            if not (np.isfinite(x0) and np.isfinite(y0) and np.isfinite(z0)):
                continue
            if abs(x0) > 4000 or abs(z0) > 4000:
                continue
            dx, dy, dz = float(fp[i] - x0), float(fp[i + 1] - y0), float(fp[i + 2] - z0)
            if np.isfinite(dx) and np.isfinite(dy) and np.isfinite(dz):
                d = (dx * dx + dz * dz) ** 0.5
                if d > 2.0:
                    pos_hits.append((d, base + i * 4, (x0, y0, z0, x0 + dx, y0 + dy, z0 + dz)))
        # velocity candidates over the walk phase
        for i in range(n):
            if not (still[i] and still[i + 1] and still[i + 2]):
                continue
            x0, y0, z0 = float(f0[i]), float(f0[i + 1]), float(f0[i + 2])
            if not (np.isfinite(x0) and np.isfinite(y0) and np.isfinite(z0)):
                continue
            if abs(x0) > 4000 or abs(z0) > 4000:
                continue
            best = 0.0
            for t, _tag, s in walk:
                fb = s.get(base)
                if fb is None:
                    continue
                fw = np.frombuffer(fb[: len(fb) // 4 * 4], dtype="<f4")
                if i + 2 >= len(fw):
                    continue
                dx = float(fw[i] - x0)
                dz = float(fw[i + 2] - z0)
                dt = t - pre[1][0]
                if dt <= 0:
                    continue
                sp = (dx * dx + dz * dz) ** 0.5 / dt
                if sp > best:
                    best = sp
            if 3.0 <= best <= 8.5:
                vel_hits.append((best, base + i * 4, (x0, y0, z0)))

    print(f"\n{len(changers)} span(s) changed u32 between pre-idle and mid-walk; top:")
    for nch, base in sorted(changers, reverse=True)[:15]:
        print(f"  {base:#014x}  {nch} u32")
    print(f"\n{len(pos_hits)} position candidate(s) (still before, moved >2 m, stayed):")
    for d, addr, v in sorted(pos_hits, reverse=True)[:40]:
        print(f"  {addr:#014x}  moved {d:7.2f} m   ({v[0]:9.2f},{v[1]:8.2f},{v[2]:9.2f})"
              f" -> ({v[3]:9.2f},{v[4]:8.2f},{v[5]:9.2f})")
    print(f"\n{len(vel_hits)} velocity candidate(s) (3-8.5 m/s during walk):")
    for sp, addr, v in sorted(vel_hits, reverse=True)[:40]:
        print(f"  {addr:#014x}  ~{sp:5.2f} m/s   ({v[0]:9.2f},{v[1]:8.2f},{v[2]:9.2f})")

    # raw series for the best position hit
    if pos_hits:
        d, addr, _ = sorted(pos_hits, reverse=True)[0]
        base = None
        for b, s in merged:
            if b <= addr < b + s:
                base = b
                break
        print(f"\nbest position hit {addr:#014x}; per-sample triple:")
        for t, tag, s in samples:
            if base not in s:
                continue
            off = addr - base
            fw = np.frombuffer(s[base][: len(s[base]) // 4 * 4], dtype="<f4")
            k = off // 4
            if k + 2 < len(fw):
                print(f"  t+{t:6.3f} {tag:5} ({fw[k]:10.3f},{fw[k+1]:10.3f},{fw[k+2]:10.3f})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
