"""Scan a Windows minidump for the page-window table or the pad.

Usage:
    python dev/scan_dump.py <dump.dmp> [--pattern w|pad|eid] [--list]

`w` takes the longest contiguous page run from dev/work/w_per_page.npy and
looks for its u16 values at stride 2..64 (covers a per-page record table).
`pad` searches for the 16 KiB loader pad (and its 19-byte rotations).
`eid` searches for the ASCII EIDOLON marker.

The dump is read-only. No game files are written.
"""
from __future__ import annotations

import argparse
import mmap
import struct
import sys

import numpy as np

MDMP = 0x504D444D
STREAM_THREAD = 3
STREAM_MODULE = 4
STREAM_MEMORY64 = 9
STREAM_THREAD_INFO = 16


class Dump:
    def __init__(self, path: str):
        self.f = open(path, "rb")
        self.m = mmap.mmap(self.f.fileno(), 0, access=mmap.ACCESS_READ)
        if self.u32(0) != MDMP:
            raise SystemExit("not a minidump (no MDMP signature)")
        self.nstreams = self.u32(8)
        self.dirdata = self.u32(12)
        self.streams = {}
        for i in range(self.nstreams):
            off = self.dirdata + i * 12
            typ, size, rva = struct.unpack_from("<III", self.m, off)
            self.streams.setdefault(typ, []).append((rva, size))
        self.ranges = []
        for rva, size in self.streams.get(STREAM_MEMORY64, []):
            self._memory64(rva, size)
        self.modules = []
        for rva, size in self.streams.get(STREAM_MODULE, []):
            self._modules(rva)
        self.threads = []
        for rva, size in self.streams.get(STREAM_THREAD, []):
            self._threads(rva)

    def u32(self, off: int) -> int:
        return struct.unpack_from("<I", self.m, off)[0]

    def u64(self, off: int) -> int:
        return struct.unpack_from("<Q", self.m, off)[0]

    def _memory64(self, rva: int, size: int) -> None:
        n = self.u64(rva)
        base = self.u64(rva + 8)
        cur = base
        p = rva + 16
        for _ in range(n):
            start, length = struct.unpack_from("<QQ", self.m, p)
            self.ranges.append((start, length, cur))
            cur += length
            p += 16

    def _modules(self, rva: int) -> None:
        n = self.u32(rva)
        p = rva + 4
        for _ in range(n):
            base = self.u64(p)
            size = self.u32(p + 8)
            name_rva = self.u32(p + 20)
            self.modules.append((base, size, self._name(name_rva)))
            p += 108

    def _name(self, rva: int) -> str:
        if not rva:
            return ""
        n = self.u32(rva)
        return self.m[rva + 4:rva + 4 + n].decode("utf-16-le", "replace")

    def _threads(self, rva: int) -> None:
        n = self.u32(rva)
        p = rva + 4
        for _ in range(n):
            tid = self.u32(p)
            teb = self.u64(p + 16)
            stack_start, stack_size, stack_rva = struct.unpack_from("<QII", self.m, p + 24)
            ctx_size, ctx_rva = struct.unpack_from("<II", self.m, p + 40)
            rip = 0
            if ctx_rva and ctx_size >= 0x100:
                rip = self.u64(ctx_rva + 0xF8)
            self.threads.append((tid, teb, stack_start, stack_size, stack_rva, rip))
            p += 48

    def bytes_at(self, rva: int, length: int) -> bytes:
        return self.m[rva:rva + length]


def load_w_runs(path: str, n: int = 3) -> list[np.ndarray]:
    arr = np.load(path)
    d = {int(p): int(w) for p, w in arr}
    pages = sorted(d)
    runs = []
    cur = [pages[0]]
    for p in pages[1:]:
        if p == cur[-1] + 1:
            cur.append(p)
        else:
            runs.append(cur); cur = [p]
    runs.append(cur)
    runs.sort(key=len, reverse=True)
    return [np.array([d[p] for p in r], dtype=np.uint16) for r in runs[:n]]


def scan_w_fast(dump: Dump, W: np.ndarray) -> int:
    total = 0
    pat = W[:4].tobytes()
    for start, length, rva in dump.ranges:
        p = dump.m.find(pat, rva, rva + length)
        while p >= 0:
            ok = True
            for k in range(len(W)):
                off = p + 2 * k
                if off + 2 > rva + length or struct.unpack_from("<H", dump.m, off)[0] != int(W[k]):
                    ok = False
                    break
            if ok:
                print(f'  W FAST HIT va {start + (p - rva):#x} stride 2 len {len(W)}')
                total += 1
            p = dump.m.find(pat, p + 1, rva + length)
    return total


def scan_w(dump: Dump, runs: list[np.ndarray]) -> int:
    total = 0
    for S in (2, 4, 8, 12, 16, 24, 32, 48, 64, 128, 256):
        for o in range(min(S, 8)):
            for W in runs[:1]:
                w0, w1, w2 = int(W[0]), int(W[1]), int(W[2])
                for start, length, rva in dump.ranges:
                    if length < S * len(W):
                        continue
                    lo = rva + o
                    hi = rva + length
                    b = dump.m[lo:hi]
                    if len(b) < 2:
                        continue
                    a = np.frombuffer(b, dtype=np.uint8)
                    v = a[:-1].astype(np.uint16) | (a[1:].astype(np.uint16) << 8)
                    # v[i] reads two bytes at lo+i; record starts every S
                    if len(v) < S * 3:
                        continue
                    idx = np.flatnonzero(v[:-2 * S:S] == w0)
                    for i in idx:
                        pos = int(i)
                        if pos + 2 * S >= len(v):
                            continue
                        if int(v[pos + S]) != w1 or int(v[pos + 2 * S]) != w2:
                            continue
                        ok = True
                        for k in range(3, len(W)):
                            if pos + k * S >= len(v) or int(v[pos + k * S]) != int(W[k]):
                                ok = False
                                break
                        if ok:
                            print(f'  W HIT stride {S} align {o} va {start + o + pos:#x} range {start:#x}+{length:#x}')
                            total += 1
    return total


def scan_pad(dump: Dump) -> int:
    pad = open(r'D:/ow-dumps/loader_disk_rva.bin', 'rb').read()[0x1d77f80:0x1d7bf80]
    total = 0
    variants = [pad, pad[19:] + pad[:19], pad[-19:] + pad[:-19]]
    for vi, v in enumerate(variants):
        needle = v[:64]
        for start, length, rva in dump.ranges:
            p = dump.m.find(needle, rva, rva + length)
            while p >= 0:
                print(f'  PAD HIT variant {vi} va {start + (p - rva):#x}')
                total += 1
                p = dump.m.find(needle, p + 1, rva + length)
    return total


def scan_eid(dump: Dump) -> int:
    total = 0
    for start, length, rva in dump.ranges:
        p = dump.m.find(b'EIDOLON', rva, rva + length)
        while p >= 0:
            print(f'  EID HIT va {start + (p - rva):#x} {dump.m[p:p + 32]!r}')
            total += 1
            p = dump.m.find(b'EIDOLON', p + 1, rva + length)
    return total


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--pattern", choices=("w", "pad", "eid"), default="w")
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--slow", action="store_true",
                    help="strided table search after the fast contiguous pass")
    args = ap.parse_args()

    d = Dump(args.dump)
    if args.list:
        print(f"streams {sorted(d.streams)} ranges {len(d.ranges)} modules {len(d.modules)} threads {len(d.threads)}")
        for b, s, n in d.modules[:40]:
            print(f"  {b:#x} {s:#x} {n}")
        for t in d.threads[:20]:
            print(f"  tid {t[0]} teb {t[1]:#x} stack {t[2]:#x}+{t[3]:#x} rip {t[5]:#x}")

    if args.pattern == "w":
        runs = load_w_runs(r"dev/work/w_per_page.npy")
        print(f"w runs: " + ", ".join(f"{len(r)}" for r in runs))
        n = scan_w_fast(d, runs[0])
        if not n and args.slow:
            print("no contiguous hit; running strided search")
            n = scan_w(d, runs)
    elif args.pattern == "pad":
        n = scan_pad(d)
    else:
        n = scan_eid(d)
    print(f"hits {n}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
