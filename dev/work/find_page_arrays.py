"""Find the arrays of page VAs inside the module images.

    python dev/work/find_page_arrays.py
"""

import numpy as np, os

def scan(blob, name, base=0):
    print('==', name, hex(len(blob)), flush=True)
    for width, dt in ((4, np.uint32), (8, np.uint64)):
        a = np.frombuffer(blob[:len(blob) // width * width].tobytes(), dtype=dt)
        for S in list(range(1, 17)) + [24, 32, 48, 64]:
            if len(a) < S * 16 + 1:
                continue
            d = (a[S:] - a[:-S]).astype(dt)
            pos = np.flatnonzero(d == dt(0x1000))
            if not len(pos):
                continue
            # extend runs: consecutive positions pos, pos+S, ...
            s = set(int(x) for x in pos)
            used = set()
            found = 0
            for x in pos:
                x = int(x)
                if x in used:
                    continue
                k = 0
                while (x + (k + 1) * S) in s:
                    k += 1
                if k >= 12:
                    start = x * width
                    print(f'  run {name} width {width} stride {S} at {start:#x} len {k + 2} first {int(a[x]):#x}', flush=True)
                    found += 1
                    for j in range(k + 1):
                        used.add(x + j * S)
                    if found > 12:
                        break
    print('  done', flush=True)

for path, name in ((r'dev/work/live_loader_flat.bin', 'ldr_live'),
                   (r'dev/work/live_main_flat.bin', 'main_live'),
                   (r'D:/ow-dumps/loader_disk_rva.bin', 'ldr_disk'),
                   (r'E:/Overwatch/_retail_/Overwatch.exe', 'exe')):
    if os.path.exists(path):
        scan(np.fromfile(path, dtype=np.uint8), name)
print('done')
