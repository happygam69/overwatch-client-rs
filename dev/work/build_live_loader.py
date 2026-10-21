"""Build a flat loader image from one page-diff run.

    python dev/work/build_live_loader.py
"""

import os, re, numpy as np

p = r'D:/ow-dumps/live_page_diff/run_20261021_105102/pages'
out = np.zeros(0x240b000 + 0x1000, dtype=np.uint8)
have = np.zeros(len(out) // 4096, bool)
n = 0
for f in os.listdir(p):
    m = re.match(r'Overwatch_loader\.dll_([0-9a-fA-F]+)\.bin$', f)
    if not m:
        continue
    rva = int(m.group(1), 16)
    b = open(os.path.join(p, f), 'rb').read()
    if len(b) < 4096:
        continue
    out[rva:rva + 4096] = np.frombuffer(b[:4096], dtype=np.uint8)
    have[rva >> 12] = True
    n += 1
print('pages', n, 'have', int(have.sum()))
np.save(r'dev/work/live_loader_flat.npy', out)
open(r'dev/work/live_loader_flat.bin', 'wb').write(out.tobytes())
