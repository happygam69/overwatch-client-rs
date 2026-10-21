"""Grab a page that the client keeps marked no-access, by polling for the moment
it becomes readable.

Measured fact this exists for: the animation record handlers live on pages whose
protection is `PAGE_NOACCESS` (`0x1`) while the dispatcher next to them is
`PAGE_EXECUTE_READ` (`0x20`). A normal read-only capture skips those pages, and a
direct ReadProcessMemory fails, so the payload layout behind them is unreadable
by static means. But a page marked no-access must be flipped readable for the
client to execute the code on it, so a fast poll can catch it in that window -
no writes, no protection changes, no injection: just reads that mostly fail.

Usage:
    python live_page_grab.py --pid N --page 0x7ff613edd000 [--seconds 60] [--out DIR]

Read-only: OpenProcess(VM_READ | VM_QUERY), ReadProcessMemory only. Nothing about
the target is modified; a failed read is the normal case and is counted.
"""

from __future__ import annotations

import ctypes
import os
import sys
import time

import dump_process as DP

k32 = ctypes.WinDLL("kernel32", use_last_error=True)


def protect_of(handle, addr: int) -> int:
    class MBI(ctypes.Structure):
        _fields_ = [("BaseAddress", ctypes.c_void_p), ("AllocationBase", ctypes.c_void_p),
                    ("AllocationProtect", ctypes.c_ulong), ("RegionSize", ctypes.c_size_t),
                    ("State", ctypes.c_ulong), ("Protect", ctypes.c_ulong), ("Type", ctypes.c_ulong)]
    mbi = MBI()
    if k32.VirtualQueryEx(handle, ctypes.c_void_p(addr), ctypes.byref(mbi), ctypes.sizeof(mbi)):
        return mbi.Protect
    return 0


def main() -> int:
    args = sys.argv[1:]
    def flag(name, default=None):
        return args[args.index(name) + 1] if name in args else default

    pid = int(flag("--pid", "0"))
    page = int(flag("--page", "0"), 16)
    seconds = float(flag("--seconds", "60"))
    out = flag("--out", "D:/ow-dumps")
    if not pid or not page:
        print(__doc__)
        return 2
    handle = DP.open_target(pid)
    print(f"polling page 0x{page:x} for {seconds:.0f}s (protect now 0x{protect_of(handle, page):x})")

    tries = readable = 0
    first = None
    grabbed = None
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        data = DP.read_region(handle, page, 0x1000)
        tries += 1
        if data and any(data):
            readable += 1
            if first is None:
                first = time.monotonic()
                grabbed = data
                print(f"  readable at try {tries} ({(seconds - (deadline - time.monotonic())):.1f}s in)")
                break
        time.sleep(0.002)
    print(f"  {tries} attempt(s), {readable} readable")
    if grabbed:
        os.makedirs(out, exist_ok=True)
        path = os.path.join(out, f"grabbed_{page:012x}.bin")
        with open(path, "wb") as fh:
            fh.write(grabbed)
        print(f"  saved {len(grabbed)} B to {path}")
        return 0
    print("  the page never became readable in the window")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
