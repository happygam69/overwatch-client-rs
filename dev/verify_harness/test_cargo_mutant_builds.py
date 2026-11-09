"""The cargo candidate path: real crate builds, mutant slots, no staleness.

The e2e test proves the ladder through the C (tier-1) candidate; this one
proves the shipped path's own machinery with real cargo builds of `ow-sim`:

  1. the baseline probe builds and answers the protocol;
  2. a mutant-style build (source_override + source_path, exactly what the
     mutation stage does) produces a binary that really carries the mutation;
  3. two mutants built CONCURRENTLY on two slots each keep their own binary -
     the slot pool must never let one mutant's build answer another's probe.

Uses `float_to_q12` / `scale_q12` (crates/ow-sim/src/fixed.rs): the input
set [2^19, 4096.0] walks both the saturating and the shift arm, so each
mutation below is observable in the dst pointee.

Skips itself (exit 0, one line) when cargo or the ow-sim crate is not on
this machine, so the suite stays runnable everywhere the C tests run.

Run:  python3 test_cargo_mutant_builds.py
"""

from __future__ import annotations

import os
import shutil
import sys
import tempfile
import threading

HERE = os.path.dirname(os.path.abspath(__file__))
HARNESS = os.path.dirname(HERE)
REPO_ROOT = os.path.dirname(HARNESS)
sys.path.insert(0, HARNESS)

from emu_oracle import ArgSpec  # noqa: E402

FIXED_RS = os.path.join(REPO_ROOT, "crates", "ow-sim", "src", "fixed.rs")

FAILURES = []


def check(name, cond, detail=""):
    if cond:
        print(f"  PASS {name}")
    else:
        print(f"  FAIL {name}  {detail}")
        FAILURES.append(name)


def q12_args(values):
    """One call per value: dst(4B) + src(4B) + count=1, like the unit's."""
    out = []
    for v in values:
        import struct

        out.append(
            [
                ArgSpec(kind="ptr", mem=b"\x00\x00\x00\x00"),
                ArgSpec(kind="ptr", mem=struct.pack("<f", v)),
                ArgSpec(kind="u64", bits=1),
            ]
        )
    return out


def dst_of(result):
    return result.pointee_after.get(0, b"").hex()


def main() -> int:
    if shutil.which("cargo") is None or not os.path.exists(FIXED_RS):
        print("cargo or crates/ow-sim not present; skipping (not tested here)")
        return 0

    import candidate_rust
    from candidate_rust import RustCandidate

    print("== verify_harness cargo candidate / mutant slot tests ==")
    src = open(FIXED_RS, encoding="utf-8").read()

    # 2^19 saturates (i32 0x7fff_ffff, LE "ffffff7f"); 4096.0 (exponent 139)
    # takes the shift arm ((0 + 0x0080_0000) << 1 = 0x0100_0000, LE "00000001").
    cases = q12_args([524288.0, 4096.0])

    work = tempfile.mkdtemp(prefix="cargo_mut_test_")
    try:
        base = RustCandidate(
            entry="float_to_q12",
            arg_kinds=["ptr", "ptr", "u64"],
            ret_kind="void",
            work_dir=os.path.join(work, "base"),
            loader="cargo",
        )
        try:
            base_out = [dst_of(base.call(a)) for a in cases]
            check(
                "cargo/baseline-saturates-and-shifts",
                base_out == ["ffffff7f", "00000001"],
                base_out,
            )
        finally:
            base.close()

        # Mutant A: the saturation constant. Mutant B: the shift base. Both
        # observable on the case set, and different from each other.
        mut_a = src.replace("0x7FFF_FFFF", "0x7FFF_FFFE")
        mut_b = src.replace("let shift = exponent - 138;", "let shift = exponent - 137;")
        check(
            "cargo/mutations-are-present-in-source",
            mut_a != src and mut_b != src and mut_a != mut_b,
        )

        candidate_rust.set_mutant_build_slots(2)
        built = {}

        def build(tag, override):
            mdir = os.path.join(work, tag)
            c = RustCandidate(
                entry="float_to_q12",
                arg_kinds=["ptr", "ptr", "u64"],
                ret_kind="void",
                work_dir=mdir,
                source_override=override,
                source_path=FIXED_RS,
                loader="cargo",
            )
            try:
                built[tag] = [dst_of(c.call(a)) for a in cases]
            finally:
                c.close()

        t1 = threading.Thread(target=build, args=("mutA", mut_a))
        t2 = threading.Thread(target=build, args=("mutB", mut_b))
        t1.start()
        t2.start()
        t1.join()
        t2.join()

        check(
            "cargo/mutant-a-carries-its-mutation",
            built.get("mutA") == ["feffff7f", "00000001"],
            built.get("mutA"),
        )
        check(
            "cargo/mutant-b-carries-its-mutation",
            built.get("mutB") == ["ffffff7f", "00000002"],
            built.get("mutB"),
        )
        check(
            "cargo/concurrent-slots-never-cross",
            built.get("mutA") != built.get("mutB") and built.get("mutA") != base_out,
            "two concurrent mutant builds returned one binary's answers",
        )
    finally:
        import shutil as _shutil

        _shutil.rmtree(work, ignore_errors=True)
        candidate_rust.MUTANT_BUILD_SLOTS = 4

    print()
    if FAILURES:
        print(f"gate: FAIL {FAILURES}")
        return 1
    print("gate: PASS - the cargo candidate and mutant slots behave as claimed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
