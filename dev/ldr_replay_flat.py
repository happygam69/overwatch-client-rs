"""Build a replay flat loader image for emu_tls.py: disk flat (offset==RVA)
with the RUNTIME .data section overlaid. Runtime pointer values that fall in
the session loader image range are rewritten to emu coordinates
(session_base - 0x180000000 arithmetic). Runtime IAT slots (.rdata)
are rewritten to the emu stub range 0x2A1A00000000 using the captured
stub blob offset.

Usage: python dev/ldr_replay_flat.py <rundir> <outflat>
"""
import struct
import sys

flat = bytearray(open(r"D:\ow-dumps\loader_disk_rva.bin", "rb").read())
run = sys.argv[1]
out = sys.argv[2]
SESS = None

manifest = json_load = None
import json
manifest = json.load(open(run + "\\manifest.json"))
SESS_LOADER = manifest["loader_base"]
SESS_EXE = manifest.get("exe_base", None)
stub_base_rt = manifest.get("stubs_base")
print("session loader base:", hex(SESS_LOADER), "exe base:",
      hex(SESS_EXE) if SESS_EXE else None)

EMU = 0x180000000
THUNK = 0x2A1A00000000

rt_data = bytearray(open(run + "\\data.bin", "rb").read())
rva = 0x1DF0000


def maybe_rewritten_qword(v):
    # loader self-pointer from session -> emu coordinate
    if SESS_LOADER <= v < SESS_LOADER + 0x2410000:
        return v - SESS_LOADER + EMU
    # main-exe pointer -> emu main base
    if SESS_EXE and SESS_EXE <= v < SESS_EXE + 0x5000000:
        return v - SESS_EXE + 0x140000000
    # heap thunk pointer -> emu stub range
    if stub_base_rt and stub_base_rt <= v < stub_base_rt + 0x100000:
        return v - stub_base_rt + THUNK
    return None

rewrites = [0, 0, 0]
n = len(rt_data)
for off in range(0, n - 7, 8):
    v = struct.unpack_from("<Q", rt_data, off)[0]
    w = maybe_rewritten_qword(v)
    if w is not None:
        struct.pack_into("<Q", rt_data, off, w)
        rewrites[0] += 1
flat[rva:rva + n] = rt_data
print(f".data overlaid: {n:#x} bytes; ptr rewrites: {rewrites[0]}")

# IAT slots in .rdata -> emu stub range via same mapping
rt_iat = open(run + "\\iat.bin", "rb").read()
ri = 0x1DCD0B8
_impl = struct.unpack_from("<I", flat, 0x3C)[0]  # sanity: PE pointer
# dd[12] rva 0x1dcdd58 size 0x960 - use runtime values
w = 0
for i in range(0, len(rt_iat) - 7, 8):
    v = struct.unpack_from("<Q", rt_iat, i)[0]
    if v == 0:
        continue
    tw = maybe_rewritten_qword(v)
    if tw is None:
        tw = THUNK + (i & 0xFFF) * 0x10  # plausible placeholder
    struct.pack_into("<Q", flat, 0x1DCDD58 + i, tw)
    w += 1
print(f"IAT slots rewritten: {w}")

open(out, "wb").write(bytes(flat))
print("wrote", out, len(flat), "bytes")
