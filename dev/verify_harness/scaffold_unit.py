"""Scaffold a verification unit from an address.

Pipeline (the same one the protocol document describes, mechanised):

    address -> .pdata extent -> Ghidra decompile -> unit JSON template

The output is a **template**, not a claim. It lands in
`dev/verify_harness/units/scaffolds/` by default, which `cli.py all` does not
scan, so an unfinished scaffold can never turn the gate green by accident.
Fill in the real argument surface, fixture and oracle address, then move it to
`units/` and run the harness - the harness decides whether it is verified.

The decompile is fetched through the running Ghidra bridge (`dev/gh.py`,
program `Overwatch.exe`). If the bridge is not up the script fails loudly: a
hand-written body would put a human in the trust path and make the unit
circular.

Usage:
    python dev/verify_harness/scaffold_unit.py 0x142069c60 --name OrthonormalizeCameraBasis
    python dev/verify_harness/scaffold_unit.py 0x7ff7c6d89c60 --name ... --state-base 0x... --state-image ...
"""

from __future__ import annotations

import argparse
import json
import os
import re
import struct
import subprocess
import sys
from dataclasses import dataclass
from typing import Optional, Tuple

HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
LOCAL_IMAGE = os.path.join(
    os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
    "dev",
    "data",
    "offline_recover",
    "module_offline.bin",
)
# Canonical: the repo-local copy. The D:\ow-dumps path is an old spelling of
# the same bytes, kept as a fallback for units written before the move.
DEFAULT_IMAGE = LOCAL_IMAGE
FALLBACK_IMAGE = r"D:\ow-dumps\offline_recover\module_offline.bin"
EXE_BASE = 0x140000000
V2B_BASE = 0x7FF7C4D20000
DECOMPILE_DIR = os.path.join(REPO_ROOT, "dev", "decompiles")
OUT_DIR = os.path.join(HERE, "units", "scaffolds")


@dataclass
class Extent:
    begin_rva: int
    end_rva: int
    run_info: int

    @property
    def size(self) -> int:
        return self.end_rva - self.begin_rva


def _sections(image: bytes):
    pe = struct.unpack_from("<I", image, 0x3C)[0]
    nsec = struct.unpack_from("<H", image, pe + 6)[0]
    opt = struct.unpack_from("<H", image, pe + 20)[0]
    s0 = pe + 24 + opt
    out = []
    for i in range(nsec):
        o = s0 + 40 * i
        name = image[o : o + 8].split(b"\x00")[0].decode("latin-1")
        vsz, va, rsz, ro = struct.unpack_from("<IIII", image, o + 8)
        out.append((name, va, vsz, ro, rsz))
    return out


def pdata_extent(image_path: str, rva: int) -> Optional[Extent]:
    """The `.pdata` function range containing ``rva``, or None.

    The recovery image is flat: its file offset equals the RVA (the docs'
    ``offset = v2b - base`` rule). The PE section headers still carry the
    original on-disk raw pointers, so the flat image's section data is read at
    ``VirtualAddress``, not ``PointerToRawData``.
    """
    with open(image_path, "rb") as f:
        image = f.read()
    pdata = None
    for name, va, vsz, _ro, _rsz in _sections(image):
        if name == ".pdata":
            pdata = (va, vsz)
            break
    if pdata is None:
        return None
    ro, size = pdata
    raw = image[ro : ro + size]
    for i in range(len(raw) // 12):
        beg, end = struct.unpack_from("<II", raw, 12 * i)
        if beg <= rva < end:
            run_info = struct.unpack_from("<I", raw, 12 * i + 8)[0]
            return Extent(beg, end, run_info)
    return None


def to_rva(address: int) -> int:
    if address >= V2B_BASE:
        return address - V2B_BASE
    if address >= EXE_BASE:
        return address - EXE_BASE
    return address  # already an RVA


def to_v2b(address: int) -> int:
    return V2B_BASE + to_rva(address)


def to_exe(address: int) -> int:
    return EXE_BASE + to_rva(address)


def snake_name(name: str) -> str:
    s = re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", name)
    s = re.sub(r"[^A-Za-z0-9]+", "_", s)
    return s.strip("_").lower()


def fetch_decompile(exe_addr: int) -> Tuple[str, str]:
    """Return (signature, body) from the Ghidra bridge, or raise."""
    gh = os.path.join(REPO_ROOT, "dev", "gh.py")
    if not os.path.exists(gh):
        raise RuntimeError(f"Ghidra bridge helper missing: {gh}")
    body = json.dumps(
        {
            "program": "Overwatch.exe",
            "function": f"0x{exe_addr:x}",
            "fields": "signature,decompiled_code,entry_point,body_start,body_end",
        }
    )
    out = subprocess.run(
        [sys.executable, gh, "/get_functions", body, "--full"],
        capture_output=True,
        text=True,
        timeout=180,
    )
    if out.returncode != 0:
        raise RuntimeError(f"Ghidra bridge call failed: {out.stderr.strip()[:400]}")
    try:
        payload = json.loads(out.stdout)
    except json.JSONDecodeError as e:
        raise RuntimeError(f"Ghidra bridge returned non-JSON: {out.stdout[:400]}") from e
    if isinstance(payload, dict) and payload.get("error"):
        raise RuntimeError(f"Ghidra bridge error: {payload['error']}")
    fn = payload.get("function") if isinstance(payload, dict) else None
    if fn is None and isinstance(payload, dict) and "decompiled_code" in payload:
        fn = payload
    if not fn or not fn.get("decompiled_code"):
        raise RuntimeError(
            "Ghidra returned no decompiled body; is the Ghidra bridge up and "
            "Overwatch.exe open?"
        )
    return fn.get("signature", "undefined FUN(void)"), fn["decompiled_code"], fn


def _body_size(fn: dict, ext: Extent) -> int:
    """The decompiler's own body extent when it is coherent, else .pdata."""
    try:
        start = int(str(fn.get("body_start", "")), 16)
        end = int(str(fn.get("body_end", "")), 16)
        if 0 < end - start <= ext.size:
            return end - start
    except (TypeError, ValueError):
        pass
    return ext.size


def _guess_args(body_size: int) -> list:
    """A conservative template surface, not a claim about the ABI.

    Ghidra's decompiler frequently reports `undefined FUN(void)` for a function
    with an unrecovered prototype. The scaffold therefore offers one pointer
    named `this`, large enough for the function's own body (capped), and the
    notes say to replace it. A scaffolded unit is not runnable until a human
    reads the decompile.
    """
    return [{"name": "this", "kind": "ptr", "ptr_len": min(max(body_size, 16), 0x1000)}]


def build_unit(
    name: str,
    address: int,
    image: str = DEFAULT_IMAGE,
    state_base: Optional[int] = None,
    state_image: Optional[str] = None,
) -> dict:
    rva = to_rva(address)
    if not os.path.exists(image) and os.path.exists(LOCAL_IMAGE):
        image = LOCAL_IMAGE
    if not os.path.exists(image) and os.path.exists(FALLBACK_IMAGE):
        image = FALLBACK_IMAGE
    ext = pdata_extent(image, rva)
    if ext is None:
        raise RuntimeError(f"no .pdata range contains RVA {rva:#x}")
    exe_addr = EXE_BASE + rva
    signature, body, fn = fetch_decompile(exe_addr)
    body_size = _body_size(fn, ext)

    os.makedirs(DECOMPILE_DIR, exist_ok=True)
    c_path = os.path.join(DECOMPILE_DIR, f"{name}_0x{exe_addr:x}.c")
    with open(c_path, "w", encoding="utf-8") as f:
        f.write(
            f"// {name} @ 0x{exe_addr:x} (v2b 0x{to_v2b(address):x}), "
            f"extent 0x{ext.begin_rva:x}-0x{ext.end_rva:x}\n"
            f"// signature: {signature}\n"
            "// Fetched from the Ghidra bridge; do not hand-edit the body.\n\n"
            + body
        )

    unit = {
        "id": f"fn/{name}",
        "title": f"SCAFFOLD: {name} at 0x{exe_addr:x} ({body_size} bytes)",
        "address": {"v2b": f"0x{to_v2b(address):x}"},
        "image": image,
        "c_file": os.path.relpath(c_path, REPO_ROOT).replace("\\", "/"),
        "entry": snake_name(name),
        "candidateLoader": "cargo",
        "scaffold": True,
        "args": _guess_args(body_size),
        "returns": "void",
        "options": {
            "iterations": 200,
            "coverageThreshold": 85,
            "mutationCount": 12,
            "mutationKillRate": 0.8,
            "seed": 4241,
        },
        "notes": [
            "SCAFFOLD - not a verification claim. Fill in the real args, returns, argFactory and",
            "entry, then remove the scaffold flag and run the harness.",
            f".pdata extent: RVA 0x{ext.begin_rva:x}..0x{ext.end_rva:x} ({ext.size} bytes), "
            f"run_info 0x{ext.run_info:x}; decompiler body {body_size} bytes",
            f"decompile: {os.path.relpath(c_path, REPO_ROOT).replace(chr(92), '/')} (signature: {signature})",
            "Evidence class: read code from Overwatch.exe (current build, preferred base 0x140000000).",
        ],
    }
    if state_base is not None:
        if not state_image:
            raise RuntimeError("--state-base needs --state-image")
        st = {
            "base": f"0x{to_v2b(state_base):x}",
            "image": state_image,
        }
        unit["state"] = st
        unit["notes"].append(
            "State unit: the snapshot is preloaded at the declared base; pointer args "
            "with a `guest` address index it, and the whole region is compared after the call."
        )
    return unit


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("address", help="v2b, exe, or RVA address")
    ap.add_argument("--name", required=True, help="PascalCase function name")
    ap.add_argument("--image", default=DEFAULT_IMAGE, help="offline PE image")
    ap.add_argument("--out", default=OUT_DIR, help="output directory")
    ap.add_argument("--state-base", default=None, help="state snapshot guest base")
    ap.add_argument("--state-image", default=None, help="state snapshot file")
    args = ap.parse_args(argv)

    address = int(args.address, 0)
    try:
        unit = build_unit(
            args.name,
            address,
            image=args.image,
            state_base=int(args.state_base, 0) if args.state_base else None,
            state_image=args.state_image,
        )
    except RuntimeError as e:
        print(f"  SCAFFOLD FAIL: {e}")
        return 1

    os.makedirs(args.out, exist_ok=True)
    path = os.path.join(args.out, f"{unit['id'].replace('/', '-')}.json")
    with open(path, "w", encoding="utf-8") as f:
        json.dump(unit, f, indent=2)
        f.write("\n")
    print(f"  wrote {os.path.relpath(path, REPO_ROOT)}")
    print(f"  oracle {unit['address']['v2b']} ({unit['notes'][2]})")
    print("  next: read the decompile, set args/returns/argFactory, move to units/, run cli.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
