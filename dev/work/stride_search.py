"""Search the flat images for the window-start run at strides.

    python dev/work/stride_search.py
"""

import numpy as np, os, struct

arr = np.load(r'dev/work/w_per_page.npy')
d = {int(p): int(w) for p, w in arr}
runs = []
cur = [sorted(d)[0]]
for p in sorted(d)[1:]:
    if p == cur[-1] + 1:
        cur.append(p)
    else:
        if len(cur) >= 15:
            runs.append(cur)
        cur = [p]
if len(cur) >= 15:
    runs.append(cur)
runs.sort(key=len, reverse=True)
run = runs[0]
print('run', run[0], run[-1], len(run))

paths = [
    r'dev/work/live_loader_flat.bin',
    r'D:/ow-dumps/loader_disk_rva.bin',
    r'D:/ow-dumps/module_heap.bin',
]

def find_strided(blob, vals, width, maxstride=256):
    n = len(blob)
    fw = width // 8
    # positions where first value occurs
    pat = int(vals[0]).to_bytes(fw, 'little')
    poss = []
    start = 0
    while True:
        i = blob.find(pat, start)
        if i < 0:
            break
        poss.append(i)
        start = i + 1
    out = []
    for S in range(fw, maxstride + 1, 1):
        for i in poss:
            if i + S * (len(vals) - 1) + fw > n:
                continue
            ok = True
            for k in range(1, len(vals)):
                if blob[i + k * S:i + k * S + fw] != int(vals[k]).to_bytes(fw, 'little'):
                    ok = False
                    break
            if ok:
                out.append((i, S))
                break
        if out:
            break
    return out[:4]

for path in paths:
    if not os.path.exists(path):
        print('missing', path)
        continue
    blob = open(path, 'rb').read()
    print('==', path, len(blob))
    for width, name, div in ((2, 'u16_w', 1), (2, 'u16_v', 4), (4, 'u32_w', 1), (4, 'u32_v', 4)):
        vals = [d[p] // div for p in run[:10]]
        hits = find_strided(blob, vals, width * 8)
        for i, S in hits:
            print(f'  {name} hit at {i:#x} stride {S}')
        if not hits:
            print(f'  {name} none')
