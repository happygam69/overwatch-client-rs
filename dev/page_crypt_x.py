"""Characterize the Overwatch.exe page-cipher transform from paired states.

For every 4 KB page where the shipped on-disk `Overwatch.exe` still holds
ciphertext and the standing warm image `module_after_diff.bin` holds
plaintext, compute X = C xor P and classify:
  - all-zero X        -> page was never encrypted in place (identity read;
                          loader maps decrypted already or content matches)
  - constant X        -> single-bytewise key (XOR)
  - period-k X        -> repeating-key XOR; k found by autocorrelation
  - else              -> block/stream cipher noise
Also reports X byte-entropy to spot pseudo-random keys, and whether the
period-1 pages share the SAME key across pages (position-keyed?).
Read-only; writes nothing.
"""
import numpy as np

C = np.frombuffer(open(r"E:\Overwatch\_retail_\Overwatch.exe", "rb").read(),
                  np.uint8)
P = np.frombuffer(open(r"D:\ow-dumps\module_after_diff.bin", "rb").read(),
                  np.uint8)

n = min(len(C), len(P)) >> 12
C = C[:n * 4096].reshape(n, 4096)
P = P[:n * 4096].reshape(n, 4096)

X = C ^ P
identical = (X == 0).all(axis=1)
print(f"pages where disk == warm (untransformed): {identical.sum()}/{n}")

diff_pages = np.flatnonzero(~identical)
print(f"encrypted-appearing pages: {len(diff_pages)}")
if not len(diff_pages):
    raise SystemExit("no transform pairs - nothing to characterize")

xd = X[diff_pages]
plain_C = C[diff_pages]
plain_P = P[diff_pages]

# non-zero-byte fraction of X per page (uniform-ish strong keys -> ~255/256)
nzfrac = (xd != 0).mean(axis=1)
# entropy of X bytes per page (uniform 8 bits => 8.0)
ent = np.zeros(len(xd))
for i, row in enumerate(xd):
    counts = np.bincount(row, minlength=256)
    pr = (counts / len(row))[counts > 0]
    ent[i] = -(pr * np.log2(pr)).sum()
print(f"X byte-entropy: mean {ent.mean():.2f} (uniform=8.0), "
      f"min {ent.min():.2f} max {ent.max():.2f}")
print(f"X non-zero-byte fraction: mean {nzfrac.mean():.3f}")

# period detection (autocorrelation on a few pages)
for idx in [0, len(xd) // 2, len(xd) - 1]:
    row = xd[idx]
    best = None
    for k in range(1, 513):
        m = (row[:-k] == row[k:]).mean()
        if m > 0.9:
            best = (k, m)
            break
    print(f"page {diff_pages[idx]:#x}: period guess "
          f"{best if best else 'none (block-ish)'}")

# is the transform deterministic across pages? compare two pages X rows
if len(xd) >= 2:
    frac = (xd[0] == xd[1]).mean()
    print(f"X[0]==X[1] byte agreement: {frac:.4f} (1.0 => one static key)")

# and the same transform across DIFFERENT pages of similar content? sanity:
# compare plain pages byte-diff structure
both_plain = 0
for i in range(min(len(xd), 32)):
    if (plain_P[i] != 0).any() and (plain_C[i] != plain_P[i]).all():
        both_plain += 1
print(f"sample sanity: {both_plain}/32 pairs have real F: content on both sides")

# decode-check: plaintext pages should decode() to real x86
# (first nonzero byte histogram of C vs P sampled pairs)
c0 = np.bincount(plain_C[0], minlength=256)
p0 = np.bincount(plain_P[0], minlength=256)
top_c = int(plain_C[0][(plain_C[0] != 0)].mean()) if 0 else 0
print(f"page {diff_pages[0]:#x} sample: C head {plain_C[0][:16].tolist()} "
      f"P head {plain_P[0][:16].tolist()}")
