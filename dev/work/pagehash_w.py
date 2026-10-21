"""Test the page metadata hashes against the window start.

    python dev/work/pagehash_w.py
"""

import struct
import sys

import numpy as np

sys.path.insert(0, r'dev')
from scan_dump import Dump

DUMP = r'D:\ow-dumps\ow_heap_full.dmp'
TABLE_VA = 0x1BA36B90000
TABLE_LEN = 0x41000
MAIN = 0x7FF6A2D80000
MAIN_SIZE = 0x4838000
REC = 0x90

d = Dump(DUMP)
src = None
for s, l, r in d.ranges:
    if s <= TABLE_VA < s + l:
        src = r + (TABLE_VA - s)
        break
assert src is not None
raw = d.m[src:src + TABLE_LEN]

wmap = {int(p): int(w) for p, w in np.load(r'dev/work/w_per_page.npy')}
q = np.frombuffer(raw[:len(raw) // 8 * 8], dtype='<u8')
main_hits = np.flatnonzero((q >= MAIN) & (q < MAIN + MAIN_SIZE))
print('main qwords', len(main_hits))
if not len(main_hits):
    raise SystemExit(1)

# find a phase that makes consecutive main qwords exactly REC apart
best = None
for ph in range(0, REC // 8):
    idx = [int(i) for i in main_hits if (int(i) - ph) % (REC // 8) == 0]
    if len(idx) > (len(best[1]) if best else 0):
        best = (ph, idx)
ph, idx = best
print('phase qword', ph, 'records', len(idx))

def fmix64(k):
    k ^= k >> 33
    k = (k * 0xFF51AFD7ED558CCD) & ((1 << 64) - 1)
    k ^= k >> 33
    k = (k * 0xC4CEB9FE1A85EC53) & ((1 << 64) - 1)
    k ^= k >> 33
    return k

rows = []
for i in idx:
    p = int(i) * 8
    page = struct.unpack_from('<Q', raw, p)[0]
    h1 = struct.unpack_from('<Q', raw, p + 8)[0]
    h2 = struct.unpack_from('<Q', raw, p + 0x20)[0]
    h3 = struct.unpack_from('<Q', raw, p + 0x30)[0]
    h4 = struct.unpack_from('<Q', raw, p + 0x38)[0]
    h5 = struct.unpack_from('<Q', raw, p + 0x40)[0]
    h6 = struct.unpack_from('<Q', raw, p + 0x48)[0]
    pg = (page - MAIN) // 0x1000
    if page % 0x1000 or pg not in wmap:
        continue
    rows.append((pg, wmap[pg], h1, h2, h3, h4, h5, h6))

print('matched oracle pages', len(rows))
print('first', [(r[0], r[1], hex(r[2]), hex(r[3])) for r in rows[:5]])
if not rows:
    raise SystemExit(1)

def test(name, fn):
    ok = 0
    for r in rows:
        if fn(r) == r[1]:
            ok += 1
    if ok:
        print(f'{name}: {ok}/{len(rows)}')

for s in range(0, 53):
    test(f'h1>>{s}&fff', lambda r, s=s: (r[2] >> s) & 0xfff)
    test(f'h2>>{s}&fff', lambda r, s=s: (r[3] >> s) & 0xfff)
    test(f'h3>>{s}&fff', lambda r, s=s: (r[4] >> s) & 0xfff)
for name, f in [
    ('h1%4096', lambda r: r[2] % 4096),
    ('h2%4096', lambda r: r[3] % 4096),
    ('h1^h2 low', lambda r: (r[2] ^ r[3]) & 0xfff),
    ('h1+h2 low', lambda r: (r[2] + r[3]) & 0xfff),
    ('h1*h2 low', lambda r: (r[2] * r[3]) & 0xfff),
    ('fmix(h1)', lambda r: fmix64(r[2]) & 0xfff),
    ('fmix(h2)', lambda r: fmix64(r[3]) & 0xfff),
    ('fmix(h1^h2)', lambda r: fmix64(r[2] ^ r[3]) & 0xfff),
]:
    test(name, f)
