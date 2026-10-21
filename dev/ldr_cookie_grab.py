"""Read-only runtime grab of the loader's working-set memory while the game
is up. Uses OpenProcess(VM_READ|VM_QUERY) + ReadProcessMemory only.

Dumps to D:/ow-dumps/live_cookie/<run>/ :
  data.bin      loader .data   (loader_base+0x1df0000, 0x107c00)
  rdata.bin     loader .rdata  (loader_base+0x1d70000, 0x7e000)
  eid_head.bin  loader .eid head (loader_base+0x1f80000, 0x10000)
  gxfg.bin      loader .gxfg   (loader_base+0x1f30000,   0x3000)
  iat.bin       loader IAT     (loader_base+0x1dcdd58,   0x960)
  stubs.bin     first 0x4000 bytes at each unique plausible address in the
                IAT (heap-thunk blob capture)
  teb.bin       first thread's TEB page (read-only; unlocks gs:[0x60] chain)
  peb.bin       PEB page via TEB+0x60 (read-only; +0x18 = Ldr, cookie found
                offline so no offset is assumed here)
  manifest.json pid, bases, sizes, timestamps (+ teb_base/peb/ldr when read)

Usage: python dev/ldr_cookie_grab.py [--pid N]
"""
import ctypes
import datetime
import json
import os
import struct
import sys

import ctypes.wintypes as wt

ntdll = ctypes.WinDLL("ntdll", use_last_error=True)

k32 = ctypes.WinDLL("kernel32", use_last_error=True)

PROCESS_VM_READ = 0x0010
PROCESS_QUERY_INFORMATION = 0x0400
TH32CS_SNAPMODULE = 0x00000008
TH32CS_SNAPMODULE32 = 0x00000010

MODULEENTRY32W_FIELDS = (
    ("dwSize", wt.DWORD),
    ("th32ModuleID", wt.DWORD),
    ("th32ProcessID", wt.DWORD),
    ("GlblcntUsage", wt.DWORD),
    ("ProccntUsage", wt.DWORD),
    ("modBaseAddr", ctypes.c_void_p),
    ("modBaseSize", wt.DWORD),
    ("hModule", ctypes.c_void_p),
    ("szModule", wt.WCHAR * 256),
    ("szExePath", wt.WCHAR * 260),
)


class MODULEENTRY32W(ctypes.Structure):
    _fields_ = MODULEENTRY32W_FIELDS


def find_pid_by_name(name):
    # pid by process name: use toolhelp process snapshot
    TH32CS_SNAPPROCESS = 0x00000002
    PROCESSENTRY32W_FIELDS = (
        ("dwSize", wt.DWORD),
        ("cntUsage", wt.DWORD),
        ("th32ProcessID", wt.DWORD),
        ("th32DefaultHeap", ctypes.c_void_p),
        ("th32ModuleID", wt.DWORD),
        ("cntThreads", wt.DWORD),
        ("th32ParentProcessID", wt.DWORD),
        ("pcPriClassBase", ctypes.c_long),
        ("dwFlags", wt.DWORD),
        ("szExeFile", wt.WCHAR * 260),
    )

    class PROCESSENTRY32W(ctypes.Structure):
        _fields_ = PROCESSENTRY32W_FIELDS

    pe = PROCESSENTRY32W()
    pe.dwSize = ctypes.sizeof(PROCESSENTRY32W)
    hsnap = k32.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    found = []
    if k32.Process32FirstW(hsnap, ctypes.byref(pe)):
        while True:
            if pe.szExeFile.lower() == name.lower():
                found.append(pe.th32ProcessID)
            if not k32.Process32NextW(hsnap, ctypes.byref(pe)):
                break
    k32.CloseHandle(hsnap)
    return found


def modules_of(pid):
    me = MODULEENTRY32W()
    me.dwSize = ctypes.sizeof(MODULEENTRY32W)
    hsnap = k32.CreateToolhelp32Snapshot(
        TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)
    out = []
    if hsnap == -1 or hsnap is None:
        return out
    if k32.Module32FirstW(hsnap, ctypes.byref(me)):
        while True:
            out.append((me.szModule, me.modBaseAddr, me.modBaseSize))
            if not k32.Module32NextW(hsnap, ctypes.byref(me)):
                break
    k32.CloseHandle(hsnap)
    return out


def read_mem(h, addr, size):
    buf = ctypes.create_string_buffer(size)
    n = wt.DWORD(0)
    ok = k32.ReadProcessMemory(h, ctypes.c_void_p(addr), buf,
                               ctypes.c_size_t(size), ctypes.byref(n))
    if not ok or n.value == 0:
        return None
    return buf.raw[:n.value]


def first_thread_teb(pid):
    """TEB base of the first thread in pid, read-only.

    Uses Toolhelp thread snapshot + OpenThread(THREAD_QUERY_INFORMATION)
    + NtQueryInformationThread(ThreadBasicInformation). No suspend, no
    context read, no writes.
    """
    TH32CS_SNAPTHREAD = 0x00000004
    THREAD_QUERY_INFORMATION = 0x0040

    class THREADENTRY32(ctypes.Structure):
        _fields_ = [("dwSize", wt.DWORD),
                    ("cntUsage", wt.DWORD),
                    ("th32ThreadID", wt.DWORD),
                    ("th32OwnerProcessID", wt.DWORD),
                    ("tpBasePri", ctypes.c_long),
                    ("tpDeltaPri", ctypes.c_long),
                    ("dwFlags", wt.DWORD)]

    class THREAD_BASIC_INFORMATION(ctypes.Structure):
        _fields_ = [("ExitStatus", ctypes.c_long),
                    ("TebBaseAddress", ctypes.c_void_p),
                    ("ClientId_UniqueProcess", ctypes.c_size_t),
                    ("ClientId_UniqueThread", ctypes.c_size_t),
                    ("AffinityMask", ctypes.c_size_t),
                    ("Priority", ctypes.c_long),
                    ("BasePriority", ctypes.c_long)]

    te = THREADENTRY32()
    te.dwSize = ctypes.sizeof(THREADENTRY32)
    hsnap = k32.CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)
    if hsnap is None or hsnap == -1:
        return None, None
    tids = []
    try:
        if k32.Thread32First(hsnap, ctypes.byref(te)):
            while True:
                if te.th32OwnerProcessID == pid:
                    tids.append(te.th32ThreadID)
                if not k32.Thread32Next(hsnap, ctypes.byref(te)):
                    break
    finally:
        k32.CloseHandle(hsnap)
    if not tids:
        return None, None
    for tid in tids:
        th = k32.OpenThread(THREAD_QUERY_INFORMATION, False, tid)
        if not th:
            continue
        try:
            tbi = THREAD_BASIC_INFORMATION()
            retlen = ctypes.c_uint(0)
            st = ntdll.NtQueryInformationThread(
                th, 0, ctypes.byref(tbi),
                ctypes.sizeof(tbi), ctypes.byref(retlen))
            if st == 0 and tbi.TebBaseAddress:
                return tid, tbi.TebBaseAddress
        finally:
            k32.CloseHandle(th)
    return None, None


def peb_of(h):
    """PEB address via NtQueryInformationProcess (query-only)."""
    class PBI(ctypes.Structure):
        _fields_ = [("ExitStatus", ctypes.c_long),
                    ("PebBaseAddress", ctypes.c_void_p),
                    ("AffinityMask", ctypes.c_size_t),
                    ("BasePriority", ctypes.c_long),
                    ("UniqueProcessId", ctypes.c_size_t),
                    ("InheritedFromUniqueProcessId", ctypes.c_size_t)]

    pbi = PBI()
    try:
        ntdll.NtQueryInformationProcess.argtypes = [
            ctypes.c_void_p, ctypes.c_uint, ctypes.c_void_p,
            ctypes.c_uint, ctypes.POINTER(ctypes.c_uint)]
        st = ntdll.NtQueryInformationProcess(h, 0, ctypes.byref(pbi),
                                             ctypes.sizeof(pbi), None)
    except Exception:
        return None
    if st != 0:
        return None
    return pbi.PebBaseAddress


def main():
    want = int(sys.argv[sys.argv.index("--pid") + 1]) if "--pid" in sys.argv \
        else None
    pids = find_pid_by_name("Overwatch.exe")
    if want:
        pids = [p for p in pids if p == want] or pids
    if not pids:
        print("no Overwatch.exe found")
        sys.exit(1)
    pid = pids[0]
    h = k32.OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, False,
                        pid)
    if not h:
        print(f"OpenProcess failed err={ctypes.get_last_error()}")
        sys.exit(1)
    mods = modules_of(pid)
    loader = next((m for m in mods if m[0].lower() == "overwatch_loader.dll"),
                  None)
    games = [m for m in mods if m[0].lower() == "overwatch.exe"]
    print(f"pid {pid}; modules seen: {len(mods)}; loader mod: "
          f"{bool(loader)}; exe mod: {bool(games)}")
    if not loader:
        print("Overwatch_loader.dll not mapped in this process (?"
              "unloadable-per-session?)")
    outdir = os.path.join(r"D:\ow-dumps", "live_cookie",
                          f"run_{datetime.datetime.now():%Y%m%d_%H%M%S}")
    os.makedirs(outdir, exist_ok=True)
    manifest = {"pid": pid, "time": datetime.datetime.now().isoformat(),
                "dumped": []}
    if loader:
        lb, ls = loader[1], loader[2]
        manifest["loader_base"] = lb
        manifest["loader_size"] = ls
        print(f"loader base {lb:#x} size {ls:#x}")
        ranges = [("data.bin", 0x1DF0000, 0x107C00),
                  ("rdata.bin", 0x1D70000, 0x7E000),
                  ("eid_head.bin", 0x1F80000, 0x10000),
                  ("gxfg.bin", 0x1F30000, 0x3000),
                  ("tlsdir.bin", 0x1DC2EE0, 0x28),
                  ("tlsarr.bin", 0x1DCD140, 0x100),
                  ("iat.bin", 0x1DCDD58, 0x960)]
        for fname, rva, size in ranges:
            d = read_mem(h, lb + rva, size)
            if d:
                with open(os.path.join(outdir, fname), "wb") as fp:
                    fp.write(d)
                manifest["dumped"].append({"file": fname, "addr": lb + rva,
                                           "size": size})
                print(f"  {fname} {lb + rva:#x} ({size:#x}) ok")
            else:
                print(f"  {fname} {lb + rva:#x}: read failed")
        # heap-thunk blob: unique plausible addresses from the IAT
        iat = read_mem(h, lb + 0x1DCDD58, 0x960)
        if iat:
            vals = [v for (_, v) in iter_unaligned_qwords(iat)
                    if 0x100000000 <= v < 0x7FFFFFFFF000]
            uniq = sorted(set(vals))
            print(f"  IAT plausible pointers: {len(uniq)}; "
                  f"min {min(uniq):#x} max {max(uniq):#x}" if uniq else
                  "  IAT plausible pointers: none")
            if uniq:
                lo, hi = min(uniq), max(uniq)
                span = min(hi + 0x1000 - lo, 0x80000)
                blob = read_mem(h, lo, span)
                if blob:
                    with open(os.path.join(outdir, "stubs.bin"), "wb") as fp:
                        fp.write(blob)
                    manifest["stubs_base"] = lo
                    manifest["stubs_span"] = span
                    print(f"  stubs.bin {lo:#x} span {span:#x} ok")
        # TEB/PEB unlock values (read-only): gs:[0x60] PEB pointer comes
        # from the TEB page; [TEB+0x18] is the second cookie term; the PEB
        # process cookie is found offline by scan, so only the page dumps
        # and the PEB/Ldr addresses are recorded here.
        tid, teb = first_thread_teb(pid)
        if teb:
            tb = read_mem(h, teb, 0x1000)
            if tb:
                with open(os.path.join(outdir, "teb.bin"), "wb") as fp:
                    fp.write(tb)
                manifest["thread_id"] = tid
                manifest["teb_base"] = teb
                manifest["teb_0x18"] = struct.unpack_from("<Q", tb, 0x18)[0]
                manifest["teb_peb_ptr"] = struct.unpack_from("<Q", tb,
                                                             0x60)[0]
                manifest["dumped"].append({"file": "teb.bin",
                                           "addr": teb, "size": 0x1000})
                print(f"  teb.bin {teb:#x} (tid {tid}) ok; "
                      f"[TEB+0x18]={manifest['teb_0x18']:#x} "
                      f"[TEB+0x60]={manifest['teb_peb_ptr']:#x}")
            else:
                print(f"  teb {teb:#x}: read failed")
        else:
            print("  TEB: no thread TEB found (query-only)")
        peb = peb_of(h)
        if peb:
            pb = read_mem(h, peb, 0x1000)
            if pb:
                with open(os.path.join(outdir, "peb.bin"), "wb") as fp:
                    fp.write(pb)
                manifest["peb_base"] = peb
                manifest["peb_ldr"] = struct.unpack_from("<Q", pb, 0x18)[0]
                manifest["dumped"].append({"file": "peb.bin",
                                           "addr": peb, "size": 0x1000})
                print(f"  peb.bin {peb:#x} ok; "
                      f"PEB+0x18 Ldr={manifest['peb_ldr']:#x}")
            else:
                print(f"  peb {peb:#x}: read failed")
        else:
            print("  PEB: query failed")
        # main exe runtime .text quick copy for pagepakv2 later? no - v3 usage
    if games:
        gb, gs = games[0][1], games[0][2]
        manifest["exe_base"] = gb
        manifest["exe_size"] = gs
        print(f"exe base {gb:#x} size {gs:#x}")
    with open(os.path.join(outdir, "manifest.json"), "w") as fp:
        json.dump(manifest, fp, indent=1)
    k32.CloseHandle(h)
    print("done ->", outdir)


def iter_unaligned_qwords(blob):
    end = len(blob) - 8
    for i in range(0, end, 8):
        v = struct.unpack_from("<Q", blob, i)[0]
        yield (i, v)


if __name__ == "__main__":
    main()
