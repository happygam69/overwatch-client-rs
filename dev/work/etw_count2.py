"""Count ETL samples of the selector-chain and descriptor addresses.

    python dev/work/etw_count2.py
"""

import mmap
import struct

ETL = r'D:\ow-dumps\etw\cpu_20261021_124450.etl'
BASE = 0x7ffc52d50000
TARGETS = {
    'hash_start': 0x14480fc,
    'hash_imul': 0x144810d,
    'hash_xorkey': 0x1448119,
    'hash_store': 0x1448120,
    'desc_start': 0x144819e,
    'w_store': 0x14481c1,
    'loop': 0x144c839,
    'vp_ret': 0x14462cb,
    'caller_ret': 0x1015a99,
    'hash_wrap': 0x1494090,
    'vm_wrap': 0x2511e0,
    'idx_wrap': 0x3fa280,
}
f = open(ETL, 'rb')
m = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
for name, rva in TARGETS.items():
    needle = struct.pack('<Q', BASE + rva)
    p = m.find(needle)
    n = 0
    first = []
    while p >= 0:
        n += 1
        if len(first) < 3:
            first.append(p)
        p = m.find(needle, p + 1)
    print(f'{name:12} {rva:#09x} {n:5} {[hex(x) for x in first]}')
