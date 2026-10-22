"""Protection-differential page capture for the running client (v3).

Why v3 exists: v2 kept the protection map as a dict of region tuples and
diffed content over `exec_pages`, a dict of session addresses.  Three
defects came out of a t+5 s startup capture (pages 3888, rejected 12180,
exec 371):

* Region-identity bleed.  VirtualQueryEx merges and splits regions as
  protections flip, so the (name, base, size) keys reshuffle constantly.
  v2 pruned `exec_pages` for every key that vanished - including pages
  still readable-executable under a new key - and never refcounted.  At
  t+5 s only 371 of ~16 k readable-exec pages were still tracked, so the
  content-diff stripe, the only mechanism that can catch an in-place
  decrypt (ciphertext at rest, plaintext while hot), was watching 2% of
  the module.
* The stripe re-read captured pages unconditionally.  With 371 pages
  tracked and --diff-pages 4096 the whole budget went to re-fingerprinting
  the same captured pages every pass (~56 MB/s of a 61 MB/s read budget),
  while 12 k readable-ciphertext pages sat outside the tracking.
* Flip handling was region-granular and poll reads were synchronous in
  the scheduler loop, so a reshuffle re-read whole regions at BASE_PRIO
  and a large --focus window could stall the loop for seconds.

What v3 does differently:

* A dedicated watcher thread walks the module protections at a target
  --watch-hz (default 250, i.e. ~4 ms worst-case flip latency) and keeps
  the protection map as per-page uint16 arrays, one per module.  A
  protection change is a numpy compare; only pages whose protection code
  actually changed get read, at page granularity, immediately, at
  FLIP_PRIO.  Region merges and splits can no longer lose tracking,
  because there are no region identities to lose.
* All scheduler state is page-indexed numpy arrays per module (tracked /
  captured / fingerprint).  The content-diff stripe rotates over
  tracked-and-not-captured pages only, so no budget is spent re-reading
  pages already on disk (--recapture brings captured pages back as a
  separate bounded tier, off by default).
* The stripe budget is adaptive: it grows while readers idle and shrinks
  on queue backlog, so the sweep rate self-tunes to the box.
* Pages in 2 MB blocks with recent flips are swept every pass (hot tier),
  and ciphertext pages whose content changes between reads (churn) get
  their retry backoff reset: a page that is being rewritten is about to
  decrypt.
* Retry reads are batched into contiguous multi-page ReadProcessMemory
  calls instead of one syscall per page.
* Poll pages (priority list + --focus windows) go through the reader pool
  with in-flight tracking instead of blocking the scheduler thread.
* coverage.csv (per-2 MB block: tracked / captured / cipher / flips) and
  gaps.focus (focus tokens for the still-missing pages) close the loop
  between runs: capture, read coverage, feed gaps.focus into the next
  run's --focus-file so those pages are polled every pass.

v3.1 throughput pass (after the first field run at t+5 s: wq and rq both
pegged, rpm collapsed to 91 MB/s and urgent flip reads queued behind
spilled stripe reads): the result queue is unbounded so readers never
sleep on it; only urgent reads spill when the work queue is full, while
regenerable tiers (stripe/retry/hot/recap) drop and are re-planned next
pass; the scheduler drains results to empty with a longer burst box;
cipher pages parked in recently-flipped 2 MB blocks are re-armed from the
retry ring every 0.5 s instead of waiting out the exponential backoff;
per-read result handling is vectorized (one numpy fingerprint compare
per run, only changed pages classified); a VirtualQueryEx failure falls
back for one walk and retries instead of latching the fallback
permanently.  The status line reports walk ms/source and queue drops.

v3.2 startup passes (for the storm at attach, when ~12 k pages read as
ciphertext in the first 5 s): every mechanism that can catch a decrypt has
a knob that starves it exactly when the storm runs.

* The adaptive stripe budget shrinks 0.7x on queue backlog and only grows
  while readers idle, so under the burst it heads from 8192 toward
  BUDGET_MIN - the rotation that catches in-place decrypts goes quiet at
  the one moment everything decrypts.
* The retry ring doubles its backoff on every read, so a page rejected 5
  times in 2 s sits at multi-second cadence; the churn detection that
  would reset it needs that same read.  Catch-22.
* pop_due (retry scheduling) refuses to run while the work queue is
  nearly full, and a result lost to the RQ drop guard leaves its page
  "in flight" for 10 s.

Surge mode (--surge-seconds, default 20, 0 disables) pins those knobs for
the first N seconds of the run: the stripe floor is the configured
--diff-pages instead of BUDGET_MIN, ring backoff is capped at
--surge-backoff (default 0.5 s), the retry budget is --retry-budget-max
every pass, and stale in-flight sweeps run at 2 s (1.5 s for poll).
After the window the tool returns to plain v3.1 behaviour, so a
practice-range capture later in the run is unchanged.  Bookkeeping was
also made storm-measurable: a page that decrypts is now rescinded from
the encrypted-page records (counted as "rescued") instead of staying in
both lists, and the run end prints a recap of captured-in-surge,
rescued-after-cipher, and still-cipher pages.  --chunk-pages (default
256 = 1 MB Reads) is a knob for batch-granularity experiments; smaller
chunks pay more syscalls but split the page-wise fallback differently
over mixed readable/cipher spans.

v3.3 post-surge field pass (first surge run at t+90 s menu boot: 12,699
pages captured, 7,511 of them rescued after reading as cipher at least
once, all inside the 20 s window - but 425,290 results dropped and a
retry ring that never turned over, tries=0 for 92% of the residual):

* The result-drop guard was counted, so a flood of cheap 4 KB
  page-fallback entries blew the cap and discarded whole chunks while
  the byte budget sat nearly empty.  The guard is now bytes-based
  (--rq-bytes-mb, default 512 MB) with a high count brake; dropped
  reads are rare, oversized bursts drain in order instead of vanishing.
* pop_due was fully blocked whenever schedule_stripe kept the work
  queue topped up (the queue never fell below its gate), so no retry
  ring reads happened after the base burst.  The gates are split:
  the stripe stops at WQ_MAX-2048, the ring may plan until
  WQ_MAX-256, and surge widens the retry budget path it always used.
* stats.csv logs one row per adapt tick (rpm, read MB, rq depth and
  bytes, both drop counters, wq depth, overflow, budget, stripe
  backlog, passes) so the next run's postmortem is measured, not
  inferred.  meta.json now also carries the total bytes read.

v3.4 tight tier (for the page-crypt recon: the loader's decrypt engine
and its .eid store decrypt transiently during play, observed only as
readable ciphertext - the plaintext window is a content change, not a
protection flip): a dedicated small page set (e.g. the 342 cold loader
pages plus the .eid range ~1.5k pages) is polled on a fixed cadence and,
on any fingerprint change, is immediately re-read a number of times
(chase) at top priority.  Every tight read is an `events.csv` row so
the decrypt-window duration becomes measurable instead of assumed:
`dev/tight_report.py` distils that trail.  The set is built by
--tight-zeros (pages that are all-zero in a warm image - the loader's
cold band and .eid after every capture to date) plus --tight-rva
module-relative spans; --tight-hz sets the cadence; --tight-chase
the per-change re-read count.  The tier is OFF unless --tight-zeros or
--tight-rva is given, so plain v3.3 behaviour is unchanged.

Defaults are tuned for a 6-core / 16-thread box writing to the same disk
as the game: --threads 12, --watch-hz 250, --diff-pages 8192, output
under E:/ow-dumps/live_page_diff.

Read-only as before: OpenProcess(VM_READ | VM_QUERY), ReadProcessMemory
and VirtualQueryEx only.  Nothing in the target is written, no protection
is changed, nothing is injected, no debugger is attached.

Usage:
    python live_page_diff.py [--pid N] [--wait] [--out DIR] [--seconds N]
                             [--all-modules] [--all-readable] [--dry-run]
                             [--priority 0xV2B,...] [--focus RVA[-RVA],...]
                             [--focus-file PATH] [--threads N]
                             [--watch-hz HZ] [--diff-pages N] [--no-diff]
                             [--include-data] [--poll-budget N]
                             [--retry-budget N] [--retry-budget-max N]
                             [--hot-window S] [--hot-cap N] [--autofocus K]
                             [--recapture] [--recap-pages N]
                             [--strict-classify] [--legacy-enum]
                             [--chunk-pages N]
                             [--surge-seconds N] [--surge-backoff S]
                             [--rq-bytes-mb N]
                             [--tight-rva A-B,...] [--tight-zeros PATH]
                             [--tight-hz HZ] [--tight-chase N]
                             [--pass-sleep S]

Press Enter in this console to mark a phase; a line starting with a
backslash stops the run.  Output goes to E:/ow-dumps/live_page_diff and
is never committed.

Practical notes: attach from the launcher (`--wait`) for the startup
yield.  For gameplay-only pages run Practice Range bursts and mark a
phase with Enter on each ability/emote trigger.  Afterwards, read
coverage.csv to see the 2 MB blocks where decryption concentrated and how
much of each was captured, and feed gaps.focus to the next run's
--focus-file.  numpy is required (the scheduler is page-indexed arrays).
"""

from __future__ import annotations

import argparse
import ctypes
import heapq
import itertools
import json
import os
import queue
import random
import sys
import threading
import time
from collections import deque

try:
    import numpy as np
except ImportError:
    print("v3 requires numpy (pip install numpy): the page-indexed state "
          "and the bitmap protection diff are built on it.", file=sys.stderr)
    raise SystemExit(2)

import dump_process as DP
import warm_poll as WP

PAGE = 0x1000
PAGE_SHIFT = 12
CHUNK = 0x100000
CHUNK_PAGES = CHUNK >> PAGE_SHIFT
CH_PAGES = [CHUNK_PAGES]   # runtime chunk size, set from --chunk-pages
MAIN_MIN_SIZE = 0x2000000

# scheduling priorities (lower reads first)
FLIP_PRIO = 0          # protection flip: read right now
BASE_PRIO = 1           # first-seen readable pages
POLL_PRIO = 1           # priority + focus pages
RETRY_HOT_PRIO = 2      # hot-tier stripe + freshly re-armed retries
RETRY_PRIO = 3          # retry page due from backoff
DIFF_PRIO = 4           # rotating content-diff stripe
RECAP_PRIO = 4          # --recapture tier over captured pages

# retry ring
RETRY_BASE_S = 0.05
RETRY_CAP_S = 30.0
LOCALITY_SPILL = 16    # bump +/- this many pages around a fresh decrypt
BUMP_CAP_PER_FLIP = 512

# hot blocks (2 MB) shared with v2's hotspots.csv
BLOCK_SHIFT = 21
BLOCK_SIZE = 1 << BLOCK_SHIFT                  # bytes per hot block (2 MB)
BLOCK_PAGES_SHIFT = BLOCK_SHIFT - PAGE_SHIFT   # 512 pages per block

FP_MASK = 0xFFFFFFFFFFFFFFFF

WQ_MAX = 16384
RQ_MAX = 4096      # drain thresholds only; the result queue is unbounded
RQ_COUNT_CAP = 65536   # overrun brake: entries beyond this are dropped
RQ_BYTES_DEFAULT_MB = 1024  # drop guard budget in queued result bytes

# VirtualQueryEx fallback policy (a transient failure must not permanently
# downgrade the walker to the DP.regions fallback - that froze the faster
# walk mid-run on the first field box)
VQE_RETRY_S = 2.0
VQE_LATCH = 20

# retry-ring re-arm cap per 0.5 s tick for cipher pages in hot blocks
HOT_RING_BUMP = 2048

MEM_COMMIT = 0x1000

DEFAULT_OUT = "E:/ow-dumps/live_page_diff"

ADAPT_TICK_S = 2.0     # adaptive stripe budget tick
STATUS_TICK_S = 5.0
ALIVE_TICK_S = 1.0
AUTOFOCUS_TICK_S = 30.0
POLL_STALE_S = 5.0
RETRY_INFLIGHT_STALE_S = 10.0

BUDGET_MIN = 512
BUDGET_MAX = 32768

# startup surge (v3.2): pin the storm-time knobs for the first N seconds
SURGE_BACKOFF_CAP = 0.5        # ring backoff clamp while surge is on
SURGE_INFLIGHT_STALE_S = 2.0   # lost-result re-arm wait during surge
SURGE_POLL_STALE_S = 1.5       # poll in-flight re-arm wait during surge

# tight tier (v3.4): high-frequency poll of a small page set plus a
# chase burst on any content change
TIGHT_PRIO = -0.5              # above flips: the chase must win the queue
TIGHT_CHUNK = 64               # pages per tight batched read (256 KB)
TIGHT_HZ = 10.0                # default poll cadence
TIGHT_CHASE = 12               # default re-reads after a change
TIGHT_MAX_KEYS = 8192          # sanity cap on the tight set size


def safe_name(name: str) -> str:
    return "".join(c if c.isalnum() or c in "-_." else "_" for c in name)


def backoff_s(tries: int, cap: float) -> float:
    return min(cap, RETRY_BASE_S * (1 << min(tries, 9)))


# --------------------------------------------------------------------------
# module-scoped region enumeration (unchanged from v2)
# --------------------------------------------------------------------------

def _setup_vqe():
    """VirtualQueryEx prototype for module-scoped walks (x64 MBI layout)."""
    try:
        class _MBI(ctypes.Structure):
            _fields_ = [
                ("BaseAddress", ctypes.c_ulonglong),
                ("AllocationBase", ctypes.c_ulonglong),
                ("AllocationProtect", ctypes.c_ulong),
                ("RegionSize", ctypes.c_ulonglong),
                ("State", ctypes.c_ulong),
                ("Protect", ctypes.c_ulong),
                ("Type", ctypes.c_ulong),
            ]

        k32 = ctypes.WinDLL("kernel32", use_last_error=True)
        k32.VirtualQueryEx.argtypes = [
            ctypes.c_void_p, ctypes.c_void_p,
            ctypes.POINTER(_MBI), ctypes.c_size_t,
        ]
        k32.VirtualQueryEx.restype = ctypes.c_size_t
        return k32, _MBI
    except Exception:
        return None, None


K32, MBI = _setup_vqe()


class RegionWalker:
    """Per-module region enumeration.

    VirtualQueryEx over just the module range keeps a pass in the low
    milliseconds.  The DP.regions() fallback (whole address space) is cached
    once per pass so a multi-module sweep does not re-walk per module.  A
    VirtualQueryEx failure falls back for that walk only and is retried
    every VQE_RETRY_S seconds; after VQE_LATCH consecutive failures the
    walker latches to the fallback so the run still produces output.
    """

    def __init__(self, handle, legacy: bool):
        self.handle = handle
        self.uses_vqe = (K32 is not None) and not legacy
        self._cache = None
        self.vqe_fails = 0
        self._vqe_retry_at = 0.0

    def begin_pass(self) -> None:
        self._cache = None

    def _walk_vqe(self, base, size):
        end = min(base + size, 0x7FFFFFFEFFFF)  # never query kernel space
        addr = base
        mbi = MBI()
        pmbi = ctypes.byref(mbi)
        hv = getattr(self.handle, "value", self.handle)
        hv = ctypes.c_void_p(hv)
        while addr < end:
            if K32.VirtualQueryEx(hv, ctypes.c_void_p(addr), pmbi,
                                  ctypes.sizeof(mbi)) != ctypes.sizeof(mbi):
                raise OSError("VirtualQueryEx failed at %#x" % addr)
            rbase = int(mbi.BaseAddress or 0)
            rsize = int(mbi.RegionSize or 0)
            if rsize == 0 or rbase + rsize <= addr:
                break
            if mbi.State == MEM_COMMIT:
                yield rbase, rsize, int(mbi.Protect)
            addr = rbase + rsize

    def _walk_legacy(self, base, size):
        if self._cache is None:
            self._cache = list(DP.regions(self.handle))
        end = base + size
        for rb, rs, prot, _kind, _alloc in self._cache:
            if rb >= end or rb + rs <= base:
                continue
            yield rb, rs, prot

    def module_regions(self, base, size):
        if self.uses_vqe and time.perf_counter() >= self._vqe_retry_at:
            try:
                regions = list(self._walk_vqe(base, size))
            except Exception:
                self.vqe_fails += 1
                self._vqe_retry_at = time.perf_counter() + VQE_RETRY_S
                if self.vqe_fails >= VQE_LATCH:
                    self.uses_vqe = False
                    print(f"VirtualQueryEx failed {self.vqe_fails} walks; "
                          "using DP.regions for the rest of the run")
                elif self.vqe_fails % 5 == 1:
                    print("VirtualQueryEx walk failed; using DP.regions "
                          f"this pass (VQE retried every {VQE_RETRY_S:.0f}s)")
            else:
                self.vqe_fails = 0
                yield from regions
                return
        yield from self._walk_legacy(base, size)


# --------------------------------------------------------------------------
# batch classification + fingerprints (numpy), calibrated against warm_poll
# --------------------------------------------------------------------------

_ENT_SCALE = 1.0     # multiply natural-log entropy by this to match WP
_ENT_NZ_ONLY = False  # True: warm_poll computes entropy over nonzero bytes
_NPY_BATCH = False
_NPY_FP = False


def _batch_stats(buf: bytes, n: int):
    """(nz[n], ent[n]) entropy in nats, for n packed 4096-byte pages."""
    a = np.frombuffer(buf, dtype=np.uint8, count=n * PAGE).reshape(n, PAGE)
    nz = np.count_nonzero(a, axis=1)
    off = (np.arange(n, dtype=np.int64) * 256)[:, None]
    counts = np.bincount((a.astype(np.int64) + off).ravel(),
                          minlength=n * 256).reshape(n, 256)
    if _ENT_NZ_ONLY:
        tot = counts.sum(axis=1).astype(np.float64)
        safe = np.where(tot > 0, tot, 1.0)
        p = counts[:, 1:].astype(np.float64) / safe[:, None]
        lg = np.zeros_like(p)
        m = p > 0
        lg[m] = np.log(p[m])
        ent = np.where(tot > 0, -(p * lg).sum(axis=1), 0.0)
    else:
        p = counts.astype(np.float64) / float(PAGE)
        lg = np.zeros_like(p)
        m = p > 0
        lg[m] = np.log(p[m])
        ent = -(p * lg).sum(axis=1)
    return nz, ent


def _fps_np(buf: bytes, n: int):
    """Cheap change-detector fingerprint per page: xor-fold ^ sum-fold."""
    a = np.frombuffer(buf, dtype=np.uint8, count=n * PAGE).reshape(n, PAGE)
    if a.ctypes.data % 8:
        a = a.copy()  # keep the uint64 view aligned
    v = a.view(np.uint64)
    x = np.bitwise_xor.reduce(v, axis=1)
    s = np.add.reduce(v, axis=1, dtype=np.uint64)
    return x * np.uint64(0x9E3779B97F4A7C15) ^ s


def _self_test_classify() -> bool:
    """Match warm_poll.classify exactly (nz and entropy) before trusting the
    batch path.  Tries both entropy bases and both zero-handling variants;
    any mismatch means the run falls back to per-page classify, so accepted/
    rejected decisions and the warm.csv entropy column keep their meaning."""
    global _ENT_SCALE, _NPY_BATCH, _NPY_FP, _ENT_NZ_ONLY
    if np is None:
        return False
    rng = random.Random(0x5EED)
    pages = [
        bytes(PAGE),                                              # all zero
        bytes(range(256)) * (PAGE // 256),                        # uniform
        bytes(rng.randrange(256) for _ in range(PAGE)),           # random
        b"\x00" * (PAGE // 2)
        + bytes(rng.randrange(256) for _ in range(PAGE // 2)),    # zeros mixed
        (b"A" * (PAGE - 32))
        + bytes(rng.randrange(256) for _ in range(32)),           # skewed
    ]
    pool = b"\x48\x89\x4c\x8b\x55\x8d\x0f\x1f\xe9\x74\x85\xff\xba\x41\x83\x3d"
    pages.append(bytes(rng.choice(pool) for _ in range(PAGE)))    # code-like
    try:
        for p in pages:
            nz_w, _ = WP.classify(p)
            nz_m, _ = _batch_stats(p, 1)
            if int(nz_m[0]) != int(nz_w):
                return False
        _, ent_w = WP.classify(pages[2])
        ent_w = float(ent_w)
        for nz_only in (False, True):
            _ENT_NZ_ONLY = nz_only
            _, ent_m = _batch_stats(pages[2], 1)
            e_n = float(ent_m[0])
            e_b = e_n / 0.6931471805599453            # same entropy in bits
            for scale, target in ((1.0, e_n),
                                  (1.0 / 0.6931471805599453, e_b)):
                if abs(ent_w - target) > 1e-6 * max(1.0, abs(target)):
                    continue
                ok = True
                for p in pages[3:]:
                    nz_w2, ent_w2 = WP.classify(p)
                    nz_m2, ent_m2 = _batch_stats(p, 1)
                    if int(nz_m2[0]) != int(nz_w2) or \
                            abs(float(ent_m2[0]) * scale
                                - float(ent_w2)) > 1e-6 * max(1.0,
                                                              abs(float(ent_w2))):
                        ok = False
                        break
                if ok:
                    _ENT_SCALE = scale
                    _NPY_BATCH = True
                    try:
                        f1 = int(_fps_np(pages[2], 1)[0])
                        f2 = int(_fps_np(pages[0], 1)[0])
                        _NPY_FP = f1 != f2
                    except Exception:
                        _NPY_FP = False
                    return True
        _ENT_NZ_ONLY = False
        return False
    except Exception:
        _ENT_NZ_ONLY = False
        return False


# --------------------------------------------------------------------------
# focus window parsing (unchanged from v2)
# --------------------------------------------------------------------------

def parse_focus(tokens, v2b_base: int):
    """Tokens -> merged, page-aligned (start_v2b, end_v2b) spans.

    Token forms: `0x...` (RVA if below V2B_BASE, else v2b), `rva:0x...`,
    `v2b:0x...`, ranges `A-B` or `A+LEN`.  `#` starts a comment token.
    """
    spans = []

    def val(s: str) -> int:
        s = s.strip()
        rva = s.lower().startswith("rva:")
        v2b = s.lower().startswith("v2b:")
        if rva or v2b:
            s = s.split(":", 1)[1]
        v = int(s, 0)
        if rva or (not v2b and 0 < v < v2b_base):
            v += v2b_base
        return v

    for raw in tokens:
        tok = raw.strip()
        if not tok or tok.startswith("#"):
            continue
        try:
            if "-" in tok:
                a, b = tok.split("-", 1)
                lo, hi = val(a), val(b)
            elif "+" in tok:
                a, ln = tok.split("+", 1)
                lo = val(a)
                hi = lo + int(ln, 0)
            else:
                lo = hi = val(tok)
        except ValueError:
            print(f"focus: cannot parse {tok!r}, skipped")
            continue
        lo &= ~(PAGE - 1)
        hi = (hi + PAGE - 1) & ~(PAGE - 1)
        if hi <= lo:
            hi = lo + PAGE  # single address (or sub-page range): its page
        spans.append((lo, hi))
    if not spans:
        return []
    spans.sort()
    merged = [list(spans[0])]
    for lo, hi in spans[1:]:
        if lo <= merged[-1][1]:
            merged[-1][1] = max(merged[-1][1], hi)
        else:
            merged.append([lo, hi])
    return merged


# --------------------------------------------------------------------------
# numpy run helpers (the core of the v3 bookkeeping)
# --------------------------------------------------------------------------

def _contig_runs(idx):
    """Sorted page indices -> [(lo, hi)] maximal consecutive runs."""
    idx = np.asarray(idx, dtype=np.int64)
    if idx.size == 0:
        return []
    brk = np.flatnonzero(np.diff(idx) > 1)
    starts = np.concatenate(([idx[0]], idx[brk + 1]))
    ends = np.concatenate((idx[brk], [idx[-1]])) + 1
    return list(zip(starts.tolist(), ends.tolist()))


def _change_runs(idx, old_vals, new_vals):
    """Sorted changed page indices + per-page old/new protect codes ->
    [(lo, hi, old, new)] runs, split wherever the pages jump or either
    code changes.  Runs are uniform in (old, new) by construction."""
    idx = np.asarray(idx, dtype=np.int64)
    if idx.size == 0:
        return []
    pair = (np.asarray(old_vals, dtype=np.int64) << 16) | \
        np.asarray(new_vals, dtype=np.int64)
    brk = np.flatnonzero((np.diff(idx) > 1) | (np.diff(pair) != 0))
    starts = np.concatenate(([0], brk + 1))
    ends = np.concatenate((brk, [idx.size - 1])) + 1
    out = []
    for s, e in zip(starts.tolist(), ends.tolist()):
        out.append((int(idx[s]), int(idx[e - 1]) + 1,
                    int(pair[s] >> 16), int(pair[s] & 0xFFFF)))
    return out


def run_kind(old: int, new: int, first_walk: bool):
    """Classify a (old, new) protect-code transition for one page run."""
    rd = (new != 0) and not (new & (DP.PAGE_NOACCESS | DP.PAGE_GUARD))
    was = (old != 0) and not (old & (DP.PAGE_NOACCESS | DP.PAGE_GUARD))
    if rd and not was:
        return "base" if (first_walk and old == 0) else "flip"
    if rd and was and new != old:
        return "reprot"  # still readable, e.g. RX -> RWX round trip
    if (not rd) and was:
        return "gone"    # readable -> unreadable
    return None


# --------------------------------------------------------------------------
# per-module page-indexed state
# --------------------------------------------------------------------------

class ModuleState:
    """All per-page state for one module, indexed by page number.

    The watcher thread owns `prot` and `scratch`; the scheduler thread owns
    everything else.  Single-writer per array, so no locks are needed.
    """

    def __init__(self, mid, name, base, size, is_main):
        self.mid = mid
        self.name = name
        self.base = base
        self.size = size
        self.is_main = is_main
        self.npages = (size + PAGE - 1) >> PAGE_SHIFT
        n = self.npages
        self.prot = np.zeros(n, np.uint16)       # last protect code seen
        self.scratch = np.zeros(n, np.uint16)    # build_prot workspace
        self.readable = np.zeros(n, bool)
        self.tracked = np.zeros(n, bool)         # readable (exec) candidate
        self.captured = np.zeros(n, bool)
        self.fp = np.zeros(n, np.uint64)         # last content fingerprint
        self.fp_valid = np.zeros(n, bool)
        self.block_of_page = np.arange(n, dtype=np.int64) >> BLOCK_PAGES_SHIFT
        self.ever_walked = False
        # stripe cursor state (scheduler thread)
        self.stripe_idx = np.empty(0, np.int64)
        self.diff_pos = 0
        self.hot_pos = 0
        self.recap_pos = 0

    def sess(self, pg: int) -> int:
        return self.base + pg * PAGE

    def v2b(self, pg: int) -> int:
        # main module: analysis-base address; others: module-relative
        return pg * PAGE + (WP.V2B_BASE if self.is_main else 0)


def build_prot(st: ModuleState, regions):
    """Fill st.scratch with per-page protect codes from a region walk.

    regions: iterable of (rbase, rsize, protect) committed regions, clipped
    to the module span.  Uncommitted gaps stay 0, which run_kind treats as
    unreadable.
    """
    buf = st.scratch
    buf.fill(0)
    for rbase, rsize, protect in regions:
        lo = max(rbase, st.base)
        hi = min(rbase + rsize, st.base + st.size)
        if hi <= lo:
            continue
        p0 = (lo - st.base) >> PAGE_SHIFT
        p1 = (((hi - st.base) - 1) >> PAGE_SHIFT) + 1
        buf[p0:p1] = protect
    return buf


def diff_runs(st: ModuleState, buf):
    """Compare a freshly built protect array against st.prot ->
    [(lo, hi, old, new)] change runs at page granularity."""
    idx = np.flatnonzero(buf != st.prot)
    if idx.size == 0:
        return []
    return _change_runs(idx, st.prot[idx], buf[idx])


def apply_prot_runs(st: ModuleState, msgs, include_data: bool):
    """Apply watcher change runs to the page-indexed state (pure update).

    This is the fix for v2's region-identity bleed: tracking is a per-page
    function of the current protect code, so region merges and splits
    cannot lose or resurrect it.
    """
    for lo, hi, _old, new, _kind in msgs:
        rd = (new != 0) and not (new & (DP.PAGE_NOACCESS | DP.PAGE_GUARD))
        want = rd and (include_data or bool(new & WP.EXEC_MASK))
        st.readable[lo:hi] = rd
        st.tracked[lo:hi] = want


def due_runs(pgs):
    """Scattered due page numbers -> [(lo, hi)] contiguous runs."""
    pgs = sorted(set(int(p) for p in pgs))
    if not pgs:
        return []
    return _contig_runs(np.array(pgs, dtype=np.int64))


def next_budget(cur: int, idle_frac: float, wq_depth: int, rq_depth: int,
                floor: int = BUDGET_MIN):
    """Adaptive stripe budget: grow while readers idle, shrink on backlog.

    `floor` is the minimum the budget may sink to; surge passes the
    configured --diff-pages so the rotation cannot go quiet in the storm.
    """
    if idle_frac > 0.35 and rq_depth < 1024:
        return min(BUDGET_MAX, int(cur * 1.5))
    if wq_depth > 6144 or rq_depth > 8192:
        return max(floor, int(cur * 0.7))
    return max(floor, cur)


def gaps_tokens(st: ModuleState):
    """Merged `rva:START-rva:END` focus tokens for tracked,
    not-yet-captured pages (main-module convention: value is relative to
    V2B_BASE once re-parsed by parse_focus)."""
    idx = np.flatnonzero(st.tracked & ~st.captured)
    toks = []
    for lo, hi in _contig_runs(idx):
        toks.append("rva:0x%x-rva:0x%x" % (lo * PAGE, hi * PAGE))
    return toks


def output_name(st: ModuleState, pg: int) -> str:
    if st.is_main:
        return f"v2b_{st.v2b(pg):012x}.bin"
    return f"{safe_name(st.name)}_{pg * PAGE:012x}.bin"


# --------------------------------------------------------------------------
# main
# --------------------------------------------------------------------------

def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--pid", type=lambda s: int(s, 0), default=0)
    ap.add_argument("--wait", action="store_true",
                    help="block until an Overwatch process appears")
    ap.add_argument("--out", default=DEFAULT_OUT)
    ap.add_argument("--seconds", type=float, default=0.0)
    ap.add_argument("--all-modules", action="store_true",
                    help="sweep all overwatch-prefixed modules")
    ap.add_argument("--all-readable", action="store_true",
                    help="implies --include-data")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--priority", default="",
                    help="comma list of v2b addresses polled every pass "
                         "(default: warm_poll.PRIORITY_V2B)")
    ap.add_argument("--focus", default="",
                    help="comma list of RVA spans polled every pass")
    ap.add_argument("--focus-file", default="",
                    help="file with focus tokens, one per line "
                         "(gaps.focus from a previous run drops in here)")
    ap.add_argument("--threads", type=int, default=12,
                    help="reader threads (default 12 for 6C/16T)")
    ap.add_argument("--watch-hz", type=float, default=250.0,
                    help="protection watcher target rate")
    ap.add_argument("--diff-pages", type=int, default=8192,
                    help="stripe pages per pass before adaptation")
    ap.add_argument("--no-diff", action="store_true")
    ap.add_argument("--include-data", action="store_true",
                    help="track readable non-exec pages too (long sweeps)")
    ap.add_argument("--poll-budget", type=int, default=1024,
                    help="poll pages enqueued per pass")
    ap.add_argument("--retry-budget", type=int, default=1024,
                    help="retry pages made due per pass (batched reads)")
    ap.add_argument("--retry-budget-max", type=int, default=8192)
    ap.add_argument("--hot-window", type=float, default=10.0,
                    help="seconds a flipped block stays hot")
    ap.add_argument("--hot-cap", type=int, default=4096,
                    help="max pages in the every-pass hot tier")
    ap.add_argument("--autofocus", type=int, default=8,
                    help="cipher blocks autofocus-bumped per 30 s "
                         "(0 disables; v2 default was 0)")
    ap.add_argument("--recapture", action="store_true",
                    help="write _rN files when a captured page changes")
    ap.add_argument("--recap-pages", type=int, default=1024,
                    help="captured pages re-checked per pass with "
                         "--recapture")
    ap.add_argument("--strict-classify", action="store_true",
                    help="per-page warm_poll.classify everywhere")
    ap.add_argument("--legacy-enum", action="store_true",
                    help="force DP.regions() instead of VirtualQueryEx")
    ap.add_argument("--chunk-pages", type=int, default=CHUNK_PAGES,
                    help="pages per batched ReadProcessMemory call "
                         "(default 256 = 1 MB)")
    ap.add_argument("--surge-seconds", type=float, default=20.0,
                    help="startup storm window with pinned budgets and "
                         "backoff caps (0 disables)")
    ap.add_argument("--surge-backoff", type=float, default=SURGE_BACKOFF_CAP,
                    help="retry-ring backoff cap while surge is on")
    ap.add_argument("--rq-bytes-mb", type=int, default=RQ_BYTES_DEFAULT_MB,
                    help="result-queue drop guard budget in MB")
    ap.add_argument("--tight-rva", default="",
                    help="module-relative RVA spans polled at --tight-hz "
                         "(e.g. 0x1f80000-0x2410000), comma list")
    ap.add_argument("--tight-zeros", default="",
                    help="warm flattened image whose all-zero pages become "
                         "tight pages of --tight-zeros-module")
    ap.add_argument("--tight-zeros-ranges", default="",
                    help="optional comma list of module-relative spans to "
                         "restrict --tight-zeros (e.g. "
                         "0x10000-0x1d70000,0x1f80000-0x2410000)")
    ap.add_argument("--tight-zeros-module", default="Overwatch_loader.dll",
                    help="module the --tight-zeros set belongs to")
    ap.add_argument("--tight-hz", type=float, default=TIGHT_HZ,
                    help="tight tier poll cadence per page (0 off)")
    ap.add_argument("--tight-chase", type=int, default=TIGHT_CHASE,
                    help="re-reads after any tight page content change")
    ap.add_argument("--pass-sleep", type=float, default=0.002)
    args = ap.parse_args()

    out_root = args.out
    wait = args.wait
    all_modules = args.all_modules
    include_data = args.all_readable or args.include_data
    seconds = args.seconds
    threads = max(1, min(32, args.threads))
    diff_pages = 0 if args.no_diff else max(0, args.diff_pages)
    autofocus_k = max(0, args.autofocus)
    strict = args.strict_classify
    recapture = args.recapture
    recap_pages = max(0, args.recap_pages)
    poll_budget = max(1, args.poll_budget)
    hot_window = args.hot_window
    hot_cap = max(0, args.hot_cap)
    watch_hz = max(1.0, args.watch_hz)
    CH_PAGES[0] = max(1, min(1024, args.chunk_pages))
    surge_seconds = max(0.0, args.surge_seconds)
    surge_backoff = max(0.05, args.surge_backoff)
    surge_cap_box = [RETRY_CAP_S]      # live backoff cap (set each pass)
    rq_bytes_cap = [args.rq_bytes_mb * 1048576]
    tight_hz = max(0.0, args.tight_hz)
    tight_chase = max(0, args.tight_chase)

    focus_tokens = []
    if args.focus:
        focus_tokens.extend(args.focus.split(","))
    if args.focus_file:
        try:
            with open(args.focus_file, "r", encoding="utf-8") as fh:
                for line in fh:
                    # A line that starts with # is a comment - the whole
                    # line, not just the first token: the file's own header
                    # would otherwise be parsed as addresses.
                    stripped = line.strip()
                    if not stripped or stripped.startswith("#"):
                        continue
                    focus_tokens.extend(line.replace(",", " ").split())
        except OSError as exc:
            print(f"focus-file unreadable: {exc}")
    focus_spans = parse_focus(focus_tokens, getattr(WP, "V2B_BASE", 0))

    forced_pid = args.pid
    pid = forced_pid or WP.find_pid()
    while not pid:
        if not wait:
            print("no Overwatch process found (use --wait to block until "
                  "it starts)")
            return 1
        time.sleep(0.05)
        pid = WP.find_pid()
    print(f"pid {pid}: attaching")

    handle = WP.open_retry(pid)
    if not handle:
        print(f"OpenProcess failed, error {ctypes.get_last_error()}")
        return 1

    # The launcher is a separate process ("Overwatch Launcher.exe") and has
    # no game module.  Wait for the real module, and if the game starts in a
    # new process, re-attach.  `warm_poll` does the same module wait.
    modules = []
    deadline = time.monotonic() + 180.0
    last_note = 0.0
    while time.monotonic() < deadline:
        all_mods = [m for m in WP.list_modules(pid) if m[2] >= MAIN_MIN_SIZE]
        game = next((m for m in all_mods
                     if m[0].lower() == "overwatch.exe"), None)
        if game:
            modules = all_mods
            break
        if not forced_pid:
            newer = WP.find_pid()
            if newer and newer != pid:
                print(f"game process changed {pid} -> {newer}; re-attaching")
                pid = newer
                handle = WP.open_retry(pid)
                if not handle:
                    print("OpenProcess failed after re-attach")
                    return 1
        now_ = time.monotonic()
        if now_ - last_note > 5.0:
            largest = max(all_mods, key=lambda m: m[2]) if all_mods else None
            if largest:
                print(f"waiting for Overwatch.exe in pid {pid}; largest now "
                      f"{largest[0]} {largest[2]/1048576:.0f} MB")
            else:
                print(f"waiting for Overwatch.exe in pid {pid} ...")
            last_note = now_
        time.sleep(0.1)
    if not modules:
        print(f"Overwatch.exe never appeared in pid {pid} after 180s")
        return 1
    modules.sort(key=lambda m: -m[2])
    main_mod = next(m for m in modules if m[0].lower() == "overwatch.exe")
    # Sweep the game and its loader; GPU driver DLLs are not game code.
    chosen = (
        [m for m in modules if m[0].lower().startswith("overwatch")]
        if all_modules
        else [main_mod]
    )
    print(f"main module {main_mod[0]} base {main_mod[1]:#x} "
          f"size {main_mod[2]:#x}")
    shown = 0
    for name, base, size in modules:
        if name.lower() == "overwatch.exe":
            continue
        print(f"  also large: {name} {base:#x} {size:#x}")
        shown += 1
        if shown >= 7:
            break
    if args.dry_run:
        print("dry run: no pages read")
        return 0
    delta = main_mod[1] - WP.V2B_BASE

    stamp = time.strftime("%Y%m%d_%H%M%S")
    out = os.path.join(out_root, f"run_{stamp}")
    pages_dir = os.path.join(out, "pages")
    os.makedirs(pages_dir, exist_ok=True)
    print(f"output: {out}")

    npy_ready = _self_test_classify()
    if npy_ready:
        print(f"classify: numpy batch, self-test passed "
              f"(entropy scale x{_ENT_SCALE:.6f})")
    else:
        print("classify: warm_poll.classify per page (numpy self-test "
              "failed; results identical, just slower)")
    if not _NPY_FP:
        print("fingerprints: python hash() fallback (numpy path failed)")

    # page-indexed state, one per swept module
    mods = []
    for m in chosen:
        mods.append(ModuleState(len(mods), m[0], m[1], m[2],
                                m is main_mod))
    main_st = mods[0] if mods[0].is_main else \
        next(st for st in mods if st.is_main)

    # ------------------------------------------------------------------
    # tight tier (v3.4): a small page set polled at high cadence, with a
    # chase burst on any content change.  Built from (a) --tight-zeros:
    # pages all-zero in a warm flattened image (the loader's cold band
    # and .eid in every capture so far), and (b) --tight-rva spans.
    # Both sets are module-relative of --tight-zeros-module.
    # ------------------------------------------------------------------
    tight_keys: set = set()
    tight_st = next((st for st in mods
                     if st.name.lower() == args.tight_zeros_module.lower()),
                    None)
    if args.tight_rva or args.tight_zeros:
        if tight_st is None:
            print(f"tight: module {args.tight_zeros_module} is not among "
                  f"the swept modules - sweep it (--all-modules) or fix "
                  f"--tight-zeros-module; tier disabled")
        else:
            if args.tight_rva:
                spans = parse_focus([t for t in args.tight_rva.split(",")
                                     if t.strip()], 0)
                for lo, hi in spans:
                    pg0 = lo >> PAGE_SHIFT
                    pg1 = hi >> PAGE_SHIFT
                    for pg in range(max(0, pg0), min(pg1, tight_st.npages)):
                        tight_keys.add((tight_st.mid, pg))
            if args.tight_zeros:
                try:
                    with open(args.tight_zeros, "rb") as fh:
                        img = fh.read()
                    npages = min(tight_st.size, len(img)) >> PAGE_SHIFT
                    a = np.frombuffer(img[:npages * PAGE], np.uint8)
                    a = a.reshape(npages, PAGE)
                    zeros = np.flatnonzero((a == 0).all(axis=1)).tolist()
                    if args.tight_zeros_ranges:
                        zr = parse_focus([t for t in
                                          args.tight_zeros_ranges.split(",")
                                          if t.strip()], 0)
                        keep = np.zeros(npages, bool)
                        for lo, hi in zr:
                            lo_pg = lo >> PAGE_SHIFT
                            hi_pg = hi >> PAGE_SHIFT
                            keep[max(0, lo_pg):max(0, hi_pg)] = True
                        zeros = [pg for pg in zeros if keep[pg]]
                    for pg in zeros:
                        tight_keys.add((tight_st.mid, int(pg)))
                except OSError as exc:
                    print(f"tight: cannot read --tight-zeros image: {exc}")
            if len(tight_keys) > TIGHT_MAX_KEYS:
                print(f"tight: {len(tight_keys)} pages requested, "
                      f"capping at {TIGHT_MAX_KEYS} (smallest RVAs kept); "
                      f"split into --tight-rva groups if that loses what "
                      f"you wanted")
                tight_keys = set(sorted(tight_keys)[:TIGHT_MAX_KEYS])
            if not tight_keys:
                print("tight: tier built ZERO pages from the given flags - "
                      "check the spans/paths above; tier stays off")
            else:
                print(f"tight tier: {len(tight_keys)} pages of "
                      f"{args.tight_zeros_module}, cadence {tight_hz:g}Hz/scan, "
                      f"chase x{tight_chase} on any change")


    # Focus windows: pages polled every pass; never enter the retry backoff.
    focus_v2b = set()
    truncated = False
    for lo, hi in focus_spans:
        for v in range(lo, hi, PAGE):
            focus_v2b.add(v)
            if len(focus_v2b) > 65536:
                truncated = True
                break
        if truncated:
            break
    if truncated:
        print("focus: more than 65536 pages requested; truncating")
    prio_list = [int(x, 0) for x in args.priority.split(",") if x.strip()] \
        or list(WP.PRIORITY_V2B)
    poll_v2b = sorted(set(prio_list) | focus_v2b)
    poll_list = []          # (ModuleState, pg) read through the pool
    dropped_poll = 0
    for v2b in poll_v2b:
        pg = (v2b - WP.V2B_BASE) >> PAGE_SHIFT
        if 0 <= pg < main_st.npages:
            poll_list.append((main_st, pg))
        else:
            dropped_poll += 1
    if dropped_poll:
        print(f"  {dropped_poll} poll/focus pages fell outside the main "
              f"module and were dropped")
    print(f"every-pass poll pages: {len(poll_list)} "
          f"({len(prio_list)} priority + {len(focus_v2b)} focus)")
    prio_v2b = set(poll_v2b)

    # ------------------------------------------------------------------
    # shared machinery
    # ------------------------------------------------------------------
    t0 = time.time()
    stop = threading.Event()
    phase_box = [0]
    seen: dict = {}            # (mid, pg) -> capture record
    rejected: dict = {}        # (mid, pg) -> ciphertext record
    rescued: dict = {}         # cipher-record saved when the page decrypted
    retry: dict = {}           # (mid, pg) -> [tries, due, block, inflight, churn]
    dueheap: list = []         # (due, seq, key), lazy invalidation
    hot_blocks: dict = {}      # (mid, block) -> [flips, last_flip_s, captures]
    rejected_by_block: dict = {}   # (mid, block) -> set(keys)
    auto_blocks: set = set()
    flip_times = deque(maxlen=65536)
    counters = {"base": 0, "flip": 0, "reprot": 0, "poll": 0, "retry": 0,
                "diff": 0, "hot": 0, "recap": 0}
    writer_stats = {"events": 0}
    dropped_results = [0]
    wq_dropped = [0]
    rq_bytes = [0]             # queued result bytes (drop guard budget)
    rq_lock = threading.Lock()
    stat_bytes = 0
    stat_bytes_lock = threading.Lock()
    rd_stats = [0, 0]          # [idle_polls, busy_polls] for adapt tick
    rd_lock = threading.Lock()
    watch_ticks = [0]          # watcher cycles completed
    walk_stat = [0.0, "-"]     # last region walk: duration ms, source
    passes = 0
    seqc = itertools.count()

    wq = queue.PriorityQueue(maxsize=WQ_MAX)   # (prio, seq, addr, size, tag)
    rq = queue.Queue()                         # unbounded; readers never stall
    fileq = queue.Queue()                      # page files + event rows
    walkq = deque()                           # watcher -> scheduler msgs
    walkq_lock = threading.Lock()
    overflow = deque()                         # wq spill, flushed by both
    overflow_lock = threading.Lock()
    poll_inflight: dict = {}                   # (mid, pg) -> enqueue time

    stripe_dirty = [True]
    diff_budget = [diff_pages]
    rpm_holder = [0.0]
    poll_pos = [0]
    tight_last = {}          # (mid, pg) -> last pass this page was enqueued
    tight_next = [0.0]       # min wall time for the next full tight scan
    tight_reads = [0]        # count of all tight/chase reads
    tight_changes = [0]      # count of tight pages that changed content
    tight_plain = [0]        # count of tight reads that hit plaintext

    def now() -> float:
        return time.time() - t0

    def wq_put(prio_v: int, addr: int, size: int, tag: tuple) -> None:
        item = (prio_v, next(seqc), addr, size, tag)
        try:
            wq.put_nowait(item)
        except queue.Full:
            if prio_v <= POLL_PRIO:
                # urgent reads (flip/base/poll) must never be lost
                with overflow_lock:
                    overflow.append(item)
            else:
                # regenerable tiers (stripe/retry/hot/recap): drop and let
                # the next pass re-plan instead of burying urgent reads
                wq_dropped[0] += 1

    def flush_overflow() -> None:
        if not overflow:
            return
        with overflow_lock:
            while overflow:
                try:
                    wq.put_nowait(overflow[0])
                except queue.Full:
                    break
                overflow.popleft()

    def rq_put(addr: int, data, tag: tuple) -> None:
        # Unbounded queue: a reader sleeping here is lost read throughput,
        # and during a startup burst the old retry loop cost seconds per
        # result.  v3.3: the guard is bytes-based, so a flood of cheap
        # 4 KB page-fallback entries (425k dropped in the first surge run
        # under the count guard) drains in order instead of vanishing,
        # while queued RAM stays bounded.  A dropped read recovers through
        # stripe rotation, the retry in-flight sweeper, or the next flip.
        data = data if data else b""
        size = len(data)
        with rq_lock:
            over = (rq_bytes[0] >= rq_bytes_cap[0]
                    or rq.qsize() >= RQ_COUNT_CAP)
            if over and rq.qsize() > 0:
                dropped_results[0] += 1
                return
            rq_bytes[0] += size
            rq.put_nowait((addr, data, tag))

    def event(kind: str, st: ModuleState, addr: int, size: int,
              old: int, new: int, note: str = "") -> None:
        fileq.put(("event",
                   f"{now():.3f},{phase_box[0]},{kind},{st.name},"
                   f"{addr:012x},{size},{old:08x},{new:08x},{note}\n"))

    # ------------------------------------------------------------------
    # retry ring bookkeeping (scheduler thread only)
    # ------------------------------------------------------------------

    def retry_add(key, bump: bool = False) -> None:
        mid, pg = key
        st = mods[mid]
        if key in seen:
            return
        if st.is_main and (pg * PAGE + WP.V2B_BASE) in prio_v2b:
            return  # polled every pass already
        info = retry.get(key)
        if info is None:
            info = [0, 0.0, pg >> BLOCK_PAGES_SHIFT, None, 0]
            retry[key] = info
            rejected_by_block.setdefault((mid, info[2]), set()).add(key)
        elif bump:
            info[0] += 1
        info[1] = now() + backoff_s(info[0], surge_cap_box[0])
        heapq.heappush(dueheap, (info[1], next(seqc), key))

    def retry_drop(key) -> None:
        info = retry.pop(key, None)
        if info is not None:
            s = rejected_by_block.get((key[0], info[2]))
            if s is not None:
                s.discard(key)
                if not s:
                    rejected_by_block.pop((key[0], info[2]), None)

    def bump(key) -> None:
        info = retry.get(key)
        if info is None or key in seen:
            return
        t = now()
        if info[3] is not None and t - info[3] < 5.0:
            return  # a read is already in flight for this page
        info[1] = t
        heapq.heappush(dueheap, (t, next(seqc), key))

    def bump_hot_ring(cap: int) -> int:
        """Re-arm retry-ring cipher pages in recently-flipped blocks.

        Ring pages live in regions that are not tracked right now
        (typically parked readable but non-exec while the packer still
        owns them), so neither the stripe nor the hot tier can see them
        and their only coverage is the exponential backoff.  A block with
        flips inside --hot-window is being actively worked on, so its
        cipher pages deserve pass-rate cadence: an in-place decrypt there
        must not wait out a 25 s backoff."""
        if not hot_ids_cache:
            return 0
        n = 0
        for mb in hot_ids_cache:
            for key in list(rejected_by_block.get(mb, ())):
                if n >= cap:
                    return n
                bump(key)
                n += 1
        return n

    def pop_due(budget_pages: int, surge_now: bool = False) -> int:
        t = now()
        # v3.3: during a storm the stripe used to keep the queue topped up
        # to WQ_MAX-1024 forever, which gated the ring out of the run; the
        # reserve is shallower under surge so ring reads get air (the ring
        # items are regenerable either way - they drop and re-plan).
        if wq.qsize() > WQ_MAX - (256 if surge_now else 512):
            return 0    # readers saturated; due keys stay armed in the heap
        due_keys = []
        while len(due_keys) < budget_pages and dueheap:
            due, _sq, key = heapq.heappop(dueheap)
            info = retry.get(key)
            if info is None or key in seen or info[1] != due:
                continue  # stale heap entry
            if info[3] is not None:
                if t - info[3] < 5.0:
                    continue  # still in flight
                info[3] = None  # its result was lost; schedule again
            if due > t:
                heapq.heappush(dueheap, (due, _sq, key))
                break
            info[3] = t
            due_keys.append(key)
        if not due_keys:
            return 0
        by_mod: dict = {}
        for key in due_keys:
            by_mod.setdefault(key[0], []).append(key[1])
        scheduled = 0
        for mid, pgs in by_mod.items():
            st = mods[mid]
            for lo, hi in due_runs(pgs):
                off = lo
                while off < hi:
                    take = min(CH_PAGES[0], hi - off)
                    run_keys = [(mid, p) for p in range(off, off + take)]
                    fresh = any(retry.get(k) and retry[k][0] == 0
                                for k in run_keys[:4])
                    prot = int(st.prot[off]) if off < st.npages else 0
                    wq_put(RETRY_HOT_PRIO if fresh else RETRY_PRIO,
                           st.sess(off), take * PAGE,
                           ("retryrun", st, prot, phase_box[0],
                            (lo, run_keys)))
                    scheduled += take
                    off += take
        return scheduled

    # ------------------------------------------------------------------
    # classification / acceptance (scheduler thread only)
    # ------------------------------------------------------------------

    def fps_of(data: bytes, n: int):
        if _NPY_FP:
            return _fps_np(data, n)
        return [hash(data[o:o + PAGE]) & FP_MASK
                for o in range(0, n * PAGE, PAGE)]

    def note_cipher(st, pg, key, nz, ent, protect, kind, fp_i) -> None:
        if key in seen:
            return
        churned = False
        if st.fp_valid[pg] and int(st.fp[pg]) != fp_i:
            churned = True  # ciphertext is being rewritten: re-arm it
        rec = rejected.get(key)
        if rec is None:
            rejected[key] = {"nz": nz, "ent": ent, "t": now(),
                             "ph": phase_box[0], "last": now(), "churn": 0}
            event("cipher", st, st.sess(pg), PAGE, protect, protect, kind)
        else:
            rec["last"] = now()
        retry_add(key, bump=(kind == "retryrun"))
        if churned:
            info = retry.get(key)
            if info is not None:
                info[0] = 0
                info[1] = now() + RETRY_BASE_S
                info[4] += 1
                rec = rejected.get(key)
                if rec is not None:
                    rec["churn"] += 1
                heapq.heappush(dueheap, (info[1], next(seqc), key))
                event("churn", st, st.sess(pg), PAGE, protect, protect, "")

    def accept_page(st, pg, key, page: bytes, nz, ent, protect: int,
                    kind: str, fp_i) -> None:
        rec = rejected.pop(key, None)
        if rec is not None:
            rescued[key] = rec  # was cipher at least once before decrypting
        info = {"nz": nz, "ent": ent, "t": now(), "ph": phase_box[0],
                "protect": protect, "src": kind, "recaps": 0}
        seen[key] = info
        st.captured[pg] = True
        stripe_dirty[0] = True
        fileq.put(("page", os.path.join(pages_dir, output_name(st, pg)),
                   page))
        ck = "retry" if kind == "retryrun" else kind
        counters[ck] = counters.get(ck, 0) + 1
        hb = hot_blocks.setdefault((st.mid, pg >> BLOCK_PAGES_SHIFT),
                                   [0, 0.0, 0])
        hb[2] += 1
        print(f"NEW {st.name} {st.v2b(pg):012x} sess {st.sess(pg):012x} "
              f"{nz}/4096 e{ent:.2f} prot {protect:#x} t+{now():6.2f}s "
              f"ph{phase_box[0]} {kind}")
        event("capture", st, st.sess(pg), PAGE, protect, protect, kind)
        retry_drop(key)
        # locality: pages around a fresh decrypt tend to decrypt next
        for off in range(-LOCALITY_SPILL, LOCALITY_SPILL + 1):
            if off and 0 <= pg + off < st.npages:
                bump((st.mid, pg + off))

    def handle_tight_result(addr: int, data, tag: tuple, kind: str) -> None:
        """v3.4 tier result: fingerprint-compare the chunk, classify every
        changed page through the shared accept/cipher pipeline, log the
        transition rows, and chase (--tight-chase immediate re-reads of
        changed pages) while attempts remain.  Unchanged chase reads stay
        silent; the transition timestamps alone give the plaintext window
        (accept row = plain-at, cipher row = cipher-at)."""
        _kind, st, protect, _ph, payload = tag
        if not data:
            return
        n = len(data) // PAGE
        if n <= 0:
            return
        pg0 = (addr - st.base) >> PAGE_SHIFT
        pg1 = min(pg0 + n, st.npages)
        if pg1 <= pg0:
            return
        n = pg1 - pg0
        is_chase = kind == "tightchase"
        attempts = int(payload) if (is_chase and isinstance(payload, int)) \
            else 0
        fps = fps_of(data, n)
        changed = []
        for i in range(n):
            pg = pg0 + i
            fp_i = int(fps[i])
            if st.fp_valid[pg] and int(st.fp[pg]) == fp_i:
                continue
            key = (st.mid, pg)
            if strict or not _NPY_BATCH:
                nz, ent = WP.classify(data[i * PAGE:(i + 1) * PAGE])
            else:
                nz_a, ent_a = _batch_stats(data[i * PAGE:(i + 1) * PAGE], 1)
                nz, ent = int(nz_a[0]), float(ent_a[0]) * _ENT_SCALE
            st.fp[pg] = np.uint64(fp_i)
            st.fp_valid[pg] = True
            tight_changes[0] += 1
            if nz >= WP.WARM_MIN and ent <= WP.ENT_MAX:
                accept_page(st, pg, key, data[i * PAGE:(i + 1) * PAGE],
                            nz, ent, protect, kind, fp_i)
                tight_plain[0] += 1
            elif nz >= WP.WARM_MIN:
                note_cipher(st, pg, key, nz, ent, protect, kind, fp_i)
            changed.append((pg, nz, ent))
        tight_reads[0] += n
        if changed:
            if is_chase:
                for pg, nz, ent in changed:
                    event("tightc", st, st.sess(pg), PAGE,
                          protect, protect,
                          f"att{attempts + 1} nz{nz} ent{ent:.2f}")
            else:
                for pg, nz, ent in changed:
                    event("tight", st, st.sess(pg), PAGE,
                          protect, protect, f"nz{nz} ent{ent:.2f}")
        if is_chase:
            # a chase always consumes its budget: transitions recorded in
            # events, unchanged reads silent
            if attempts > 0:
                wq_put(TIGHT_PRIO, addr, n * PAGE,
                       ("tightchase", st, protect, phase_box[0],
                        attempts - 1))
            return
        if changed and tight_chase > 0:
            for pg, _nz, _ent in changed:
                wq_put(TIGHT_PRIO, st.sess(pg), PAGE,
                       ("tightchase", st, protect, phase_box[0],
                        tight_chase - 1))

    def handle_result(addr: int, data, tag: tuple) -> None:
        kind, st, protect, _ph, payload = tag
        if kind in ("tight", "tightchase"):
            handle_tight_result(addr, data, tag, kind)
            return
        keys = None
        if kind == "retryrun":
            run_pg0, run_keys = payload
            i0 = ((addr - st.base) >> PAGE_SHIFT) - run_pg0
            if not (0 <= i0 < len(run_keys)):
                return
            n0 = (len(data) // PAGE) if data else 0
            my_keys = run_keys[i0:i0 + max(n0, 1)]
            for key in my_keys:
                info = retry.get(key)
                if info is not None:
                    info[3] = None
            if not data or n0 == 0:
                # unreadable again (flipped back or gone): the watcher owns
                # re-entry, so stop polling it here.
                for key in my_keys:
                    retry_drop(key)
                return
            keys = my_keys
        if kind == "poll":
            # single-page reads: clear the in-flight slot even on failure
            # (a page that went NOACCESS must not starve the rotation)
            pg0 = (addr - st.base) >> PAGE_SHIFT
            if 0 <= pg0 < st.npages:
                poll_inflight.pop((st.mid, pg0), None)
        if not data:
            return
        n = len(data) // PAGE
        if n <= 0:
            return
        fps = fps_of(data, n)
        if keys is None:
            # v3.1 fast path for stripe/flip/base/hot/poll reads: one
            # numpy compare fingerprints the whole run and only changed
            # pages get classified.  (retryrun keeps the per-page path
            # below: it interleaves ring bookkeeping per key.)
            pg0 = (addr - st.base) >> PAGE_SHIFT
            if 0 <= pg0 and pg0 + n <= st.npages:
                pgs = np.arange(pg0, pg0 + n, dtype=np.int64)
                fps_a = np.asarray(fps, dtype=np.uint64)
                same_a = st.fp_valid[pgs] & (st.fp[pgs] == fps_a)
                todo = np.flatnonzero(~same_a).tolist()
                cls = {}
                if todo:
                    if strict or not _NPY_BATCH:
                        for i in todo:
                            cls[i] = WP.classify(
                                data[i * PAGE:(i + 1) * PAGE])
                    else:
                        buf = b"".join(data[i * PAGE:(i + 1) * PAGE]
                                      for i in todo)
                        nz_a, ent_a = _batch_stats(buf, len(todo))
                        for j, i in enumerate(todo):
                            cls[i] = (int(nz_a[j]),
                                      float(ent_a[j]) * _ENT_SCALE)
                for i in todo:
                    pg = pg0 + i
                    key = (st.mid, pg)
                    nz, ent = cls[i]
                    if nz < WP.WARM_MIN:
                        continue
                    if ent > WP.ENT_MAX:
                        note_cipher(st, pg, key, nz, ent, protect, kind,
                                    int(fps[i]))
                        continue
                    if key in seen:
                        cnt = seen[key].get("recaps", 0) + 1
                        if recapture:
                            seen[key]["recaps"] = cnt
                            path = os.path.join(
                                pages_dir,
                                output_name(st, pg)[:-4] + f"_r{cnt}.bin")
                            fileq.put(("page", path,
                                       data[i * PAGE:(i + 1) * PAGE]))
                            event("recapture", st, st.sess(pg), PAGE,
                                  protect, protect, f"r{cnt}")
                            counters["recap"] = counters.get("recap", 0) + 1
                        else:
                            event("changed", st, st.sess(pg), PAGE,
                                  protect, protect, "")
                        continue
                    accept_page(st, pg, key,
                                data[i * PAGE:(i + 1) * PAGE],
                                nz, ent, protect, kind, int(fps[i]))
                st.fp[pgs] = fps_a
                st.fp_valid[pgs] = True
                return
        pg_of_i = []
        same_of_i = []
        todo = []
        for i in range(n):
            pg = (addr + i * PAGE - st.base) >> PAGE_SHIFT
            if not (0 <= pg < st.npages):
                pg_of_i.append(-1)
                same_of_i.append(False)
                continue
            pg_of_i.append(pg)
            fp_i = int(fps[i])
            same = bool(st.fp_valid[pg]) and int(st.fp[pg]) == fp_i
            same_of_i.append(same)
            if not same:
                todo.append(i)
        cls = {}
        if todo:
            if strict or not _NPY_BATCH:
                for i in todo:
                    cls[i] = WP.classify(data[i * PAGE:(i + 1) * PAGE])
            else:
                buf = b"".join(data[i * PAGE:(i + 1) * PAGE] for i in todo)
                nz_a, ent_a = _batch_stats(buf, len(todo))
                for j, i in enumerate(todo):
                    cls[i] = (int(nz_a[j]), float(ent_a[j]) * _ENT_SCALE)
        for i in range(n):
            pg = pg_of_i[i]
            if pg < 0:
                continue
            key = (st.mid, pg)
            rkey = keys[i] if (keys is not None and i < len(keys)) else None
            if same_of_i[i]:
                # unchanged content: the fast path that keeps sweeps cheap
                if rkey is not None:
                    info = retry.get(rkey)
                    if info is not None and rkey not in seen:
                        info[0] += 1
                        info[1] = now() + backoff_s(info[0],
                                                    surge_cap_box[0])
                        heapq.heappush(dueheap, (info[1], next(seqc), rkey))
                continue
            nz, ent = cls[i]
            if nz < WP.WARM_MIN:
                if rkey is not None:
                    retry_drop(rkey)  # no longer ciphertext; stop polling
                continue
            if ent > WP.ENT_MAX:
                note_cipher(st, pg, key, nz, ent, protect, kind, int(fps[i]))
                continue
            if key in seen:
                # plaintext changed on an already-captured page
                cnt = seen[key].get("recaps", 0) + 1
                if recapture:
                    seen[key]["recaps"] = cnt
                    path = os.path.join(
                        pages_dir, output_name(st, pg)[:-4] + f"_r{cnt}.bin")
                    fileq.put(("page", path, data[i * PAGE:(i + 1) * PAGE]))
                    event("recapture", st, st.sess(pg), PAGE, protect,
                          protect, f"r{cnt}")
                    counters["recap"] = counters.get("recap", 0) + 1
                else:
                    event("changed", st, st.sess(pg), PAGE, protect,
                          protect, "")
                continue
            accept_page(st, pg, key, data[i * PAGE:(i + 1) * PAGE],
                        nz, ent, protect, kind, int(fps[i]))
        # fingerprint store: every page we just read, changed or not
        for i in range(n):
            pg = pg_of_i[i]
            if pg >= 0:
                st.fp[pg] = np.uint64(int(fps[i])) if _NPY_FP else \
                    np.uint64(int(fps[i]) & FP_MASK)
                st.fp_valid[pg] = True


    # ------------------------------------------------------------------
    # threads
    # ------------------------------------------------------------------

    def watcher_loop() -> None:
        """Dedicated protection watcher.

        Walks the chosen modules' protection map at --watch-hz and turns
        each per-page protection change into an immediate FLIP_PRIO read
        plus a bookkeeping message for the scheduler.  Page-granular
        bitmap diffing means a region merge or split cannot hide a change
        or lose tracking the way v2's region-identity keys did.
        """
        period = 1.0 / watch_hz
        next_t = time.perf_counter()
        while not stop.is_set():
            try:
                walker.begin_pass()
                for st in mods:
                    t_w0 = time.perf_counter()
                    try:
                        regions = list(walker.module_regions(st.base,
                                                             st.size))
                    except Exception as exc:
                        print(f"watcher walk error: {exc!r}")
                        continue
                    walk_stat[0] = (time.perf_counter() - t_w0) * 1000.0
                    walk_stat[1] = "vqe" if walker.uses_vqe else "leg"
                    if not regions:
                        if st.prot.any():
                            # suspicious empty walk (handle hiccup): keep
                            # the last known state rather than wiping it
                            continue
                        st.ever_walked = True
                        continue
                    buf = build_prot(st, regions)
                    runs = diff_runs(st, buf)
                    if runs:
                        msgs = []
                        for lo, hi, old, new in runs:
                            kind = run_kind(old, new, not st.ever_walked)
                            if kind in ("base", "flip", "reprot"):
                                off = lo
                                while off < hi and not stop.is_set():
                                    take = min(CH_PAGES[0], hi - off)
                                    wq_put(BASE_PRIO if kind == "base"
                                           else FLIP_PRIO,
                                           st.sess(off), take * PAGE,
                                           (kind, st, int(buf[off]),
                                            phase_box[0], None))
                                    off += take
                            if kind is not None:
                                msgs.append((lo, hi, old, new, kind))
                        if msgs:
                            with walkq_lock:
                                walkq.append((st, msgs))
                    st.prot, st.scratch = buf, st.prot
                    st.ever_walked = True
            except Exception as exc:
                print(f"watcher cycle error: {exc!r}")
            watch_ticks[0] += 1
            next_t += period
            sleep = next_t - time.perf_counter()
            if sleep > 0:
                time.sleep(sleep)
            else:
                next_t = time.perf_counter()  # running flat; drop the debt

    def reader_loop() -> None:
        nonlocal stat_bytes  # stat_bytes lives in main(); without this the
        # augmented assignment below rebinds it as an uninitialized local
        while True:
            try:
                item = wq.get(timeout=0.25)
            except queue.Empty:
                if stop.is_set():
                    break
                with rd_lock:
                    rd_stats[0] += 1
                continue
            with rd_lock:
                rd_stats[1] += 1
            _prio, _seq, addr, size, tag = item
            if tag is None:
                break
            if size <= PAGE:
                data = DP.read_region(handle, addr, size)
                if data:
                    with stat_bytes_lock:
                        stat_bytes += len(data)
                rq_put(addr, data, tag)
            else:
                data = DP.read_region(handle, addr, size)
                if data is None:
                    # a chunk can contain a page that flipped back already:
                    # fall back to page-wise and keep whatever answers
                    for off in range(0, size, PAGE):
                        part = DP.read_region(handle, addr + off, PAGE)
                        if part:
                            with stat_bytes_lock:
                                stat_bytes += len(part)
                            rq_put(addr + off, part, tag)
                else:
                    with stat_bytes_lock:
                        stat_bytes += len(data)
                    rq_put(addr, data, tag)

    def writer_loop() -> None:
        ev_path = os.path.join(out, "events.csv")
        ev = open(ev_path, "w", encoding="utf-8", newline="")
        ev.write("t,phase,kind,module,addr,size,old_prot,new_prot,note\n")
        count = 0
        try:
            while True:
                item = fileq.get()
                if item is None:
                    break
                try:
                    if item[0] == "page":
                        _k, path, data = item
                        with open(path, "wb") as fh:
                            fh.write(data)
                    else:
                        _k, line = item
                        ev.write(line)
                        count += 1
                        if count % 512 == 0:
                            ev.flush()
                except OSError as exc:
                    print(f"write error: {exc}")
        finally:
            ev.close()
            writer_stats["events"] = count

    def input_thread() -> None:
        for line in sys.stdin:
            line = line.strip()
            if line.startswith("\\"):
                print(f"stop requested at t+{now():.2f}s")
                stop.set()
                break
            phase_box[0] += 1
            print(f"phase {phase_box[0]} marked at t+{now():.2f}s "
                  f"(poll pages re-read next pass)")
            if stop.is_set():
                break

    # ------------------------------------------------------------------
    # scheduler helpers
    # ------------------------------------------------------------------

    def apply_walk_msgs(st, msgs) -> None:
        for lo, hi, old, new, kind in msgs:
            if kind == "flip":
                t = now()
                flip_times.append(t)
                event("flip", st, st.sess(lo), (hi - lo) * PAGE, old, new, "")
                bumped = 0
                for b in range(lo >> BLOCK_PAGES_SHIFT,
                               (((hi - 1) >> BLOCK_PAGES_SHIFT)) + 1):
                    hb = hot_blocks.setdefault((st.mid, b), [0, 0.0, 0])
                    hb[0] += 1
                    hb[1] = t
                    s = rejected_by_block.get((st.mid, b))
                    if s:
                        for k2 in list(s):
                            if bumped >= BUMP_CAP_PER_FLIP:
                                break
                            bump(k2)
                            bumped += 1
            elif kind == "gone":
                event("gone", st, st.sess(lo), (hi - lo) * PAGE, old, new, "")
        apply_prot_runs(st, msgs, include_data)
        stripe_dirty[0] = True

    def enqueue_pages(st, pages_idx, prio_v: int, kind: str) -> None:
        pages_idx = np.asarray(pages_idx, dtype=np.int64)
        if pages_idx.size == 0:
            return
        for lo, hi in _contig_runs(pages_idx):
            off = lo
            while off < hi:
                take = min(CH_PAGES[0], hi - off)
                prot = int(st.prot[off]) if off < st.npages else 0
                wq_put(prio_v, st.sess(off), take * PAGE,
                       (kind, st, prot, phase_box[0], None))
                off += take

    def rebuild_stripe() -> None:
        if stripe_dirty[0]:
            for st in mods:
                st.stripe_idx = np.flatnonzero(st.tracked & ~st.captured)
            stripe_dirty[0] = False

    def schedule_poll() -> None:
        if not poll_list:
            return
        rot = poll_pos[0] % len(poll_list)
        order = poll_list[rot:] + poll_list[:rot]
        n = 0
        for st, pg in order:
            if n >= poll_budget:
                break
            key = (st.mid, pg)
            if key in poll_inflight:
                continue
            poll_inflight[key] = now()
            prot = int(st.prot[pg]) if pg < st.npages else 0
            wq_put(POLL_PRIO, st.sess(pg), PAGE,
                   ("poll", st, prot, phase_box[0], None))
            n += 1
        poll_pos[0] = (rot + n) % len(poll_list)

    def schedule_stripe() -> None:
        rebuild_stripe()
        if not diff_pages:
            return
        total = sum(int(st.stripe_idx.size) for st in mods)
        if total == 0:
            return
        budget = diff_budget[0]
        for st in mods:
            if wq.qsize() >= WQ_MAX - 2048:
                break   # leave room for urgent + retry reads this pass
            pop = int(st.stripe_idx.size)
            if pop == 0:
                continue
            share = max(64, budget * pop // total)
            # hot tier: pages in blocks that flipped recently are swept
            # every pass; the cold rotation may re-read a few of them, and
            # that bounded duplication is fine.
            hot_ids = [b for (m, b) in hot_ids_cache if m == st.mid]
            hot_idx = np.empty(0, np.int64)
            if hot_ids and hot_cap:
                mask = np.isin(st.block_of_page[st.stripe_idx], hot_ids)
                hot_idx = st.stripe_idx[mask][:hot_cap]
            hot_take = min(int(hot_idx.size), share)
            if hot_take:
                rot = st.hot_pos % int(hot_idx.size)
                if hot_take < int(hot_idx.size):
                    sel = np.concatenate((hot_idx[rot:], hot_idx[:rot]))
                    sel = sel[:hot_take]
                else:
                    sel = hot_idx
                st.hot_pos = (rot + hot_take) % int(hot_idx.size)
                enqueue_pages(st, sel, RETRY_HOT_PRIO, "hot")
            rest = share - hot_take
            if rest > 0:
                rot = st.diff_pos % pop
                if rot:
                    sel = np.concatenate((st.stripe_idx[rot:],
                                          st.stripe_idx[:rot]))[:rest]
                else:
                    sel = st.stripe_idx[:rest]
                st.diff_pos = (rot + min(rest, pop)) % pop
                enqueue_pages(st, sel, DIFF_PRIO, "diff")

    def schedule_recap() -> None:
        if not (recapture and recap_pages):
            return
        for st in mods:
            idxr = np.flatnonzero(st.captured & st.fp_valid)
            if idxr.size == 0:
                continue
            take = min(recap_pages, int(idxr.size))
            rot = st.recap_pos % int(idxr.size)
            if take < int(idxr.size):
                sel = np.concatenate((idxr[rot:], idxr[:rot]))[:take]
            else:
                sel = idxr
            st.recap_pos = (rot + take) % int(idxr.size)
            enqueue_pages(st, sel, RECAP_PRIO, "recap")

    def schedule_tight() -> None:
        """v3.4 tier: poll --tight-* pages on cadence; the chase that a
        result triggers re-reads at once (in handle_tight_result)."""
        if not tight_keys or tight_hz <= 0.0:
            return
        t = now()
        if t < tight_next[0]:
            return
        tight_next[0] = t + 1.0 / tight_hz
        st = tight_st
        by_mod: dict = {}
        for key in tight_keys:
            if key in tight_last and t - tight_last[key] < 0.5 / tight_hz:
                continue
            by_mod.setdefault(key[0], []).append(key[1])
        for mid, pgs in by_mod.items():
            st = mods[mid]
            pgs.sort()
            off = 0
            while off < len(pgs):
                take = min(TIGHT_CHUNK, len(pgs) - off)
                chunk = pgs[off:off + take]
                for pg in chunk:
                    tight_last[(st.mid, pg)] = t
                wq_put(TIGHT_PRIO, st.sess(chunk[0]), take * PAGE,
                       ("tight", st, int(st.prot[chunk[0]]),
                        phase_box[0], None))
                off += take

    # ------------------------------------------------------------------
    # start everything
    # ------------------------------------------------------------------
    hot_ids_cache: set = set()
    stats_path = os.path.join(out, "stats.csv")
    stats_fh = open(stats_path, "w", encoding="utf-8", newline="")
    stats_fh.write("t,phase,rpm_mb_s,read_mb,rq,rq_mb,drop_results,drop_wq,"
                   "wq,overflow,budget,stripe,passes\n")
    walker = RegionWalker(handle, args.legacy_enum)
    readers = []
    for _ in range(threads):
        th = threading.Thread(target=reader_loop, daemon=True)
        th.start()
        readers.append(th)
    writer_th = threading.Thread(target=writer_loop, daemon=True)
    writer_th.start()
    threading.Thread(target=input_thread, daemon=True).start()
    watcher_th = threading.Thread(target=watcher_loop, daemon=True)
    watcher_th.start()
    if include_data:
        print("note: --include-data tracks readable non-exec pages; the "
              "stripe will rotate over data pages too (long sweeps)")
    if surge_seconds > 0:
        print(f"surge: first {surge_seconds:.0f}s pinned (stripe floor "
              f"{diff_budget[0] if diff_pages else 0}, ring backoff <="
              f" {surge_backoff:.2f}s, retry budget {args.retry_budget_max})")
    print("watcher running; accepted pages print as NEW ...")

    last_report = 0.0
    last_bytes = 0
    last_alive = 0.0
    last_af = 0.0
    last_adapt = 0.0
    last_hotcalc = 0.0
    last_ticks = 0
    last_passes = 0
    sweep_est = float("inf")
    idle_frac = 0.0
    try:
        while not stop.is_set():
            tnow = now()
            if seconds and tnow > seconds:
                break
            surge = surge_seconds > 0.0 and tnow < surge_seconds

            flush_overflow()

            # watcher bookkeeping
            drained = 0
            while drained < 512:
                with walkq_lock:
                    if not walkq:
                        break
                    st, msgs = walkq.popleft()
                try:
                    apply_walk_msgs(st, msgs)
                except Exception as exc:
                    print(f"walk apply error: {exc!r}")
                drained += 1

            # refresh the hot-block cache occasionally (the stripe's hot
            # tier selection reads it every pass)
            if tnow - last_hotcalc > 0.5:
                last_hotcalc = tnow
                hot_ids_cache.clear()
                for (m, b), v in hot_blocks.items():
                    if v[1] > 0.0 and tnow - v[1] < hot_window:
                        hot_ids_cache.add((m, b))
                # cipher pages parked in hot blocks get pass-rate cadence
                bump_hot_ring(HOT_RING_BUMP)
            # the backoff cap is read from the result path, so set it even
            # between hotcalc ticks
            surge_cap_box[0] = surge_backoff if surge else RETRY_CAP_S

            schedule_poll()
            schedule_stripe()
            schedule_recap()
            schedule_tight()

            # autofocus: bump cipher pages in the blocks where flips
            # concentrated recently
            if autofocus_k and tnow - last_af > AUTOFOCUS_TICK_S:
                last_af = tnow
                cands = [(v[0], m, b) for (m, b), v in hot_blocks.items()
                         if v[1] > 0.0 and tnow - v[1] < 120.0
                         and (m, b) not in auto_blocks]
                cands.sort(reverse=True)
                for _flips, m, b in cands[:autofocus_k]:
                    auto_blocks.add((m, b))
                    cnt = 0
                    for k2 in list(rejected_by_block.get((m, b), ())):
                        if cnt >= BUMP_CAP_PER_FLIP:
                            break
                        bump(k2)
                        cnt += 1
                    print(f"autofocus: {mods[m].name} block "
                          f"{b << BLOCK_SHIFT:012x} ({_flips} flips) "
                          f"bumped {cnt} cipher pages")
                    event("autofocus", mods[m],
                          mods[m].base + (b << BLOCK_SHIFT), BLOCK_SIZE,
                          0, 0, f"flips{_flips}")

            # retries due now; a flip burst widens the budget, surge runs
            # it at full width
            burst = 0
            for t in reversed(flip_times):
                if tnow - t <= 1.0:
                    burst += 1
                else:
                    break
            pop_due(args.retry_budget_max if surge
                    else min(args.retry_budget_max,
                             args.retry_budget + 16 * burst), surge)

            # results: drain to empty when shallow (fast passes), longer
            # time-box during bursts; readers never block on this queue
            q0 = rq.qsize()
            deadline = time.perf_counter() + (0.08 if q0 > RQ_MAX else 0.02)
            processed = 0
            while processed < 65536 and time.perf_counter() <= deadline:
                try:
                    addr, data, tag = rq.get_nowait()
                except queue.Empty:
                    break
                with rq_lock:
                    rq_bytes[0] -= len(data)
                try:
                    handle_result(addr, data, tag)
                except Exception as exc:
                    print(f"result handler error: {exc!r}")
                processed += 1

            # adaptive stripe budget + stale in-flight recovery; during
            # surge the tick shortens so lost results re-arm fast
            adapt_tick = 0.5 if surge else ADAPT_TICK_S
            if tnow - last_adapt >= adapt_tick:
                last_adapt = tnow
                with rd_lock:
                    idle, busy = rd_stats[0], rd_stats[1]
                    rd_stats[0] = rd_stats[1] = 0
                idle_frac = idle / (idle + busy) if (idle + busy) else 0.0
                floor = diff_pages if surge else BUDGET_MIN
                budget = next_budget(diff_budget[0], idle_frac,
                                     wq.qsize(), rq.qsize(), floor)
                if surge and budget < BUDGET_MAX:
                    # saturate the storm window: the stripe rotation is the
                    # only catch for silent in-place decrypts, and its
                    # leftover overflow drops are re-planned every pass
                    budget = min(BUDGET_MAX, max(budget * 2, diff_pages))
                diff_budget[0] = budget
                poll_stale = SURGE_POLL_STALE_S if surge else POLL_STALE_S
                inflight_stale = (SURGE_INFLIGHT_STALE_S if surge
                                  else RETRY_INFLIGHT_STALE_S)
                for key, ts in list(poll_inflight.items()):
                    if tnow - ts > poll_stale:
                        poll_inflight.pop(key, None)
                for key, info in list(retry.items()):
                    if info[3] is not None and \
                            tnow - info[3] > inflight_stale:
                        info[3] = None
                total_pop = sum(int(st.stripe_idx.size) for st in mods)
                if rpm_holder[0] > 1e6:
                    sweep_est = total_pop * PAGE / rpm_holder[0]
                # one measured row per adapt tick: the postmortem basis
                with stat_bytes_lock:
                    _stat_mb = stat_bytes / 1048576.0
                stats_fh.write(
                    f"{tnow:.3f},{phase_box[0]},{rpm_holder[0]/1048576:.1f},"
                    f"{_stat_mb:.1f},{rq.qsize()},{rq_bytes[0]/1048576:.1f},"
                    f"{dropped_results[0]},{wq_dropped[0]},{wq.qsize()},"
                    f"{len(overflow)},{diff_budget[0]},{total_pop},"
                    f"{passes}\n")

            passes += 1
            tnow = now()
            if tnow - last_report > STATUS_TICK_S:
                with stat_bytes_lock:
                    total_b = stat_bytes
                dt = max(tnow - last_report, 1e-6)
                rpm = (total_b - last_bytes) / dt / 1048576.0
                rpm_holder[0] = rpm * 1048576.0
                last_bytes = total_b
                watch_hz_real = (watch_ticks[0] - last_ticks) / dt
                last_ticks = watch_ticks[0]
                pass_hz = (passes - last_passes) / dt
                last_passes = passes
                total_pop = sum(int(st.stripe_idx.size) for st in mods)
                tracked_n = sum(int(st.tracked.sum()) for st in mods)
                tight_part = (f" tight {tight_reads[0]}r/"
                              f"{tight_changes[0]}chg" if tight_keys else "")
                print(f"t+{tnow:6.1f}s pages {len(seen)} rejected "
                      f"{len(rejected)} rescued {len(rescued)} "
                      f"retry {len(retry)} tracked "
                      f"{tracked_n} stripe {total_pop} budget "
                      f"{diff_budget[0]} sweep {sweep_est:4.1f}s passes "
                      f"{passes}({pass_hz:.0f}Hz) watch "
                      f"{watch_hz_real:.0f}Hz "
                      f"walk {walk_stat[0]:.0f}ms/{walk_stat[1]} "
                      f"rpm {rpm:.0f}MB/s "
                      f"wq {wq.qsize()}+{len(overflow)} rq {rq.qsize()} "
                      f"idle {idle_frac*100:.0f}% "
                      f"drop {dropped_results[0]}/{wq_dropped[0]} "
                      f"burst {burst} surge "
                      f"{'on' if surge else 'off(%.0fs)' % surge_seconds} "
                      f"{tight_part} {counters}")
                last_report = tnow
            if tnow - last_alive > ALIVE_TICK_S:
                last_alive = tnow
                if not WP.find_pid():
                    print("process gone")
                    break
            if args.pass_sleep:
                time.sleep(args.pass_sleep)
    except KeyboardInterrupt:
        print("stopped")
    finally:
        stop.set()
        # drain pending watcher bookkeeping so coverage reflects reality
        while True:
            with walkq_lock:
                if not walkq:
                    break
                st, msgs = walkq.popleft()
            try:
                apply_walk_msgs(st, msgs)
            except Exception:
                pass
        for _th in readers:
            _th.join(timeout=2.0)
        watcher_th.join(timeout=1.0)
        try:
            fileq.put(None)
            writer_th.join(timeout=5.0)
        except Exception:
            pass
        try:
            stats_fh.close()
        except Exception:
            pass

    # ------------------------------------------------------------------
    # outputs
    # ------------------------------------------------------------------
    ran_s = time.time() - t0
    surge_end = min(surge_seconds, ran_s) if surge_seconds > 0.0 else 0.0
    in_surge = sum(1 for info in seen.values() if info["t"] <= surge_end)
    rescued_in_surge = sum(1 for info in rescued.values()
                           if info["t"] <= surge_end)
    with stat_bytes_lock:
        _stat_mb_final = stat_bytes / 1048576.0
    warm_csv = os.path.join(out, "warm.csv")
    with open(warm_csv, "w", encoding="utf-8") as fh:
        fh.write("module,v2b,session,nonzero,entropy,first_s,phase,protect,"
                 "src,recaps\n")
        for (mid, pg), info in sorted(seen.items(),
                                      key=lambda kv: kv[1]["t"]):
            st = mods[mid]
            fh.write(f"{st.name},{st.v2b(pg):012x},{st.sess(pg):012x},"
                     f"{info['nz']},{info['ent']:.3f},{info['t']:.3f},"
                     f"{info['ph']},{info['protect']:08x},{info['src']},"
                     f"{info.get('recaps', 0)}\n")
    enc_csv = os.path.join(out, "encrypted.csv")
    with open(enc_csv, "w", encoding="utf-8") as fh:
        fh.write("module,v2b,session,nonzero,entropy,first_s,phase,tries,"
                 "last_s,churn\n")
        for (mid, pg), info in sorted(rejected.items(),
                                      key=lambda kv: kv[1]["t"]):
            st = mods[mid]
            inf = retry.get((mid, pg))
            fh.write(f"{st.name},{st.v2b(pg):012x},{st.sess(pg):012x},"
                     f"{info['nz']},{info['ent']:.3f},{info['t']:.3f},"
                     f"{info['ph']},{inf[0] if inf else 0},"
                     f"{info.get('last', info['t']):.3f},"
                     f"{info.get('churn', 0)}\n")
    hot_csv = os.path.join(out, "hotspots.csv")
    with open(hot_csv, "w", encoding="utf-8") as fh:
        fh.write("module,block_v2b,flips,last_flip_s,captures,cipher\n")
        for (mid, b), v in sorted(hot_blocks.items(),
                                  key=lambda kv: -kv[1][0]):
            st = mods[mid]
            addr = (b << BLOCK_SHIFT) + (WP.V2B_BASE if st.is_main else 0)
            cipher = len(rejected_by_block.get((mid, b), ()))
            fh.write(f"{st.name},{addr:012x},{v[0]},{v[1]:.3f},{v[2]},"
                     f"{cipher}\n")
    cov_csv = os.path.join(out, "coverage.csv")
    with open(cov_csv, "w", encoding="utf-8") as fh:
        fh.write("module,block_rva,block_v2b,pages,tracked,captured,cipher,"
                 "flips,last_flip_s,captured_share\n")
        for st in mods:
            blocks = set(int(x) for x in
                         np.unique(st.block_of_page[st.tracked]))
            blocks |= set(int(x) for x in
                          np.unique(st.block_of_page[st.captured]))
            blocks |= set(b for (m, b) in hot_blocks if m == st.mid)
            blocks |= set(b for (m, b) in rejected_by_block
                          if m == st.mid)
            for b in sorted(blocks):
                p0 = b << BLOCK_PAGES_SHIFT
                if p0 >= st.npages:
                    continue
                p1 = min(p0 + (1 << BLOCK_PAGES_SHIFT), st.npages)
                tr = int(st.tracked[p0:p1].sum())
                ca = int(st.captured[p0:p1].sum())
                hb = hot_blocks.get((st.mid, b))
                ci = len(rejected_by_block.get((st.mid, b), ()))
                addr = (b << BLOCK_SHIFT) + \
                    (WP.V2B_BASE if st.is_main else 0)
                share = ca / tr if tr else 0.0
                flips = hb[0] if hb else 0
                lastf = f"{hb[1]:.3f}" if hb else ""
                fh.write(f"{st.name},{b << BLOCK_SHIFT:x},{addr:012x},"
                         f"{p1 - p0},{tr},{ca},{ci},{flips},{lastf},"
                         f"{share:.3f}\n")
    gaps_path = os.path.join(out, "gaps.focus")
    gap_toks = gaps_tokens(main_st)
    kind_note = "readable incl. data" if include_data else "readable-exec"
    with open(gaps_path, "w", encoding="utf-8") as fh:
        fh.write("# focus tokens for the next run:\n")
        fh.write("#   python live_page_diff.py --focus-file gaps.focus\n")
        fh.write(f"# tracked ({kind_note}) pages this run never captured,\n")
        fh.write("# one span per line; lines starting with # are ignored\n")
        for t in gap_toks:
            fh.write(t + "\n")
    with open(os.path.join(out, "meta.json"), "w", encoding="utf-8") as fh:
        json.dump({
            "pid": pid, "main": list(main_mod), "delta": delta,
            "modules": [list(m) for m in modules],
            "pages": len(seen), "rejected": len(rejected),
            "rescued": len(rescued),
            "captured_in_surge": in_surge,
            "rescued_in_surge": rescued_in_surge,
            "read_mb": _stat_mb_final,
            "seconds": time.time() - t0,
            "engine": {
                "version": 3,
                "threads": threads,
                "watch_hz_target": watch_hz,
                "watch_cycles": watch_ticks[0],
                "numpy_batch": bool(_NPY_BATCH),
                "numpy_fp": bool(_NPY_FP),
                "entropy_scale": _ENT_SCALE,
                "enum": "vqe" if walker.uses_vqe else "legacy",
                "diff_budget": diff_budget[0],
                "diff_pages_initial": diff_pages,
                "tracked_pages": sum(int(st.tracked.sum()) for st in mods),
                "stripe_pages": sum(int(st.stripe_idx.size) for st in mods),
                "poll_pages": len(poll_list),
                "poll_budget": poll_budget,
                "retry_budget": args.retry_budget,
                "autofocus_blocks": sorted(f"{m}:{b}"
                                            for (m, b) in auto_blocks),
                "focus_v2b": sorted(focus_v2b)[:2048],
                "counters": counters,
                "dropped_results": dropped_results[0],
                "events": writer_stats["events"],
                "strict_classify": strict,
                "recapture": recapture,
                "include_data": include_data,
                "surge_seconds": surge_seconds,
                "surge_backoff": surge_backoff,
                "chunk_pages": CH_PAGES[0],
                "rq_bytes_cap_mb": rq_bytes_cap[0] // 1048576,
                "tight_pages": len(tight_keys),
                "tight_cli": {"rva": args.tight_rva,
                              "zeros": args.tight_zeros,
                              "zeros_ranges": args.tight_zeros_ranges},
                "tight_module": args.tight_zeros_module,
                "tight_hz": tight_hz,
                "tight_chase": tight_chase,
                "tight_reads": tight_reads[0],
                "tight_changes": tight_changes[0],
                "tight_plain_hits": tight_plain[0],
                "rescued_total": len(rescued),
            },
        }, fh, indent=2)
    print(f"wrote {len(seen)} pages, {len(rejected)} still-cipher records")
    if surge_seconds > 0:
        print(f"surge recap (window t+0..{surge_end:.1f}s): "
              f"{in_surge} pages captured in window; "
              f"{len(rescued)} rescued after reading as cipher at least "
              f"once ({rescued_in_surge} in window); "
              f"{len(rejected)} never read as plaintext this run")
    print(f"  {warm_csv}")
    print(f"  {enc_csv}")
    print(f"  {hot_csv}")
    print(f"  {cov_csv}")
    print(f"  {gaps_path} ({len(gap_toks)} spans)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
