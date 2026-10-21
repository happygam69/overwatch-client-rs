"""Write a read-only full-memory minidump of a process.

Usage:
    python dev/dump_process.py <pid|name> <out.dmp>

Uses dbghelp!MiniDumpWriteDump with MiniDumpWithPrivateReadWriteMemory by
default; `--full` adds MiniDumpWithFullMemory. On the retail game the
private-RW default returns E_INVALIDARG; `--normal` and `--full` work. It only
reads the target; the dump file is written by us. Needs the same user as the
target (or elevation).
"""
from __future__ import annotations

import ctypes
import ctypes.wintypes as wt
import os
import sys

PROCESS_QUERY_INFORMATION = 0x0400
PROCESS_VM_READ = 0x0010
GENERIC_WRITE = 0x40000000
CREATE_ALWAYS = 2
FILE_ATTRIBUTE_NORMAL = 0x80
MiniDumpWithFullMemory = 0x00000002
MiniDumpWithPrivateReadWriteMemory = 0x00000200
MiniDumpWithThreadInfo = 0x00001000


def find_pid(name: str) -> int:
    if name.isdigit():
        return int(name)
    import subprocess
    out = subprocess.check_output(
        ["tasklist", "/FO", "CSV", "/NH", "/FI", f"IMAGENAME eq {name}"],
        text=True, errors="replace")
    for line in out.splitlines():
        parts = [p.strip('"') for p in line.split('","')]
        if len(parts) >= 2 and parts[0].lower() == name.lower():
            return int(parts[1])
    raise SystemExit(f"process not found: {name}")


def main() -> int:
    args = [a for a in sys.argv[1:] if a not in ("--full", "--normal")]
    if len(args) != 2:
        print(__doc__)
        return 2
    pid = find_pid(args[0])
    out = args[1]
    if "--full" in sys.argv:
        flags = MiniDumpWithFullMemory | MiniDumpWithThreadInfo
    elif "--normal" in sys.argv:
        flags = 0
    else:
        flags = MiniDumpWithPrivateReadWriteMemory | MiniDumpWithThreadInfo
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)

    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    dbg = ctypes.WinDLL("dbghelp", use_last_error=True)

    k32.OpenProcess.restype = wt.HANDLE
    k32.OpenProcess.argtypes = (wt.DWORD, wt.BOOL, wt.DWORD)
    k32.CreateFileW.restype = wt.HANDLE
    k32.CreateFileW.argtypes = (wt.LPCWSTR, wt.DWORD, wt.DWORD, ctypes.c_void_p,
                                wt.DWORD, wt.DWORD, wt.HANDLE)
    k32.CloseHandle.argtypes = (wt.HANDLE,)

    hproc = k32.OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, False, pid)
    if not hproc:
        raise SystemExit(f"OpenProcess failed: {ctypes.get_last_error()}")
    hfile = k32.CreateFileW(out, GENERIC_WRITE, 0, None, CREATE_ALWAYS,
                            FILE_ATTRIBUTE_NORMAL, None)
    if hfile in (None, wt.HANDLE(-1).value):
        k32.CloseHandle(hproc)
        raise SystemExit(f"CreateFileW failed: {ctypes.get_last_error()}")
    try:
        dbg.MiniDumpWriteDump.restype = wt.BOOL
        dbg.MiniDumpWriteDump.argtypes = (wt.HANDLE, wt.DWORD, wt.HANDLE,
                                          wt.DWORD, ctypes.c_void_p,
                                          ctypes.c_void_p, ctypes.c_void_p)
        ok = dbg.MiniDumpWriteDump(hproc, pid, hfile, flags, None, None, None)
        if not ok:
            err = ctypes.get_last_error()
            hint = ""
            if err == -2147024809:
                hint = " (E_INVALIDARG; on the game try --normal or --full)"
            raise SystemExit(f"MiniDumpWriteDump failed: {err}{hint}")
    finally:
        k32.CloseHandle(hfile)
        k32.CloseHandle(hproc)
    print(f"wrote {out} ({os.path.getsize(out)} bytes) pid {pid}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
