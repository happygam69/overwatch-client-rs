"""Test candidate keys/counters against the known page keystream.

The keystream is strong (BM ~ n/2), so we test standard schemes with the
install's own build keys from .build.info:
  Build Key 8a45b1bf46ec30890a691969298b6c38
  KeyRing   3dabb1bf65a3a797ccc6eff63c51b509

For each key and for each counter layout, compare the first 32 bytes of
the derived stream with the known keystream of several page indices.
"""
import struct
import sys

import numpy as np
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes

KS = np.load(r"D:\ow-dumps\pagepak\keystream_current.npy", mmap_mode="r")
TEXT_VA = 0x1000
TEXT_RAW = 0x400

KEYS = {
    "build": bytes.fromhex("8a45b1bf46ec30890a691969298b6c38"),
    "keyring": bytes.fromhex("3dabb1bf65a3a797ccc6eff63c51b509"),
    "cdn": bytes.fromhex("5e6493c15acdcb2d94426880016e12a8"),
}


def first_page():
    for p in range(KS.shape[0]):
        page = np.asarray(KS[p])
        if page.any():
            return p, page
    raise SystemExit("no keystream")


def aes_ecb(key, blocks):
    enc = Cipher(algorithms.AES(key), modes.ECB()).encryptor()
    return enc.update(blocks) + enc.finalize()


def ctr_layouts(page_index):
    rva = TEXT_VA + page_index * 0x1000
    raw = TEXT_RAW + page_index * 0x1000
    layouts = {}
    # 16-byte little/big-endian page index, RVA, file offset
    for name, v in (("index", page_index), ("rva", rva), ("raw", raw)):
        layouts[f"{name}_le"] = struct.pack("<QQ", v, 0)
        layouts[f"{name}_be"] = struct.pack(">QQ", v, 0)
        layouts[f"{name}_le32"] = struct.pack("<II", v & 0xFFFFFFFF, v >> 32)
        layouts[f"{name}_be32"] = struct.pack(">II", v >> 32, v & 0xFFFFFFFF)
    layouts["zero"] = b"\x00" * 16
    return layouts


def main():
    p, page = first_page()
    want = bytes(page[:32])
    print(f"page index {p} rva {TEXT_VA + p * 0x1000:#x}; want {want.hex()}")
    hits = 0
    for kname, key in KEYS.items():
        # CTR with various counter blocks
        for lname, block in ctr_layouts(p).items():
            try:
                enc = Cipher(algorithms.AES(key), modes.CTR(block)).encryptor()
                got = enc.update(b"\x00" * 32)
            except Exception as e:
                continue
            if got[:16] == want[:16] or got == want:
                print(f"MATCH aes-ctr {kname} {lname}: {got.hex()}")
                hits += 1
        # ECB counter (equivalent to BE counter)
        for lname, block in ctr_layouts(p).items():
            ctr = int.from_bytes(block, "little")
            blocks = b"".join(
                (ctr + i).to_bytes(16, "little") for i in range(2)
            )
            got = aes_ecb(key, blocks)
            if got == want:
                print(f"MATCH aes-ecb-ctr-le {kname} {lname}")
                hits += 1
            blocks = b"".join(
                (ctr + i).to_bytes(16, "big") for i in range(2)
            )
            got = aes_ecb(key, blocks)
            if got == want:
                print(f"MATCH aes-ecb-ctr-be {kname} {lname}")
                hits += 1
        # ChaCha20 with nonce layouts
        for lname, block in ctr_layouts(p).items():
            nonce = block[:16]
            try:
                enc = Cipher(
                    algorithms.ChaCha20(key + key[:16], nonce)
                    if len(key) == 16
                    else algorithms.ChaCha20(key, nonce),
                    None,
                ).encryptor()
                got = enc.update(b"\x00" * 32)
            except Exception:
                continue
            if got == want or got[:16] == want[:16]:
                print(f"MATCH chacha {kname} {lname}: {got.hex()}")
                hits += 1
    print("hits:", hits)


if __name__ == "__main__":
    main()
