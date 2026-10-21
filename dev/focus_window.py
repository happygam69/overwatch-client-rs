"""Bring a process's main window to the foreground (un-minimise + focus).

Used to unblock a live read: a backgrounded or minimised game stops rendering
frames, and therefore stops animating, which is what the animation probes need.

This is a desktop action, not a process action: it calls ShowWindow and
SetForegroundWindow on the window the process owns. It never reads or writes the
target process's memory, and it is reversible — anything else can be focused again.

Windows only foregrounds a window whose process already has the foreground, so the
usual AttachThreadInput dance is used to borrow that right for the call.

Usage:
    python focus_window.py --pid N
"""

from __future__ import annotations

import ctypes
import sys
import ctypes.wintypes as wt

u = ctypes.windll.user32
k32 = ctypes.windll.kernel32

SW_RESTORE = 9
GA_ROOT = 2


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else 0
    if not pid:
        print(__doc__)
        return 2

    # the process's largest visible top-level window
    best: list[tuple[int, int]] = []          # (area, hwnd)

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def visit(hwnd, _lparam):
        owner = wt.DWORD(0)
        u.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value != pid or not u.IsWindowVisible(hwnd):
            return True
        rect = wt.RECT()
        if not u.GetWindowRect(hwnd, ctypes.byref(rect)):
            return True
        area = (rect.right - rect.left) * (rect.bottom - rect.top)
        if area > 0:
            best.append((area, hwnd))
        return True

    u.EnumWindows(visit, 0)
    if not best:
        print(f"no visible window for pid {pid}")
        return 1
    best.sort(reverse=True)
    area, hwnd = best[0]
    print(f"pid {pid}: window 0x{hwnd:x} ({area} px²), currently "
          f"{'minimised' if u.IsIconic(hwnd) else 'shown'}")

    if u.IsIconic(hwnd):
        u.ShowWindow(hwnd, SW_RESTORE)

    fg = u.GetForegroundWindow()
    fg_thread = u.GetWindowThreadProcessId(fg, None)
    my_thread = k32.GetCurrentThreadId()
    attached = False
    if fg_thread != my_thread:
        attached = bool(u.AttachThreadInput(fg_thread, my_thread, True))
    try:
        u.BringWindowToTop(hwnd)
        u.SetForegroundWindow(hwnd)
    finally:
        if attached:
            u.AttachThreadInput(fg_thread, my_thread, False)

    now = u.GetForegroundWindow()
    owner = wt.DWORD(0)
    u.GetWindowThreadProcessId(now, ctypes.byref(owner))
    print(f"foreground is now pid {owner.value} (0x{now:x})")
    return 0 if owner.value == pid else 1


if __name__ == "__main__":
    raise SystemExit(main())
