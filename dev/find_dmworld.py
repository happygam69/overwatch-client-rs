"""Find the physics world instance, then enumerate its real bodies.

Shape-only scanning for dmBody structures over-matches badly: the fields are
all floats, and bone/transform pools are full of unit quaternions and 1.0
pairs, so thousands of false positives pass any plausible invariant set.  The
anchor is the dmWorld object instead (field offsets from docs/formats/domino.md):

    +0x194  m_index            u16, small (<= 0x100)
    +0x198  m_inv_dt0          float; accept 0.005..0.5 (a dt) or 1..500
                               (an inverse dt) — the field name says inv, a
                               Box2D lineage stores ~60
    +0x1a0  m_gravity          vec3, mostly -y, |gy| in [0.5,500], |gx|,|gz|<=5
    +0x288  m_bodies           the body container; the first qword must be a
                               mapped-memory pointer
    +0x4c4  m_useSleep         byte {0,1} with its two neighbours

The first run required the container to be {ptr,size,cap} with small dwords —
which reads `end` as a size when the container is {begin,end,cap} (three
pointers) and rejects the real world.  Verification now tries all three
container interpretations and decides by walking: a candidate is the world
only when at least one interpretation yields shape-passing dmBody records.

`--selftest` plants a known-good world in a buffer and requires it to be
found; run that before trusting a scan.

  - offline:  OW_DUMPS=dumps_v2b python find_dmworld.py [--selftest]
  - imported: live_trace.py's `world` command reuses scan_chunk().
"""

from __future__ import annotations

import struct
import sys

import numpy as np

import regions as R

# Byte offsets relative to the world base.
O_INDEX = 0x194
O_INV_DT = 0x198
O_GRAVITY = 0x1A0
O_BODIES = 0x288
O_FLAGS = 0x4C4
WORLD_SPAN = 0x4C7

# dmBody offsets (docs/formats/domino.md).
B_POS = 0x10
B_QUAT = 0x20
B_MASS = 0x164
B_INVMASS = 0x168
B_BODYTYPE = 0x1AC
B_FLAGS = 0x1AD
BODY_SIZE = 0x1C0

# Integer windows of the two float interpretations of m_inv_dt0.
DT_LO, DT_HI = 0x3BA3D70A, 0x3F000000   # 0.005 .. 0.5
INV_LO, INV_HI = 0x3F800000, 0x43FA0000  # 1.0 .. 500.0


def body_ok(b: bytes, off: int = 0) -> bool:
    """Same shape contract as live_trace.looks_like_body, looser mass window."""
    if len(b) < off + BODY_SIZE:
        return False
    q = struct.unpack_from("<4f", b, off + B_QUAT)
    if any(c != c or abs(c) > 1.001 for c in q):
        return False
    if max(q) - min(q) < 1e-3:
        return False
    n = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]
    if not (0.98 <= n <= 1.02):
        return False
    mass, invmass = struct.unpack_from("<2f", b, off + B_MASS)
    if not (0.0 < mass < 1e6 and 0.0 < invmass <= 2.0):
        return False
    if not (0.9 <= mass * invmass <= 1.1):
        return False
    if b[off + B_BODYTYPE] > 4:
        return False
    if any(v > 1 for v in b[off + B_FLAGS : off + B_FLAGS + 10]):
        return False
    return True


def scan_chunk(data: bytes, base: int, chunk_lo: int, chunk_hi: int) -> list[dict]:
    """Vectorized world-candidate detection inside one buffer.

    `data` starts at address `base`; candidates are reported only when their
    world base falls inside [chunk_lo, chunk_hi) so overlapped reads across
    chunk boundaries do not double-report.
    """
    n = len(data)
    if n < WORLD_SPAN + 0x40:
        return []
    u8 = np.frombuffer(data, dtype=np.uint8)
    n4 = (n // 4) * 4
    u32 = np.frombuffer(u8[:n4], dtype="<u4")
    fl = u32.view(np.float32)

    dt_pos = np.nonzero(((u32 >= DT_LO) & (u32 < DT_HI)) | ((u32 >= INV_LO) & (u32 < INV_HI)))[0]
    if dt_pos.size == 0:
        return []
    wpos = dt_pos.astype(np.int64) * 4 - O_INV_DT
    ok = (wpos >= 0) & ((wpos % 8) == 0) & (wpos + WORLD_SPAN <= n)
    wpos = wpos[ok]
    if wpos.size == 0:
        return []

    def u16_at(off: np.ndarray) -> np.ndarray:
        return u8[off].astype(np.uint32) | (u8[off + 1].astype(np.uint32) << 8)

    def f32_at(off: np.ndarray) -> np.ndarray:
        return fl[off // 4]

    keep = u16_at(wpos + O_INDEX) <= 0x100
    gx = f32_at(wpos + O_GRAVITY)
    gy = f32_at(wpos + O_GRAVITY + 4)
    gz = f32_at(wpos + O_GRAVITY + 8)
    keep &= (np.abs(gx) <= 5.0) & (np.abs(gz) <= 5.0)
    keep &= (np.abs(gy) >= 0.5) & (np.abs(gy) <= 500.0)
    for k in range(3):
        keep &= u8[wpos + O_FLAGS + k] <= 1
    n8 = (n // 8) * 8
    q64 = np.frombuffer(u8[:n8], dtype="<u8")
    q0 = q64[(wpos + O_BODIES) // 8]
    keep &= (q0 > 0x1000000) & (q0 < 0x800000000000)
    wpos = wpos[keep]
    if wpos.size == 0:
        return []

    def u16v(w: int, off: int) -> int:
        return int(u8[w + off]) | (int(u8[w + off + 1]) << 8)

    def u32v(w: int, off: int) -> int:
        return int(struct.unpack_from("<I", data, w + off)[0])

    def f32v(w: int, off: int) -> float:
        return float(struct.unpack_from("<f", data, w + off)[0])

    def q64v(w: int, off: int) -> int:
        return int(struct.unpack_from("<Q", data, w + off)[0])

    out = []
    for w in wpos.tolist():
        if not (chunk_lo <= base + w < chunk_hi):
            continue
        out.append(
            {
                "world": base + w,
                "index": u16v(w, O_INDEX),
                "inv_dt": f32v(w, O_INV_DT),
                "gravity": (f32v(w, O_GRAVITY), f32v(w, O_GRAVITY + 4), f32v(w, O_GRAVITY + 8)),
                "q0": q64v(w, O_BODIES),
                "q1": q64v(w, O_BODIES + 8),
                "q2": q64v(w, O_BODIES + 0x10),
            }
        )
    return out


def verify_world(space, cand: dict, verbose: bool = True) -> tuple[int, list[int]]:
    """Try every container interpretation; return (best body count, addresses)."""
    q0, q1, q2 = cand["q0"], cand["q1"], cand["q2"]
    trials: list[tuple[str, int, int]] = []
    # {ptr, u32 size, u32 cap}
    size_a, cap_a = (q1 & 0xFFFFFFFF), (q1 >> 32)
    if 1 <= size_a <= 0x40000 and size_a <= cap_a:
        trials.append(("ptr/size/cap", q0, size_a))
    # {begin, end, cap_end} as three pointers
    for tag, endq in (("begin/end/cap(1)", q1), ("begin/end/cap(2)", q2)):
        if endq > q0 and (endq - q0) % 8 == 0 and (endq - q0) // 8 <= 0x40000:
            trials.append((tag, q0, (endq - q0) // 8))

    best = 0
    best_bodies: list[int] = []
    best_tag = "-"
    for tag, arr, n in trials:
        found: list[int] = []
        for k in range(min(n, 512)):
            p = space.u64(arr + k * 8)
            if p is None or not (0x1000000 < p < 0x800000000000):
                continue
            b = space.read(p, BODY_SIZE)
            if b is not None and body_ok(b):
                found.append(p)
                if len(found) >= 8:
                    break
        if len(found) > best:
            best, best_bodies, best_tag = len(found), found, tag
    if verbose:
        print(
            f"world {cand['world']:#x} index={cand['index']} inv_dt={cand['inv_dt']:.6f} "
            f"gravity=({cand['gravity'][0]:.2f},{cand['gravity'][1]:.2f},{cand['gravity'][2]:.2f}) "
            f"-> {best} bod(y/ies) via {best_tag}"
        )
    return best, best_bodies


# -- selftest ----------------------------------------------------------------


def make_test_world(base: int, buf: bytearray, world_off: int, n_bodies: int = 2) -> int:
    """Plant a well-formed world + body array + bodies in `buf`. Returns n."""
    w = world_off
    struct.pack_into("<H", buf, w + O_INDEX, 0)
    struct.pack_into("<f", buf, w + O_INV_DT, 60.0)
    struct.pack_into("<3f", buf, w + O_GRAVITY, 0.0, -30.0, 0.0)
    struct.pack_into("<BBB", buf, w + O_FLAGS, 1, 1, 1)
    arr_off = w + 0x100
    struct.pack_into("<Q", buf, w + O_BODIES, base + arr_off)
    struct.pack_into("<II", buf, w + O_BODIES + 8, n_bodies, 8)  # size, cap
    for k in range(n_bodies):
        body_off = w + 0x400 + k * 0x200
        struct.pack_into("<Q", buf, arr_off + k * 8, base + body_off)
        struct.pack_into("<3f", buf, body_off + B_POS, float(k), 1.0, float(k) * 2.0)
        struct.pack_into("<4f", buf, body_off + B_QUAT, 0.0, 0.0, 0.0, 1.0)
        mass = 85.0
        struct.pack_into("<2f", buf, body_off + B_MASS, mass, 1.0 / mass)
        buf[body_off + B_BODYTYPE] = 2
        struct.pack_into("<BBB", buf, body_off + B_FLAGS, 1, 1, 1)
    return n_bodies


def selftest() -> int:
    base = 0x20000000000
    buf = bytearray(0x2000)
    n = make_test_world(base, buf, 0x400, 2)
    hits = scan_chunk(bytes(buf), base, base, base + len(buf))
    if len(hits) != 1:
        print(f"FAIL: selftest world not found (hits={len(hits)})")
        for h in hits:
            print(f"  {h}")
        return 1
    h = hits[0]
    if h["world"] != base + 0x400 or h["index"] != 0:
        print(f"FAIL: selftest world located wrong: {h}")
        return 1

    class BufSpace:
        def __init__(self, b: bytes, base: int):
            self.b, self.base = b, base

        def read(self, addr: int, length: int):
            o = addr - self.base
            if o < 0 or o + length > len(self.b):
                return None
            return self.b[o : o + length]

        def u64(self, addr: int):
            b = self.read(addr, 8)
            return int.from_bytes(b, "little") if b else None

    best, bodies = verify_world(BufSpace(bytes(buf), base), h, verbose=True)
    if best != n or len(bodies) != n:
        print(f"FAIL: selftest bodies not walked (best={best}, n={n})")
        return 1
    print(f"selftest: PASS (world found at {h['world']:#x}, {n} bodies walked)")
    return 0


# -- offline main -------------------------------------------------------------


def main() -> int:
    if "--selftest" in sys.argv:
        return selftest()

    import concurrent.futures as cf
    import threading

    space = R.Space(R.load(R.capture_dir("dumps_v2b")))
    CHUNK = 64 * 1024 * 1024
    OVERLAP = 0x400
    regions = [r for r in space.regions if not (r.protect & 0x101) and r.size >= 0x1000]
    print(f"{len(regions)} regions to scan", flush=True)

    def work(reg: R.Region) -> tuple[int, list[dict]]:
        """Scan one region, reading the file directly (Region.data() caches
        whole files and the capture is 10.6 GB)."""
        hits: list[dict] = []
        scanned = 0
        try:
            f = open(reg.path, "rb")
        except OSError:
            return 0, []
        with f:
            off = 0
            while off < reg.size:
                take = min(CHUNK, reg.size - off)
                read_len = min(take + OVERLAP, reg.size - off)
                f.seek(off)
                data = f.read(read_len)
                if not data:
                    break
                hits += scan_chunk(data, reg.base + off, reg.base + off, reg.base + off + take)
                off += take
                scanned += take
        return scanned, hits

    kept: list[dict] = []
    scanned_total = 0
    done = 0
    lock = threading.Lock()
    with cf.ThreadPoolExecutor(max_workers=12) as ex:
        futures = [ex.submit(work, r) for r in regions]
        for fut in cf.as_completed(futures):
            nn, hits = fut.result()
            with lock:
                scanned_total += nn
                kept += hits
                done += 1
                if done % 4000 == 0:
                    print(
                        f"  ... {done}/{len(regions)} regions, "
                        f"{scanned_total/2**30:.1f} GB, {len(kept)} raw candidate(s)",
                        flush=True,
                    )
    print(f"scan done: {scanned_total/2**30:.1f} GB, {len(kept)} raw candidate(s)", flush=True)

    uniq = {c["world"]: c for c in kept}
    real = 0
    for cand in sorted(uniq.values(), key=lambda c: c["world"]):
        best, bodies = verify_world(space, cand, verbose=True)
        if best > 0:
            real += 1
            for p in bodies:
                b = space.read(p, BODY_SIZE)
                if b is None:
                    continue
                pos = struct.unpack_from("<3f", b, B_POS)
                vel = struct.unpack_from("<3f", b, 0x30)
                print(
                    f"    body {p:#x} pos=({pos[0]:9.3f},{pos[1]:9.3f},{pos[2]:9.3f}) "
                    f"vel=({vel[0]:7.3f},{vel[1]:7.3f},{vel[2]:7.3f})"
                )
    print(f"{real} verified world(s) of {len(uniq)} candidate(s)")
    return 0 if real else 1


if __name__ == "__main__":
    raise SystemExit(main())
