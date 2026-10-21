"""Do the pointers in an image belong to one session?

    python dev/pointer_check.py <image-a> <image-b> [image-c ...]

Merging pages from several sessions gives the *code* of the right build (the
bytes are identical in every session) but not the *data*: a global that
holds an absolute pointer holds the address that was current in whichever
session captured that page. This tool measures the difference, so the rule
"merged images are for code, session images are for pointers" is a number
rather than a habit.

Each image is a flat module image; its base comes from the `<image>.json`
sidecar (`dev/session_merge.py` writes one). Words that look like pointers
into the module or into the heap are counted, and the ones that differ
between two images are reported.
"""

from __future__ import annotations

import json
import pathlib
import struct
import sys

EXE = pathlib.Path(r"E:\Overwatch\_retail_\Overwatch.exe")


def sections(exe: bytes) -> list[tuple[str, int, int]]:
    """(name, rva, size) from the PE headers."""
    pe = struct.unpack_from("<I", exe, 0x3C)[0]
    nsec = struct.unpack_from("<H", exe, pe + 6)[0]
    opt = struct.unpack_from("<H", exe, pe + 20)[0]
    size_opt = struct.unpack_from("<H", exe, pe + 20)[0]
    sec_off = pe + 24 + size_opt
    out = []
    for i in range(nsec):
        off = sec_off + i * 40
        name = exe[off : off + 8].rstrip(b"\x00").decode("ascii", "replace")
        vsize, va = struct.unpack_from("<II", exe, off + 8)
        out.append((name, va, vsize))
    return out


def base_of(image: pathlib.Path) -> int:
    side = image.with_suffix(image.suffix + ".json")
    if side.is_file():
        try:
            return int(json.loads(side.read_text(encoding="utf-8"))["base"])
        except Exception:
            pass
    return 0


def pointer_words(blob: bytes, base: int, lo: int, hi: int) -> dict[int, int]:
    """offset -> value, for the words that look like pointers into [lo, hi)."""
    out = {}
    for off in range(0, len(blob) - 8, 8):
        (v,) = struct.unpack_from("<Q", blob, off)
        if lo <= v < hi:
            out[off] = v
    return out


def main() -> int:
    args = sys.argv[1:]
    if len(args) < 2:
        print(__doc__)
        return 2
    images = [pathlib.Path(a) for a in args]
    bases = [base_of(p) for p in images]
    exe = EXE.read_bytes()
    secs = sections(exe)
    module_size = max(va + size for _, va, size in secs)

    for path, base in zip(images, bases):
        print(f"{path.name}: base {base:#x}")

    # The module's own address range in each image's frame, and the heap's
    # (the heap is far above the module in every session).
    for i, path in enumerate(images):
        blob = path.read_bytes()
        base = bases[i]
        print(f"\n{path.name}:")
        for name, va, size in secs:
            chunk = blob[va : va + size]
            if not chunk:
                continue
            zeros = chunk.count(0)
            # pointer-shaped words into the module and into the heap
            mod = pointer_words(chunk, base, base, base + module_size)
            heap = pointer_words(chunk, base, 0x160000000000, 0x170000000000)
            if name in (".text", ".pdata", ".reloc"):
                continue
            print(
                f"  {name:<8} {len(chunk):>10} bytes  zero {100 * zeros / len(chunk):>5.1f}%  "
                f"module-pointers {len(mod):>6}  heap-pointers {len(heap):>5}"
            )

    if len(images) >= 2:
        print("\nthe same offsets, compared between the first two images:")
        a, b = images[0], images[1]
        ba, bb = bases[0], bases[1]
        ab, bbl = a.read_bytes(), b.read_bytes()
        for name, va, size in secs:
            if name in (".text", ".pdata", ".reloc"):
                continue
            ca = ab[va : va + size]
            cb = bbl[va : va + size]
            if not ca or len(ca) != len(cb):
                continue
            same = sum(1 for off in range(0, len(ca) - 8, 8) if ca[off : off + 8] == cb[off : off + 8])
            total = len(ca) // 8
            diff_ptr = 0
            examples = []
            for off in range(0, len(ca) - 8, 8):
                if ca[off : off + 8] == cb[off : off + 8]:
                    continue
                (va_, vb_) = struct.unpack_from("<QQ", ca, off)
                if 0x10000 < va_ < 0x800000000000 and 0x10000 < vb_ < 0x800000000000:
                    diff_ptr += 1
                    if len(examples) < 3:
                        examples.append((va + off, va_, vb_))
            print(
                f"  {name:<8} {total:>9} words  {same:>9} identical  "
                f"{diff_ptr:>6} pointer-shaped and different"
            )
            for rva, x, y in examples:
                print(f"      rva {rva:#x}: {x:#018x} vs {y:#018x}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
