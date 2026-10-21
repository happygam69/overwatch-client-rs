"""Find the branches that guard the candidate VirtualProtect calls.

    python dev/work/branch_targets.py
"""

import struct
d = open(r'D:/ow-dumps/loader_disk_rva.bin', 'rb').read()
lo, hi = 0x14380e0, 0x144ee19

def near_targets(site, back=0x80, fwd=0x30):
    out = []
    a0 = site - back
    a1 = site + fwd
    for pos in range(a0, a1):
        # E9 rel32
        if d[pos] == 0xE9:
            rel = struct.unpack_from('<i', d, pos + 1)[0]
            t = pos + 5 + rel
            if a0 <= t <= a1:
                out.append((pos, 'jmp', t))
        # 0F 8x rel32
        if d[pos] == 0x0F and 0x80 <= d[pos + 1] <= 0x8F:
            rel = struct.unpack_from('<i', d, pos + 2)[0]
            t = pos + 6 + rel
            if a0 <= t <= a1:
                out.append((pos, 'jcc', t))
        # EB rel8
        if d[pos] == 0xEB:
            rel = struct.unpack_from('<b', d, pos + 1)[0]
            t = pos + 2 + rel
            if a0 <= t <= a1:
                out.append((pos, 'jmps', t))
        if 0x70 <= d[pos] <= 0x7F:
            rel = struct.unpack_from('<b', d, pos + 1)[0]
            t = pos + 2 + rel
            if a0 <= t <= a1:
                out.append((pos, 'jccs', t))
    return out

for site in (0x14462c5, 0x144df0a):
    print('==== site', hex(site))
    for pos, kind, t in near_targets(site):
        print(f'  branch at {pos:#x} {kind} -> {t:#x}   (distance to site {t-site:+#x})')
