"""The harness's own gate: inject known defects, require detection.

Port of the Sandustry harness's selftest.js discipline. A verifier that
cannot fail is worse than none, because it launders unverified code as
verified. This injects realistic reconstruction defects into the one
currently-verified unit and requires the differential stage to catch every
single one. If any defect survives, the gate fails and no unit may be
called verified until it is fixed.

    python dev/verify_harness/cli.py selftest

The oracle is built ONCE and its results over the corpus are computed ONCE:
the corpus is fixed and the oracle is deterministic (that is stage 2's
claim), so every injected defect is compared against the same recorded
oracle outputs instead of re-emulating the image per defect.
"""

from __future__ import annotations

import os
import sys
import tempfile

sys.path.insert(0, os.path.dirname(__file__))

from emu_oracle import EmuOracle, bits_f32  # noqa: E402
from candidate_c import CCandidate  # noqa: E402
from gen import UnitArg, generate_args, focused_f32_edge_cases  # noqa: E402

# The pilot that is currently verified. The selftest mutates *this* source;
# when another unit becomes the verified anchor, point the selftest at it.
ANCHOR_UNIT = {
    "address_v2b": 0x7FF7C58B1EF0,
    "c_file": os.path.join(
        os.path.dirname(__file__), "..", "decompiles", "AnimTimeWrap_0x140b91ef0.c"
    ),
    "entry": "FUN_140b91ef0",
    "args": [UnitArg("range", "ptr", 16), UnitArg("t", "f32")],
    "ret_kind": "f32",
    "image": os.path.join(
        os.path.dirname(__file__), "..", "data", "offline_recover", "module_offline.bin"
    ),
}

# name -> (old substring, new substring). Each must be present in the anchor
# source; a missing pattern is a selftest failure, not a skip.
DEFECTS = [
    ("drop-wrap-flag", "& 1) != 0", "& 2) != 0"),
    ("wrap-always", "if ((*(uint *)((char *)param_1 + 12) & 1) != 0)", "if (1)"),
    ("clamp-never", "if ((*(uint *)((char *)param_1 + 12) & 1) != 0)", "if (0)"),
    ("period-is-min", "param_1[1]", "*param_1"),
    ("min-is-max", "param_2 = sse_min(param_2, fVar1);",
     "param_2 = sse_min(param_2, *param_1);"),
    ("wrap-add", "param_2 - (float)(int)(param_2 / fVar1) * fVar1",
     "param_2 + (float)(int)(param_2 / fVar1) * fVar1"),
    ("wrap-trunc-away", "(float)(int)(param_2 / fVar1)",
     "(float)((int)(param_2 / fVar1) + 1)"),
    ("minss-flipped", "sse_min(param_2, fVar1)", "sse_min(fVar1, param_2)"),
    ("maxss-min-max", "sse_max(param_2, *param_1)",
     "sse_max(param_2, fVar1)"),
    ("minss-comparison-reverse", "return (dst < src) ? dst : src;",
     "return (src < dst) ? dst : src;"),
]


def _corpus() -> list:
    specs = ANCHOR_UNIT["args"]
    cases = list(focused_f32_edge_cases(specs))
    for i in range(120):
        cases.append(generate_args(specs, 4241, i))
    return cases


def _oracle_expectations(oracle: EmuOracle, cases: list) -> list:
    """(ok, ret_bits) per case - the fixed reference every defect is
    compared against. Emulation is the expensive side; run it once."""
    out = []
    addr = ANCHOR_UNIT["address_v2b"]
    for args in cases:
        o = oracle.call(addr, args, ret_kind="f32")
        out.append((o.ok, o.ret_bits))
    return out


def _differential_detects(src: str, cases: list, expectations: list) -> bool:
    """Run the differential on `src` vs the recorded oracle results; True if
    any divergence."""
    try:
        cand = CCandidate(
            c_file=ANCHOR_UNIT["c_file"],
            entry=ANCHOR_UNIT["entry"],
            arg_kinds=["ptr", "f32"],
            ret_kind="f32",
            work_dir=tempfile.mkdtemp(prefix="selftest_"),
            source_override=src,
        )
    except RuntimeError:
        # A defect that breaks the build is still a defect the harness
        # cannot silently pass; treat as detected (the original verified,
        # the broken candidate cannot be run, so it cannot agree).
        return True
    try:
        import math

        for (o_ok, o_bits), args in zip(expectations, cases):
            c = cand.call(args)
            if not c.ok:
                return True
            if not o_ok:
                continue
            fa, fb = bits_f32(o_bits), bits_f32(c.ret_bits)
            if math.isnan(fa) and math.isnan(fb):
                continue
            if o_bits != c.ret_bits:
                return True
        return False
    finally:
        cand.close()


def main(argv=None) -> int:
    path = ANCHOR_UNIT["c_file"]
    if not os.path.exists(path):
        print(f"  SELFAIL: anchor source not found: {path}")
        return 1
    with open(path, "r", encoding="utf-8") as f:
        base = f.read()

    cases = _corpus()
    oracle = EmuOracle(ANCHOR_UNIT["image"])
    expectations = _oracle_expectations(oracle, cases)

    injected = 0
    detected = 0
    missed = []
    for name, old, new in DEFECTS:
        if old not in base:
            print(f"  SELFAIL: pattern for {name!r} not found in anchor source")
            return 1
        injected += 1
        src = base.replace(old, new, 1)
        if src == base:
            print(f"  SELFAIL: replacement for {name!r} was a no-op")
            return 1
        hit = _differential_detects(src, cases, expectations)
        if hit:
            detected += 1
            print(f"  detected: {name}")
        else:
            missed.append(name)
            print(f"  MISSED:   {name}")

    print()
    print(f"  injected defects: {injected}   detected: {detected}   missed: {len(missed)}")
    if missed:
        print(f"  gate: FAIL - the harness missed {missed}")
        return 1

    # ---- Tier-2 (rust probe) exercise --------------------------------------
    # The correct probe must agree with the oracle; the deliberately-broken
    # entry (the original Ghidra clamp bug) must be caught. This is the
    # selftest's proof that the tier-2 path has detection power before any
    # real Rust implementation depends on it.
    print("  rust probe:")
    rust_fail = _rust_probe_check(oracle, cases[:40], expectations[:40])
    if rust_fail:
        print(f"  gate: FAIL - {rust_fail}")
        return 1

    print("  gate: PASS - the verifier detects every injected defect")
    return 0


def _rust_probe_check(oracle: EmuOracle, cases: list, expectations: list):
    """Return None on success, or a failure description."""
    import math

    from candidate_rust import RustCandidate

    addr = ANCHOR_UNIT["address_v2b"]
    try:
        good = RustCandidate("anim_time_wrap", ["ptr", "f32"], "f32")
    except RuntimeError as e:
        return f"good probe failed to build: {e}"
    try:
        bad = RustCandidate("anim_time_wrap_broken", ["ptr", "f32"], "f32")
    except RuntimeError as e:
        good.close()
        return f"broken probe failed to build: {e}"

    try:
        missed_good = 0
        caught_bad = 0
        for (o_ok, o_bits), args in zip(expectations, cases):
            g = good.call(args)
            if not g.ok:
                return f"good probe fault: {g.fault}"
            if o_ok:
                fa, fb = bits_f32(o_bits), bits_f32(g.ret_bits)
                if not (math.isnan(fa) and math.isnan(fb)) and o_bits != g.ret_bits:
                    missed_good += 1
            b = bad.call(args)
            if b.ok and o_ok:
                fa, fb = bits_f32(o_bits), bits_f32(b.ret_bits)
                if not (math.isnan(fa) and math.isnan(fb)) and o_bits != b.ret_bits:
                    caught_bad += 1
        print(f"    good probe: {len(cases) - missed_good}/{len(cases)} cases agree")
        print(f"    broken probe: {caught_bad} divergences detected")
        if missed_good:
            return f"good rust probe diverged on {missed_good} cases"
        if caught_bad == 0:
            return "broken rust probe was NOT detected"
        return None
    finally:
        good.close()
        bad.close()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
