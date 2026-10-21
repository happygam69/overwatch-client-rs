"""Continuous read-only warm-page accumulator for the client.

The client decrypts a code page only while it is in use and lets it go cold
again.  Three states matter and only one is wanted: a cold page reads as
zeros; a still-encrypted page has entropy ~8.0 and *more* non-zero bytes than
real code (so "most non-zero" would prefer ciphertext); real code sits near
entropy 6.0-6.5.  A snapshot is therefore accepted only when it has enough
non-zero bytes *and* code-like entropy.  The tool keeps re-reading the module
for the whole session and stores the most-populated accepted snapshot of every
page it ever catches, keyed by the flattened-image address (v2b) so captures
from different sessions merge exactly.

Usage:
    python warm_poll.py --wait [--out DIR] [--autopilot] [--all-regions]
                        [--seconds N] [--priority 0xv2b,0xv2b,...]

Press Enter in this console to drop a phase marker; with --autopilot the
first marker also starts the auto-play (walk/look/fire), never before, so
keys cannot land in a menu. A line starting with a backslash stops the run
and writes the index (Ctrl+C also works, but the terminal has to be focused).

Read-only: OpenProcess(VM_READ | VM_QUERY), ReadProcessMemory only.
"""

from __future__ import annotations

import ctypes
import ctypes.wintypes as wt
import json
import os
import sys
import threading
import time

import numpy as np

import dump_process as DP
from hunt_actor import find_pid, focus_window, send_key

V2B_BASE = 0x7FF7C4D20000
PAGE = 0x1000
CHUNK = 0x40000
WARM_MIN = 64
ENT_MAX = 7.2
EXEC_MASK = 0x10 | 0x20 | 0x40 | 0x80
PROCESS_QUERY_INFORMATION = 0x0400
PROCESS_VM_READ = 0x0010
TH32CS_SNAPMODULE = 0x00000008
TH32CS_SNAPMODULE32 = 0x00000010

# v2b pages read every iteration (fast, catches one-shot startup stubs).
PRIORITY_V2B = [
    0x7FF7C52C2000,  # camera config registration stub band
    0x7FF7C52C3000,
    0x7FF7C52C4000,
    0x7FF7C52A1000,  # config getter family
    0x7FF7C56B2000,  # first-person camera / near-far setup
    0x7FF7C56B3000,
    0x7FF7C58E0000,  # animation evaluator band
    0x7FF7C58E1000,
    0x7FF7C58EA000,
    0x7FF7C58F2000,
]

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
u32 = ctypes.WinDLL("user32", use_last_error=True)
k32.OpenProcess.restype = wt.HANDLE
k32.OpenProcess.argtypes = [wt.DWORD, wt.BOOL, wt.DWORD]
k32.CloseHandle.argtypes = [wt.HANDLE]
k32.CreateToolhelp32Snapshot.restype = wt.HANDLE
k32.CreateToolhelp32Snapshot.argtypes = [wt.DWORD, wt.DWORD]
u32.mouse_event.argtypes = [wt.DWORD, ctypes.c_long, ctypes.c_long, wt.DWORD,
                            ctypes.c_void_p]


class MODULEENTRY32W(ctypes.Structure):
    _fields_ = [
        ("dwSize", wt.DWORD),
        ("th32ModuleID", wt.DWORD),
        ("th32ProcessID", wt.DWORD),
        ("GlblcntUsage", wt.DWORD),
        ("ProccntUsage", wt.DWORD),
        ("modBaseAddr", ctypes.c_void_p),
        ("modBaseSize", wt.DWORD),
        ("hModule", ctypes.c_void_p),
        ("szModule", ctypes.c_wchar * 256),
        ("szExePath", ctypes.c_wchar * 260),
    ]


def list_modules(pid: int) -> list[tuple[str, int, int]]:
    snap = k32.CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)
    if not snap or snap == ctypes.c_void_p(-1).value:
        return []
    k32.Module32FirstW.argtypes = [wt.HANDLE, ctypes.POINTER(MODULEENTRY32W)]
    k32.Module32NextW.argtypes = [wt.HANDLE, ctypes.POINTER(MODULEENTRY32W)]
    out = []
    entry = MODULEENTRY32W()
    entry.dwSize = ctypes.sizeof(entry)
    ok = k32.Module32FirstW(snap, ctypes.byref(entry))
    while ok:
        out.append((entry.szModule, int(entry.modBaseAddr or 0),
                    int(entry.modBaseSize)))
        ok = k32.Module32NextW(snap, ctypes.byref(entry))
    k32.CloseHandle(snap)
    return out


def classify(blob: bytes) -> tuple[int, float]:
    """(non-zero bytes, byte entropy) of one page snapshot."""
    arr = np.frombuffer(blob, dtype=np.uint8)
    counts = np.bincount(arr, minlength=256).astype(np.float64)
    nz = int(len(blob) - counts[0])
    p = counts / len(blob)
    ent = float(-(p[p > 0] * np.log2(p[p > 0])).sum())
    return nz, ent


def open_retry(pid: int, tries: int = 600):
    for _ in range(tries):
        handle = k32.OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, False, pid)
        if handle:
            return handle
        time.sleep(0.05)
    return None


def main() -> int:
    args = sys.argv[1:]
    def opt(name, default=None):
        return args[args.index(name) + 1] if name in args else default

    if "--help" in args or "-h" in args:
        print(__doc__)
        return 0

    out_root = opt("--out", "D:/ow-dumps/warm_poll")
    wait = "--wait" in args
    autopilot = "--autopilot" in args
    all_regions = "--all-regions" in args
    seconds = float(opt("--seconds", "0") or 0)
    prio_arg = opt("--priority", "")
    prio = [int(x, 0) for x in prio_arg.split(",") if x] or PRIORITY_V2B

    pid = find_pid()
    while not pid:
        if not wait:
            print("no Overwatch process found (use --wait to block until it starts)")
            return 1
        time.sleep(0.05)
        pid = find_pid()
    print(f"pid {pid}: attaching")

    handle = open_retry(pid)
    if not handle:
        print(f"OpenProcess failed, error {ctypes.get_last_error()}")
        return 1

    mod_name, mod_base, mod_size = "", 0, 0
    for _ in range(1200):
        mods = [m for m in list_modules(pid) if m[2] >= 0x2000000]
        if mods:
            # The GPU driver DLLs can be larger than the game image; select
            # the game by name, not by size.
            main = next(
                (m for m in mods if m[0].lower() == "overwatch.exe"), None)
            if main is None:
                mods.sort(key=lambda m: -m[2])
                main = mods[0]
            mod_name, mod_base, mod_size = main
            print(f"module {mod_name} base {mod_base:#x} size {mod_size:#x}")
            for name, base, size in mods[1:5]:
                print(f"  also large: {name} {base:#x} {size:#x}")
            break
        time.sleep(0.1)
    if not mod_base:
        print("client module never appeared")
        return 1

    delta = mod_base - V2B_BASE

    def to_session(v2b: int) -> int:
        return v2b + delta

    def to_v2b(sess: int) -> int:
        return sess - delta

    stamp = time.strftime("%Y%m%d_%H%M%S")
    out = os.path.join(out_root, f"run_{stamp}")
    pages_dir = os.path.join(out, "pages")
    os.makedirs(pages_dir, exist_ok=True)
    print(f"output: {out}")

    probe = DP.read_region(handle, to_session(0x7FF7C86967A4), 64)
    ok = probe is not None and b"camera.lookInterpolationSpeedInverse" in probe
    print(f"name-table probe: {'warm (translation verified)' if ok else 'cold or unreadable'}")

    pages: dict[int, list] = {}
    phases: list[tuple[float, int]] = []
    t0 = time.time()
    phase = 0
    lock = threading.Lock()

    def record(sess_addr: int, data: bytes) -> bool:
        nonlocal pages
        page = data[:PAGE]
        if len(page) < 64:
            return False
        nz, ent = classify(page)
        if nz < WARM_MIN or ent > ENT_MAX:
            return False
        v2b = to_v2b(sess_addr & ~(PAGE - 1))
        t = time.time() - t0
        with lock:
            ent_page = pages.get(v2b)
            if ent_page is None:
                pages[v2b] = [nz, t, t, phase]
                with open(os.path.join(pages_dir, f"{v2b:012x}.bin"), "wb") as fh:
                    fh.write(page)
                print(f"NEW v2b {v2b:012x} sess {sess_addr:012x} {nz}/4096 e{ent:.2f} t+{t:6.2f}s ph{phase}")
                return True
            ent_page[2] = t
            if nz > ent_page[0]:
                ent_page[0] = nz
                with open(os.path.join(pages_dir, f"{v2b:012x}.bin"), "wb") as fh:
                    fh.write(page)
                return True
        return False

    def sweep_regions() -> list[tuple[int, int]]:
        regs = []
        for base, size, protect, _kind, _alloc in DP.regions(handle):
            if protect & (DP.PAGE_NOACCESS | DP.PAGE_GUARD):
                continue
            if not all_regions and not (protect & EXEC_MASK):
                continue
            lo = max(base, mod_base)
            hi = min(base + size, mod_base + mod_size)
            if lo < hi:
                regs.append((lo, hi - lo))
        return regs

    stop = threading.Event()

    def input_thread():
        nonlocal phase
        for line in sys.stdin:
            line = line.strip()
            if line.startswith("\\"):
                print(f"stop requested at t+{time.time() - t0:.2f}s")
                stop.set()
                break
            phase += 1
            phases.append((time.time() - t0, phase))
            print(f"phase {phase} marked at t+{time.time() - t0:.2f}s")
            if autopilot:
                auto_start.set()

    auto_start = threading.Event()
    stop_keys = threading.Event()

    def autopilot_thread():
        if not auto_start.wait(timeout=3600):
            return
        if not focus_window(pid):
            print("autopilot: could not confirm game window is foreground")
        held = []

        def hold(keys):
            for k in keys:
                send_key(k, True)
            held.extend(keys)

        def release():
            for k in held:
                send_key(k, False)
            held.clear()

        def drag(px):
            step = 25 if px > 0 else -25
            for _ in range(abs(px) // 25):
                u32.mouse_event(0x0001, step, 0, 0, None)
                time.sleep(0.006)

        def fire(ms=80):
            u32.mouse_event(0x0002, 0, 0, 0, None)
            time.sleep(ms / 1000.0)
            u32.mouse_event(0x0004, 0, 0, 0, None)

        print("autopilot running (Ctrl+C to stop)")
        try:
            while not stop_keys.is_set():
                hold(["w"])
                time.sleep(0.8)
                drag(650 if (phase + int(time.time())) % 2 else -650)
                fire()
                release()
                time.sleep(0.15)
                hold(["d"])
                time.sleep(0.5)
                drag(-420)
                release()
                send_key("space", True); time.sleep(0.12); send_key("space", False)
                send_key("ctrl", True); time.sleep(0.6); send_key("ctrl", False)
                fire()
                send_key("r", True); time.sleep(0.15); send_key("r", False)
                time.sleep(0.3)
        finally:
            release()

    threading.Thread(target=input_thread, daemon=True).start()
    if autopilot:
        threading.Thread(target=autopilot_thread, daemon=True).start()

    regs = sweep_regions()
    ri = 0
    roff = 0

    def next_chunk():
        nonlocal ri, roff
        while ri < len(regs):
            base, size = regs[ri]
            if roff >= size:
                ri += 1
                roff = 0
                continue
            take = min(CHUNK, size - roff)
            addr = base + roff
            roff += take
            return addr, take
        return None

    print(f"sweep: {sum(s for _, s in regs) / 1048576:.1f} MB executable"
          f"{' + data' if all_regions else ''}")
    passes = 0
    budget = 8
    last_report = 0.0
    last_alive = 0.0
    try:
        while not stop.is_set():
            for v2b in prio:
                data = DP.read_region(handle, to_session(v2b), PAGE)
                if data:
                    record(to_session(v2b), data)
            for _ in range(budget):
                chunk = next_chunk()
                if chunk is None:
                    regs = sweep_regions()
                    ri = 0
                    roff = 0
                    passes += 1
                    break
                addr, take = chunk
                data = DP.read_region(handle, addr, take)
                if data is None:
                    for off in range(0, take, PAGE):
                        part = DP.read_region(handle, addr + off, PAGE)
                        if part:
                            record(addr + off, part)
                elif data.count(b"\x00") != len(data):
                    for off in range(0, len(data), PAGE):
                        record(addr + off, data[off:off + PAGE])
            now = time.time() - t0
            if now - last_report > 5.0:
                last_report = now
                print(f"t+{now:6.1f}s  warm pages {len(pages)}  sweeps {passes}")
            if now - last_alive > 1.0:
                last_alive = now
                if not find_pid():
                    print("process gone")
                    break
            if seconds and now > seconds:
                break
            time.sleep(0.001)
    except KeyboardInterrupt:
        print("stopped")
    finally:
        stop_keys.set()

    csv_path = os.path.join(out, "warm.csv")
    with open(csv_path, "w", encoding="utf-8") as fh:
        fh.write("v2b,session,nonzero,first_s,last_s,phase\n")
        for v2b, (nz, first, last, ph) in sorted(pages.items()):
            fh.write(f"{v2b:012x},{v2b + delta:012x},{nz},{first:.3f},{last:.3f},{ph}\n")
    with open(os.path.join(out, "meta.json"), "w", encoding="utf-8") as fh:
        json.dump({
            "pid": pid,
            "module": mod_name,
            "module_base": f"{mod_base:012x}",
            "module_size": f"{mod_size:x}",
            "v2b_base": f"{V2B_BASE:012x}",
            "delta": f"{delta:#x}",
            "translation": "session = v2b + delta; v2b = session - delta",
            "started": stamp,
            "seconds": round(time.time() - t0, 1),
            "warm_pages": len(pages),
            "phases": [{"t": t, "phase": p} for t, p in phases],
        }, fh, indent=2)
    print(f"done: {len(pages)} warm page(s), {csv_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
