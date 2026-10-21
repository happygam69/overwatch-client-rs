"""Walk every class descriptor the registry knows and dump its slots.

Each registry row's statics include (for many classes) a static descriptor
object built by InitializeIntrospectionDescriptor (0x7ff64c094b90). The layout
is known:

    +0x00 vtable        (0x7ff64ce43a00 = introspection node)
    +0x08 instances     pointer to the registration-instance table
    +0x18 ?             
    +0x20 parent        descriptor of the base class
    +0x28 field array   field entries, stride 0x38, count at +0x98
    +0x50 token         u64 program token
    +0x58..0x90         static pointers: ctor, comparators, dispatch chains, ...
    +0xa0 size          u32
    +0xb8 hash          u32 class hash
    +0xbc flags         u32

Entries in the +0x38..+0x48 window may be *obfuscated pointers* (ror/xor/sub
idiom); they are reported raw with a marker rather than decoded silently.

Usage: python dump_class_descriptors.py
"""

from __future__ import annotations

import glob
import struct

import regions as R

NODE_VTABLE = 0x7FF64CE43A00 + R.DELTA
SLOT_NAMES = {
    0x00: "vtable",
    0x08: "instances",
    0x10: "slot_10",
    0x18: "slot_18",
    0x20: "parent",
    0x28: "field_array",
    0x30: "slot_30",
    0x38: "slot_38",
    0x40: "slot_40",
    0x48: "slot_48",
    0x50: "token",
    0x58: "static_58",
    0x60: "static_60",
    0x68: "static_68",
    0x70: "static_70",
    0x78: "static_78",
    0x80: "static_80",
    0x88: "static_88",
    0x90: "static_90",
    0x98: "field_count",
    0xa0: "size",
    0xb8: "hash",
    0xbc: "flags",
}


def main() -> int:
    space = R.Space(R.load(R.capture_dir("dumps_range")))
    rows = []
    for path in sorted(glob.glob(R.out_name("class_registry.txt"))):
        for line in open(path, encoding="utf-8"):
            if line.startswith("#"):
                continue
            m = line.split("statics[")
            if len(m) < 2:
                continue
            head = m[0].split()
            statics = [int(x, 16) for x in m[1].rstrip("]\n").split(",")]
            rows.append((head[0], int(head[1], 16), statics))

    found = 0
    out_path = R.out_name("class_descriptors.txt")
    out = open(out_path, "w", encoding="utf-8")
    out.write("# class_hash size descriptor_addr slots...\n")
    for hash_s, size, statics in rows:
        for cand in statics:
            v = space.u64(cand)
            if v != NODE_VTABLE:
                continue
            found += 1
            parts = []
            for off, name in SLOT_NAMES.items():
                if name in ("size", "hash", "flags", "field_count"):
                    b = space.read(cand + off, 4)
                    if b:
                        parts.append(f"{name}={struct.unpack('<I', b)[0]:#x}")
                else:
                    q = space.u64(cand + off)
                    if q:
                        tag = ""
                        if off in (0x38, 0x40, 0x48) and q >> 48 == 0:
                            tag = "?"
                        parts.append(f"{name}={q:#x}{tag}")
            out.write(f"{hash_s} {size:#x} {cand:#x} " + " ".join(parts) + "\n")
            break
    out.close()
    print(f"descriptors found: {found} of {len(rows)} rows -> {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
