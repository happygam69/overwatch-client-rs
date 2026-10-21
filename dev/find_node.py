"""Find the player transform node by its measured layout signature.

The movement traces (`player_keyed.jsonl` and the flat ones) were recorded from
the player transform node found by a straight-walk scan. Its layout was then
read out of a live session (`docs/formats/domino.md`, "The player node found"):

    +0xd0   feet position, 3 f32
    +0xdc   yaw in degrees (170.57 at spawn)
    +0xe0   orientation quaternion (unit)
    +0xf0   scale (1, 1, 1)
    +0x108, +0x138, +0x178   the position repeated
    +0x1b8  capsule centre (feet + ~1.71 m)

That signature is strong enough to find the node in a *single* pass - no
walking needed - and the recorder (`live_trace.py node`) needs the node's base
address to record the movement traces the differ consumes.

    python find_node.py [--pid N] [--max-mb 12288] [--watch-ms 1500]
                        [--cands-out F]      # save the candidate list
    python find_node.py --from F [--watch-ms 4000] [--rounds 8]
                                             # re-run the walk confirm only
    python find_node.py --selftest

Default run: scan committed memory once for the signature, then re-read every
candidate after `--watch-ms` and report which ones moved (walk during that
window; the player's node moves, a stale copy does not). `--selftest` runs the
matcher on a synthetic buffer offline and exits - no game, no process.

Stand up straight while scanning: the capsule-centre check expects the
standing half-height, so a crouched player is not found.

READ-ONLY: VM_READ|VM_QUERY + ReadProcessMemory; nothing is written to the
target.
"""

from __future__ import annotations

import struct
import sys
import time

import numpy as np

import dump_process as DP
import find_moving_objects as FMO

CHUNK = 16 << 20

# Node layout offsets, in f32 units, relative to the feet triple (the scan's
# address). Byte offsets: yaw +0xc, quat +0x10, scale +0x20, repeats +0x38/
# +0x68/+0xa8, capsule +0xe8.
D_YAW = 3          # +0xdc - +0xd0
D_QUAT = 4         # +0xe0 - +0xd0
D_SCALE = 8        # +0xf0 - +0xd0
D_REPS = (14, 26, 42)            # +0x108/+0x138/+0x178
D_CAPSULE = 58     # +0x1b8 - +0xd0
LAST = D_CAPSULE + 1             # floats needed from the triple onward


def scan_buffer(buf: bytes, origin: int) -> list[int]:
    """Every feet-triple address in one buffer that carries the node layout.

    `origin` is the buffer's base address, added to the local offsets. The
    checks are the measured layout's: a map-scale finite triple, a plausible
    yaw, a unit quaternion, a uniform scale, the position repeated at three
    fixed offsets, and a capsule centre between 1.0 and 2.4 m above the feet
    (hero-dependent, so a range, not the 1.71 of one hero).
    """
    nf = len(buf) // 4
    if nf <= LAST:
        return []
    fa = np.frombuffer(buf[: nf * 4], dtype="<f4")
    # The candidates are *every* float offset (stride 1 float), so a check at
    # `base + k` over all candidates is the contiguous slice fa[k:k+m], not a
    # gather: `fa[idx + k]` with idx = arange(m) is fancy indexing and cost
    # ~40x more than the view it is equal to (measured: 842 ms for a 16 MB
    # buffer with gathers, 2.7 ms with slices).  Same predicates, same hits.
    m = nf - LAST
    with np.errstate(all="ignore"):
        v0, v1, v2 = fa[0:m], fa[1 : m + 1], fa[2 : m + 2]
        ok = np.isfinite(v0) & np.isfinite(v1) & np.isfinite(v2)
        ok &= (np.abs(v0) < 4000.0) & (np.abs(v2) < 4000.0)
        yaw = fa[D_YAW : D_YAW + m]
        ok &= np.isfinite(yaw) & (np.abs(yaw) <= 720.0)
        qn = np.sqrt(
            sum(qk * qk for qk in
                (fa[D_QUAT + k : D_QUAT + k + m] for k in range(4)))
        )
        ok &= np.abs(qn - 1.0) < 0.01
        s0 = fa[D_SCALE : D_SCALE + m]
        s1 = fa[D_SCALE + 1 : D_SCALE + 1 + m]
        s2 = fa[D_SCALE + 2 : D_SCALE + 2 + m]
        ok &= (s0 > 0.05) & (s0 < 20.0)
        ok &= (np.abs(s0 - s1) < 1e-3) & (np.abs(s1 - s2) < 1e-3)
        for d in D_REPS:
            ok &= np.abs(fa[d : d + m] - v0) < 0.25
            ok &= np.abs(fa[d + 1 : d + 1 + m] - v1) < 0.25
            ok &= np.abs(fa[d + 2 : d + 2 + m] - v2) < 0.25
        cap = fa[D_CAPSULE : D_CAPSULE + m]
        ok &= np.isfinite(cap) & (cap - v1 > 1.0) & (cap - v1 < 2.4)
        hits = [origin + int(i) * 4 for i in np.nonzero(ok)[0]]
    return hits


def selftest() -> int:
    """The matcher on synthetic buffers: one planted node, three near-misses."""
    def record(pos, yaw=170.57, quat=(0.0, 0.0, 0.0, 1.0), scale=(1.0, 1.0, 1.0),
               rep_off=0.0, cap_off=1.71):
        b = bytearray(0x200)
        struct.pack_into("<3f", b, 0xD0, *pos)
        struct.pack_into("<f", b, 0xDC, yaw)
        struct.pack_into("<4f", b, 0xE0, *quat)
        struct.pack_into("<3f", b, 0xF0, *scale)
        for off in (0x108, 0x138, 0x178):
            struct.pack_into("<3f", b, off, pos[0] + rep_off, pos[1], pos[2])
        struct.pack_into("<f", b, 0x1B8, pos[1] + cap_off)
        return bytes(b)

    pos = (55.9, 0.76, 20.0)
    plant = 0x1000 + 0xD0
    buf = bytearray(0x8000)
    buf[0x1000:0x1200] = record(pos)
    hits = scan_buffer(bytes(buf), 0x0)
    assert hits == [plant], f"planted node not found exactly: {[hex(h) for h in hits]}"

    cases = {
        "capsule 1 m off": record(pos, cap_off=2.71),
        "repeat 1 m off": record(pos, rep_off=1.0),
        "yaw 9999": record(pos, yaw=9999.0),
        "quat not unit": record(pos, quat=(0.0, 0.0, 0.0, 2.0)),
        "scale 1,1,2": record(pos, scale=(1.0, 1.0, 2.0)),
    }
    for name, rec in cases.items():
        buf = bytearray(0x8000)
        buf[0x1000:0x1200] = rec
        assert scan_buffer(bytes(buf), 0x0) == [], f"near-miss matched: {name}"
    print("selftest: planted node found; 5 near-misses rejected")
    return 0


def main() -> int:
    args = sys.argv[1:]
    if "--selftest" in args:
        return selftest()
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else FMO.find_pid()
    if not pid:
        print("no Overwatch process found")
        return 1
    max_mb = int(args[args.index("--max-mb") + 1]) if "--max-mb" in args else 12288
    watch_ms = int(args[args.index("--watch-ms") + 1]) if "--watch-ms" in args else 1500
    cands_out = (args[args.index("--cands-out") + 1] if "--cands-out" in args else None)

    handle = DP.open_target(pid)
    regions = [r for r in DP.regions(handle)
               if not (r[2] & (DP.PAGE_NOACCESS | DP.PAGE_GUARD)) and r[3] != DP.MEM_IMAGE]
    total = sum(r[1] for r in regions)

    hits: list[int] = []
    scanned = 0
    if "--from" in args:
        # The scan is the expensive part (minutes); the motion confirm is not.
        # A saved candidate list lets the walk round be repeated on its own.
        src = args[args.index("--from") + 1]
        with open(src) as fh:
            hits = [int(ln, 16) for ln in fh if ln.strip() and not ln.startswith("#")]
        print(f"{len(hits)} candidate(s) loaded from {src} — scan skipped")
    else:
        print(f"pid {pid}: {total >> 20} MB present, cap {max_mb} MB — scanning for the node layout")
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
                hits.extend(scan_buffer(b, base + offset - take))
            if (scanned >> 20) % 2048 < 20:
                print(f"  {scanned >> 20} MB scanned, {len(hits)} candidate(s)", flush=True)

        print(f"\n{len(hits)} candidate(s) over {scanned >> 20} MB")
        if cands_out and hits:
            with open(cands_out, "w") as fh:
                fh.write("# find_node.py candidates, one feet-triple address per line\n")
                for a in hits:
                    fh.write(f"{a:#x}\n")
            print(f"candidates saved to {cands_out} — re-run the walk confirm with "
                  f"--from {cands_out}")
    if not hits:
        print("none: stand up straight (the capsule check expects standing height) "
              "and re-run; the record may also have been freed (re-enter the range)")
        return 1
    # Motion confirm: the player's node moves; a stale copy does not.  Re-read
    # in rounds, so the operator has time to click into the game and walk.
    rounds = int(args[args.index("--rounds") + 1]) if "--rounds" in args else 6
    movers: list[tuple[float, int]] = []
    for rnd in range(rounds):
        before = {a: DP.read_region(handle, a, 12) for a in hits}
        print(f"walk now — round {rnd + 1}/{rounds}, re-reading in {watch_ms} ms", flush=True)
        time.sleep(watch_ms / 1000.0)
        for a in hits:
            b0, b1 = before.get(a), DP.read_region(handle, a, 12)
            if not b0 or not b1:
                continue
            p0 = struct.unpack_from("<3f", b0, 0)
            p1 = struct.unpack_from("<3f", b1, 0)
            d = ((p1[0] - p0[0]) ** 2 + (p1[2] - p0[2]) ** 2) ** 0.5 / (watch_ms / 1000.0)
            if d > 0.5:
                movers.append((d, a))
                yaw_b = DP.read_region(handle, a + 4 * D_YAW, 4) or b"\0\0\0\0"
                yaw = struct.unpack_from("<f", yaw_b, 0)[0]
                print(f"  moves: triple {a:#014x}  node base {a - 0xD0:#014x}  "
                      f"pos ({p1[0]:9.3f},{p1[1]:8.3f},{p1[2]:9.3f})  yaw {yaw:8.2f}  "
                      f"{d:5.2f} m/s")
        if movers:
            break
    if movers:
        _, best = max(movers)
        print("\nrecord the mover:")
        print(f"  python dev/live_trace.py node --addr {best - 0xD0:#x} --seconds 300 "
              f"--out D:/ow-dumps/trace/player_ctrl.jsonl")
    else:
        print(f"\nnothing moved in {rounds} rounds — walk (W held) while it re-reads "
              f"and re-run")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
