"""Build a clean analysis PE: the install's headers plus the recovered .text.

The recovered flat image (`module_offline.bin`) has valid decrypted `.text`
bytes but invalid PE data directories, so Ghidra's PE loader cannot import it
directly. The install's `Overwatch.exe` has valid headers and plaintext
`.rdata`/`.data`/`.pdata`, but its `.text` is encrypted on disk. This tool writes
the install's PE with only `.text` replaced by the recovered bytes: a valid PE
that imports cleanly and has exactly the client's real `.pdata` functions.

    python dev/patch_clean_pe.py
    python dev/patch_clean_pe.py --out dev/data/offline_recover/Overwatch_clean.exe
    python dev/patch_clean_pe.py --check

The output is game code, so the default is under the git-ignored
`dev/data/offline_recover/`; never commit it. The tool only reads the install
and the recovered image.
"""

from __future__ import annotations

import argparse
import hashlib
import pathlib
import struct

INSTALL = pathlib.Path(r"E:\Overwatch\_retail_\Overwatch.exe")
OFFLINE = pathlib.Path(__file__).resolve().parent / "data" / "offline_recover" / "module_offline.bin"
DEFAULT_OUT = pathlib.Path(__file__).resolve().parent / "data" / "offline_recover" / "Overwatch_clean.exe"


def sections(image: bytes) -> dict[str, dict[str, int]]:
    pe = struct.unpack_from("<I", image, 0x3C)[0]
    nsec = struct.unpack_from("<H", image, pe + 6)[0]
    opt = struct.unpack_from("<H", image, pe + 20)[0]
    s0 = pe + 24 + opt
    out = {}
    for i in range(nsec):
        o = s0 + 40 * i
        name = image[o : o + 8].split(b"\x00")[0].decode("latin-1")
        vsz, va, rsz, ro = struct.unpack_from("<IIII", image, o + 8)
        out[name] = {"va": va, "vsz": vsz, "raw": ro, "rsz": rsz}
    return out


def patch(install: bytes, offline: bytes) -> bytes:
    sec = sections(install)
    text = sec[".text"]
    raw, rsz, rva = text["raw"], text["rsz"], text["va"]
    if raw + rsz > len(install) or rva + rsz > len(offline):
        raise SystemExit("section lies outside the install or the recovered image")
    out = bytearray(install)
    out[raw : raw + rsz] = offline[rva : rva + rsz]
    return bytes(out)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--install", type=pathlib.Path, default=INSTALL)
    ap.add_argument("--offline", type=pathlib.Path, default=OFFLINE)
    ap.add_argument("--out", type=pathlib.Path, default=DEFAULT_OUT)
    ap.add_argument("--check", action="store_true", help="compare an existing output")
    args = ap.parse_args()

    if not args.install.is_file():
        raise SystemExit(f"install not found: {args.install}")
    if not args.offline.is_file():
        raise SystemExit(f"recovered image not found: {args.offline}")
    install = args.install.read_bytes()
    offline = args.offline.read_bytes()
    sec = sections(install)[".text"]
    recovered = offline[sec["va"] : sec["va"] + sec["rsz"]]
    want = hashlib.sha256(recovered).hexdigest()

    if args.check:
        if not args.out.is_file():
            print(f"missing {args.out}")
            return 1
        got = hashlib.sha256(args.out.read_bytes()[sec["raw"] : sec["raw"] + sec["rsz"]]).hexdigest()
        print(f"clean PE .text sha256 {got}")
        print(f"recovered .text sha256 {want}")
        return 0 if got == want else 1

    data = patch(install, offline)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(data)
    print(f"wrote {args.out} ({len(data)} bytes)")
    print(f"recovered .text sha256 {want}")
    print(f"patched   .text sha256 "
          f"{hashlib.sha256(data[sec['raw'] : sec['raw'] + sec['rsz']]).hexdigest()}")
    print("import with -noanalysis, then run DisassemblePdataFunctions.java")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
