"""Find the references to the first-person camera setup.

    python dev/work/find_camera_refs.py
"""

import struct
import bisect

PATH = r'dev/work/live_main_flat.bin'
d = open(PATH, 'rb').read()
LIVE = 0x7FF736E60000
STATIC = 0x7FF7C4D20000
TARGET = 0x992770  # FirstPersonCameraSetup RVA
print('rva', hex(TARGET), 'live', hex(LIVE + TARGET), 'static', hex(STATIC + TARGET))

for name, val in (('live', LIVE + TARGET), ('static', STATIC + TARGET)):
    needle = struct.pack('<Q', val)
    pos = d.find(needle)
    print(name, 'qword hits', pos >= 0)
    n = 0
    while pos >= 0 and n < 10:
        # qwords around
        print('  at', hex(pos), 'va', hex(LIVE + pos))
        pos = d.find(needle, pos + 1)
        n += 1

# direct calls/jumps
for i in range(len(d) - 5):
    op = d[i]
    if op in (0xE8, 0xE9):
        disp = struct.unpack_from('<i', d, i + 1)[0]
        if i + 5 + disp == TARGET:
            print(('call' if op == 0xE8 else 'jmp'), 'at', hex(i), 'live', hex(LIVE + i))
