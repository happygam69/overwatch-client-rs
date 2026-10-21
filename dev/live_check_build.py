"""Quick live diagnostic: is the running client the build our captures match?

Reads (read-only) the live process's module bytes at the mover vtable offsets
learned from the v2b capture, compares them byte-for-byte against the capture,
and scans the module image for a version-looking string to report the build.
Decides whether a hard-coded address is stale or the scanner has a bug.

    python live_check_build.py [--pid N]
"""

from __future__ import annotations

import ctypes
import re
import sys
from ctypes import wintypes as wt

import regions as R

VT_OFFS = {
    "vt0": (0x31E31F8, 0x7FF7C7F031F8),
    "vt8": (0x31E30C8, 0x7FF7C7F030C8),
    "vt48": (0x31E3160, 0x7FF7C7F03160),
}
RET_STUB_OFF = 0x53A6C0  # the shared `ret` stub, v2b 0x7ff7c525a6c0


def find_pid(name: str) -> int:
    TH32CS_SNAPPROCESS = 0x2

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
    snap = k32.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    entry = PROCESSENTRY32W()
    entry.dwSize = ctypes.sizeof(entry)
    ok = k32.Process32FirstW(snap, ctypes.byref(entry))
    pid = None
    while ok:
        if name.lower() in entry.szExeFile.lower():
            pid = entry.th32ProcessID
            break
        ok = k32.Process32NextW(snap, ctypes.byref(entry))
    k32.CloseHandle(snap)
    return pid


def module_info(pid: int, name: str = "Overwatch.exe") -> tuple[int, int]:
    class MODULEENTRY32W(ctypes.Structure):
        _fields_ = [
            ("dwSize", wt.DWORD), ("th32ModuleID", wt.DWORD), ("th32ProcessID", wt.DWORD),
            ("GlblcntUsage", wt.DWORD), ("ProccntUsage", wt.DWORD),
            ("modBaseAddr", ctypes.c_void_p), ("modBaseSize", wt.DWORD),
            ("hModule", wt.HMODULE), ("szModule", ctypes.c_wchar * 256),
            ("szExePath", ctypes.c_wchar * 260),
        ]

    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.CreateToolhelp32Snapshot.restype = wt.HANDLE
    snap = k32.CreateToolhelp32Snapshot(0x8 | 0x10, pid)
    entry = MODULEENTRY32W()
    entry.dwSize = ctypes.sizeof(entry)
    ok = k32.Module32FirstW(snap, ctypes.byref(entry))
    base = size = None
    while ok:
        if name.lower() in entry.szModule.lower():
            base, size = int(entry.modBaseAddr or 0), int(entry.modBaseSize or 0)
            break
        ok = k32.Module32NextW(snap, ctypes.byref(entry))
    k32.CloseHandle(snap)
    return base or 0, size or 0


def main() -> int:
    pid = None
    for i, a in enumerate(sys.argv):
        if a == "--pid":
            pid = int(sys.argv[i + 1])
    if pid is None:
        pid = find_pid("Overwatch.exe")
    print(f"pid: {pid}")
    base, size = module_info(pid)
    print(f"module base {base:#x} size {size:#x}")

    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    h = k32.OpenProcess(0x0410, False, pid)  # QUERY_INFORMATION | VM_READ
    if not h:
        raise SystemExit(f"OpenProcess failed: {ctypes.get_last_error()}")

    def read(addr: int, length: int) -> bytes | None:
        buf = ctypes.create_string_buffer(length)
        got = ctypes.c_size_t(0)
        ok = k32.ReadProcessMemory(h, ctypes.c_void_p(addr), buf, length, ctypes.byref(got))
        return buf.raw[: got.value] if ok else None

    live = {k: read(base + off, 64) for k, (off, _) in VT_OFFS.items()}
    cap = R.Space(R.load(R.capture_dir("dumps_v2b")))
    same = True
    for k, (off, cap_addr) in VT_OFFS.items():
        capb = cap.read(cap_addr, 64)
        liveb = live[k]
        if liveb is None:
            print(f"{k}: live read FAILED")
            same = False
            continue
        match = capb is not None and liveb == capb
        same &= match
        print(f"{k}: {'MATCH' if match else 'DIFFER'}")
        if not match:
            print(f"   live: {liveb.hex(' ')}")
            print(f"   capt: {capb.hex(' ') if capb else '<none>'}")
    rb = read(base + RET_STUB_OFF, 1)
    print(f"ret-stub byte at base+{RET_STUB_OFF:#x}: {rb.hex() if rb else '<unreadable>'} (expect c3)")
    if live["vt0"]:
        first = int.from_bytes(live["vt0"][:8], "little")
        print(f"vt0[0] = {first:#x}  (module-relative {first - base:#x})")

    # Version-looking strings in the first 8 MB of the image.
    found = []
    for off in range(0, min(size, 0x800000), 0x100000):
        chunk = read(base + off, 0x100000)
        if not chunk:
            continue
        for m in re.finditer(rb"\d{1,2}\.\d{1,2}\.\d{1,3}\.\d{1,6}\.\d{1,6}", chunk):
            s = m.group(0).decode()
            ctx = chunk[max(0, m.start() - 24) : m.end() + 8]
            found.append((s, ctx))
            if len(found) > 12:
                break
        if len(found) > 12:
            break
    seen = set()
    for s, ctx in found:
        if s in seen:
            continue
        seen.add(s)
        printable = "".join(chr(c) if 32 <= c < 127 else "." for c in ctx)
        print(f"version-like: {s}   ctx: {printable[:60]}")
    if not found:
        print("no version-like string found in the first 8 MB")

    if same:
        print("\nVERDICT: module bytes at the vtable offsets match the capture — "
              "same build at these addresses; a zero mover count would point at the scanner.")
    else:
        print("\nVERDICT: bytes DIFFER from the capture — the client build has moved; "
              "hard-coded offsets are stale.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
