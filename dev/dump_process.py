"""Read a process's memory, and write a read-only full-memory minidump of it.

    python dev/dump_process.py <pid|name> <out.dmp>

Uses dbghelp!MiniDumpWriteDump with MiniDumpWithPrivateReadWriteMemory by
default; `--full` adds MiniDumpWithFullMemory. On the retail game the
private-RW default returns E_INVALIDARG; `--normal` and `--full` work. It only
reads the target; the dump file is written by us. Needs the same user as the
target (or elevation).

The module is also the toolbox's **process memory** half: `open_target`,
`regions` and `read_region` are what the live tools use to read a running
client. Those open the process with read and query rights only - no write, no
injection, no hooking (see AGENTS.md).
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

# Memory protection and type bits (winnt.h).
PAGE_NOACCESS = 0x01
PAGE_READONLY = 0x02
PAGE_READWRITE = 0x04
PAGE_WRITECOPY = 0x08
PAGE_EXECUTE = 0x10
PAGE_EXECUTE_READ = 0x20
PAGE_EXECUTE_READWRITE = 0x40
PAGE_EXECUTE_WRITECOPY = 0x80
PAGE_GUARD = 0x100
MEM_COMMIT = 0x1000
MEM_IMAGE = 0x1000000
MEM_MAPPED = 0x40000
MEM_PRIVATE = 0x20000

# The address space of a 64-bit process, and the granularity the kernel
# reports regions in.
MAX_ADDRESS = 0x7FFFFFFF0000
ALLOCATION_GRANULARITY = 0x10000


class MEMORY_BASIC_INFORMATION(ctypes.Structure):
    _fields_ = [
        ("BaseAddress", ctypes.c_void_p),
        ("AllocationBase", ctypes.c_void_p),
        ("AllocationProtect", wt.DWORD),
        ("PartitionId", wt.WORD),
        ("RegionSize", ctypes.c_size_t),
        ("State", wt.DWORD),
        ("Protect", wt.DWORD),
        ("Type", wt.DWORD),
    ]


def _kernel32():
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.OpenProcess.restype = wt.HANDLE
    k32.OpenProcess.argtypes = (wt.DWORD, wt.BOOL, wt.DWORD)
    k32.CloseHandle.argtypes = (wt.HANDLE,)
    k32.VirtualQueryEx.restype = ctypes.c_size_t
    k32.VirtualQueryEx.argtypes = (wt.HANDLE, ctypes.c_void_p,
                                   ctypes.POINTER(MEMORY_BASIC_INFORMATION),
                                   ctypes.c_size_t)
    k32.ReadProcessMemory.restype = wt.BOOL
    k32.ReadProcessMemory.argtypes = (wt.HANDLE, ctypes.c_void_p, ctypes.c_void_p,
                                      ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t))
    return k32


def open_target(pid: int | str):
    """Open a process for reading: query and VM_READ, nothing else."""
    if isinstance(pid, str):
        pid = find_pid(pid)
    k32 = _kernel32()
    handle = k32.OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, False, pid)
    if not handle:
        raise OSError(f"OpenProcess({pid}) failed: {ctypes.get_last_error()}")
    return handle


def regions(handle) -> list[tuple[int, int, int, int, int]]:
    """Every committed region: (base, size, protect, kind, allocation base).

    The fallback walker for the live tools: it asks the kernel about the
    whole address space, which is slow but never misses a region.
    """
    k32 = _kernel32()
    out = []
    addr = 0
    while addr < MAX_ADDRESS:
        mbi = MEMORY_BASIC_INFORMATION()
        got = k32.VirtualQueryEx(handle, ctypes.c_void_p(addr),
                                 ctypes.byref(mbi), ctypes.sizeof(mbi))
        if not got:
            break
        base = mbi.BaseAddress or 0
        size = mbi.RegionSize
        if size == 0:
            break
        if mbi.State == MEM_COMMIT:
            out.append((base, size, mbi.Protect, mbi.Type, mbi.AllocationBase or 0))
        addr = base + size
    return out


def read_region(handle, addr: int, size: int) -> bytes:
    """Read up to `size` bytes at `addr`; a short read is not an error."""
    if size <= 0:
        return b""
    k32 = _kernel32()
    buf = ctypes.create_string_buffer(size)
    got = ctypes.c_size_t(0)
    ok = k32.ReadProcessMemory(handle, ctypes.c_void_p(addr), buf, size,
                               ctypes.byref(got))
    if not ok:
        return b""
    return buf.raw[: got.value]


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
