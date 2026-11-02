#!/usr/bin/env python3
"""Recover the node-type registry table from the image.

Every node type in the map scene is installed by a small registrar that looks
like this (0x1404f6e10 is node type 2):

    SUB   RSP, 0x48
    CALL  0x141cd5da0              ; some context object
    MOV   ECX, <type id>
    MOV   [RSP+0x30], RAX
    MOV   word [RSP+0x20], CX
    LEA   RDX, [RSP+0x20]          ; descriptor
    LEA   RCX, [<build fn>]
    MOV   [RSP+0x28], RCX
    LEA   RCX, [0x1441ba618]       ; the registry
    CALL  0x141c5ff40              ; registry add

Ghidra never created a function at 0x141c5ff40, so every one of these callers
is invisible in the project ("0 callers"). This script finds them by scanning
for the call itself and reading the two constants out of the call site.

It then answers the question the collision ingest needs: for each type, which
build function does it, and does that build call any of the collider
producers? The chunk consumer must fill a type-3 mesh source, so it will reach
one of them without going through a brush tree.

Usage:
  python dev/probe_node_types.py                # the whole table
  python dev/probe_node_types.py --ingest       # only candidates
"""

from __future__ import annotations

import argparse
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
IMAGE = ROOT / "dev" / "data" / "offline_recover" / "module_offline.bin"
BASE = 0x140000000
TEXT_LO, TEXT_HI = 0x140001000, 0x1433c4000

REGISTRY = 0x1441BA618
REGISTRAR = 0x141C5FF40

# Producers that every collider path uses.
INTEREST = {
    0x142CF2010: "MeshToColliderFaces",
    0x142CF23C0: "BrushTreeToMesh",
    0x142CB7F10: "ShapeEntryFromDescriptor",
    0x142139080: "CellU16Append",
    0x14213D0B0: "GeometryFinalize",
    0x142132CF0: "CollisionParamsCopy",
    0x142CB3940: "ShapeAddToBody",
    0x142CD4EF0: "ShapeAddToOwner",
    0x142CBCEE0: "RunCharacterControllerSweep",
    0x142CCC120: "ColliderMeshEmit",
    0x142CCA460: "ColliderFaceAppend",
}


def load() -> bytes:
    if not IMAGE.is_file():
        sys.exit(f"missing image: {IMAGE}")
    return IMAGE.read_bytes()


def at(blob: bytes, va: int) -> int:
    return va - BASE


def call_sites(blob: bytes, target: int) -> list[int]:
    """Addresses of `E8 rel32` instructions whose target is `target`."""
    out = []
    i = blob.find(b"\xe8")
    while i != -1:
        if i + 5 <= len(blob):
            va = BASE + i
            rel = struct.unpack_from("<i", blob, i + 1)[0]
            if TEXT_LO <= va < TEXT_HI and va + 5 + rel == target:
                out.append(va)
        i = blob.find(b"\xe8", i + 1)
    return out


def near(blob: bytes, site: int, back: int = 0x60) -> tuple[int | None, int | None]:
    """Read (type id, build fn) out of the registrar call's preceding bytes.

    The build fn is the LEA r64,[rip+rel] target that is not the registry;
    the type id is the last `MOV ECX, imm32` before the call.
    """
    start = max(0, at(blob, site) - back)
    end = at(blob, site)
    window = blob[start:end]

    build = None
    type_id = None
    i = 0
    while i < len(window) - 7:
        # MOV ECX, imm32 : B9
        if window[i] == 0xB9 and i + 5 <= len(window):
            type_id = struct.unpack_from("<I", window, i + 1)[0]
            i += 5
            continue
        # LEA r64, [rip+rel32] : REX.W 8D modrm=05
        if window[i] in (0x48, 0x4C) and window[i + 1] == 0x8D and window[i + 2] & 0xC7 == 0x05:
            rel = struct.unpack_from("<i", window, i + 3)[0]
            tgt = BASE + start + i + 7 + rel
            if tgt != REGISTRY and TEXT_LO <= tgt < TEXT_HI:
                build = tgt
            i += 7
            continue
        i += 1
    return type_id, build


def enclosing(builds: list[int], site: int) -> int | None:
    best = None
    for b in builds:
        if b <= site:
            best = b
        else:
            break
    return best


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ingest", action="store_true", help="only types whose build touches a collider producer")
    args = ap.parse_args()

    blob = load()
    sites = call_sites(blob, REGISTRAR)
    rows = []
    for s in sites:
        t, b = near(blob, s)
        if t is None or b is None:
            rows.append((None, None, s))
        else:
            rows.append((t, b, s))

    builds = sorted({b for _, b, _ in rows if b})
    calls: dict[int, list[str]] = {b: [] for b in builds}
    for target, name in INTEREST.items():
        for site in call_sites(blob, target):
            owner = enclosing(builds, site)
            if owner is not None:
                calls[owner].append(name)

    print(f"registrar 0x{REGISTRAR:x}: {len(sites)} call site(s)")
    print(f"{'type':>5}  {'build':>12}  producers")
    unresolved = 0
    for t, b, s in sorted(rows, key=lambda r: (r[0] is None, r[0] or 0)):
        if t is None or b is None:
            unresolved += 1
            print(f"{'?':>5}  {'?':>12}  call site 0x{s:x}")
            continue
        prod = sorted(set(calls.get(b, [])))
        if args.ingest and not prod:
            continue
        print(f"{t:>5}  0x{b:x}  {', '.join(prod) if prod else '-'}")
    if unresolved:
        print(f"\n{unresolved} call site(s) did not yield a (type, build) pair")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
