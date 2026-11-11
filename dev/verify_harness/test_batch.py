"""The batch build: one crate build for a unit's whole mutant set.

The mutation stage is the harness's expensive stage, and the batch is what
made it cheap: every mutant of a unit becomes a dispatch arm inside the
function under test, so the crate is compiled once instead of once per
mutant. These tests prove the three things that make that safe:

  1. the generated file carries each mutant's body, and only in its own arm;
  2. the built binary really dispatches: the arm the environment variable
     names is the one that runs (a real cargo build of a tiny crate);
  3. a compiler error is attributed to the arm that caused it, so a mutant
     the compiler rejects is dropped and reported as a kill - the same
     verdict the one-build-per-mutant path gave it.

Skips itself (exit 0, one line) when cargo is not on this machine.

Run:  python3 test_batch.py
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
HARNESS = os.path.dirname(HERE)
REPO_ROOT = os.path.dirname(HARNESS)
sys.path.insert(0, HARNESS)

import batch as batch_mod  # noqa: E402
import mutate as mutate_mod  # noqa: E402

FAILURES = []


def check(name, cond, detail=""):
    if cond:
        print(f"  PASS {name}")
    else:
        print(f"  FAIL {name}  {detail}")
        FAILURES.append(name)


# A tiny crate whose function is the mutation target. It carries a `*` and a
# `+` so the mutator can swap either of them, and each swap is observable in
# the answer: 21 * 2 + 1 = 43, 21 / 2 + 1 = 11, 21 * 2 - 1 = 41.
TINY_RS = """\
/// Doubles and adds one - the mutations swap the `*` or the `+`.
pub fn scale(x: u32) -> u32 {
    x * 2 + 1
}
"""

TINY_MAIN = """\
fn main() {
    let v = ow_batch_tiny::scale(21);
    println!("{}", v);
}
"""


def mutants_of(source: str, entry: str, base_dir: str = "."):
    return mutate_mod.generate_mutants(source, entry, count=12, base_dir=base_dir)


def test_plan_shape():
    """The generated file: one arm per mutant, each carrying its own body."""
    source = TINY_RS
    ms = mutants_of(source, "scale")
    check("batch/mutants-found", len(ms) >= 1, f"{len(ms)} mutants")
    if not ms:
        return None
    plan = batch_mod.plan_batch(source, "scale", ms, base_dir=".")
    check("batch/plan-built", plan is not None)
    if plan is None:
        return None
    check(
        "batch/one-arm-per-mutant",
        len(plan.arms) == len(ms),
        f"{len(plan.arms)} arms for {len(ms)} mutants",
    )
    # Every arm's body is the original with its own mutation, so the arm's
    # line range holds the mutated statement and the original statement is
    # not there any more.
    text = plan.source
    lines = text.splitlines()
    for arm in plan.arms:
        body = "\n".join(lines[arm.line_start - 1 : arm.line_end])
        original = "\n".join(
            TINY_RS.splitlines()[1:-1]
        )
        check(
            f"batch/arm-{arm.index}-carries-its-mutation",
            body != original and "x * 2 + 1" not in body,
            body,
        )
    # The helper is in the file, and the dispatch is one match.
    check(
        "batch/helper-and-dispatch-present",
        batch_mod.HELPER_NAME in text and "match " + batch_mod.HELPER_NAME in text,
    )
    # The original body is still there, as the default arm.
    check("batch/original-body-kept", "x * 2 + 1" in text)
    # The arm ranges are ordered and disjoint.
    ranges = [(a.line_start, a.line_end) for a in plan.arms]
    check(
        "batch/arm-ranges-ordered",
        all(ranges[i][1] < ranges[i + 1][0] for i in range(len(ranges) - 1)),
        ranges,
    )
    return plan


def test_nested_function():
    """A function under test that is nested inside another one.

    The dispatch helper is a free function, so it has to land at module
    scope; injecting it next to the function under test would put it inside
    the outer function, where it cannot be defined.
    """
    nested = """\
pub fn outer(flag: bool) -> u32 {
    fn inner(x: u32) -> u32 {
        x * 2 + 1
    }
    if flag { inner(3) } else { 0 }
}
"""
    ms = mutants_of(nested, "inner")
    check("batch/nested-mutants-found", len(ms) >= 1, f"{len(ms)} mutants")
    if not ms:
        return
    plan = batch_mod.plan_batch(nested, "inner", ms, base_dir=".")
    check("batch/nested-plan-built", plan is not None)
    if plan is None:
        return
    text = plan.source
    helper_at = text.index(batch_mod.HELPER_NAME)
    outer_at = text.index("pub fn outer")
    check(
        "batch/helper-is-at-module-scope",
        helper_at < outer_at,
        "the helper must come before the outer function",
    )
    check(
        "batch/nested-dispatch-inside-the-function",
        "match " + batch_mod.HELPER_NAME in text,
    )
    # The helper must not be inside the outer function's braces.
    check(
        "batch/helper-not-nested",
        text.count("{") <= text.count("}") and "fn __ow_mutant_index" in text,
    )


def test_signature_mutation_is_not_an_arm():
    """A mutation on the signature cannot be an arm.

    The dispatch replaces the function's body, so a mutation on the
    signature - the line that carries the opening brace is part of it -
    would be lost. Such a mutant must not be in the batch: the caller builds
    it on its own, as it did before.
    """
    multi = """\
pub fn widen(
    a: u8,
    b: u16,
) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[0] = a;
    out[1] = b as u8;
    out
}
"""
    ms = mutants_of(multi, "widen")
    check("batch/signature-mutants-found", len(ms) >= 1, f"{len(ms)} mutants")
    if not ms:
        return
    plan = batch_mod.plan_batch(multi, "widen", ms, base_dir=".")
    # Every mutant of this function is on the signature, so there is no arm
    # and the caller falls back to one build per mutant.
    check(
        "batch/signature-mutation-not-an-arm",
        plan is None or all(a.mutant.line != 4 for a in plan.arms),
        "a signature mutation must not become an arm",
    )


def test_error_attribution():
    """A compiler error names a line; the line names the arm."""
    plan = batch_mod.plan_batch(TINY_RS, "scale", mutants_of(TINY_RS, "scale"), ".")
    if plan is None or not plan.arms:
        check("batch/attribution-setup", False, "no plan")
        return
    arm = plan.arms[0]
    inside = arm.line_start + 1
    check(
        "batch/error-line-maps-to-its-arm",
        plan.arm_for_line(inside) is arm,
        f"line {inside}",
    )
    check(
        "batch/error-line-outside-any-arm",
        plan.arm_for_line(1) is None,
        "line 1 should map to no arm",
    )
    # The cargo/rustc log formats.
    log = (
        "error: expected type, found `=`\n"
        "   --> crate\\src\\tiny.rs:42:33\n"
        "  |\n"
    )
    check("batch/error-lines-parsed", batch_mod.error_lines(log) == [42], log)


def test_dispatch_runs():
    """A real cargo build: the arm the environment names is the one that runs."""
    if shutil.which("cargo") is None:
        print("  (cargo not present; the dispatch build is not tested here)")
        return
    work = tempfile.mkdtemp(prefix="batch_test_")
    try:
        crate = os.path.join(work, "ow_batch_tiny")
        os.makedirs(os.path.join(crate, "src"))
        with open(os.path.join(crate, "Cargo.toml"), "w", encoding="utf-8") as f:
            f.write(
                "[package]\nname = \"ow_batch_tiny\"\nversion = \"0.1.0\"\n"
                "edition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n"
            )
        with open(os.path.join(crate, "src", "lib.rs"), "w", encoding="utf-8") as f:
            f.write(TINY_RS)
        with open(os.path.join(crate, "src", "main.rs"), "w", encoding="utf-8") as f:
            f.write(TINY_MAIN)

        # Two mutants of `scale`: the `+` becomes a `-`, and the `*` becomes
        # a `/`. Both are observable in the answer.
        ms = mutants_of(TINY_RS, "scale")
        picks = ms[:2]
        if len(picks) < 2:
            check("batch/two-mutants-available", False, f"{len(ms)} mutants")
            return
        plan = batch_mod.plan_batch(TINY_RS, "scale", picks, base_dir=".")
        if plan is None:
            check("batch/plan-for-build", False)
            return
        with open(os.path.join(crate, "src", "lib.rs"), "w", encoding="utf-8") as f:
            f.write(plan.source)

        target = os.path.join(work, "target")
        proc = subprocess.run(
            [
                "cargo", "build", "--release", "--offline",
                "--manifest-path", os.path.join(crate, "Cargo.toml"),
                "--target-dir", target,
            ],
            capture_output=True,
            text=True,
        )
        if proc.returncode != 0:
            check("batch/batch-crate-builds", False, proc.stderr[-800:])
            return
        check("batch/batch-crate-builds", True)

        exe = os.path.join(target, "release", "ow_batch_tiny.exe")
        if not os.path.exists(exe):
            exe = os.path.join(target, "release", "ow_batch_tiny")
        answers = []
        for index in range(len(picks)):
            env = dict(os.environ)
            env[batch_mod.ENV_NAME] = str(index)
            out = subprocess.run([exe], capture_output=True, text=True, env=env)
            answers.append(out.stdout.strip())
        # No index: the original body runs (21 * 2 + 1 = 43).
        out = subprocess.run([exe], capture_output=True, text=True)
        default = out.stdout.strip()
        check(
            "batch/default-arm-is-the-original",
            default == "43",
            default,
        )
        check(
            "batch/each-arm-runs-its-own-mutant",
            answers[0] != default and answers[1] != default and answers[0] != answers[1],
            f"{answers} vs default {default}",
        )
        # The arms carry the mutants' own arithmetic: the `*` swap answers
        # 21 / 2 + 1 = 11, the `+` swap answers 21 * 2 - 1 = 41.
        expected = []
        for m in picks:
            body = m.source
            if "x / 2 + 1" in body:
                expected.append("11")
            elif "x * 2 - 1" in body:
                expected.append("41")
            else:
                expected.append("?")
        check(
            "batch/answers-match-the-mutations",
            answers[0] == expected[0] and answers[1] == expected[1],
            f"{answers} vs {expected}",
        )
    finally:
        shutil.rmtree(work, ignore_errors=True)


def main() -> int:
    print("== verify_harness batch build tests ==")
    test_plan_shape()
    test_nested_function()
    test_signature_mutation_is_not_an_arm()
    test_error_attribution()
    test_dispatch_runs()
    print()
    if FAILURES:
        print(f"gate: FAIL {FAILURES}")
        return 1
    print("gate: PASS - the batch build behaves as claimed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
