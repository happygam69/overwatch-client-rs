"""Build a flat main-module image from one page-diff run.

    python dev/work/build_live_main.py
"""

import os, re, numpy as np

p = r'D:/ow-dumps/live_page_diff/run_20261021_105102/pages'
BASE = 0x7ff7c4d20000
SIZE = 0x7ff7c9557000 - BASE + 0x1000
out = np.zeros(SIZE, dtype=np.uint8)
have = np.zeros(SIZE // 4096, bool)
n = 0
for f in os.listdir(p):
    m = re.match(r'v2b_([0-9a-fA-F]+)\.bin$', f)
    if not m:
        continue
    va = int(m.group(1), 16)
    if not (BASE <= va < BASE + SIZE):
        continue
    rva = va - BASE
    b = open(os.path.join(p, f), 'rb').read()
    out[rva:rva + 4096] = np.frombuffer(b[:4096], dtype=np.uint8)
    have[rva >> 12] = True
    n += 1
print('pages', n, 'have', int(have.sum()), 'size', SIZE)
open(r'dev/work/live_main_flat.bin', 'wb').write(out.tobytes())
