"""Extract the descriptor-stub table from a session image.

Stubs (as seen at 0xfc0f0 in run 113932):

    4c8d05  disp32   ; lea r8,  [shared descriptor]
    488d15  disp32   ; lea rdx, [record A]
    488d0d  disp32   ; lea rcx, [record B]
    e9      disp32   ; jmp <registrar>

Usage: python dev/stub_extract.py <img.bin> <registrar_rva> [--csv path]
Prints rows (stub_rva, r8_target, rdx_target, rcx_target).
"""
import struct
import sys

PATTERNS = (
    b"\x4c\x8d\x05",  # lea r8
    b"\x48\x8d\x15",  # lea rdx
    b"\x48\x8d\x0d",  # lea rcx
)


def main() -> int:
    img, reg_hex = sys.argv[1], sys.argv[2]
    reg = int(reg_hex, 16)
    a = open(img, "rb").read()
    n = len(a)
    rows = []
    i = 0
    while i < n - 40:
        if a[i:i + 3] != PATTERNS[0]:
            i += 1
            continue
        s = i
        slots = []
        ok = True
        for which in range(3):
            s += 0 if which == 0 else 0
            if a[s:s + 3] != PATTERNS[which]:
                ok = False
                break
            disp = struct.unpack_from("<i", a, s + 3)[0]
            slots.append(s + 7 + disp)
            s += 7
        if ok and a[s] == 0xE9:
            jreg = s + 5 + struct.unpack_from("<i", a, s + 1)[0]
            if jreg == reg:
                rows.append((i, *slots))
                i = s + 5
                continue
        i += 1
    for r in rows:
        print(f"{r[0]:#x},{r[1]:#x},{r[2]:#x},{r[3]:#x}")
    print(f"; total stubs: {len(rows)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
