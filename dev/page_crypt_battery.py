"""Cipher-structure battery for the Overwatch.exe page transform.

Real-code plaintext filter (ent 3..7.2, non-zero>=64), then:
1. Per-page period test on X = C ^ P (repeating-key XOR detector).
2. Cross-page invariance: does X repeat across pages (static key)?
3. Block-repeat test (AES-ECB detector): for EQUAL 16-byte plaintext
   blocks in different pages, are the ciphertext blocks equal too?
4. Cross-session: same rva read as plaintext in two different captures
   (module_v2full.bin vs module_after_diff.bin) -> same X both times?
   (deterministic per-page keystream vs per-boot key)
"""
import numpy as np

disk = np.frombuffer(open(r"E:\Overwatch\_retail_\Overwatch.exe", "rb").read(),
                     np.uint8)
P_v2full = None

TEXT_VA, TEXT_RAW, TEXT_SZ = 0x1000, 0x400, 0x2f94913


def foff(rva):
    if rva < 0x1000:
        return None
    if rva < TEXT_VA + TEXT_SZ:
        return TEXT_RAW + (rva - TEXT_VA)
    return None


def load_image(path):
    return np.frombuffer(open(path, "rb").read(), np.uint8)


def pairs_for(img):
    img = img[:min(len(img), TEXT_VA + TEXT_SZ)]
    n = len(img) >> 12
    out = []
    for pg in range(1, n):
        rva = pg << 12
        fo = foff(rva)
        if fo is None:
            continue
        p = img[pg * 4096:(pg + 1) * 4096]
        nz = int((p != 0).sum())
        if nz < 64:
            continue
        counts = np.bincount(p, minlength=256)
        pr = (counts / len(p))[counts > 0]
        ent = float(-(pr * np.log2(pr)).sum())
        if not (3.0 <= ent <= 7.2):
            continue
        c = disk[fo:fo + 4096]
        out.append((rva, p, c))
    return out


img_after = load_image(r"D:\ow-dumps\module_after_diff.bin")
pairs = pairs_for(img_after)
print(f"real-code pairs (after image): {len(pairs)}")

# 1+2: period + cross-page X invariance
Xs = []
periods = []
for rva, p, c in pairs:
    X = p ^ c
    Xs.append(X)
per_none = 0
per_found = {}
for i, X in enumerate(Xs):
    best = None
    for k in (16, 32, 64, 128, 256, 512):
        if (X[:-k] == X[k:]).mean() > 0.95:
            best = k
            break
    if best is None:
        per_none += 1
    else:
        per_found[best] = per_found.get(best, 0) + 1
print(f"per-page period: none {per_none}, found {per_found}")

A = np.array(Xs)
agree00 = (A[0] == A[1]).mean() if len(Xs) > 1 else -1
print(f"X[0] vs X[1] agreement: {agree00:.4f}")

# 3: block-repeat / ECB test with hash buckets
block_map = {}
dup_p = 0
same_c = 0
FIXED = 16
for rva, p, c in pairs:
    pb = p.tobytes()
    cb = c.tobytes()
    for off in range(0, 4096, FIXED):
        pb16 = pb[off:off + FIXED]
        cb16 = cb[off:off + FIXED]
        if pb16 in block_map:
            dup_p += 1
            if block_map[pb16] == cb16:
                same_c += 1
        else:
            block_map[pb16] = cb16
print(f"ECB test: duplicate plaintext16 blocks {dup_p}, "
      f"same-cipher {same_c} "
      f"({'ECB-like' if dup_p and same_c / dup_p > 0.3 else 'not ECB-fixed-key'})")
