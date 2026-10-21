"""Test hash-based keystream constructions against the known page stream."""
import hashlib
import hmac
import struct

import numpy as np

KS = np.load(r"D:\ow-dumps\pagepak\keystream_current.npy", mmap_mode="r")
TEXT_VA = 0x1000

KEYS = {
    "build": bytes.fromhex("8a45b1bf46ec30890a691969298b6c38"),
    "keyring": bytes.fromhex("3dabb1bf65a3a797ccc6eff63c51b509"),
    "cdn": bytes.fromhex("5e6493c15acdcb2d94426880016e12a8"),
}


def first_page():
    for p in range(KS.shape[0]):
        page = np.asarray(KS[p])
        if page.any():
            return p, bytes(page[:64])
    raise SystemExit("no keystream")


def main():
    p, want = first_page()
    rva = TEXT_VA + p * 0x1000
    print(f"page {p} rva {rva:#x} want {want[:32].hex()}")
    fields = {
        "rva64le": struct.pack("<Q", rva),
        "rva64be": struct.pack(">Q", rva),
        "idx64le": struct.pack("<Q", p),
        "idx64be": struct.pack(">Q", p),
        "rva32le": struct.pack("<I", rva),
        "rva32be": struct.pack(">I", rva),
    }
    hits = 0
    algs = {
        "sha256": hashlib.sha256,
        "sha1": hashlib.sha1,
        "md5": hashlib.md5,
        "sha512": hashlib.sha512,
        "blake2s": hashlib.blake2s,
        "blake2b": hashlib.blake2b,
    }

    def block(fn, *parts):
        h = fn()
        for x in parts:
            h.update(x)
        return h.digest()

    for kname, key in KEYS.items():
        for fname, f in fields.items():
            for algname, alg in algs.items():
                for order in ("key_first", "field_first"):
                    for ctr in range(4):
                        c = struct.pack("<Q", ctr)
                        for cplace in ("end", "start"):
                            if order == "key_first":
                                parts = (key, f, c) if cplace == "end" else (key, c, f)
                            else:
                                parts = (f, c, key) if cplace == "end" else (c, f, key)
                            d = block(alg, *parts)
                            if d[:16] == want[:16]:
                                print(f"MATCH {algname} {kname} {fname} {order} {cplace} ctr {ctr}")
                                hits += 1
            # HMAC variants
            for algname in ("sha256", "sha1"):
                for ctr in range(2):
                    c = struct.pack("<Q", ctr)
                    d = hmac.new(key, f + c, algname).digest()
                    if d[:16] == want[:16]:
                        print(f"MATCH hmac-{algname} {kname} {fname} ctr {ctr}")
                        hits += 1
    print("hits:", hits)


if __name__ == "__main__":
    main()
