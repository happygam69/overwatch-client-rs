"""Paced differential capture: what changes in a live client while the *player* acts?

The clip format's values are the last unknown, and no capture so far has the
animation system's code resident. This tool takes the other route the operator
suggested: hold the client in labelled phases (idle, melee, jump, crouch, spin),
sample memory continuously, and report **what changes in one phase and not in
another** — which finds the animation runtime by its *change signature* instead of
by decoding a file.

It is paced: the tool prints the beats ("MELEE — beat 3/24") and the player
follows them, so the labels in the log are honest about what was happening.

Two samplers, both read-only:

* `--window ADDR:LEN` — high-rate diff of one window: which 16-byte rows change,
  how often, per phase, plus the float/quaternion reading of the rows that change.
* `--pages` — a bounded page-hash pass over private/mapped regions at a few Hz:
  the *set* of 4 KB pages that change in a phase (this is what *finds* the
  structures when the address is not known).

Usage:
    python live_diff.py --pid N --find-pose --phases "idle:4,melee:10,jump:5,crouch:5,spin:6"
    python live_diff.py --pid N --window 0x15ea624c380:0x2000 --phases "idle:3,melee:8"
    python live_diff.py --pid N --pages --phases "idle:4,melee:10" --budget-mb 2048

Read-only: OpenProcess(VM_READ | VM_QUERY), ReadProcessMemory only.
"""

from __future__ import annotations

import struct
import sys
import time

import numpy as np

import dump_process as DP

SKIP = (DP.PAGE_NOACCESS | DP.PAGE_GUARD)


# ---------------------------------------------------------------- pose finding

def unit_runs(buf: bytes, min_run: int) -> list[tuple[int, int]]:
    """(offset, count) of runs of >= min_run consecutive unit quaternions."""
    n = len(buf) // 4
    if n < 4 * min_run:
        return []
    a = np.frombuffer(buf, dtype="<f4", count=n)
    if len(a) < 4:
        return []
    w, x, y, z = a[:-3], a[1:-2], a[2:-1], a[3:]
    with np.errstate(over="ignore", invalid="ignore"):
        m = w * w + x * x + y * y + z * z
    ok = np.isfinite(m) & (m > 0.98) & (m < 1.02)
    idx = np.flatnonzero(ok)
    if idx.size == 0:
        return []
    breaks = np.flatnonzero(np.diff(idx) > 1)
    starts = np.concatenate(([0], breaks + 1))
    ends = np.concatenate((breaks, [idx.size - 1]))
    out = []
    for s, e in zip(starts, ends):
        if e - s + 1 >= min_run:
            out.append((int(idx[s]) * 4, int(e - s + 1)))
    return out


def find_pose(handle, min_run: int = 40, top: int = 5) -> list[tuple[int, int, int]]:
    """The biggest quaternion runs in the process: (address, count, non_identity)."""
    chunk = 4 << 20
    found: list[tuple[int, int, int, int]] = []
    scanned = 0
    for base, size, protect, kind, _alloc in DP.regions(handle):
        if protect & SKIP or size > (512 << 20):
            continue
        off = 0
        while off < size:
            data = DP.read_region(handle, base + off, min(chunk, size - off))
            if not data:
                off += chunk
                continue
            scanned += len(data)
            for at, count in unit_runs(data, min_run):
                addr = base + off + at
                picked = min(16 * count, 0x4000)
                sample = DP.read_region(handle, addr, picked)
                nonid = 0
                if sample:
                    for k in range(len(sample) // 16):
                        q = struct.unpack_from("<4f", sample, 16 * k)
                        if abs(abs(q[3]) - 1.0) > 1e-3 or abs(q[0]) + abs(q[1]) + abs(q[2]) > 1e-3:
                            nonid += 1
                # A *static* pose also looks like a pose; the one that matters is the
                # one that moves, so each candidate is sampled twice and ranked by
                # how much of it changed in 200 ms.
                time.sleep(0.2)
                again = DP.read_region(handle, addr, picked)
                moved = 0
                if sample and again and len(sample) == len(again):
                    a = np.frombuffer(sample, dtype=np.uint8).reshape(-1, 16)
                    b = np.frombuffer(again, dtype=np.uint8).reshape(-1, 16)
                    moved = int((a != b).any(axis=1).sum())
                found.append((addr, count, nonid, moved))
            off += chunk
    print(f"  (pose scan: {scanned/1048576:.0f} MB)")
    found.sort(key=lambda r: (-r[3], -r[2]))
    return found[:top]


# ---------------------------------------------------------------- samplers

class WindowSampler:
    """Which 16-byte rows of a window change, and how the floats read."""

    def __init__(self, handle, addr: int, length: int):
        self.handle, self.addr, self.length = handle, addr, length
        self.rows = length // 16
        self.prev: bytes | None = None
        self.row_hits: dict[str, np.ndarray] = {}
        self.samples: dict[str, int] = {}

    def phase(self, name: str) -> None:
        self.row_hits.setdefault(name, np.zeros(self.rows, dtype=np.int64))
        self.samples.setdefault(name, 0)
        self.prev = None

    def sample(self, name: str) -> int:
        data = DP.read_region(self.handle, self.addr, self.length)
        if not data or len(data) < self.length:
            return -1
        self.samples[name] += 1
        if self.prev is not None:
            a = np.frombuffer(self.prev, dtype=np.uint8)
            b = np.frombuffer(data, dtype=np.uint8)
            diff = (a != b).reshape(self.rows, 16)
            changed = diff.any(axis=1)
            self.row_hits[name] += changed
        self.prev = data
        return int(np.count_nonzero(np.frombuffer(data, dtype=np.uint8)))

    def report(self, say) -> None:
        print("\n--- window 0x%x (%d rows of 16 B)" % (self.addr, self.rows))
        for name, hits in self.row_hits.items():
            n = max(1, self.samples[name] - 1)
            frac = hits / n
            busy = np.flatnonzero(frac > 0.5)
            say("  %-8s %4d samples, rows changing >50%% of samples: %d" % (name, self.samples[name], busy.size))
            for row in busy[:24]:
                at = self.addr + 16 * int(row)
                data = DP.read_region(self.handle, at, 16)
                if not data:
                    continue
                q = struct.unpack("<4f", data)
                f = struct.unpack("<4f", data)
                say("      +0x%04x  0x%x  q (%+.4f, %+.4f, %+.4f, %+.4f)" % (16 * row, at, *q))


class PageSampler:
    """Which 4 KB pages change, within a byte budget, at a few Hz."""

    def __init__(self, handle, budget: int):
        self.handle, self.budget = handle, budget
        self.pages: list[int] = []
        self.tables: dict[str, list[dict[int, int]]] = {}
        for base, size, protect, kind, _alloc in DP.regions(handle):
            if protect & SKIP or kind == DP.MEM_IMAGE:
                continue
            if size < 0x10000 or size > (256 << 20):
                continue
            for a in range(base, base + size - 0x1000, 0x1000):
                self.pages.append(a)
                if len(self.pages) * 0x1000 >= self.budget:
                    break
            if len(self.pages) * 0x1000 >= self.budget:
                break
        print(f"  page sampler: {len(self.pages)} pages ({len(self.pages)*4/1024:.0f} MB)")

    def snapshot(self, name: str) -> None:
        table: dict[int, int] = {}
        for a in self.pages:
            data = DP.read_region(self.handle, a, 0x1000)
            if not data:
                continue
            table[a] = hash(data) & 0xFFFFFFFFFFFFFFFF
        self.tables.setdefault(name, []).append(table)

    def report(self, say) -> None:
        """Change *rate* per page per phase: how often each page's content
        differed between consecutive snapshots. A page that only carries a timer
        or a HUD counter changes a little in every phase; a page rewritten while
        the animation runs changes much more in the acting phases, and the
        difference against the idle baseline ranks exactly those."""
        rates: dict[str, dict[int, int]] = {}
        for name, tables in self.tables.items():
            per_page: dict[int, int] = {}
            for a, b in zip(tables, tables[1:]):
                for page, h in b.items():
                    if page in a and a[page] != h:
                        per_page[page] = per_page.get(page, 0) + 1
            rates[name] = per_page
        say("\n--- page change rates (content differing between consecutive snapshots)")
        names = list(rates)
        for name in names:
            say("  %-16s %d snapshot(s), %d page(s) changed at least once"
                % (name, len(self.tables[name]), len(rates[name])))
        if len(names) < 2:
            return
        reference = names[0]
        ranked: list[tuple[int, str, int, int]] = []
        for name in names[1:]:
            for page, count in rates[name].items():
                base = rates[reference].get(page, 0)
                if count > base:
                    ranked.append((count - base, name, page, count))
        ranked.sort(reverse=True)
        say("  ranked by (changes in a phase) - (changes in '%s'):" % reference)
        for delta, name, page, count in ranked[:24]:
            say("     0x%-12x  %-16s %3d change(s), %d in '%s'"
                % (page, name, count, rates[reference].get(page, 0), reference))


# ---------------------------------------------------------------- driver

def main() -> int:
    args = sys.argv[1:]
    def flag(name, default=None):
        return args[args.index(name) + 1] if name in args else default

    pid = int(flag("--pid", "0"))
    if not pid:
        print("pass --pid")
        return 2
    phases = []
    for part in (flag("--phases", "idle:4,melee:10") or "").split(","):
        if ":" in part:
            name, secs = part.split(":")
            phases.append((name.strip(), float(secs)))
    out = flag("--out", "D:/ow-dumps/live_diff.txt")
    handle = DP.open_target(pid)
    log = open(out, "w", encoding="utf-8")

    def say(line: str) -> None:
        print(line)
        log.write(line + "\n")
        log.flush()

    say(f"pid {pid}; phases {phases}")

    window = flag("--window")
    if not window and "--find-pose" in args:
        say("locating a pose to watch …")
        poses = find_pose(handle)
        for addr, count, nonid, moved in poses:
            say(f"  pose 0x{addr:x}: {count} quaternions, {nonid} non-identity, {moved} rows moved in 200 ms")
        if poses:
            addr, count, _, _ = poses[0]
            window = f"0x{addr:x}:{min(16*count, 0x8000):#x}"
            say(f"watching the busiest pose: {window}")
    if not window:
        print("pass --window ADDR:LEN or --find-pose")
        return 2

    addr_s, len_s = window.split(":")
    ws = WindowSampler(handle, int(addr_s, 16), int(len_s, 16))
    ps = PageSampler(handle, int(flag("--budget-mb", "2048")) << 20) if "--pages" in args else None

    for name, secs in phases:
        ws.phase(name)
        if ps:
            ps.snapshot("idle-baseline" if not ps.tables else name)
        beats = max(1, int(secs / 0.5))
        say(f"\n=== phase '{name}' for {secs:.0f}s — {beats} beats")
        t0 = time.monotonic()
        for i in range(beats):
            beat_label = "MELEE NOW" if name == "melee" else ("JUMP NOW" if name == "jump" else ("HOLD CROUCH" if name == "crouch" else ("SPIN THE MOUSE" if name == "spin" else "stay still")))
            say(f"   [{name}] beat {i+1}/{beats}  {beat_label}")
            end = t0 + 0.5 * (i + 1)
            while time.monotonic() < end:
                ws.sample(name)
                time.sleep(0.02)
            if ps and i % 4 == 3:
                ps.snapshot(name)
    say("\n" + "=" * 60)
    ws.report(say)
    if ps:
        ps.report(say)
    say(f"\nlog: {out}")
    log.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
