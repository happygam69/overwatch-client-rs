"""Build the per-page keystream artifact for Overwatch.exe.

For each page of the uninstalled on-disk Overwatch.exe whose plaintext we
hold in any standing capture image, X = C ^ P is the page's deterministic
keystream (verified equal across two independent captures).

Outputs (D:/ow-dumps/ only - captures stay outside the repo):
  pagepak/keystream.npy   memmap uint8 [npages, 4096]; 0 row = unknown
  pagepak/keystream_meta.json  coverage numbers and provenance
Registers how much of .text is covered - the pages we can decrypt
without ever seeing their plaintext from a capture.
"""
import json
import os

import numpy as np

EID = r"D:\ow-dumps"

disk = np.frombuffer(open(r"E:\Overwatch\_retail_\Overwatch.exe", "rb").read(),
                     np.uint8)
TEXT_VA, TEXT_RAW, TEXT_SZ = 0x1000, 0x400, 0x2f94913
n_total = (min(len(disk), TEXT_VA + TEXT_SZ) + 4095) >> 12

img_paths = {
    "module_after_diff.bin": r"D:\ow-dumps\module_after_diff.bin",
    "module_v2full.bin": r"D:\ow-dumps\module_v2full.bin",
}

ks = np.zeros((n_total, 4096), np.uint8)
have = np.zeros(n_total, bool)
per_source = {}

for name, path in img_paths.items():
    img = np.frombuffer(open(path, "rb").read(), np.uint8)
    n_img = min(len(img), TEXT_VA + TEXT_SZ) >> 12
    n_new = 0
    for pg in range(1, n_img):
        if have[pg]:
            continue
        p = img[pg * 4096:(pg + 1) * 4096]
        if (p == 0).all():
            continue
        rva = pg << 12
        fo = TEXT_RAW + (rva - TEXT_VA)
        c = disk[fo:fo + 4096]
        X = p ^ c
        ks[pg] = X
        have[pg] = True
        n_new += 1
    per_source[name] = n_new

covered = int(have.sum())
text_total = TEXT_SZ >> 12
os.makedirs(os.path.join(EID, "pagepak"), exist_ok=True)
np.save(os.path.join(EID, "pagepak", "keystream.npy"), ks)
meta = {
    "note": ("X_rva = C_rva ^ P_rva keystream per 4 KB page of "
             "Overwatch.exe; C = on-disk encrypted bytes, P = warm image "
             "plaintext; verified equal across two independent captures "
             "(5714 pages); pages not in the table were never seen "
             "decrypted in any capture"),
    "pages_ks_total": n_total,
    "text_pages": text_total,
    "pages_covered": covered,
    "coverage_fraction": round(covered / text_total, 4),
    "added_per_image": per_source,
    "images": list(img_paths),
    "disk_layout": {"text_va": TEXT_VA, "text_raw": TEXT_RAW,
                    "text_size": TEXT_SZ},
}
with open(os.path.join(EID, "pagepak", "keystream_meta.json"), "w") as fh:
    json.dump(meta, fh, indent=1)
print(json.dumps(meta, indent=1))
