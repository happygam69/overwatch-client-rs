#!/usr/bin/env python3
"""Census record kinds across extracted type-0x006 animation clips.

For each clip:
  1. Parse the header (family, frames, fps, sections).
  2. Find the record grid: scan all byte offsets for the alignment where
     u16@0 == 1 appears at 32-byte stride (the confirmed grid signature).
  3. Read the kind byte at +0xe of each record in the grid.
  4. Count kinds across all clips, grouped by family.

Usage: python dev/clip_kinds.py [clip_dir]
"""
import os
import struct
import sys
from collections import defaultdict

def parse_clip(path):
    with open(path, 'rb') as f:
        data = f.read()
    if len(data) < 0x80:
        return None
    family = struct.unpack_from('<I', data, 0x00)[0]
    frames = struct.unpack_from('<H', data, 0x0c)[0]
    fps = struct.unpack_from('<H', data, 0x0e)[0]
    sections = []
    for i in range(10):
        off = 0x30 + i * 8
        if off + 8 > len(data):
            break
        sections.append(struct.unpack_from('<q', data, off)[0])
    return {
        'family': family,
        'frames': frames,
        'fps': fps,
        'sections': sections,
        'data': data,
    }

def find_record_grid(clip):
    """Find the 32-byte record grid by scanning all byte offsets for the
    alignment where u16@0 == 1 appears at 32-byte stride."""
    data = clip['data']
    frames = clip['frames']
    if frames == 0:
        return None
    best_start = None
    best_score = 0
    # Scan all byte offsets in the body
    for start in range(0x80, min(len(data) - 32, 0x1000)):
        score = 0
        count = 0
        for i in range(100):
            off = start + i * 32
            if off + 32 > len(data):
                break
            u16_0 = struct.unpack_from('<H', data, off + 0x00)[0]
            if u16_0 == 1:
                score += 1
            count += 1
        if count >= 10 and score > best_score:
            best_score = score
            best_start = start
    # Require at least a few matches to call it a grid
    if best_score >= 3:
        return best_start
    return None

def census_kinds(clip, grid_start):
    """Read kind bytes from the record grid."""
    data = clip['data']
    frames = clip['frames']
    kinds = []
    off = grid_start
    while off + 32 <= len(data):
        u16_0 = struct.unpack_from('<H', data, off + 0x00)[0]
        if u16_0 == 1:
            kind = data[off + 0x0e]
            kinds.append(kind)
            off += 32
        else:
            break
    return kinds

def main():
    clip_dir = sys.argv[1] if len(sys.argv) > 1 else 'D:/ow-dumps'
    files = [f for f in os.listdir(clip_dir) if f.startswith('asset_') and f.endswith('.bin')]
    if not files:
        print(f"No asset_*.bin files found in {clip_dir}")
        return

    kind_counts = defaultdict(int)
    family_kinds = defaultdict(lambda: defaultdict(int))
    total_clips = 0
    clips_with_grid = 0
    total_records = 0

    for fname in sorted(files):
        path = os.path.join(clip_dir, fname)
        clip = parse_clip(path)
        if clip is None:
            continue
        total_clips += 1
        grid_start = find_record_grid(clip)
        if grid_start is None:
            continue
        clips_with_grid += 1
        kinds = census_kinds(clip, grid_start)
        for k in kinds:
            kind_counts[k] += 1
            family_kinds[clip['family']][k] += 1
            total_records += 1

    print(f"Scanned {total_clips} clips, found grid in {clips_with_grid}")
    print(f"Total records: {total_records}")
    print()
    print("Kind census (all families):")
    for k in sorted(kind_counts.keys()):
        print(f"  kind {k:2d}: {kind_counts[k]:6d} records")
    print()
    print("Kinds by family:")
    for fam in sorted(family_kinds.keys()):
        kinds_str = ', '.join(f"{k}:{v}" for k, v in sorted(family_kinds[fam].items()))
        print(f"  0x{fam:08x}: {kinds_str}")

if __name__ == '__main__':
    main()
