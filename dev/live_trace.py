"""Sample a running client's physics bodies into a trace file.

The dmBody layout recovered from the v2b capture (`docs/formats/domino.md`) is
what makes this possible: a body is recognisable by shape alone, and its
transform/velocity/mass fields are at known offsets.  Two commands:

    python live_trace.py scan [--pid N] [--max-mb 4096] [--watch MS]
        Find dmBody-shaped structures in committed, readable, non-image
        memory.  Invariants used (tightened after uniform-filler blocks
        passed a looser cut):
            +0x20  quaternion (x,y,z,w), unit norm, not a constant group
            +0x164 mass > 0, +0x168 invMass > 0, mass * invMass ~ 1 (+-1%)
            +0x16c gravityScale in [-0.01, 4]; pos/vel finite, map-scale
            +0x1ac bodyType in 0..4; the bytes +0x1ad..+0x1b6 in {0,1}
        With --watch MS, every candidate is re-read after MS and only those
        whose state changed are printed — a real body moves, a filler block
        never does.  Hold still while scanning, then move when it says so.

    python live_trace.py capture [--pid N] [--seconds 90] [--out trace/walk.jsonl]
        The one-command flow for the operator: find the local mover by its
        vtables, confirm which instance is the player's by watching it move
        (walk for the few seconds it asks), then record it — keys included —
        for `--seconds`, and print the replay command. Nothing but reads.

    python live_trace.py node --addr 0x... [--pid N] [--hz 60] [--seconds 60]
                              [--out D:/ow-dumps/trace/player_ctrl.jsonl]
        Record the player transform node — feet position (+0xd0), yaw in
        degrees (+0xdc), keys — in trace v1, the format the movement differ
        (`trace_diff keys`) and `dev/ctrl_probe.py` consume.  `--addr` is the
        node *base* that `dev/find_node.py` prints; the layout is verified
        before recording starts, so a wrong address is refused, not written.

    python live_trace.py record --addr 0x... [--pid N] [--hz 60]
                                [--seconds 60] [--out dev/trace/body.jsonl]
        Sample the body at the given address every 1/hz seconds and write
        trace-format JSON lines (raw u32 bit patterns; the Rust differ is the
        consumer).  Verifies the shape each sample and notes a break.

READ-ONLY: OpenProcess with VM_READ|QUERY only; ReadProcessMemory only; the
target is never written and no debugger is attached.  Attaching to a live
session is the operator's decision with the Warden cost stated in
docs/roadmap.md; this tool exists so that decision is the only step needed.

Output format (trace v1), one JSON object per line:
    {"trace":1,"kind":"dmBody","addr":"0x...","pid":N,"hz":H,"started":"...","keys":1}
    {"i":0,"t":0.0,"pos":[u32 x4],"vel":[u32 x4],"angvel":[u32 x4],
     "flags":[u8 x10],"keys":{"w":1,"a":0,"s":0,"d":0,"space":0,"ctrl":0,
                              "shift":0,"lmb":0,"rmb":0}}
For `record` (dmBody): pos = transform translation (+0x10), the quaternion is
not included in v1 (add when a consumer needs it), flags = bytes +0x1ad..+0x1b6.
For `node`: pos = the node's feet (+0xd0), plus `"yaw"` in degrees (+0xdc);
`vel` is zeros, because the traced node is the display transform, not the body
(`docs/formats/character-controller.md`).  Both writers emit compact JSON — no
space after `:` — which the Rust parser requires.

The key timeline (recorded by default; disable with --no-keys) makes a trace a
complete (input, state) recording: `ow_sim::trace::diff_trace_keys` replays it
without a hand-written script, and the velocity response to the recorded keys
is what the mover's ramps (plan question Q3) are fitted from. Keys are read
with GetAsyncKeyState — no injection, no hooks, nothing written to the target.
"""

from __future__ import annotations

import ctypes
import json
import math
import os
import struct
import sys
import time
from ctypes import wintypes as wt

import dump_process as DP
import numpy as np

CHUNK = 0x400000
SCAN_CHUNK = 0x1000000  # 16 MB; the vectorized scan trades chunk size for
                        # gather-array memory, and this keeps spikes ~100 MB
BODY_SIZE = 0x1C0

# Offsets, from docs/formats/domino.md (dmBody).
O_POS = 0x10
O_QUAT = 0x20
O_VEL = 0x30
O_ANGVEL = 0x40
O_MASS = 0x164
O_INVMASS = 0x168
O_GRAVITY_SCALE = 0x16C
O_BODYTYPE = 0x1AC
O_FLAGS = 0x1AD  # .. 0x1B6 inclusive
FLAG_LEN = 0x1B7 - O_FLAGS

# Player transform node (`docs/formats/domino.md`, "The player node found") —
# the source the movement traces were recorded from.  Offsets are relative to
# the node base, which `dev/find_node.py` prints.
O_NODE_POS = 0xD0     # feet position, 3 f32
O_NODE_YAW = 0xDC     # yaw in degrees
O_NODE_QUAT = 0xE0
O_NODE_SCALE = 0xF0
# The position repeats.  Three layouts are known, and the check accepts any:
#   * 2026-10-13: +0x108/+0x138/+0x178 (the old node object's spacing);
#   * the 0x30-byte record (pos + yaw, quat, scale) at +0x100/+0x140;
#   * the custom-map session's 0x110 copy: its repeats are the *capsule*
#     variants — same x/z, +1.39 m in y — at base-relative +0x110.
# `node_ok` compares the horizontal exactly (0.25 m) and allows a vertical
# offset up to 2 m, because two of the three layouts repeat the position at a
# different height.  A wrong address has no agreeing pair at any of these.
O_NODE_REPEATS = (0x100, 0x108, 0x110, 0x138, 0x140, 0x178)
NODE_READ = 0x1A0     # pos + yaw + quat + scale + every repeat candidate


# GetAsyncKeyState: read-only, no injection, no hooks. The virtual-key codes
# are the plain Win32 ones for the movement binds.
VK = {
    "w": 0x57, "a": 0x41, "s": 0x53, "d": 0x44,
    "space": 0x20, "ctrl": 0x11, "shift": 0x10,
    "lmb": 0x01, "rmb": 0x02,
}


def read_keys() -> dict:
    """The movement key state right now, as a JSON-friendly dict of 0/1."""
    u = ctypes.windll.user32
    return {name: 1 if (u.GetAsyncKeyState(vk) & 0x8000) else 0 for name, vk in VK.items()}


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
    if not snap or snap == wt.HANDLE(-1).value:
        raise SystemExit(f"snapshot failed: {ctypes.get_last_error()}")
    entry = PROCESSENTRY32W()
    entry.dwSize = ctypes.sizeof(entry)
    ok = k32.Process32FirstW(snap, ctypes.byref(entry))
    found = None
    while ok:
        if name.lower() in entry.szExeFile.lower():
            found = entry.th32ProcessID
            break
        ok = k32.Process32NextW(snap, ctypes.byref(entry))
    k32.CloseHandle(snap)
    if found is None:
        raise SystemExit(f"no process matching {name!r}")
    return found


def looks_like_body(buf: bytes, off: int, any_mass: bool = False) -> bool:
    """Shape check at buf[off:off+BODY_SIZE]; buf must cover that range.

    A first cut with a [0.98,1.02] quaternion bound matched blocks of uniform
    filler: a `(0.5,0.5,0.5,0.5)` group has norm exactly 1, and some offsets
    inside such a block land on near-`(1.0,1.0)` pairs that satisfy the loose
    mass-product window.  The bounds are tighter here, the degenerate uniform
    quaternion is rejected outright, and both position and velocity must be
    finite and within map scale.
    """
    q = struct.unpack_from("<4f", buf, off + O_QUAT)
    if any(not math.isfinite(c) or not (-1.001 <= c <= 1.001) for c in q):
        return False
    if max(q) - min(q) < 1e-3:  # (0.5,...) and other constant groups
        return False
    n = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]
    if not (0.995 <= n <= 1.005):
        return False
    mass, invmass, gs = struct.unpack_from("<3f", buf, off + O_MASS)
    if any_mass:
        # A kinematic body has mass 0 / invMass 0 and is still a body; the
        # character may be exactly that (the shape filter without this mode
        # found no walking body at all).
        if not (0.0 <= mass < 1e6 and 0.0 <= invmass <= 2.0):
            return False
    else:
        if not (0.0 < mass < 1e6 and 0.0 < invmass <= 2.0):
            return False
        if not (0.99 <= mass * invmass <= 1.01):
            return False
    if not (-0.01 <= gs <= 4.0):
        return False
    if buf[off + O_BODYTYPE] > 4:
        return False
    if any(b > 1 for b in buf[off + O_FLAGS : off + O_FLAGS + FLAG_LEN]):
        return False
    vals = struct.unpack_from("<3f", buf, off + O_POS) + struct.unpack_from("<3f", buf, off + O_VEL)
    if any(not math.isfinite(v) or abs(v) > 1e5 for v in vals):
        return False
    return True


def body_summary(b: bytes, off: int = 0) -> str:
    pos = struct.unpack_from("<3f", b, off + O_POS)
    vel = struct.unpack_from("<3f", b, off + O_VEL)
    gs = struct.unpack_from("<f", b, off + O_GRAVITY_SCALE)[0]
    return (
        f"pos=({pos[0]:9.3f},{pos[1]:9.3f},{pos[2]:9.3f}) "
        f"vel=({vel[0]:7.3f},{vel[1]:7.3f},{vel[2]:7.3f}) gs={gs:.2f}"
    )


def find_candidates(data: bytes, base: int, any_mass: bool = False) -> list[int]:
    """Vectorized dmBody shape scan over one buffer; returns body addresses.

    The per-offset Python loop this replaces was ~660M iterations over a full
    address space — tens of minutes of indistinguishable-from-hang silence.
    Same invariants as looks_like_body(), as numpy gathers.
    """
    n = len(data)
    if n < BODY_SIZE + 0x10:
        return []
    n4 = (n // 4) * 4
    u8 = np.frombuffer(data, dtype=np.uint8)
    u32 = np.frombuffer(u8[:n4], dtype="<u4")
    fl = u32.view(np.float32)

    o = np.arange(0, n - BODY_SIZE, 0x10, dtype=np.int64)

    def f(off: int) -> np.ndarray:
        return fl[(o + off) // 4]

    q0, q1, q2, q3 = f(O_QUAT), f(O_QUAT + 4), f(O_QUAT + 8), f(O_QUAT + 12)
    keep = (q0 == q0) & (q1 == q1) & (q2 == q2) & (q3 == q3)
    keep &= (np.abs(q0) <= 1.001) & (np.abs(q1) <= 1.001) & (np.abs(q2) <= 1.001) & (np.abs(q3) <= 1.001)
    qmax = np.maximum(np.maximum(q0, q1), np.maximum(q2, q3))
    qmin = np.minimum(np.minimum(q0, q1), np.minimum(q2, q3))
    keep &= (qmax - qmin) >= 1e-3  # reject (0.5,...) constant groups
    norm2 = q0 * q0 + q1 * q1 + q2 * q2 + q3 * q3
    keep &= (norm2 >= 0.995) & (norm2 <= 1.005)

    mass, invmass, gs = f(O_MASS), f(O_INVMASS), f(O_GRAVITY_SCALE)
    if any_mass:
        keep &= (mass >= 0.0) & (mass < 1e6) & (invmass >= 0.0) & (invmass <= 2.0)
    else:
        keep &= (mass > 0.0) & (mass < 1e6) & (invmass > 0.0) & (invmass <= 2.0)
        prod = mass * invmass
        keep &= (prod >= 0.99) & (prod <= 1.01)
    keep &= (gs >= -0.01) & (gs <= 4.0)

    keep &= u8[o + O_BODYTYPE] <= 4
    flag_ok = np.ones(o.shape, dtype=bool)
    for k in range(10):
        flag_ok &= u8[o + O_FLAGS + k] <= 1
    keep &= flag_ok

    for k in (0, 4, 8):
        p = f(O_POS + k)
        v = f(O_VEL + k)
        keep &= (p == p) & (v == v) & (np.abs(p) <= 1e5) & (np.abs(v) <= 1e5)

    hits = np.nonzero(keep)[0]
    return [int(base + o[i]) for i in hits]


def cmd_scan(pid: int, max_mb: int, watch_ms: int, any_mass: bool = False) -> int:
    handle = DP.open_target(pid)
    regions = [r for r in DP.regions(handle)]
    total = sum(r[1] for r in regions)
    print(
        f"pid {pid}: scanning committed readable non-image memory "
        f"({total/1048576:.0f} MB present, cap {max_mb} MB) — about a minute"
    )
    scanned = 0
    candidates: list[tuple[int, bytes]] = []
    last_report = 0
    for base, size, protect, kind, _alloc in regions:
        if protect & (DP.PAGE_NOACCESS | DP.PAGE_GUARD) or kind == DP.MEM_IMAGE:
            continue
        if scanned >= max_mb * 1048576:
            print(f"  (stopping at the {max_mb} MB cap)")
            break
        offset = 0
        while offset < size:
            origin = base + offset
            data = DP.read_region(handle, origin, min(SCAN_CHUNK + BODY_SIZE, size - offset))
            offset += SCAN_CHUNK
            if not data or len(data) < BODY_SIZE:
                continue
            scanned += len(data)
            for addr in find_candidates(data, origin):
                if len(candidates) < 20000:
                    off = addr - origin
                    candidates.append((addr, data[off : off + BODY_SIZE]))
            if scanned - last_report >= 512 * 1048576:
                last_report = scanned
                print(f"  ... {scanned/1048576:.0f} MB scanned, {len(candidates)} candidates", flush=True)
    print(f"{len(candidates)} shape-passing candidate(s) after {scanned/1048576:.0f} MB", flush=True)

    if not candidates:
        return 1

    if watch_ms <= 0:
        for addr, b in candidates[:200]:
            print(f"  {addr:#014x}  {body_summary(b)}")
        return 0

    # Watch mode: a real body's state changes between reads; a filler block
    # never does.  Three samples, because junk changes once, while a body
    # holds a consistent velocity — hold still, run it, then move when it
    # says so, and keep moving until the samples are done.
    half = max(600, watch_ms // 2)
    print(f"watching candidates for {watch_ms} ms — move now and keep moving...")
    time.sleep(half / 1000.0)
    mid: dict[int, bytes] = {}
    for addr, _before in candidates:
        b = DP.read_region(handle, addr, BODY_SIZE)
        if b and len(b) == BODY_SIZE:
            mid[addr] = b
    time.sleep(half / 1000.0)
    changed = []
    for addr, before in candidates:
        after = DP.read_region(handle, addr, BODY_SIZE)
        if not after or len(after) != BODY_SIZE:
            continue
        span = slice(O_POS, O_ANGVEL + 0x10)
        if after[span] == before[span]:
            continue
        m = mid.get(addr)
        ap = struct.unpack_from("<3f", after, O_POS)
        av = struct.unpack_from("<3f", after, O_VEL)
        dpos = 0.0
        if m:
            mp = struct.unpack_from("<3f", m, O_POS)
            dpos = math.dist(mp, ap)
        speed = math.sqrt(av[0] * av[0] + av[1] * av[1] + av[2] * av[2])
        likely = dpos > 0.05 and 2.5 <= speed <= 8.5
        changed.append((likely, dpos, speed, addr, before, after))
    changed.sort(key=lambda c: (not c[0], -c[1]))
    likely_n = sum(1 for c in changed if c[0])
    print(f"{len(changed)} changed state, {likely_n} look like a walking body (LIKELY):")
    for likely, dpos, speed, addr, before, after in changed[:60]:
        bv = struct.unpack_from("<3f", before, O_VEL)
        av = struct.unpack_from("<3f", after, O_VEL)
        tag = "  << LIKELY" if likely else ""
        print(f"  {addr:#014x}  moved {dpos:6.3f} m  speed {speed:5.2f}{tag}")
        print(f"                vel ({bv[0]:7.3f},{bv[1]:7.3f},{bv[2]:7.3f}) -> ({av[0]:7.3f},{av[1]:7.3f},{av[2]:7.3f})")
    return 0 if changed else 1


def cmd_world(pid: int, max_mb: int) -> int:
    """Find the physics world in the live process and print its bodies.

    Same algorithm as dev/find_dmworld.py (validated by its --selftest): a
    world-shaped object is located by signature, then verified by walking
    m_bodies and requiring real dmBody records at the pointed-to addresses.
    This replaces shape-scanning for bodies: the world's list is the truth.
    """
    import find_dmworld as FD

    handle = DP.open_target(pid)
    print(f"pid {pid}: scanning committed readable non-image memory (cap {max_mb} MB)")

    class LiveSpace:
        """R.Space-shaped reader over the live process."""

        def read(self, addr: int, length: int):
            b = DP.read_region(handle, addr, length)
            return b if b and len(b) == length else None

        def u64(self, addr: int):
            b = self.read(addr, 8)
            return int.from_bytes(b, "little") if b else None

    scanned = 0
    kept: list[dict] = []
    for base, size, protect, kind, _alloc in DP.regions(handle):
        if protect & (DP.PAGE_NOACCESS | DP.PAGE_GUARD) or kind == DP.MEM_IMAGE:
            continue
        if scanned >= max_mb * 1048576:
            break
        offset = 0
        while offset < size:
            origin = base + offset
            data = DP.read_region(handle, origin, min(CHUNK + 0x400, size - offset))
            offset += CHUNK
            if not data or len(data) < 0x1000:
                continue
            scanned += len(data)
            take = min(CHUNK, len(data))
            kept += FD.scan_chunk(data, origin, origin, origin + take)
        if scanned // (512 * 1048576) > (scanned - CHUNK) // (512 * 1048576):
            print(f"  ... {scanned/1048576:.0f} MB, {len(kept)} raw candidate(s)", flush=True)
    uniq = {c["world"]: c for c in kept}
    print(f"{len(uniq)} candidate(s) after {scanned/1048576:.0f} MB")

    live = LiveSpace()
    real = 0
    for cand in sorted(uniq.values(), key=lambda c: c["world"]):
        best, bodies = FD.verify_world(live, cand, verbose=True)
        if best > 0:
            real += 1
            for p in bodies:
                b = live.read(p, FD.BODY_SIZE)
                pos = struct.unpack_from("<3f", b, FD.B_POS)
                vel = struct.unpack_from("<3f", b, 0x30)
                flags = b[FD.B_FLAGS : FD.B_FLAGS + 10]
                print(
                    f"    body {p:#x} pos=({pos[0]:9.3f},{pos[1]:9.3f},{pos[2]:9.3f}) "
                    f"vel=({vel[0]:7.3f},{vel[1]:7.3f},{vel[2]:7.3f}) flags={list(flags)}"
                )
    print(f"{real} verified world(s)")
    return 0 if real else 1


def module_base(pid: int, name: str = "Overwatch.exe") -> int:
    """Base address of a loaded module, via the Toolhelp module snapshot."""
    TH32CS_SNAPMODULE = 0x8
    TH32CS_SNAPMODULE32 = 0x10

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
    snap = k32.CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)
    if not snap or snap == wt.HANDLE(-1).value:
        raise SystemExit(f"module snapshot failed: {ctypes.get_last_error()}")
    entry = MODULEENTRY32W()
    entry.dwSize = ctypes.sizeof(entry)
    ok = k32.Module32FirstW(snap, ctypes.byref(entry))
    base = None
    while ok:
        if name.lower() in entry.szModule.lower():
            base = entry.modBaseAddr or 0
            break
        ok = k32.Module32NextW(snap, ctypes.byref(entry))
    k32.CloseHandle(snap)
    if not base:
        raise SystemExit(f"module {name!r} not found (is the game running?)")
    return int(base)


# Mover interface vtables, as offsets inside the module (stable across
# sessions for one build; validated in dumps_v2b where the triple was found on
# 173 live instances and the first vtable slot is the shared `ret` stub, the
# known delta anchor).
MOVER_VT_OFF = (0x31E31F8, 0x31E30C8, 0x31E3160)
MOVER_SIZE = 0x130


def find_movers(handle, vt0: int, vt8: int, vt48: int, max_mb: int) -> list[int]:
    """Every object carrying the mover's vtable triple (vt0@0, vt8@8, vt48@0x48)."""
    needle = vt0.to_bytes(8, "little")
    found: list[int] = []
    scanned = 0
    for base, size, protect, kind, _alloc in DP.regions(handle):
        if protect & (DP.PAGE_NOACCESS | DP.PAGE_GUARD) or kind == DP.MEM_IMAGE:
            continue
        if scanned >= max_mb * 1048576:
            break
        offset = 0
        while offset < size:
            origin = base + offset
            data = DP.read_region(handle, origin, min(SCAN_CHUNK + 8, size - offset))
            offset += SCAN_CHUNK
            if not data:
                continue
            scanned += len(data)
            at = 0
            while True:
                at = data.find(needle, at)
                if at < 0 or at >= SCAN_CHUNK:
                    break
                candidate = origin + at
                if candidate != vt0:
                    obj = DP.read_region(handle, candidate, MOVER_SIZE)
                    if (
                        obj
                        and len(obj) == MOVER_SIZE
                        and int.from_bytes(obj[8:16], "little") == vt8
                        and int.from_bytes(obj[0x48:0x50], "little") == vt48
                    ):
                        if candidate not in found:
                            found.append(candidate)
                at += 1
    return found


def cmd_mover(pid: int, max_mb: int, watch_ms: int, out_path: str | None) -> int:
    """Find mover components by their vtables and identify the local one.

    Instance identity here is pointer identity, not shape: an object whose
    +0/+8/+0x48 are the component's own interface vtables is the component.
    The local player's instance is the one whose fields change when the
    player moves — the watch prints which offsets move, which also maps the
    input/state slots empirically (no assumed field semantics).
    """
    base = module_base(pid)
    vt0, vt8, vt48 = (base + off for off in MOVER_VT_OFF)
    print(f"module base {base:#x}; mover vtables {vt0:#x} / {vt8:#x} / {vt48:#x}")
    handle = DP.open_target(pid)
    movers = find_movers(handle, vt0, vt8, vt48, max_mb)
    print(f"{len(movers)} mover instance(s)")

    if watch_ms <= 0:
        for addr in movers[:40]:
            obj = DP.read_region(handle, addr, MOVER_SIZE)
            if not obj:
                continue
            sig = struct.unpack_from("<6f", obj, 0x50)
            speedish = struct.unpack_from("<4f", obj, 0xD0)
            print(
                f"  {addr:#014x}  jump=({sig[0]:g},{sig[1]:g},{sig[2]:g},{sig[3]:g},{sig[4]:g},{sig[5]:g}) "
                f"f@D0=({speedish[0]:g},{speedish[1]:g},{speedish[2]:g},{speedish[3]:g})"
            )
        return 0 if movers else 1

    print(f"watching for {watch_ms} ms — press W and walk, then stop, then jump once...")
    before = {a: DP.read_region(handle, a, MOVER_SIZE) for a in movers}
    time.sleep(watch_ms / 1000.0)
    rows = []
    for addr in movers:
        b0 = before.get(addr)
        b1 = DP.read_region(handle, addr, MOVER_SIZE)
        if not b0 or not b1:
            continue
        changed = [
            off
            for off in range(0, MOVER_SIZE - 3, 4)
            if b0[off : off + 4] != b1[off : off + 4]
        ]
        if changed:
            rows.append((len(changed), addr, changed, b0, b1))
    rows.sort(reverse=True)
    print(f"{len(rows)} instance(s) changed:")
    for n, addr, changed, b0, b1 in rows[:20]:
        offs = " ".join(f"+{o:#04x}" for o in changed[:24])
        print(f"\n  {addr:#014x}  {n} field(s) changed: {offs}")
        for o in changed[:12]:
            f0 = struct.unpack_from("<f", b0, o)[0]
            f1 = struct.unpack_from("<f", b1, o)[0]
            i0 = int.from_bytes(b0[o : o + 4], "little")
            i1 = int.from_bytes(b1[o : o + 4], "little")
            print(f"     +{o:#04x}  float {f0:12.6g} -> {f1:12.6g}   int {i0:#x} -> {i1:#x}")
    if out_path and rows:
        # Save the full raw before/after for the most-changed instance, so the
        # layout can be mapped offline without another live session.
        with open(out_path, "w") as f:
            n, addr, changed, b0, b1 = rows[0]
            f.write(f"# mover watch: {addr:#x} before -> after ({watch_ms} ms)\n")
            for row in range(0, MOVER_SIZE, 0x10):
                f.write(f"+{row:#04x}  {b0[row:row+0x10].hex(' ')}  ->  {b1[row:row+0x10].hex(' ')}\n")
        print(f"\nmost-changed instance raw dump -> {out_path}")
    return 0 if rows else 1


def cmd_capture(pid: int, seconds: float, out_path: str, max_mb: int,
                watch_ms: int, hz: float) -> int:
    """watch -> record, so the operator plays and one command does the rest."""
    base = module_base(pid)
    vt0, vt8, vt48 = (base + off for off in MOVER_VT_OFF)
    handle = DP.open_target(pid)
    movers = find_movers(handle, vt0, vt8, vt48, max_mb)
    print(f"{len(movers)} mover instance(s) found")
    if not movers:
        print("no mover instance: is a game/range loaded, and is this the right process?")
        return 1

    target = movers[0]
    if len(movers) > 1:
        print(f"WALK NOW — picking the player's mover among {len(movers)} over {watch_ms} ms...")
        before = {a: DP.read_region(handle, a, MOVER_SIZE) for a in movers}
        time.sleep(watch_ms / 1000.0)
        rows = []
        for a in movers:
            b0, b1 = before.get(a), DP.read_region(handle, a, MOVER_SIZE)
            if not b0 or not b1:
                continue
            changed = sum(1 for off in range(0, MOVER_SIZE - 3, 4)
                          if b0[off:off + 4] != b1[off:off + 4])
            rows.append((changed, a))
        rows.sort(reverse=True)
        if not rows or rows[0][0] == 0:
            print("nothing moved; hold the walk for the whole watch window and retry")
            return 1
        target, best = rows[0][1], rows[0][0]
        print(f"player mover = {target:#014x} ({best} slot(s) moved)")

    print(f"recording {seconds:.0f} s at {hz:.0f} Hz from {target:#014x} — move, strafe, jump, crouch")
    # Mover objects are recorded raw: their slot layout is not dmBody's, and a
    # raw trace preserves every slot for offline fitting.
    rc = cmd_record(pid, target, hz, seconds, out_path, raw_mode=True,
                    size=MOVER_SIZE, with_keys=True)
    if rc != 0:
        return rc
    print("")
    print("replay it (script-free; keys are in the trace):")
    print(f"  cargo run -p ow-sim --bin trace_diff -- keys {out_path}")
    print(f"  cargo run -p ow-game -- --headless 1 --range --trace {out_path}   # same report in the game")
    print("both --edges and the default (held) reading are worth running; Q3 leaves it open")
    return 0


def cmd_record(pid: int, addr: int, hz: float, seconds: float, out_path: str,
               raw_mode: bool = False, size: int = BODY_SIZE,
               with_keys: bool = True) -> int:
    handle = DP.open_target(pid)
    period = 1.0 / hz
    with open(out_path, "w") as out:
        header = {
            "trace": 2 if raw_mode else 1,
            "kind": "mover_raw" if raw_mode else "dmBody",
            "addr": f"{addr:#x}", "pid": pid, "hz": hz,
            "size": size,
            "keys": 1 if with_keys else 0,
            "started": time.strftime("%Y-%m-%dT%H:%M:%S"),
        }
        out.write(json.dumps(header, separators=(",", ":")) + "\n")
        t0 = time.perf_counter()
        i = 0
        broken = 0
        while True:
            target = t0 + i * period
            now = time.perf_counter()
            if now < target:
                time.sleep(target - now)
            b = DP.read_region(handle, addr, size)
            if not b or len(b) < size:
                print(f"  read failed at sample {i}; stopping")
                break
            if raw_mode:
                rec = {"i": i, "t": round(now - t0, 6), "raw": b.hex()}
            else:
                if broken == 0 and not looks_like_body(b, 0):
                    broken = i
                    print(f"  WARNING: shape check failed at sample {i} — body moved or died?")
                elif broken and looks_like_body(b, 0):
                    broken = 0
                    print(f"  shape holds again at sample {i}")
                pos = struct.unpack_from("<4I", b, O_POS)
                vel = struct.unpack_from("<4I", b, O_VEL)
                ang = struct.unpack_from("<4I", b, O_ANGVEL)
                flags = list(b[O_FLAGS : O_FLAGS + FLAG_LEN])
                rec = {
                    "i": i, "t": round(now - t0, 6),
                    "pos": list(pos), "vel": list(vel), "angvel": list(ang),
                    "flags": flags,
                }
                if with_keys:
                    rec["keys"] = read_keys()
            out.write(json.dumps(rec, separators=(",", ":")) + "\n")
            if i % int(hz) == 0:
                out.flush()
            i += 1
            if seconds and (now - t0) >= seconds:
                break
    print(f"{i} samples -> {out_path}")
    return 0


def node_ok(b: bytes) -> bool:
    """The node's stable layout, checked on one `NODE_READ`-byte read.

    Everything here holds in every movement state, crouch included: a finite
    map-scale feet triple, a plausible yaw, a unit quaternion, a uniform sane
    scale, and the position repeated at one of `O_NODE_REPEATS` (the two
    measured layouts put it in different places; a wrong address has it in
    neither).  The capsule centre is deliberately *not* checked — it tracks
    the stance height, so a crouched player would fail it, and this session's
    records do not carry one at all.
    """
    if len(b) < NODE_READ:
        return False
    f = lambda o: struct.unpack_from("<f", b, o)[0]
    pos = struct.unpack_from("<3f", b, O_NODE_POS)
    yaw = f(O_NODE_YAW)
    q = struct.unpack_from("<4f", b, O_NODE_QUAT)
    s = struct.unpack_from("<3f", b, O_NODE_SCALE)
    qn = sum(v * v for v in q) ** 0.5
    repeats = any(
        abs(f(O_NODE_POS + off) - pos[0]) < 0.25
        and abs(f(O_NODE_POS + off + 8) - pos[2]) < 0.25
        and abs(f(O_NODE_POS + off + 4) - pos[1]) < 2.0
        for off in (o - O_NODE_POS for o in O_NODE_REPEATS)
    )
    return (
        math.isfinite(pos[0]) and math.isfinite(pos[1]) and math.isfinite(pos[2])
        and abs(pos[0]) < 4000.0 and abs(pos[2]) < 4000.0
        and math.isfinite(yaw) and abs(yaw) <= 1440.0
        and abs(qn - 1.0) < 0.01
        and 0.05 < s[0] < 20.0
        and abs(s[0] - s[1]) < 1e-3 and abs(s[1] - s[2]) < 1e-3
        and repeats
    )


def cmd_node(pid: int, addr: int, hz: float, seconds: float, out_path: str,
             with_keys: bool = True, with_yaw: bool = True,
             yaw_row: int | None = None) -> int:
    """Record the player transform node: feet position, yaw, keys (trace v1).

    `--no-yaw` omits the yaw field; `--yaw-row OFF` derives it instead from a
    rotation row.  Three layouts are known:

    * the 2026-10-13 node: a degree-valued yaw at +0xdc (the default path);
    * the 2026-10-15 custom-map session: the player's copies are 4x4 transform
      matrices — rotation rows at base+0x80/+0x90/+0xA0, the translation row at
      base+0xD0 (the recorded feet triple) — with no yaw field at all.  Its
      third row (base+0xC0, `--yaw-row 0xc0`) is the object's +Z axis in world
      space: `yaw = atan2(rz.x, rz.z)` matched the walk direction to 0.02-0.27
      degrees on three facings (measured), which is exactly the differ's
      convention (forward = (sin yaw, cos yaw));
    * or omit both, and the differ infers one yaw from the first free-movement
      step (`trace.rs`, measured to 0.006 deg) — exact while the facing holds.
    """
    handle = DP.open_target(pid)
    first = DP.read_region(handle, addr, NODE_READ)
    if not first or not node_ok(first):
        print(f"{addr:#x} does not carry the node layout — find the node first:")
        print("  python dev/find_node.py")
        return 1
    if yaw_row is not None:
        row = struct.unpack_from("<3f", first, yaw_row)
        rn = sum(v * v for v in row) ** 0.5
        if not (0.999 < rn < 1.001):
            print(f"{addr:#x}: the row at +{yaw_row:#x} is not a unit rotation row "
                  f"(|r| = {rn:.4f}) — wrong address or wrong offset")
            return 1
        print(f"yaw from the unit rotation row at +{yaw_row:#x}: "
              f"{math.degrees(math.atan2(row[0], row[2])):.3f} deg")
    print(f"recording {seconds:g} s at {hz:g} Hz from node {addr:#x} — "
          f"drive the schedule now (the injector, or the five hand steps)")
    period = 1.0 / hz
    with open(out_path, "w") as out:
        header = {
            "trace": 1, "kind": "dmBody",
            "addr": f"node_{addr:#x}_pos_feet", "pid": pid, "hz": hz,
            "keys": 1 if with_keys else 0,
            "started": time.strftime("%Y-%m-%dT%H:%M:%S"),
        }
        out.write(json.dumps(header, separators=(",", ":")) + "\n")
        t0 = time.perf_counter()
        i = 0
        broken = False
        while True:
            target = t0 + i * period
            now = time.perf_counter()
            if now < target:
                time.sleep(target - now)
            b = DP.read_region(handle, addr, NODE_READ)
            if not b or len(b) < NODE_READ:
                print(f"  read failed at sample {i}; stopping")
                break
            if not node_ok(b):
                if not broken:
                    broken = True
                    print(f"  WARNING: node shape lost at sample {i} "
                          f"(freed, or the wrong address); still recording")
            elif broken:
                broken = False
                print(f"  shape holds again at sample {i}")
            pos = struct.unpack_from("<3I", b, O_NODE_POS)
            rec = {
                "i": i, "t": round(now - t0, 6),
                "pos": list(pos) + [0], "vel": [0, 0, 0, 0],
            }
            if with_yaw:
                if yaw_row is not None:
                    rzy, rzx = (struct.unpack_from("<f", b, yaw_row + 4)[0],
                                struct.unpack_from("<f", b, yaw_row)[0])
                    rzz = struct.unpack_from("<f", b, yaw_row + 8)[0]
                    rec["yaw"] = round(math.degrees(math.atan2(rzx, rzz)), 4) \
                        if all(map(math.isfinite, (rzx, rzy, rzz))) else 0.0
                else:
                    rec["yaw"] = round(struct.unpack_from("<f", b, O_NODE_YAW)[0], 4)
            if with_keys:
                rec["keys"] = read_keys()
            out.write(json.dumps(rec, separators=(",", ":")) + "\n")
            if i % int(hz) == 0:
                out.flush()
            i += 1
            if seconds and (now - t0) >= seconds:
                break
    print(f"{i} samples -> {out_path}")
    print(f"measure it:  python dev/ctrl_probe.py {out_path}")
    return 0


def node_selftest() -> int:
    """Run `cmd_node`'s loop against a synthetic node and check the file.

    The recorder has never run against a live client.  This exercises the whole
    loop — the shape check, the per-sample reads, the writer — with the memory
    reads stubbed, and asserts the written lines carry the differ's exact
    format: compact `"pos":[` (the parser rejects a space after the colon),
    four-element `pos`/`vel`, and `yaw`/`keys` present.  It leaves the file on
    disk and prints its path, so `trace_diff keys <path>` can be run on it —
    that file will not diff *clean* (the stub's step is fixed while the
    recorder's timestamps are real), which is fine: the point is that the
    differ parses and compares the recorder's own output.
    """
    import tempfile

    pos = [53.897, 0.711, 20.094]
    vel = 0.0
    yaw_rad = math.radians(189.5668)

    def record() -> bytes:
        nonlocal vel
        vel = min(5.5, vel + 121.4 / 60.0)   # the measured ACCEL: a model-consistent ramp
        pos[0] += vel * math.sin(yaw_rad) / 60.0
        pos[2] += vel * math.cos(yaw_rad) / 60.0
        b = bytearray(NODE_READ)
        struct.pack_into("<3f", b, O_NODE_POS, *pos)
        struct.pack_into("<f", b, O_NODE_YAW, 189.5668)
        struct.pack_into("<4f", b, O_NODE_QUAT, 0.0, 0.0, 0.0, 1.0)
        struct.pack_into("<3f", b, O_NODE_SCALE, 1.0, 1.0, 1.0)
        struct.pack_into("<3f", b, 0x108, *pos)   # one of O_NODE_REPEATS
        return bytes(b)

    real_open, real_read, real_keys = DP.open_target, DP.read_region, read_keys
    DP.open_target = lambda _pid: 0
    DP.read_region = lambda _h, _a, n: record()[:n] if n <= NODE_READ else None
    globals()["read_keys"] = lambda: {k: (1 if k == "w" else 0) for k in VK}
    out_path = os.path.join(tempfile.gettempdir(), "live_trace_node_selftest.jsonl")
    try:
        rc = cmd_node(1, 0x24100000000, 60.0, 0.25, out_path, with_keys=True)
    finally:
        DP.open_target, DP.read_region = real_open, real_read
        globals()["read_keys"] = real_keys
    if rc != 0:
        print("selftest: cmd_node refused the synthetic node")
        return 1
    lines = open(out_path).read().strip().splitlines()
    if len(lines) < 10:
        print(f"selftest: only {len(lines)} lines written")
        return 1
    hdr = json.loads(lines[0])
    if hdr.get("trace") != 1 or not str(hdr.get("addr", "")).startswith("node_"):
        print(f"selftest: bad header {hdr}")
        return 1
    for ln in lines[1:]:
        if '"pos":[' not in ln:
            print(f"selftest: not the compact format the differ needs: {ln[:70]}")
            return 1
        rec = json.loads(ln)
        if len(rec.get("pos", [])) != 4 or len(rec.get("vel", [])) != 4:
            print(f"selftest: array widths wrong: {ln[:70]}")
            return 1
        if "yaw" not in rec or "keys" not in rec:
            print(f"selftest: yaw/keys missing: {ln[:70]}")
            return 1
    print(f"selftest: cmd_node wrote {len(lines) - 1} samples in the differ's format -> {out_path}")
    return 0


def main() -> int:
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return 2
    cmd = args[0]
    if cmd == "node" and "--selftest" in args:
        return node_selftest()   # offline: no game process needed
    pid = None
    if "--pid" in args:
        pid = int(args[args.index("--pid") + 1])
    if pid is None:
        pid = find_pid("Overwatch.exe")
    if cmd == "scan":
        max_mb = int(args[args.index("--max-mb") + 1]) if "--max-mb" in args else 4096
        watch = int(args[args.index("--watch") + 1]) if "--watch" in args else 0
        return cmd_scan(pid, max_mb, watch, any_mass="--any-mass" in args)
    if cmd == "mover":
        max_mb = int(args[args.index("--max-mb") + 1]) if "--max-mb" in args else 12288
        watch = int(args[args.index("--watch") + 1]) if "--watch" in args else 0
        out = args[args.index("--out") + 1] if "--out" in args else "trace/mover_watch.txt"
        return cmd_mover(pid, max_mb, watch, out)
    if cmd == "capture":
        seconds = float(args[args.index("--seconds") + 1]) if "--seconds" in args else 90.0
        hz = float(args[args.index("--hz") + 1]) if "--hz" in args else 120.0
        out = args[args.index("--out") + 1] if "--out" in args else "trace/mover_capture.jsonl"
        max_mb = int(args[args.index("--max-mb") + 1]) if "--max-mb" in args else 12288
        watch_ms = int(args[args.index("--watch-ms") + 1]) if "--watch-ms" in args else 2500
        return cmd_capture(pid, seconds, out, max_mb, watch_ms, hz)
    if cmd == "node":
        if "--selftest" in args:
            return node_selftest()
        if "--addr" not in args:
            print("node needs --addr, the node base that find_node.py prints:")
            print("  python dev/find_node.py")
            print("  python live_trace.py node --addr 0x241ddb61880 --seconds 300 "
                  "--out D:/ow-dumps/trace/player_ctrl.jsonl")
            return 2
        addr = int(args[args.index("--addr") + 1], 16)
        hz = float(args[args.index("--hz") + 1]) if "--hz" in args else 60.0
        seconds = float(args[args.index("--seconds") + 1]) if "--seconds" in args else 300.0
        out_path = args[args.index("--out") + 1] if "--out" in args else "trace/node.jsonl"
        yaw_row = (int(args[args.index("--yaw-row") + 1], 0)
                   if "--yaw-row" in args else None)
        return cmd_node(pid, addr, hz, seconds, out_path,
                        with_keys="--no-keys" not in args,
                        with_yaw="--no-yaw" not in args or yaw_row is not None,
                        yaw_row=yaw_row)
    if cmd == "record":
        if "--addr" not in args:
            print("record needs --addr from a mover/scan result, e.g.")
            print("  python live_trace.py record --mover --addr 0x1234abcd --seconds 90 --out trace/walk1.jsonl")
            return 2
        addr = int(args[args.index("--addr") + 1], 16)
        hz = float(args[args.index("--hz") + 1]) if "--hz" in args else 60.0
        seconds = float(args[args.index("--seconds") + 1]) if "--seconds" in args else 60.0
        out_path = args[args.index("--out") + 1] if "--out" in args else "trace/body.jsonl"
        with_keys = "--no-keys" not in args
        if "--mover" in args:
            return cmd_record(pid, addr, hz, seconds, out_path, raw_mode=True,
                              size=MOVER_SIZE, with_keys=with_keys)
        return cmd_record(pid, addr, hz, seconds, out_path, with_keys=with_keys)
    print(f"unknown command {cmd!r}")
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
