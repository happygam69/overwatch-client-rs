"""Find the physics world through its broadphase trees.

Value-based world signatures do not have enough specificity: 29k candidates in
the v2b capture, none real.  The structure is the signature.  dmBroadPhase is
INLINE in dmWorld at +0x1b0 and holds four dmDynamicTree instances at +0x30
(onward), with proxyCount at +0xd0 — the arithmetic closes exactly:
0x30 + 4*0x28 = 0xd0.  So world+0x1e0 is trees[0], +0x208 trees[1], +0x230
trees[2], +0x258 trees[3].

A dmDynamicTree (field offsets from docs/formats/domino.md) is:
    +0x00 nodes      pointer, 8-aligned, mapped
    +0x08 capacity   u32, a power of two (the tree doubles as it grows)
    +0x0c count      u32, 0 <= count <= capacity
    +0x10 freeList   u32, < capacity
    +0x14 root        u32, < capacity
    +0x18 proxyCount u32, <= count
    +0x1c path       u32, small

and its nodes (0x40 each) hold a bcAABB at +0x0 (min xyz then max xyz as
8 floats) with min <= max per axis, plus a userData u64 at +0x20.  The
conjunction of the integer relations, the power-of-two capacity, the live
node array, and the AABB ordering is what no incidental data satisfies.

`--selftest` plants a synthetic world with one populated tree and requires
the finder to locate it and walk its nodes; run that before a scan.

    OW_DUMPS=dumps_v2b python find_broadphase.py [--selftest]
"""

from __future__ import annotations

import struct
import sys

import numpy as np

import regions as R
from find_dmworld import BODY_SIZE, body_ok

O_BROADPHASE = 0x1B0
O_TREES = 0x30
TREE_SIZE = 0x28
N_TREES = 4
O_PROXY_COUNT = 0xD0
O_INDEX = 0x194
O_INV_DT = 0x198
O_GRAVITY = 0x1A0
O_BODIES = 0x288
O_FLAGS = 0x4C4
WORLD_SPAN = 0x4C7

# Tree field offsets.
T_NODES, T_CAP, T_COUNT, T_FREE, T_ROOT, T_PROXY = 0, 8, 0xC, 0x10, 0x14, 0x18

NODE_SIZE = 0x40


def tree_ok(space, tree_addr: int, verbose: bool = False) -> int:
    """Return the number of sane nodes if this is a dynamic tree, else 0."""
    hdr = space.read(tree_addr, 0x20)
    if hdr is None:
        return 0
    nodes, cap, count, free, root, proxy, path = struct.unpack(
        "<QIIIIII", hdr[:0x1C].ljust(0x20, b"\x00")
    )
    if not (0x1000000 < nodes < 0x800000000000) or nodes % 8:
        return 0
    if cap < 8 or cap > 0x400000 or (cap & (cap - 1)) != 0:
        return 0
    if not (0 <= count <= cap) or free >= cap or root >= cap or proxy > count or path > 0x10000:
        return 0
    if count == 0:
        return 0
    good = 0
    for k in range(min(count, 32)):
        nd = space.read(nodes + k * NODE_SIZE, NODE_SIZE)
        if nd is None:
            break
        aabb = struct.unpack_from("<8f", nd, 0)
        if any(v != v or abs(v) > 1e7 for v in aabb):
            continue
        if aabb[0] > aabb[4] or aabb[1] > aabb[5] or aabb[2] > aabb[6]:
            continue
        ud = struct.unpack_from("<Q", nd, 0x20)[0]
        if ud != 0 and ud > 0x100000000:
            continue
        good += 1
    if verbose:
        print(
            f"    tree {tree_addr:#x} nodes={nodes:#x} cap={cap} count={count} "
            f"free={free} root={root} proxy={proxy} good_nodes={good}"
        )
    return good


def world_shape(space, world: int, verbose: bool = False) -> int:
    """Check the world field frame around a tree candidate; return tree score."""
    tail = space.read(world + O_FLAGS, 3)
    if tail is None or any(b > 1 for b in tail):
        return 0
    idx = space.read(world + O_INDEX, 2)
    if idx is None or struct.unpack("<H", idx)[0] > 0x400:
        return 0
    dt = space.read(world + O_INV_DT, 4)
    gv = space.read(world + O_GRAVITY, 12)
    if dt is None or gv is None:
        return 0
    dtf = struct.unpack("<f", dt)[0]
    if not (-0.01 <= dtf <= 2000.0):
        return 0
    g = struct.unpack("<3f", gv)
    gmax = max(abs(v) for v in g)
    if any(v != v for v in g) or not (0.0 <= gmax <= 2000.0):
        return 0
    # At least one tree slot populated; score = sum of sane nodes.
    score = 0
    used = 0
    for j in range(N_TREES):
        t = world + O_BROADPHASE + O_TREES + j * TREE_SIZE
        g2 = tree_ok(space, t, verbose=verbose)
        if g2 > 0:
            used += 1
            score += g2
    if used == 0:
        return 0
    return score


# -- vectorized prefilter -----------------------------------------------------


def scan_chunk(data: bytes, base: int, lo: int, hi: int) -> list[dict]:
    n = len(data)
    n8 = (n // 8) * 8
    if n8 < 0x200:
        return []
    u8 = np.frombuffer(data, dtype=np.uint8)
    n4 = (n // 4) * 4
    u32 = np.frombuffer(u8[:n4], dtype="<u4")
    q64 = np.frombuffer(u8[:n8], dtype="<u8")

    # Candidate tree bases: every 8-aligned offset with tree-shaped integers.
    max_o = (n8 - TREE_SIZE) & ~7
    o = np.arange(0, max_o, 8, dtype=np.int64)
    cap = u32[(o + T_CAP) // 4]
    cnt = u32[(o + T_COUNT) // 4]
    fre = u32[(o + T_FREE) // 4]
    root = u32[(o + T_ROOT) // 4]
    proxy = u32[(o + T_PROXY) // 4]
    q0 = q64[o // 8]
    pow2 = (cap != 0) & ((cap & (cap - 1)) == 0)
    keep = (
        pow2
        & (cap >= 8)
        & (cap <= 0x400000)
        & (cnt >= 1)
        & (cnt <= cap)
        & (fre < cap)
        & (root < cap)
        & (proxy <= cnt)
        & (q0 > 0x1000000)
        & (q0 < 0x800000000000)
        & ((q0 & 7) == 0)
    )
    o = o[keep]
    if o.size == 0:
        return []

    # world = tree_base - 0x1e0 - 0x28*slot, slot 0..3
    out = []
    for slot in range(N_TREES):
        wpos = o - (O_BROADPHASE + O_TREES + slot * TREE_SIZE)
        ok = (wpos >= 0) & (wpos + WORLD_SPAN <= n) & ((wpos & 7) == 0)
        wp = wpos[ok]
        if wp.size == 0:
            continue
        # Field frame checks, vectorized (same conditions as world_shape).
        f_ok = np.ones(wp.shape, dtype=bool)
        for k in range(3):
            f_ok &= u8[wp + O_FLAGS + k] <= 1
        idx16 = u8[wp + O_INDEX].astype(np.uint32) | (u8[wp + O_INDEX + 1].astype(np.uint32) << 8)
        f_ok &= idx16 <= 0x400
        dtv = u32[(wp + O_INV_DT) // 4].view(np.float32)
        f_ok &= (dtv == dtv) & (dtv >= -0.01) & (dtv <= 2000.0)
        gbase = (wp + O_GRAVITY) // 4
        gxf = u32[gbase].view(np.float32)
        gyf = u32[gbase + 1].view(np.float32)
        gzf = u32[gbase + 2].view(np.float32)
        f_ok &= (gxf == gxf) & (gyf == gyf) & (gzf == gzf)
        f_ok &= (
            (np.abs(gxf) <= 2000.0) & (np.abs(gyf) <= 2000.0) & (np.abs(gzf) <= 2000.0)
        )
        wp = wp[f_ok]
        for w in wp.tolist():
            if not (lo <= base + w < hi):
                continue
            out.append({"world": base + w, "slot": slot, "tree": base + w + O_BROADPHASE + O_TREES + slot * TREE_SIZE})
    return out


# -- selftest -----------------------------------------------------------------


def make_test_world(base: int, buf: bytearray, world_off: int) -> None:
    w = world_off
    struct.pack_into("<H", buf, w + O_INDEX, 0)
    struct.pack_into("<f", buf, w + O_INV_DT, 60.0)
    struct.pack_into("<3f", buf, w + O_GRAVITY, 0.0, -30.0, 0.0)
    struct.pack_into("<BBB", buf, w + O_FLAGS, 1, 1, 1)
    t = w + O_BROADPHASE + O_TREES
    nodes_off = w + 0x800
    nodes = base + nodes_off
    struct.pack_into("<QIIIIII", buf, t, nodes, 16, 3, 3, 0, 3, 0)
    for k in range(3):
        off = nodes_off + k * NODE_SIZE
        struct.pack_into("<8f", buf, off, -1.0, -2.0, -3.0, 1.0, 2.0, 3.0, 0.0, 0.0)
        struct.pack_into("<Q", buf, off + 0x20, k)


def selftest() -> int:
    base = 0x30000000000
    buf = bytearray(0x2000)
    make_test_world(base, buf, 0x400)
    hits = scan_chunk(bytes(buf), base, base, base + len(buf))
    if not hits:
        print("FAIL: synthetic tree/world not detected")
        return 1
    h = hits[0]
    if h["world"] != base + 0x400 or h["slot"] != 0:
        print(f"FAIL: located wrong: {h}")
        return 1

    class BufSpace:
        def __init__(self, b: bytes, base: int):
            self.b, self.base = b, base

        def read(self, addr: int, length: int):
            off = addr - self.base
            if off < 0 or off + length > len(self.b):
                return None
            return self.b[off : off + length]

        def u64(self, addr: int):
            b = self.read(addr, 8)
            return int.from_bytes(b, "little") if b else None

    sp = BufSpace(bytes(buf), base)
    score = world_shape(sp, h["world"], verbose=True)
    if score < 3:
        print(f"FAIL: world_shape scored {score}, want >= 3")
        return 1
    print(f"selftest: PASS (world found at {h['world']:#x}, node score {score})")
    return 0


# -- scan ---------------------------------------------------------------------


def main() -> int:
    if "--selftest" in sys.argv:
        return selftest()

    import concurrent.futures as cf
    import threading

    space = R.Space(R.load(R.capture_dir("dumps_v2b")))
    CHUNK = 64 * 1024 * 1024
    OVERLAP = 0x600
    regions = [r for r in space.regions if not (r.protect & 0x101) and r.size >= 0x1000]
    print(f"{len(regions)} regions to scan", flush=True)

    def work(reg: R.Region):
        hits = []
        scanned = 0
        try:
            f = open(reg.path, "rb")
        except OSError:
            return 0, []
        with f:
            off = 0
            while off < reg.size:
                take = min(CHUNK, reg.size - off)
                f.seek(off)
                data = f.read(min(take + OVERLAP, reg.size - off))
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
        futs = [ex.submit(work, r) for r in regions]
        for fut in cf.as_completed(futs):
            nn, hits = fut.result()
            with lock:
                scanned_total += nn
                kept += hits
                done += 1
                if done % 4000 == 0:
                    print(
                        f"  ... {done}/{len(regions)} regions, {scanned_total/2**30:.1f} GB, "
                        f"{len(kept)} raw candidate(s)",
                        flush=True,
                    )
    print(f"scan done: {scanned_total/2**30:.1f} GB, {len(kept)} raw candidate(s)", flush=True)

    uniq = {c["world"]: c for c in kept}
    real = 0
    for cand in sorted(uniq.values(), key=lambda c: c["world"]):
        score = world_shape(space, cand["world"], verbose=True)
        if score <= 0:
            continue
        real += 1
        print(f"  WORLD {cand['world']:#x} tree score {score}")
        # Bonus: try the body container both ways, if the frame has one.
        for tag, arr_off, size_off in (("ptr/size/cap", 0, 8),):
            q0 = space.u64(cand["world"] + O_BODIES)
            sc = space.read(cand["world"] + O_BODIES + 8, 8)
            if not q0 or not sc:
                continue
            size, cap = struct.unpack("<II", sc)
            if 1 <= size <= 0x40000 and size <= cap:
                bodies = 0
                for k in range(min(size, 512)):
                    p = space.u64(q0 + k * 8)
                    if not p or not (0x1000000 < p < 0x800000000000):
                        continue
                    b = space.read(p, BODY_SIZE)
                    if b is not None and body_ok(b):
                        bodies += 1
                print(f"    bodies via {tag}: {bodies} shaped of size={size}")
    print(f"{real} world(s) with a live broadphase of {len(uniq)} candidate(s)")
    return 0 if real else 1


if __name__ == "__main__":
    raise SystemExit(main())
