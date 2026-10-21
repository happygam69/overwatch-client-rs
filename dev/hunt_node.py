#!/usr/bin/env python3
"""Hunt the player transform node by behaviour, not by shape.

The signature scan (`dev/find_node.py`) matched only static data in the
2026-10-15 session: 26 candidates, every one with a zero-ish position and a
junk header, and none of them reacted to injected input.  So this tool keeps
the documented node layout *loosely* (a map-scale position at +0x00, a non-zero
yaw at +0x0c, a unit quaternion at +0x10, a uniform scale at +0x20) and then
discriminates by behaviour, which static data cannot fake:

  phase 1  loose scan of committed non-image memory -> candidate list
  phase 2  walk (W): the node's position advances at a walking pace
  phase 3  stop, then cluster: the player's transform is copied into many
           arenas and every copy walks the *same* distance, so the largest
           cluster of agreeing (start, end) positions is the player
  phase 4  crouch: the feet copy holds its height (the body's), the stance
           copies drop ~0.47 m — the recorder wants the feet copy
  phase 5  report it as `node base = anchor - 0xd0`, in find_node.py's own
           wording, so live_trace.py node can record it

Injected input is OS-level (`keybd_event`/`mouse_event`, scancodes) and reaches
the client: a fire-mode run moved the player and dropped the ammo counter
28 -> 19 in the HUD, and 3000 px of relative `mouse_event` motion turned the
view by -143.5 deg (measured against a *known* real node, 2026-10-15).  An
earlier "mouse injection does not turn the view" reading was wrong: that test
watched only template junk, which cannot show a yaw change.  This tool screens
by walking regardless, because walking is the behaviour it needs to measure.

    python dev/hunt_node.py [--pid N] [--max-mb 14000]
                            [--cands-out D:/ow-dumps/trace/hunt_cands.txt]

READ-ONLY on the process (VM_READ|VM_QUERY + ReadProcessMemory); the only
side effects are OS-level keyboard and mouse events.
"""

from __future__ import annotations

import ctypes
import struct
import sys
import time

import numpy as np

import dump_process as DP

CHUNK = 16 << 20

# Offsets in f32 units, from the measured layout (docs/formats/domino.md,
# "The player node found"): the scan's anchor is the feet triple.
D_YAW = 3      # +0xdc - +0xd0
D_QUAT = 4     # +0xe0 - +0xd0
D_SCALE = 8    # +0xf0 - +0xd0
LAST = D_SCALE + 2

VK_W = 0x57
SC_W = 0x11
KEYUP = 0x0002
SCANCODE = 0x0008


def scan_loose(buf: bytes, origin: int) -> list[int]:
    """Every anchor address whose neighbourhood *could* be the node.

    Looser than find_node.scan_buffer in exactly the ways that made that
    matcher miss this session: no repeat fields, no capsule-height window,
    and a position that must be *non-trivial* (a map coordinate, not the
    zero of a template block).  The quaternion and the uniform scale stay —
    they are what keeps the candidate list small enough to re-read.
    """
    nf = len(buf) // 4
    if nf <= LAST:
        return []
    fa = np.frombuffer(buf[: nf * 4], dtype="<f4")
    m = nf - LAST
    with np.errstate(all="ignore"):
        v0, v1, v2 = fa[0:m], fa[1 : m + 1], fa[2 : m + 2]
        ok = np.isfinite(v0) & np.isfinite(v1) & np.isfinite(v2)
        ok &= (np.abs(v0) < 4000.0) & (np.abs(v2) < 4000.0) & (np.abs(v1) < 2000.0)
        ok &= (np.abs(v0) >= 0.5) | (np.abs(v2) >= 0.5)   # not a zero template
        yaw = fa[D_YAW : D_YAW + m]
        ok &= np.isfinite(yaw) & (np.abs(yaw) > 1e-3) & (np.abs(yaw) <= 720.0)
        qn = np.sqrt(sum(qk * qk for qk in
                         (fa[D_QUAT + k : D_QUAT + k + m] for k in range(4))))
        ok &= np.abs(qn - 1.0) < 0.005
        s0 = fa[D_SCALE : D_SCALE + m]
        s1 = fa[D_SCALE + 1 : D_SCALE + 1 + m]
        s2 = fa[D_SCALE + 2 : D_SCALE + 2 + m]
        ok &= (s0 > 0.05) & (s0 < 20.0)
        ok &= (np.abs(s0 - s1) < 1e-3) & (np.abs(s1 - s2) < 1e-3)
        hits = [origin + int(i) * 4 for i in np.nonzero(ok)[0]]
    return hits


def read_state(handle, anchors: list[int]) -> dict[int, bytes]:
    """0x120 bytes from each anchor (pos + yaw + quat + scale)."""
    out = {}
    for a in anchors:
        b = DP.read_region(handle, a, 0x120)
        if b and len(b) == 0x120:
            out[a] = b
    return out


def state_of(b: bytes):
    pos = struct.unpack_from("<3f", b, 0)
    yaw = struct.unpack_from("<f", b, 0xC)[0]
    quat = struct.unpack_from("<4f", b, 0x10)
    return pos, yaw, quat


def key_event(vk: int, scan: int, down: bool) -> None:
    flags = SCANCODE | (0 if down else KEYUP)
    ctypes.windll.user32.keybd_event(vk, scan, flags, 0)


def hold_w(seconds: float) -> None:
    key_event(VK_W, SC_W, True)
    time.sleep(seconds)
    key_event(VK_W, SC_W, False)


VK_CTRL, SC_CTRL = 0x11, 0x1D


def key_down(name: str) -> None:
    key_event(VK_CTRL, SC_CTRL, True)


def key_up(name: str) -> None:
    key_event(VK_CTRL, SC_CTRL, False)


def foreground_pid() -> int:
    u = ctypes.windll.user32
    pid = ctypes.c_ulong(0)
    u.GetWindowThreadProcessId(u.GetForegroundWindow(), ctypes.byref(pid))
    return pid.value


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else None
    if pid is None:
        import find_moving_objects as FMO
        pid = FMO.find_pid()
    if not pid:
        print("no Overwatch process found")
        return 1
    max_mb = int(args[args.index("--max-mb") + 1]) if "--max-mb" in args else 14000
    cands_out = args[args.index("--cands-out") + 1] if "--cands-out" in args else None

    handle = DP.open_target(pid)
    regions = [r for r in DP.regions(handle)
               if not (r[2] & (DP.PAGE_NOACCESS | DP.PAGE_GUARD)) and r[3] != DP.MEM_IMAGE]
    total = sum(r[1] for r in regions)
    print(f"pid {pid}: {total >> 20} MB present, cap {max_mb} MB — loose node scan")

    anchors: list[int] = []
    scanned = 0
    for base, size, protect, kind, _alloc in regions:
        if scanned >= max_mb * 1048576:
            print(f"  (stopping at the {max_mb} MB cap)")
            break
        offset = 0
        while offset < size:
            take = min(CHUNK, size - offset)
            if take < 0x400:
                break
            b = DP.read_region(handle, base + offset, take)
            scanned += take
            offset += take
            if not b:
                continue
            anchors.extend(scan_loose(b, base + offset - take))
        if (scanned >> 20) % 2048 < 20:
            print(f"  {scanned >> 20} MB scanned, {len(anchors)} candidate(s)", flush=True)

    print(f"\n{len(anchors)} loose candidate(s) over {scanned >> 20} MB")
    if not anchors:
        print("none: the layout guessed here does not exist in this session")
        return 1
    if cands_out:
        with open(cands_out, "w") as fh:
            fh.write("# hunt_node.py loose candidates (anchors), one per line\n")
            for a in anchors:
                fh.write(f"{a:#x}\n")

    # Phase 2: the walk.  The player's node advances while W is held and stops
    # when it is released; that pair is what separates it from static data and
    # from anything that keeps moving on its own (a wandering bot).  A mouse
    # turn would screen faster, but it needs a candidate list that *contains*
    # the node to show anything — the scan cannot yet guarantee that — and a
    # turn also changes the facing the capture depends on, so this walks.
    print(f"\nphase 2: walking W for 2.5 s — the node's feet must advance ~10-14 m "
          f"(foreground pid {foreground_pid()}, want {pid})")
    before = read_state(handle, anchors)
    hold_w(2.5)
    time.sleep(0.4)
    after = read_state(handle, anchors)
    walkers = []
    for a, b0 in before.items():
        b1 = after.get(a)
        if not b1:
            continue
        p0, y0, _ = state_of(b0)
        p1, y1, _ = state_of(b1)
        d = ((p1[0] - p0[0]) ** 2 + (p1[2] - p0[2]) ** 2) ** 0.5
        if 3.0 < d < 40.0 and abs(p1[1] - p0[1]) < 3.0:
            walkers.append((d, a, p0, p1, y0, y1))
    walkers.sort(reverse=True)
    print(f"{len(walkers)} of {len(before)} candidates advanced at a walking pace:")
    for d, a, p0, p1, y0, y1 in walkers[:20]:
        print(f"  anchor {a:#014x}  node base {a - 0xD0:#014x}  "
              f"({p0[0]:9.3f},{p0[1]:8.3f},{p0[2]:9.3f}) -> "
              f"({p1[0]:9.3f},{p1[1]:8.3f},{p1[2]:9.3f})  {d:6.3f} m  "
              f"yaw {y0:9.3f} -> {y1:9.3f}")
    if not walkers:
        print("  none advanced — either the walk key did not reach the game or the "
              "player is wedged; check the foreground and the spot, then re-run")
        return 1

    # Phase 3: the stop plus the mirror family.  A player's transform is
    # *copied* into many arenas (renderer, animation, gameplay — measured: ~16
    # copies in the 2026-10-15 sessions), and every copy walks the same
    # distance.  Noise candidates move once and stop, but they do not come in
    # families.  So: keep the candidates that stood still, then take the
    # largest cluster of (start, end) positions that agree to within a metre —
    # that cluster is the player.
    print("\nphase 3: idle 1.5 s, then cluster the identical walkers")
    time.sleep(1.5)
    s0 = read_state(handle, [a for _, a, _, _, _, _ in walkers])
    time.sleep(1.0)
    s1 = read_state(handle, list(s0))
    still = []
    for d, a, p0, p1, y0, y1 in walkers:
        b0, b1 = s0.get(a), s1.get(a)
        if not b0 or not b1:
            continue
        q0, _, _ = state_of(b0)
        q1, _, _ = state_of(b1)
        drift = ((q1[0] - q0[0]) ** 2 + (q1[2] - q0[2]) ** 2) ** 0.5
        if drift < 0.10 and abs(q0[0]) < 4000.0 and abs(q0[2]) < 4000.0:
            still.append((d, a, p0, p1, q1))
    clusters: list[list] = []
    for row in sorted(still, reverse=True):
        _, _, p0, p1, _ = row
        for cl in clusters:
            _, _, q0, q1, _ = cl[0]
            if (abs(p0[0] - q0[0]) < 1.0 and abs(p0[2] - q0[2]) < 1.0
                    and abs(p1[0] - q1[0]) < 1.0 and abs(p1[2] - q1[2]) < 1.0):
                cl.append(row)
                break
        else:
            clusters.append([row])
    clusters.sort(key=len, reverse=True)
    if not clusters:
        print("  no candidate both walked and stopped — the player is wedged or the "
              "keys did not land; check the spot and re-run")
        return 1
    print(f"{len(clusters)} cluster(s); the player's copy family:")
    for cl in clusters[:3]:
        d, a, p0, p1, q1 = cl[0]
        print(f"  {len(cl):2d} copies  ({p0[0]:9.3f},{p0[1]:8.3f},{p0[2]:9.3f}) -> "
              f"({p1[0]:9.3f},{p1[1]:8.3f},{p1[2]:9.3f})  walked {d:.3f} m")

    # Phase 4: which copy is the feet?  Crouch drops a stance-following copy by
    # ~0.47 m (measured) and leaves the feet copy bit-stable.  The recorder
    # wants the feet: its vertical is the body's, not the stance's.
    family = clusters[0]
    anchors = [a for _, a, _, _, _ in family]
    print(f"\nphase 4: holding ctrl 2.0 s — the feet copy must not drop "
          f"(the stance copies drop ~0.47 m)")
    c0 = read_state(handle, anchors)
    key_down("ctrl")
    time.sleep(2.0)
    c1 = read_state(handle, anchors)
    key_up("ctrl")
    feet, stance = [], []
    for a in anchors:
        b0, b1 = c0.get(a), c1.get(a)
        if not b0 or not b1:
            continue
        p0, _, _ = state_of(b0)
        p1, _, _ = state_of(b1)
        dy = p1[1] - p0[1]
        if abs(dy) < 0.05:
            feet.append((p0[1], a))
        elif dy < -0.2:
            stance.append((dy, a))
    for y, a in sorted(feet)[:8]:
        print(f"  feet    anchor {a:#014x}  node base {a - 0xD0:#014x}  y {y:9.3f}")
    for dy, a in sorted(stance)[:4]:
        print(f"  stance  anchor {a:#014x}  node base {a - 0xD0:#014x}  dropped {dy:+.3f} m")
    if not feet:
        print("  every copy of the family moved with the crouch — the body copy is "
              "not in it; the stance copies still work for horizontal tracing")
        best = sorted(stance)[len(stance) // 2][1]
    else:
        best = sorted(feet)[len(feet) // 2][1]
    print("\nrecord it (the anchor is the feet triple; the recorder wants the base):")
    print(f"  python dev/live_trace.py node --addr {best - 0xD0:#x} --seconds 300 "
          f"--out D:/ow-dumps/trace/player_ctrl.jsonl")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
