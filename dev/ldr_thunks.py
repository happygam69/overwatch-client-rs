"""Parse the captured heap-thunk blob (stubs.bin): decode each thunk's
destination, match it against the process's loaded system modules, and
name the API via each module's on-disk PE export table. Then map the
loader IAT slots (runtime iat.bin) to API names.

Usage: python dev/ldr_thunks.py [rundir]
"""
import ctypes
import ctypes.wintypes as wt
import json
import os
import struct
import sys

run = sys.argv[1] if len(sys.argv) > 1 else \
    r"D:\ow-dumps\live_cookie\run_20261021_012934"
k32 = ctypes.WinDLL("kernel32", use_last_error=True)
PROCESS_VM_READ = 0x0010
PROCESS_QUERY_INFORMATION = 0x0400
TH32CS_SNAPMODULE = 8
TH32CS_SNAPMODULE32 = 16
MODULEENTRY32W_FIELDS = (
    ("dwSize", wt.DWORD), ("th32ModuleID", wt.DWORD),
    ("th32ProcessID", wt.DWORD), ("GlblcntUsage", wt.DWORD),
    ("ProccntUsage", wt.DWORD), ("modBaseAddr", ctypes.c_void_p),
    ("modBaseSize", wt.DWORD), ("hModule", ctypes.c_void_p),
    ("szModule", wt.WCHAR * 256), ("szExePath", wt.WCHAR * 260),
)


class MODULEENTRY32W(ctypes.Structure):
    _fields_ = MODULEENTRY32W_FIELDS


manifest = json.load(open(run + "\\manifest.json"))
pid = manifest["pid"]
h = k32.OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, False, pid)
print("handle open:", bool(h), "pid:", pid)

me = MODULEENTRY32W()
me.dwSize = ctypes.sizeof(MODULEENTRY32W)
hsnap = k32.CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32,
                                     pid)
mods = []
if k32.Module32FirstW(hsnap, ctypes.byref(me)):
    while True:
        mods.append((me.szModule, me.modBaseAddr, me.modBaseSize,
                     me.szExePath))
        if not k32.Module32NextW(hsnap, ctypes.byref(me)):
            break
k32.CloseHandle(hsnap)
print("modules:", len(mods))
sys_modules = [m for m in mods if 0x7FF000000000 <= m[1] < 0x7FFFFFFFFFFFF
               and m[0].lower() not in ("overwatch_loader.dll",
                                        "overwatch.exe")]


def mod_of(addr):
    for name, base, size, path in sys_modules:
        if base <= addr < base + size:
            return name, base, size, path
    return None


stub = open(run + "\\stubs.bin", "rb").read()
base = manifest["stubs_base"]
iat = open(run + "\\iat.bin", "rb").read()

# decode stub patterns
import capstone as cs
md = cs.Cs(cs.CS_ARCH_X86, cs.CS_MODE_64)
md.detail = True
dests = {}
for off in range(0, len(stub) - 12):
    # mov rax, imm64 ; jmp rax  -> 48 B8 <imm64> FF E0
    if stub[off] == 0x48 and stub[off + 1] == 0xB8:
        try:
            ins = list(md.disasm(stub[off:off + 0x20], off))[:2]
        except Exception:
            continue
        if ins and ins[0].mnemonic == "mov" and ins[0].op_str.startswith("rax,"):
            d = int(ins[0].op_str.split(",")[1], 16)
            if ins[1].mnemonic == "jmp" and ins[1].op_str == "rax":
                dests[base + off] = d

print("stub-prologue hits:", len(dests))
uniq = sorted(set(dests.values()))
print("unique dests:", len(uniq))

cache = {}


def export_name_for(dll_path, rva):
    if dll_path in cache:
        et = cache[dll_path]
    else:
        try:
            blob = open(dll_path, "rb").read()
        except OSError:
            cache[dll_path] = None
            return None
        pe = struct.unpack_from("<I", blob, 0x3C)[0]
        opt = pe + 0x18
        hdr = opt + struct.unpack_from("<H", blob, pe + 20)[0]
        export_rva, export_sz = struct.unpack_from(
            "<II", blob, opt + 0x70 + 4 * 0)
        if not export_rva:
            cache[dll_path] = None
            return None
        secmap = []
        nsec = struct.unpack_from("<H", blob, pe + 6)[0]
        for i in range(nsec):
            s = hdr + i * 40
            vsz, va, rsz, ro = struct.unpack_from("<IIII", blob, s + 8)
            secmap.append((va, vsz, ro, rsz))
        def r2o(r):
            for va, vsz, ro, rsz in secmap:
                if va <= r < va + vsz:
                    return ro + (r - va)
            return None
        eo = r2o(export_rva)
        names, funcs = None, None
        namecnt, fncnt = struct.unpack_from("<II", blob, eo + 20)
        fno = struct.unpack_from("<I", blob, eo + 28)[0]
        ano = struct.unpack_from("<I", blob, eo + 32)[0]
        names = {}
        for k in range(namecnt):
            nrva = struct.unpack_from("<I", blob, ano + k * 4)[0]
            no = r2o(nrva)
            e = blob.index(b"\0", no)
            fname = blob[no:e].decode(errors="replace")
            fo = struct.unpack_from("<I", blob, fno + k * 4)[0]
            names.setdefault(fo, fname)
        cache[dll_path] = names
    return cache[dll_path].get(rva)


res = {}
for d in uniq:
    m = mod_of(d)
    if not m:
        res[d] = ("?", "unknown")
        continue
    name, base_, size, path = m
    rva = d - base_
    n = export_name_for(path, rva)
    res[d] = (name, n or f"rva_{rva:#x}")

# map IAT slots to names via runtime iat values
blob_map = {}
for slot_off in range(0, len(iat) - 7, 8):
    v = struct.unpack_from("<Q", iat, slot_off)[0]
    if 0x100000000 <= v < 0x7FFFFFFFF000:
        # thunk pointer may not equal a prologue start exactly; find nearest
        tgt = dests.get(v)
        if tgt is None:
            blob_map[0x1DC0DD58 + slot_off] = ("?", "no-prologue")
        else:
            dll, fname = res[tgt]
            blob_map[0x1DCDD58 + slot_off] = (dll, fname)

print()
print("== IAT slot -> API ==")
named = {k: v for k, v in blob_map.items() if v[1] not in ("no-prologue",) and
         v[1].startswith("rva_") is False}
for slot in sorted(blob_map):
    dll, fname = blob_map[slot]
    mark = "" if not fname.startswith("rva_") else "(rva-only)"
    print(f"  slot {slot:#x}: {dll}!{fname} {mark}")

json.dump({f"{k:#x}": v for k, v in blob_map.items()},
          open(run + "\\iat_names.json", "w"), indent=1)
json.dump({f"{k:#x}": [v[0], v[1]] for k, v in res.items()},
          open(run + "\\thunk_dests.json", "w"), indent=1)
print("saved iat_names.json / thunk_dests.json")
if h:
    k32.CloseHandle(h)
