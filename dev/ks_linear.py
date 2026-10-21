"""Berlekamp-Massey linear-complexity test on the page keystream.

If the per-page keystream comes from a linear generator (xorshift/LFSR
family), its linear complexity over GF(2) is the register size (<= a few
hundred bits) and the whole stream is predictable from a short prefix.
A cryptographic stream shows complexity near n/2.

usage: python dev/ks_linear.py [pages]
"""
import sys

import numpy as np


def berlekamp_massey(bits):
    n = len(bits)
    c = [1]
    b = [1]
    l = 0
    m = 1
    for i in range(n):
        d = bits[i]
        for j in range(1, l + 1):
            if c[j] and i - j >= 0:
                d ^= bits[i - j]
        if d == 1:
            t = c[:]
            if len(c) < len(b) + m:
                c += [0] * (len(b) + m - len(c))
            for j in range(len(b)):
                c[j + m] ^= b[j]
            if 2 * l <= i:
                l = i + 1 - l
                b = t
                m = 1
            else:
                m += 1
        else:
            m += 1
    return l


def main():
    n_pages = int(sys.argv[1]) if len(sys.argv) > 1 else 4
    ks = np.load(r"D:\ow-dumps\pagepak\keystream_current.npy", mmap_mode="r")
    print("keystream pages:", ks.shape)
    used = 0
    for p in range(ks.shape[0]):
        page = np.asarray(ks[p])
        if not page.any():
            continue
        bits = np.unpackbits(page[:256])  # 2048 bits
        lc = berlekamp_massey([int(b) for b in bits])
        print(f"page {p}: linear complexity {lc} / {len(bits)}")
        used += 1
        if used >= n_pages:
            break


if __name__ == "__main__":
    main()
