#!/usr/bin/env python3
"""Dump the entire file structure to find pose data."""
import struct
import sys
import math

def main():
    path = sys.argv[1] if len(sys.argv) > 1 else 'D:/ow-dumps/asset_0a00000000000050.bin'
    with open(path, 'rb') as f:
        data = f.read()

    family = struct.unpack_from('<I', data, 0x00)[0]
    frames = struct.unpack_from('<H', data, 0x0c)[0]
    fps = struct.unpack_from('<H', data, 0x0e)[0]
    print(f"Clip {path}")
    print(f"  family: 0x{family:08x}, frames: {frames}, fps: {fps}, size: {len(data)}")

    # Section table
    print("\nSection table:")
    for i in range(10):
        off = 0x30 + i * 8
        if off + 8 > len(data):
            break
        s = struct.unpack_from('<q', data, off)[0]
        if s > 0 and s < len(data):
            print(f"  [{i}]: {s} (0x{s:x})")

    # Dump the file in 128-byte windows, showing what's there
    print("\nFile dump (128-byte windows):")
    for base in range(0, len(data), 128):
        # Classify the window
        zeros = sum(1 for j in range(base, min(base+128, len(data))) if data[j] == 0)
        printable = sum(1 for j in range(base, min(base+128, len(data))) if 0x20 <= data[j] <= 0x7e)
        # Count f32 values in [-1, 1]
        f32_count = 0
        for j in range(base, min(base+124, len(data)), 4):
            val = struct.unpack_from('<f', data, j)[0]
            if not math.isnan(val) and not math.isinf(val) and -1.5 <= val <= 2.5:
                f32_count += 1
        marker = ""
        if zeros > 100:
            marker = " [mostly zeros]"
        elif f32_count > 20:
            marker = f" [f32-like: {f32_count}]"
        elif printable > 50:
            marker = " [printable]"
        print(f"  0x{base:04x}: zeros={zeros:3d} f32={f32_count:2d} printable={printable:3d}{marker}")
        # If it has f32-like data, dump it
        if f32_count > 10:
            for j in range(base, min(base+128, len(data)-3), 4):
                val = struct.unpack_from('<f', data, j)[0]
                if not math.isnan(val) and not math.isinf(val) and -1.5 <= val <= 2.5:
                    print(f"    0x{j:04x}: {val:.6f}")

if __name__ == '__main__':
    main()
