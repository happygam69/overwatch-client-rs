"""The harness's own gate: inject known defects, require detection.

Port of the Sandustry harness's selftest.js discipline. A verifier that
cannot fail is worse than none, because it launders unverified code as
verified. This injects realistic reconstruction defects into the one
currently-verified unit and requires the differential stage to catch every
single one. If any defect survives, the gate fails and no unit may be
called verified until it is fixed.

    python dev/verify_harness/cli.py selftest
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
    "image": r"D:\ow-dumps\offline_recover\module_offline.bin",
}

# name -> (old substring, new substring). Each must be present in the anchor
# source; a missing pattern is a selftest failure, not a skip.
DEFECTS = [
    ("drop-wrap-flag", "flag_bits & 1", "flag_bits & 2"),
    ("wrap-always", "if ((flag_bits & 1) != 0)", "if (1)"),
    ("clamp-never", "if ((flag_bits & 1) != 0)", "if (0)"),
    ("period-is-min", "param_1[1]", "*param_1"),
    ("min-is-max", "lo = *param_1", "lo = param_1[1]"),
    ("wrap-add", "param_2 - (float)(int)(param_2 / fVar1) * fVar1",
     "param_2 + (float)(int)(param_2 / fVar1) * fVar1"),
    ("wrap-trunc-away", "(float)(int)(param_2 / fVar1)",
     "(float)((int)(param_2 / fVar1) + 1)"),
    ("minss-flipped", "minss(param_2, fVar1)", "minss(fVar1, param_2)"),
    ("maxss-min-max", "maxss(minss(param_2, fVar1), lo)",
     "maxss(minss(param_2, lo), fVar1)"),
    ("minss-comparison-reverse", "return (a < b) ? a : b;",
     "return (b < a) ? a : b;"),
]


def _differential_detects(src: str) -> bool:
    """Run the differential on `src` vs the oracle; True if any divergence."""
    oracle = EmuOracle(ANCHOR_UNIT["image"])
    specs = ANCHOR_UNIT["args"]
    cases = list(focused_f32_edge_cases(specs))
    for i in range(120):
        cases.append(generate_args(specs, 4241, i))
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
    import math

    for args in cases:
        o = oracle.call(ANCHOR_UNIT["address_v2b"], args, ret_kind="f32")
        c = cand.call(args)
        if not c.ok:
            return True
        fa, fb = bits_f32(o.ret_bits), bits_f32(c.ret_bits)
        if math.isnan(fa) and math.isnan(fb):
            continue
        if o.ret_bits != c.ret_bits:
            return True
    return False


def main(argv=None) -> int:
    path = ANCHOR_UNIT["c_file"]
    with open(path, "r", encoding="utf-8") as f:
        base = f.read()

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
        hit = _differential_detects(src)
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
    rust_fail = _rust_probe_check()
    if rust_fail:
        print(f"  gate: FAIL - {rust_fail}")
        return 1

    print("  gate: PASS - the verifier detects every injected defect")
    return 0


def _rust_probe_check():
    """Return None on success, or a failure description."""
    import math

    from candidate_rust import RustCandidate
    from emu_oracle import EmuOracle, bits_f32

    oracle = EmuOracle(ANCHOR_UNIT["image"])
    specs = ANCHOR_UNIT["args"]
    cases = list(focused_f32_edge_cases(specs))
    for i in range(40):
        cases.append(generate_args(specs, 4241, i))

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
        for args in cases:
            o = oracle.call(ANCHOR_UNIT["address_v2b"], args, ret_kind="f32")
            g = good.call(args)
            if not g.ok:
                good.close()
                bad.close()
                return f"good probe fault: {g.fault}"
            fa, fb = bits_f32(o.ret_bits), bits_f32(g.ret_bits)
            if not (math.isnan(fa) and math.isnan(fb)) and o.ret_bits != g.ret_bits:
                missed_good += 1
            b = bad.call(args)
            if b.ok:
                fa, fb = bits_f32(o.ret_bits), bits_f32(b.ret_bits)
                if not (math.isnan(fa) and math.isnan(fb)) and o.ret_bits != b.ret_bits:
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
