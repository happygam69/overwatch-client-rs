"""The five-stage verification ladder, native edition.

Port of the Sandustry harness's runner.js protocol to the Overwatch target:
the oracle is the real machine code emulated over a captured image, never a
decompile or a reimplementation. Each stage can invalidate the conclusions of
the next; the order is deliberate.

  1. load        both sides load and expose the declared surface
  2. determinism the oracle agrees with itself on identical inputs
  3. differential same inputs, every channel compared
  4. coverage    measured on the oracle, on the very runs that were compared
  5. mutation    the candidate is mutated; the differential must notice

A unit is `verified` only when all five pass. `inconclusive` is not a pass.
"""

from __future__ import annotations

import json
import math
import os
import shutil
import struct
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Sequence, Tuple

from emu_oracle import EmuOracle, ArgSpec, bits_f32, f32_bits
from gen import UnitArg, generate_args, focused_f32_edge_cases
from ledger import (
    HARNESS_VERSION,
    LedgerEntry,
    check_entry,
    load_ledger,
    record,
    sha256_bytes,
    sha256_file,
)
import mutate as mutate_mod

# The oracle image: the offline-recovered complete image (every .text page,
# `dev/offline_page_recover.py`), laid out with file offset == RVA. The v2b
# delta equals the exe RVA (v2b - 0x7ff7c4d20000 == exe - 0x140000000), so
# unit addresses stay in the v2b frame and the offset math is unchanged.
LOCAL_IMAGE = os.path.join(
    os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
    "dev",
    "data",
    "offline_recover",
    "module_offline.bin",
)
# The canonical copy is repo-local (gitignored); a one-time copy of the same
# bytes lived on the dumps drive, and old unit JSONs still cite it there.
DEFAULT_IMAGE = LOCAL_IMAGE
FALLBACK_IMAGE = r"D:\ow-dumps\offline_recover\module_offline.bin"
DEFAULT_BASE = 0x7FF7C4D20000


_WARNED_IMAGES: set = set()


def resolve_image(path: str) -> str:
    if os.path.exists(path):
        return path
    for alt in (FALLBACK_IMAGE, LOCAL_IMAGE):
        if alt != path and os.path.exists(alt):
            if path not in _WARNED_IMAGES:
                _WARNED_IMAGES.add(path)
                _warn(f"image {path} not found; using the same recovery image at {alt}")
            return alt
    return path

SHIM_PATH = os.path.join(os.path.dirname(__file__), "ghidra_shim.h")

# Mutation-stage budget. A mutant is broken code by construction, so an
# unbounded loop is an expected outcome, not a slow call: the mutant is killed
# at the first case that fails, and a timeout is a failure like any other.
# The default is >1000x the slowest observed non-hanging mutant call (see
# PERFORMANCE.md); the baseline candidate keeps DEFAULT_CALL_TIMEOUT.
DEFAULT_MUTATION_CALL_TIMEOUT = 10.0
# The pinned mutation corpus: the first 48 cases (declared specials first,
# then the focused edges). Do not lengthen - a mutant only has to be caught
# once.
MUTATION_CORPUS_CASES = 48
DEFAULT_MUTANT_JOBS = 4

# Unit descriptor keys this harness understands. A typo'd key ("argfactory",
# "candidateLoader ":"rust ") would otherwise be silently ignored and the
# unit would quietly verify with a weaker surface - warn loudly instead.
_KNOWN_UNIT_KEYS = {
    "id", "title", "address", "image", "c_file", "entry", "args",
    "returns", "options", "base", "knownEquivalentMutants",
    "acknowledgedGaps", "notes", "argFactory", "candidateLoader", "state",
    "scaffold", "candidateCrate", "candidateProbe", "suite", "startRegs",
}
_KNOWN_ARG_KEYS = {"name", "kind", "ptr_len"}
_KNOWN_OPTION_KEYS = {
    "iterations", "coverageThreshold", "mutationCount", "mutationKillRate",
    "seed", "allowedImageReads", "unreachableBranches", "mutationCallTimeout",
    # Rebuild the emulator for every corpus case. A callee that leaves state
    # behind (an allocator the arena reset does not restore, say) then cannot
    # make one case's answer depend on the previous one. Slower, and only for
    # units that need it.
    "freshOraclePerCase",
    # A unit-declared exact function extent, for leaf functions the image's
    # .pdata does not describe. Without it the sweep uses the 0x2000 ceiling
    # and would disassemble trailing jump/dispatch tables as code.
    "coverageWindow",
}
_ARG_KINDS = ("f32", "f64", "u64")
_RET_KINDS = ("f32", "f64", "u64", "vec4", "u8", "void")


@dataclass
class StateSpec:
    """A controlled memory snapshot for a stateful unit.

    The snapshot is written at ``base`` before every oracle call, the whole
    region is compared after the call (the state channel), and the region is
    restored before the next call. ``data`` is the snapshot bytes; the file it
    came from is part of the surfaces digest.
    """

    base: int
    data: bytes
    source: str = ""


@dataclass
class Unit:
    id: str
    title: str
    address_v2b: int
    c_file: str
    entry: str
    args: List[UnitArg]
    ret_kind: str  # f32 | f64 | u64 | void
    options: Dict[str, Any]
    image: str = DEFAULT_IMAGE
    base: int = DEFAULT_BASE
    known_equivalent_mutants: List[Dict[str, Any]] = field(default_factory=list)
    acknowledged_gaps: List[Dict[str, Any]] = field(default_factory=list)
    notes: List[str] = field(default_factory=list)
    # Optional dotted path to a fixture module exporting make_args(i) ->
    # List[ArgSpec]. When set, it replaces the generic generator for this
    # unit; structured inputs (linked structs) need it.
    arg_factory: Optional[str] = None
    # Candidate loader: "c" compiles the decompile (tier 1); "rust" runs
    # rust_probe.rs through the subprocess protocol (tier 2, legacy);
    # "cargo" builds crates/<crate>/src/bin/verify_probe.rs and runs the real
    # crate functions (tier 2, shipped path). `candidate_crate` and
    # `candidate_probe` select that crate/probe (default ow-sim).
    candidate_loader: str = "c"
    candidate_crate: str = ""
    candidate_probe: str = ""
    # v2b addresses of compiler literal-pool constants the decompile
    # already inlined (e.g. 1.0f, -0.0f for XORPS). Reads of these are
    # not "undeclared global state"; the candidate hardcodes the same
    # value. Declared per unit so the exception is visible, not silent.
    allowed_image_reads: List[int] = field(default_factory=list)
    # Inclusive ranges of allowed image reads, for state/global blocks. The
    # unit's own state region is added automatically.
    allowed_read_ranges: List[Tuple[int, int]] = field(default_factory=list)
    # Declared dead branch edges: (branch site, never-taken next address,
    # rationale), all in the unit's v2b frame. The coverage stage validates
    # each one (the site ran, exactly one edge was observed, and it is not the
    # declared-dead one) before taking it out of the denominator.
    unreachable_branches: List[Tuple[int, int, str]] = field(default_factory=list)
    # Preloaded state snapshot, if the unit is stateful.
    state: Optional[StateSpec] = None
    # Registers seeded after the normal argument placement. Values are
    # integers or "argN" (the resolved arena pointer of argument N). Used for
    # interior-start leaf blocks whose entry convention takes the pointer in a
    # non-argument register (e.g. RBX).
    start_regs: Dict[str, Any] = field(default_factory=dict)


@dataclass
class Divergence:
    iteration: int
    channel: str  # ret | mem | fault
    oracle: str
    candidate: str
    detail: str = ""


@dataclass
class MutantOutcome:
    """One mutant's result from the mutation worker pool.

    ``killed`` is the verdict; ``detail`` records why (the first diverging
    case and channel, including a timeout), and ``build_error`` is set when
    the mutant did not compile. A build failure caused by the mutated source
    is still killed, exactly as before, and counted in both numerator and
    denominator. ``infra`` marks a mutant the harness could not evaluate at
    all (a missing artifact, a transport failure, a timeout after a retry):
    that is not a kill and must make the unit inconclusive, never verified.
    """

    mutant: Any
    killed: bool
    detail: str = ""
    build_error: str = ""
    infra: bool = False


def _warn(msg: str) -> None:
    print(f"  WARNING: {msg}", file=sys.stderr)


def load_unit(path: str) -> Unit:
    with open(path, "r", encoding="utf-8") as f:
        d = json.load(f)
    unknown = sorted(set(d) - _KNOWN_UNIT_KEYS)
    if unknown:
        _warn(f"unit {d.get('id', path)}: unknown keys {unknown} are ignored (typo?)")
    try:
        args = []
        for a in d["args"]:
            bad = sorted(set(a) - _KNOWN_ARG_KEYS)
            if bad:
                _warn(f"unit {d.get('id', path)}: arg {a.get('name')!r} has unknown keys {bad}")
            kind = a["kind"]
            if not (kind in _ARG_KINDS or kind.startswith("ptr")):
                raise ValueError(
                    f"unknown arg kind {kind!r} (expected f32/f64/u64 or ptr)"
                )
            args.append(UnitArg(**{k: a[k] for k in ("name", "kind") if k in a},
                                 ptr_len=a.get("ptr_len", 0)))
        ret_kind = d.get("returns", "f32")
        if ret_kind not in _RET_KINDS:
            raise ValueError(f"unknown returns {ret_kind!r} (expected one of {_RET_KINDS})")
        v2b_raw = d["address"]["v2b"]
        address_v2b = int(v2b_raw, 16)
    except (KeyError, ValueError, TypeError) as e:
        raise ValueError(f"unit {path}: malformed descriptor: {e}") from e
    # Unit paths are repo-relative; resolve against the repo root (two levels
    # up from dev/verify_harness/).
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    def abspath(p: str) -> str:
        return p if os.path.isabs(p) else os.path.join(repo_root, p)
    base_raw = d.get("base", DEFAULT_BASE)
    base = int(base_raw, 16) if isinstance(base_raw, str) else int(base_raw)
    # c_file is tier-1 only; rust units carry candidateLoader and omit it.
    c_file = d.get("c_file", "")
    opts = d.get("options", {})
    unknown_opts = sorted(set(opts) - _KNOWN_OPTION_KEYS)
    if unknown_opts:
        _warn(f"unit {d.get('id', path)}: unknown options {unknown_opts} are ignored (typo?)")
    allowed_reads_raw = opts.get("allowedImageReads", []) or []
    allowed_image_reads: List[int] = []
    allowed_read_ranges: List[Tuple[int, int]] = []
    try:
        for a in allowed_reads_raw:
            if isinstance(a, str) and "-" in a:
                lo, hi = a.split("-", 1)
                allowed_read_ranges.append((int(lo, 16), int(hi, 16)))
            elif isinstance(a, (list, tuple)) and len(a) == 2:
                allowed_read_ranges.append((int(a[0], 16) if isinstance(a[0], str) else int(a[0]),
                                            int(a[1], 16) if isinstance(a[1], str) else int(a[1])))
            else:
                allowed_image_reads.append(int(a, 16) if isinstance(a, str) else int(a))
    except (TypeError, ValueError) as e:
        raise ValueError(f"unit {path}: bad allowedImageReads: {e}") from e
    unreachable: List[Tuple[int, int, str]] = []
    for u in opts.get("unreachableBranches", []) or []:
        try:
            site = int(u["site"], 16) if isinstance(u["site"], str) else int(u["site"])
            edge = int(u["edge"], 16) if isinstance(u["edge"], str) else int(u["edge"])
            why = str(u["why"])
        except (KeyError, TypeError, ValueError) as e:
            raise ValueError(f"unit {path}: bad unreachableBranches entry: {e}") from e
        unreachable.append((site, edge, why))
    state = None
    st = d.get("state")
    if st is not None:
        try:
            sbase = int(st["base"], 16) if isinstance(st["base"], str) else int(st["base"])
            if "hex" in st:
                data = bytes.fromhex(st["hex"])
                source = "inline"
            else:
                spath = abspath(st["image"])
                with open(spath, "rb") as f:
                    data = f.read()
                source = spath
            if "size" in st:
                data = data[: int(st["size"])]
            if not data:
                raise ValueError("empty snapshot")
            state = StateSpec(base=sbase, data=data, source=source)
        except (KeyError, OSError, ValueError, TypeError) as e:
            raise ValueError(f"unit {path}: bad state block: {e}") from e
    return Unit(
        id=d["id"],
        title=d.get("title", ""),
        address_v2b=address_v2b,
        c_file=abspath(c_file) if c_file else "",
        entry=d["entry"],
        args=args,
        ret_kind=ret_kind,
        options=opts,
        image=resolve_image(abspath(d.get("image", DEFAULT_IMAGE))),
        base=base,
        known_equivalent_mutants=d.get("knownEquivalentMutants", []),
        acknowledged_gaps=d.get("acknowledgedGaps", []),
        notes=d.get("notes", []),
        arg_factory=d.get("argFactory"),
        candidate_loader=d.get("candidateLoader", "c"),
        candidate_crate=d.get("candidateCrate", ""),
        candidate_probe=d.get("candidateProbe", ""),
        start_regs=d.get("startRegs", {}),
        allowed_image_reads=allowed_image_reads,
        allowed_read_ranges=allowed_read_ranges,
        unreachable_branches=unreachable,
        state=state,
    )


def _args_sha(unit: Unit) -> str:
    """Digest of everything that determines the INPUT SIDE: the declared
    surface, the fixture module that builds the structured inputs, and (for
    tier-1) the shim header the candidate compiles against. Editing any of
    them invalidates the verdict - the fixture IS the corpus."""
    payload = json.dumps(
        [{"name": a.name, "kind": a.kind, "ptr_len": a.ptr_len} for a in unit.args],
        sort_keys=True,
    ).encode()
    parts = [sha256_bytes(payload)]
    if unit.arg_factory:
        fac_path = os.path.join(os.path.dirname(__file__), unit.arg_factory + ".py")
        if os.path.exists(fac_path):
            parts.append(sha256_file(fac_path))
        else:
            _warn(f"unit {unit.id}: argFactory module file not found for digest: {fac_path}")
    if unit.state is not None:
        payload = f"{unit.state.base:#x}:{len(unit.state.data)}".encode()
        parts.append(sha256_bytes(payload))
        if unit.state.source and unit.state.source != "inline":
            if os.path.exists(unit.state.source):
                parts.append(sha256_file(unit.state.source))
            else:
                _warn(f"unit {unit.id}: state snapshot file not found for digest")
        else:
            parts.append(sha256_bytes(unit.state.data))
    if unit.candidate_loader not in ("rust", "cargo") and os.path.exists(SHIM_PATH):
        parts.append(sha256_file(SHIM_PATH))
    return sha256_bytes(b"".join(p.encode() for p in parts))


def _options_sha(unit: Unit) -> str:
    return sha256_bytes(json.dumps(unit.options, sort_keys=True).encode())


def _bits_equal(a: int, b: int, ret_kind: str) -> bool:
    """The equivalence relation. NaN equal to NaN (f32 and f64: NaN payloads
    differ across codegen paths and carry no contract), +0 distinct from -0,
    everything else bit-exact. Printed normalisations: none silent."""
    if a == b:
        return True
    if ret_kind == "f32":
        fa, fb = bits_f32(a), bits_f32(b)
        return math.isnan(fa) and math.isnan(fb)
    if ret_kind == "f64":
        mask = (1 << 64) - 1
        fa = struct.unpack("<d", struct.pack("<Q", a & mask))[0]
        fb = struct.unpack("<d", struct.pack("<Q", b & mask))[0]
        return math.isnan(fa) and math.isnan(fb)
    if ret_kind == "vec4":
        # Four f32 lanes; each lane is compared exactly, including NaN
        # payloads. A position is state, not an arithmetic result: a payload
        # change is a divergence.
        return a == b
    if ret_kind == "u8":
        # The client contracts only AL (status byte) and leaves RAX's upper
        # bits holding unrelated arithmetic. Masking is the contract, not a
        # tolerance: the two values are compared bit-exactly inside the byte.
        return (a & 0xFF) == (b & 0xFF)
    return False


def _args_key(args: Sequence[ArgSpec]) -> str:
    """Stable identity of one input. The oracle is verified deterministic
    (stage 2) and stateless (arena/stack/registers reset per call), so a
    cached CallResult for this key IS the result of another call with it."""
    return json.dumps(
        [
            [a.kind, a.mem.hex(), repr(a.value), a.bits, a.arena_offset,
             getattr(a, "guest", None), list(a.relocs)]
            for a in args
        ]
    )


def _oracle_call_cached(
    oracle: EmuOracle,
    v2b_addr: int,
    args: Sequence[ArgSpec],
    ret_kind: str,
    cache: Dict[str, Any],
    start_regs: Optional[Dict[str, Any]] = None,
) -> Any:
    key = _args_key(args) + "|" + repr(sorted((start_regs or {}).items()))
    r = cache.get(key)
    if r is None:
        r = oracle.call(
            v2b_addr, args, ret_kind=ret_kind, start_regs=start_regs or {}
        )
        cache[key] = r
    return r


def _mem_channel(
    o_pointee_after: bytes,
    cand_pointee_after: Dict[int, bytes],
    args: Sequence[ArgSpec],
) -> Optional[Tuple[str, str, str]]:
    """Return (oracle_hex, candidate_hex, detail) when the memory channel
    disagrees, else None. Reloc slots hold the pointee's own base address
    (arena vs host), which necessarily differs between the two sides; mask
    them so only the data the callee wrote is compared."""
    cand_bytes = b"".join(cand_pointee_after[k] for k in sorted(cand_pointee_after))
    o_after = bytearray(o_pointee_after)
    c_after = bytearray(cand_bytes)
    pos = 0
    for a in args:
        if a.kind != "ptr":
            continue
        for off in a.relocs:
            s = pos + off
            o_after[s : s + 8] = b"\x00" * 8
            c_after[s : s + 8] = b"\x00" * 8
        pos += len(a.mem)
    if bytes(o_after) == bytes(c_after):
        return None
    diff_at = next(
        (i for i, (x, y) in enumerate(zip(o_after, c_after)) if x != y),
        min(len(o_after), len(c_after)),
    )
    return (
        bytes(o_after).hex(),
        bytes(c_after).hex(),
        f"pointee bytes differ after the call (first at byte {diff_at})",
    )


def _state_channel(
    o_after: Optional[bytes],
    c_after: Optional[bytes],
    unit: Unit,
    args: Sequence[ArgSpec],
) -> Optional[Tuple[str, str, str]]:
    """Compare the whole preloaded state region after the call.

    Declared reloc slots inside the state hold a pointer to the state itself;
    the oracle and the probe use different host addresses there, so those
    8-byte slots are masked exactly as the pointee channel masks them. Any
    other difference is a state divergence.
    """
    if unit.state is None:
        return None
    if o_after is None or c_after is None:
        return ("<missing>", "<missing>", "state channel missing on one side")
    o = bytearray(o_after)
    c = bytearray(c_after)
    if len(o) != len(c):
        return (o_after.hex(), c_after.hex(), "state region length differs")
    for a in args:
        if a.kind != "ptr" or getattr(a, "guest", None) is None:
            continue
        base_off = a.guest - unit.state.base
        for off in a.relocs:
            s = base_off + off
            if 0 <= s and s + 8 <= len(o):
                o[s : s + 8] = b"\x00" * 8
                c[s : s + 8] = b"\x00" * 8
    if bytes(o) == bytes(c):
        return None
    diff_at = next(
        (i for i, (x, y) in enumerate(zip(o, c)) if x != y),
        min(len(o), len(c)),
    )
    return (
        bytes(o).hex(),
        bytes(c).hex(),
        f"state bytes differ after the call (first at state+0x{diff_at:x})",
    )


def _read_allowed(unit: Unit, addr: int, size: int = 1) -> bool:
    if addr in unit.allowed_image_reads:
        return True
    if (
        unit.state is not None
        and unit.state.base <= addr
        and addr + size <= unit.state.base + len(unit.state.data)
    ):
        # The state region is declared by the unit's `state` block; every read
        # in it is part of the controlled struct, not an undeclared global.
        return True
    return any(lo <= addr and addr + size - 1 <= hi for lo, hi in unit.allowed_read_ranges)


def run_unit(
    unit: Unit,
    work_dir: Optional[str] = None,
    skip_mutation: bool = False,
    verbose: bool = True,
    mutant_workers: int = DEFAULT_MUTANT_JOBS,
    mutation_call_timeout: Optional[float] = None,
) -> LedgerEntry:
    def log(msg: str) -> None:
        if verbose:
            print(msg)

    opts = unit.options
    iterations = int(opts.get("iterations", 300))
    cov_gate = float(opts.get("coverageThreshold", 85.0))
    mutation_count = int(opts.get("mutationCount", 12))
    kill_gate = float(opts.get("mutationKillRate", 0.8))
    seed = int(opts.get("seed", 4241))
    fresh_oracle = bool(opts.get("freshOraclePerCase", False))
    cov_window = opts.get("coverageWindow")
    if cov_window is not None:
        cov_window = int(cov_window)
        if not (0 < cov_window <= 0x2000):
            raise ValueError(
                f"unit {unit.id}: coverageWindow must be 1..0x2000 bytes, "
                f"got {cov_window:#x}"
            )
    if mutation_call_timeout is None:
        mutation_call_timeout = float(
            opts.get("mutationCallTimeout", DEFAULT_MUTATION_CALL_TIMEOUT)
        )
    if not (mutation_call_timeout > 0):
        raise ValueError(
            f"unit {unit.id}: mutationCallTimeout must be positive, "
            f"got {mutation_call_timeout!r}"
        )

    oracle_sha = "unloaded"
    # The candidate artifact the ledger binds to: the decompile for tier 1,
    # the probe source for tier 2 rustc, the crate function source + crate
    # revision for the shipped cargo probe.
    candidate_sources: List[str] = []
    crate_rev = ""
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    # A cargo unit names its crate and probe; the defaults preserve the old
    # ow-sim units. These paths are used again by the mutation builds, so
    # resolve them once here.
    cand_crate = unit.candidate_crate or "ow-sim"
    cand_probe_rel = unit.candidate_probe or "crates/ow-sim/src/bin/verify_probe.rs"
    cand_crate_dir = os.path.join(repo_root, "crates", cand_crate)
    cand_probe = (
        cand_probe_rel
        if os.path.isabs(cand_probe_rel)
        else os.path.join(repo_root, cand_probe_rel)
    )
    if unit.candidate_loader == "cargo":
        from candidate_rust import candidate_binding, entry_sources

        cand_sha, candidate_sources, crate_rev = candidate_binding(
            unit.entry, crate=cand_crate, probe=cand_probe
        )
        cand_path = os.path.join(repo_root, entry_sources(unit.entry, cand_crate)[0])
    elif unit.candidate_loader == "rust":
        from candidate_rust import PROBE_SRC

        cand_path = PROBE_SRC
        cand_sha = sha256_file(cand_path) if os.path.exists(cand_path) else "missing"
    else:
        cand_path = unit.c_file
        cand_sha = sha256_file(cand_path) if os.path.exists(cand_path) else "missing"
    opt_sha = _options_sha(unit)
    surf_sha = _args_sha(unit)

    def entry(status: str, **kw) -> LedgerEntry:
        kw_notes = kw.pop("notes", [])
        return LedgerEntry(
            unit_id=unit.id,
            status=status,
            oracle_sha256=oracle_sha,
            candidate_sha256=cand_sha,
            options_sha256=opt_sha,
            surfaces_sha256=surf_sha,
            harness_version=HARNESS_VERSION,
            equivalent_mutants=unit.known_equivalent_mutants,
            acknowledged_gaps=unit.acknowledged_gaps,
            notes=list(unit.notes) + list(kw_notes),
            candidate_sources=list(candidate_sources),
            crate_rev=crate_rev,
            **kw,
        )

    # ---- Stage 1: load ----------------------------------------------------
    log(f"[1/5] load {unit.id}")
    if not os.path.exists(unit.image):
        log(f"  FAIL: image not found: {unit.image}")
        return entry("inconclusive", notes=["image missing: " + unit.image])
    if unit.candidate_loader == "cargo":
        if not os.path.exists(cand_probe):
            log(f"  FAIL: cargo probe source not found: {cand_probe}")
            return entry("inconclusive", notes=[f"cargo probe missing: {cand_probe}"])
        if not os.path.isdir(cand_crate_dir):
            log(f"  FAIL: cargo crate not found: {cand_crate_dir}")
            return entry("inconclusive", notes=[f"cargo crate missing: {cand_crate_dir}"])
        if not os.path.exists(cand_path):
            log(f"  FAIL: candidate source not found: {cand_path}")
            return entry("inconclusive", notes=[f"candidate source missing: {cand_path}"])
    elif unit.candidate_loader == "rust":
        if not os.path.exists(cand_path):
            log(f"  FAIL: rust probe source not found: {cand_path}")
            return entry("inconclusive", notes=[f"rust probe missing: {cand_path}"])
    elif not os.path.exists(unit.c_file):
        log(f"  FAIL: decompile not found: {unit.c_file}")
        return entry("inconclusive", notes=["c_file missing: " + unit.c_file])

    oracle = EmuOracle(unit.image, base=unit.base)
    if unit.state is not None:
        try:
            oracle.set_state(unit.state.base, unit.state.data)
        except ValueError as e:
            log(f"  FAIL: state preload: {e}")
            return entry("inconclusive", notes=[f"state preload failed: {e}"])
    try:
        fn_bytes = oracle.load_function(unit.address_v2b)
    except ValueError as e:
        log(f"  FAIL: {e}")
        return entry("inconclusive", notes=[str(e)])
    oracle_sha = sha256_bytes(fn_bytes)
    log(f"  oracle bytes @ {unit.address_v2b:#x}: {len(fn_bytes)} (sha256 {oracle_sha[:12]}...)")

    # Fresh ledger check: a stale verdict is not a verdict.
    entries = load_ledger()
    prev = entries.get(unit.id)
    if prev is not None:
        freshness = check_entry(prev, oracle_sha, cand_sha, opt_sha, surf_sha)
        if freshness == "stale" and prev.status == "verified":
            log("  NOTE: previous verdict is stale (inputs or harness changed); re-verifying")

    own_workdir = work_dir is None
    work_dir = work_dir or tempfile.mkdtemp(prefix=f"verify_{unit.id.replace('/', '_')}_")
    candidate = None
    # Install the coverage tracker BEFORE the first oracle.call. Unicorn's
    # CODE hooks become unreliable if added after call() has cycled its
    # MEM hooks (the handle table is rebuilt); measuring coverage of the
    # compared runs requires the tracker to be in place from the start.
    from coverage import BranchTracker

    if cov_window is not None:
        log(f"  coverage window: {cov_window:#x} bytes (unit-declared function extent)")
    try:
        tracker = BranchTracker(oracle, unit.address_v2b, window=cov_window)
    except Exception as e:
        log(f"  INCONCLUSIVE: coverage tracking failed: {e}")
        return entry("inconclusive", notes=[f"coverage tracking failed: {e}"])
    try:
        if unit.candidate_loader in ("rust", "cargo"):
            from candidate_rust import RustCandidate

            candidate = RustCandidate(
                entry=unit.entry,
                arg_kinds=[a.kind if a.kind != "ptr" else "ptr" for a in unit.args],
                ret_kind=unit.ret_kind,
                work_dir=work_dir,
                loader=unit.candidate_loader,
                state_base=unit.state.base if unit.state is not None else None,
                state_bytes=unit.state.data if unit.state is not None else None,
                crate_dir=cand_crate_dir,
                cargo_probe=cand_probe,
            )
            log(f"  candidate {unit.candidate_loader} probe: {candidate.exe}")
        else:
            from candidate_c import CCandidate

            candidate = CCandidate(
                c_file=unit.c_file,
                entry=unit.entry,
                arg_kinds=[a.kind if a.kind != "ptr" else "ptr" for a in unit.args],
                ret_kind=unit.ret_kind,
                work_dir=work_dir,
            )
            log(f"  candidate compiled: {candidate.dll_path}")
    except RuntimeError as e:
        log(f"  FAIL: {e}")
        return entry("inconclusive", notes=[f"compile failed: {e}"])

    # Load the arg factory (if any) before any stage that builds inputs.
    factory = None
    if unit.arg_factory:
        import importlib

        mod = importlib.import_module(unit.arg_factory)
        factory = mod.make_args

    try:
        # ---- Stage 2: determinism ----------------------------------------
        log("[2/5] determinism")
        if factory is not None:
            det_args = factory(0)
        else:
            det_args = generate_args(unit.args, seed, 0)
        cache: Dict[str, Any] = {}
        tracker.begin_call()
        r1 = _oracle_call_cached(
            oracle, unit.address_v2b, det_args, unit.ret_kind, cache, unit.start_regs
        )
        # The second run must be a REAL second run - a cache hit would make
        # the determinism claim true by construction.
        tracker.begin_call()
        r2 = oracle.call(
            unit.address_v2b,
            det_args,
            ret_kind=unit.ret_kind,
            start_regs=unit.start_regs,
        )
        if not r1.ok or not r2.ok:
            log(f"  FAIL: oracle fault on determinism probe: {r1.fault or r2.fault}")
            return entry("invalid", notes=[f"oracle fault: {r1.fault or r2.fault}"])
        if r1.ret_bits != r2.ret_bits or r1.pointee_after != r2.pointee_after:
            log("  FAIL: oracle disagrees with itself on identical input")
            return entry("invalid", notes=["oracle nondeterministic on identical input"])
        if r1.state_after != r2.state_after:
            log("  FAIL: oracle state channel disagrees with itself on identical input")
            return entry("invalid", notes=["oracle state nondeterministic on identical input"])
        log("  ok")

        # ---- Stage 3: differential ---------------------------------------
        log(f"[3/5] differential ({iterations} iterations + edge pool)")
        divergences: List[Divergence] = []
        undeclared_reads: List[Tuple[int, int]] = []
        undeclared_writes: List[Tuple[int, int]] = []
        corpus_args: List[List[ArgSpec]] = []

        if factory is not None:
            # A structured unit's inputs come only from its factory; the generic
            # random corpus would build invalid linked structures.
            cases: List[List[ArgSpec]] = []
            for i in range(iterations + 64):
                cases.append(factory(i))
        else:
            cases = list(focused_f32_edge_cases(unit.args))
            for i in range(iterations):
                cases.append(generate_args(unit.args, seed, i))

        # Coverage is measured on THESE runs (stage 4 analyses the tracker;
        # no second pass over the corpus). The tracker was installed before
        # stage 2 so Unicorn's CODE hook is live for every oracle.call.

        # Compiler literal-pool constants the unit has declared (and the
        # decompile already inlined). A read of one of these is not
        # undeclared global state: the candidate hardcodes the same value.
        try:
            for idx, args in enumerate(cases):
                tracker.begin_call()
                if fresh_oracle:
                    call_oracle = EmuOracle(unit.image, base=unit.base)
                    if unit.state is not None:
                        call_oracle.set_state(unit.state.base, unit.state.data)
                    tracker.attach(call_oracle)
                    o = call_oracle.call(
                        unit.address_v2b,
                        args,
                        ret_kind=unit.ret_kind,
                        start_regs=unit.start_regs or {},
                    )
                else:
                    o = _oracle_call_cached(
                        oracle, unit.address_v2b, args, unit.ret_kind, cache, unit.start_regs
                    )
                if not o.ok:
                    log(f"  FAIL: oracle fault at iteration {idx}: {o.fault}")
                    return entry("inconclusive", notes=[f"oracle fault at {idx}: {o.fault}"])
                if o.reads_outside:
                    undeclared_reads.extend(
                        r for r in o.reads_outside if not _read_allowed(unit, r[0], r[1])
                    )
                if o.out_of_arena_writes:
                    undeclared_writes.extend(o.out_of_arena_writes)

                c = candidate.call(args)
                if not c.ok:
                    divergences.append(
                        Divergence(idx, "fault", "ok", f"candidate fault: {c.fault}")
                    )
                else:
                    if unit.ret_kind != "void" and not _bits_equal(
                        o.ret_bits, c.ret_bits, unit.ret_kind
                    ):
                        divergences.append(
                            Divergence(
                                idx,
                                "ret",
                                f"{o.ret_bits:#x} ({_fmt_ret(o.ret_bits, unit.ret_kind)})",
                                f"{c.ret_bits:#x} ({_fmt_ret(c.ret_bits, unit.ret_kind)})",
                            )
                        )
                    mem = _mem_channel(o.pointee_after, c.pointee_after, args)
                    if mem is not None:
                        divergences.append(Divergence(idx, "mem", mem[0], mem[1], mem[2]))
                    st = _state_channel(
                        getattr(o, "state_after", None),
                        getattr(c, "state_after", None),
                        unit,
                        args,
                    )
                    if st is not None:
                        divergences.append(Divergence(idx, "state", st[0], st[1], st[2]))
                corpus_args.append(args)
                if len(divergences) >= 3:
                    # Enough counterexamples to act on; stop.
                    break

            if undeclared_reads:
                uniq = sorted(set(undeclared_reads))[:8]
                msg = (
                    f"oracle reads outside declared args/stack at "
                    f"{', '.join(f'{a:#x}+{s}' for a, s in uniq)} - unit needs a "
                    "wider fixture or is not verifiable by emulation alone"
                )
                log(f"  INCONCLUSIVE: {msg}")
                return entry("inconclusive", notes=[msg])
            if undeclared_writes:
                uniq = sorted(set(undeclared_writes))[:8]
                msg = (
                    f"oracle writes outside the arena at "
                    f"{', '.join(f'{a:#x}+{s}' for a, s in uniq)}"
                )
                log(f"  INCONCLUSIVE: {msg}")
                return entry("inconclusive", notes=[msg])

            if divergences:
                log(f"  DIVERGENT: {len(divergences)} channel disagreements")
                for d in divergences[:5]:
                    log(f"    iter {d.iteration} [{d.channel}] oracle={d.oracle} cand={d.candidate}")
                    if d.detail:
                        log(f"      {d.detail}")
                return entry(
                    "divergent",
                    divergences=[d.__dict__ for d in divergences],
                )
            log(f"  ok ({len(cases)} cases, every channel)")

            # ---- Stage 4: coverage -------------------------------------------
            log("[4/5] coverage (oracle, both branch edges of the compared runs)")
            cov = tracker.result(unreachable=unit.unreachable_branches)
        finally:
            tracker.close()

        if cov.invalid_declarations:
            msg = "; ".join(cov.invalid_declarations[:4])
            log(f"  INCONCLUSIVE: unreachable-branch declaration does not hold: {msg}")
            return entry(
                "inconclusive",
                coverage=cov.coverage,
                notes=[f"invalid unreachableBranches declaration: {msg}"],
            )
        log(
            f"  branch coverage {cov.coverage:.1f}% ({cov.hit}/{cov.total} reachable "
            f"branches of {cov.discovered} sites)"
        )
        if cov.declared_unreachable:
            log(
                f"  {cov.declared_unreachable} branch site(s) declared with one dead "
                "edge; each site ran and the dead edge was never observed"
            )
        if cov.coverage < cov_gate:
            gaps = ", ".join(f"{a:#x}" for a in cov.missed[:8])
            log(
                f"  INCONCLUSIVE: coverage {cov.coverage:.1f}% < gate {cov_gate}%; "
                f"missed {gaps}"
            )
            return entry(
                "inconclusive",
                coverage=cov.coverage,
                notes=[f"coverage {cov.coverage:.1f}% < gate {cov_gate}%; missed {gaps}"],
            )

        # ---- Stage 5: mutation -------------------------------------------
        if skip_mutation:
            log("[5/5] mutation SKIPPED (not a verified path)")
            return entry(
                "inconclusive",
                coverage=cov.coverage,
                notes=["mutation skipped; a unit is not verified without it"],
            )

        log(f"[5/5] mutation ({mutation_count} mutants, kill gate {kill_gate:.0%})")
        with open(cand_path, "r", encoding="utf-8") as f:
            base_src = f.read()

        from candidate_rust import mutation_entry

        mutants = mutate_mod.generate_mutants(
            base_src,
            mutation_entry(unit.entry) if unit.candidate_loader == "cargo" else unit.entry,
            count=mutation_count,
            base_dir=os.path.dirname(cand_path) or ".",
        )
        active = [
            m
            for m in mutants
            if not any(
                m.kind == eq.get("kind") and m.line == eq.get("line")
                for eq in unit.known_equivalent_mutants
            )
        ]
        for m in mutants:
            if m not in active:
                log(f"  skip mutant {m.kind}@{m.line} (declared equivalent)")

        # Mutation corpus: a subset is enough to decide kill/survive. The
        # first 48 are the pinned specials (factories put discriminating cases
        # at the front) plus the focused edges; oracle results come from the
        # stage-3 cache - identical inputs were already executed and stage 2
        # proved the oracle agrees with itself. The results are gathered here,
        # in the main thread, because the emulator is not shared with the
        # mutant workers and a cache miss must not change that.
        mut_cases = cases[: min(len(cases), MUTATION_CORPUS_CASES)]
        mutation_oracle: List[Tuple[Any, Any]] = []
        for args in mut_cases:
            key = _args_key(args)
            o = cache.get(key)
            if o is None:  # defensive; every case ran in stage 3
                o = oracle.call(unit.address_v2b, args, ret_kind=unit.ret_kind)
                cache[key] = o
            mutation_oracle.append((args, o))

        # Each mutant is an independent probe subprocess and the oracle
        # results above are read-only, so build AND run each mutant in a
        # worker. `pool.map` keeps the report in mutant order; the verdicts
        # are per-mutant and do not depend on scheduling.
        def _probe(m) -> MutantOutcome:
            # A mutant the harness could not build, start or drive is
            # retried once with a fresh work dir and a doubled call timeout.
            # A second infrastructure failure is NOT a kill: the mutant was
            # never evaluated, so the unit cannot be verified on it.
            last_infra = ""
            for attempt in range(2):
                mdir = tempfile.mkdtemp(prefix="mutant_")
                mc = None
                try:
                    try:
                        if unit.candidate_loader in ("rust", "cargo"):
                            from candidate_rust import RustCandidate

                            mc = RustCandidate(
                                entry=unit.entry,
                                arg_kinds=[
                                    a.kind if a.kind != "ptr" else "ptr"
                                    for a in unit.args
                                ],
                                ret_kind=unit.ret_kind,
                                work_dir=mdir,
                                source_override=m.source,
                                source_path=cand_path
                                if unit.candidate_loader == "cargo"
                                else None,
                                loader=unit.candidate_loader,
                                state_base=unit.state.base
                                if unit.state is not None
                                else None,
                                state_bytes=unit.state.data
                                if unit.state is not None
                                else None,
                                call_timeout=mutation_call_timeout * (attempt + 1),
                                crate_dir=cand_crate_dir,
                                cargo_probe=cand_probe,
                            )
                        else:
                            from candidate_c import CCandidate

                            mc = CCandidate(
                                c_file=unit.c_file,
                                entry=unit.entry,
                                arg_kinds=[
                                    a.kind if a.kind != "ptr" else "ptr"
                                    for a in unit.args
                                ],
                                ret_kind=unit.ret_kind,
                                work_dir=mdir,
                                source_override=m.source,
                            )
                    except RuntimeError as e:
                        msg = str(e)
                        # A source that does not compile is a killed mutant
                        # (the compiler is the detector). Anything else is an
                        # infrastructure failure and gets the retry.
                        compile_failed = (
                            unit.candidate_loader == "c"
                            or "build failed" in msg
                            or "rustc" in msg
                        )
                        if compile_failed:
                            return MutantOutcome(m, killed=True, build_error=msg)
                        last_infra = msg
                        continue
                    evaluated = True
                    for idx, (args, o) in enumerate(mutation_oracle):
                        c = mc.call(args)
                        if not c.ok:
                            if getattr(c, "infra", False):
                                # A timeout is retried once with a doubled
                                # budget; a second timeout is a non-terminating
                                # mutant, which is a divergence (killed), not a
                                # harness failure. A closed pipe / missing
                                # artifact is a harness failure.
                                timed_out = "timed out" in (c.fault or "")
                                if timed_out and attempt == 1:
                                    return MutantOutcome(
                                        m,
                                        killed=True,
                                        detail=(
                                            f"case {idx}: no return within "
                                            f"{mutation_call_timeout * 2:g}s"
                                        ),
                                    )
                                last_infra = f"case {idx}: {c.fault}"
                                evaluated = False
                                break
                            # A fault is a divergence.
                            return MutantOutcome(
                                m,
                                killed=True,
                                detail=f"case {idx}: candidate fault: {c.fault}",
                            )
                        if unit.ret_kind != "void" and not _bits_equal(
                            o.ret_bits, c.ret_bits, unit.ret_kind
                        ):
                            return MutantOutcome(
                                m, killed=True, detail=f"case {idx}: return differs"
                            )
                        if _mem_channel(o.pointee_after, c.pointee_after, args) is not None:
                            return MutantOutcome(
                                m, killed=True, detail=f"case {idx}: pointee differs"
                            )
                        if (
                            _state_channel(
                                getattr(o, "state_after", None),
                                getattr(c, "state_after", None),
                                unit,
                                args,
                            )
                            is not None
                        ):
                            return MutantOutcome(
                                m, killed=True, detail=f"case {idx}: state differs"
                            )
                    if evaluated:
                        return MutantOutcome(m, killed=False)
                finally:
                    close = getattr(mc, "close", None)
                    if close is not None:
                        close()
                    shutil.rmtree(mdir, ignore_errors=True)
            return MutantOutcome(m, killed=False, infra=True, detail=last_infra)

        workers = max(1, min(mutant_workers, len(active)))
        log(
            f"  probing {len(active)} mutant(s) on {workers} worker(s), "
            f"mutation call timeout {mutation_call_timeout:g}s"
        )
        if workers > 1:
            with ThreadPoolExecutor(max_workers=workers) as pool:
                outcomes = list(pool.map(_probe, active))
        else:
            outcomes = [_probe(m) for m in active]

        killed = 0
        scored = 0
        survivors: List[Any] = []
        # A mutant that could not be built/started/driven was never evaluated.
        # Counting it as killed would inflate the kill rate and could turn an
        # untested unit into a false "verified". Report it and stop.
        infra = [r for r in outcomes if r.infra]
        if infra:
            for r in infra:
                log(
                    f"  mutant {r.mutant.kind}@{r.mutant.line} not evaluated: "
                    f"{r.detail}"
                )
            return entry(
                "inconclusive",
                coverage=cov.coverage,
                notes=[
                    f"{len(infra)} mutant(s) could not be evaluated "
                    f"(infrastructure, not a kill): {infra[0].detail}"
                ],
            )
        for res in outcomes:
            m = res.mutant
            scored += 1
            if res.build_error:
                # A mutant that does not compile is a killed mutant: the
                # harness cannot run a broken build, and the original still
                # verified, so the detection-power claim stands for the
                # mutants that do build. Count it in BOTH numerator and
                # denominator so the kill rate stays in [0, 1].
                log(
                    f"  mutant {m.kind}@{m.line} does not compile "
                    f"(counted killed): {res.build_error}"
                )
                killed += 1
            elif res.killed:
                killed += 1
                log(f"  mutant {m.kind}@{m.line} killed: {res.detail}")
            else:
                survivors.append(m)
                log(f"  SURVIVOR: {m.kind}@{m.line}: {m.excerpt[:60]}")

        kill_rate = (killed / scored) if scored else 0.0
        log(f"  kill rate {kill_rate:.0%} ({killed}/{scored})")
        if survivors:
            log("  survivors must be fixed or declared observationally equivalent")
            for s in survivors:
                log(f"    {s.kind}@{s.line}: {s.excerpt[:70]}")
        if kill_rate < kill_gate:
            return entry(
                "inconclusive",
                coverage=cov.coverage,
                mutation_kill_rate=kill_rate,
                mutants_killed=killed,
                mutants_total=scored,
                notes=[
                    f"mutation kill rate {kill_rate:.0%} < gate {kill_gate:.0%}; "
                    f"{len(survivors)} survivor(s)"
                ],
            )

        log("  VERIFIED")
        return entry(
            "verified",
            coverage=cov.coverage,
            mutation_kill_rate=kill_rate,
            mutants_killed=killed,
            mutants_total=scored,
        )
    finally:
        close = getattr(candidate, "close", None)
        if close is not None:
            close()
        if own_workdir:
            shutil.rmtree(work_dir, ignore_errors=True)


def _fmt_ret(bits: int, kind: str) -> str:
    try:
        if kind == "f32":
            return repr(bits_f32(bits))
        if kind == "f64":
            return repr(struct.unpack("<d", struct.pack("<Q", bits & (2**64 - 1)))[0])
        if kind == "vec4":
            return f"0x{bits:032x}"
        return str(bits)
    except Exception:
        return "?"


def run_and_record(unit: Unit, **kw) -> LedgerEntry:
    e = run_unit(unit, **kw)
    entries = load_ledger()
    record(entries, e)
    return e
