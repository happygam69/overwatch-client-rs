import mmap
import struct
import sys

ETL = r'D:\ow-dumps\etw\cpu_20261021_124450.etl'
BASE = 0x7ffc52d50000
TARGETS = {
    'cand_entry': 0x14380e0,
    'cand_page_block': 0x14388e7,
    'cand_desc_store': 0x14481c1,
    'cand_vp_call': 0x14462c5,
    'cand_vp_ret': 0x14462cb,
    'cand_loop': 0x144c839,
    'cand_loop_entry': 0x144968f,
    'cand_ret': 0x144ede1,
    'padA_entry': 0x10b72f0,
    'padA_arm': 0x10b7366,
    'padB_entry': 0x10c1e20,
    'padB_arm': 0x10c1e5d,
    'padB_mask1': 0x10c7692,
    'padB_mask2': 0x10c9835,
    'padB_hot': 0x10c35a0,
    'padB_hot_ret': 0x10c35a5,
    'loader_hot': 0xf3fd0,
    'shift_block': 0x1e13250,
}

f = open(ETL, 'rb')
m = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
print('size', len(m))
for name, rva in TARGETS.items():
    needle = struct.pack('<Q', BASE + rva)
    p = m.find(needle)
    n = 0
    first = []
    while p >= 0:
        n += 1
        if len(first) < 6:
            first.append(p)
        p = m.find(needle, p + 1)
    print(f'{name:18} {rva:#09x} count {n:6} offsets {[hex(x) for x in first]}')
