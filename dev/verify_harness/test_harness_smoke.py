"""Smoke tests for the verify_harness fixes.

These build SYNTHETIC images (no game dump, no Windows needed) and prove the
correctness fixes at the heart of the harness:

  1. oracle basic call + clean-return detection
  2. step budget: an infinite loop is a FAULT, not ok=True garbage
  3. MS x64 ABI: entry RSP % 16 == 8 (movaps spills work)
  4. register state does not leak between oracle calls
  5. image reads are recorded as undeclared
  6. coverage: exact per-branch edge attribution (no global-target cheat)
  7. .pdata function bounds parsing
  8. gen: f64/u64 pools + focused pair sweep includes NaN/inf
  9. mutate: hex-literal boundary mutants
 10. ledger: harness version binding + tolerant from_json
 11. runner helpers: _bits_equal NaN rule, _mem_channel first-diff
 12. candidate_c: f64/u64 returns are NOT truncated to 32 bits (needs gcc)
 13. load_unit: unknown-key warning, kind validation

Run:  python3 tests/test_harness_smoke.py
"""

from __future__ import annotations

import json
import os
import struct
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
HARNESS = os.path.dirname(HERE)
sys.path.insert(0, HARNESS)

from emu_oracle import ArgSpec, EmuOracle  # noqa: E402
from coverage import BranchTracker, _pdata_function_bounds  # noqa: E402
from gen import F64_EDGE_POOL, U64_EDGE_VALUES, UnitArg, focused_f32_edge_cases, generate_args  # noqa: E402
from mutate import generate_mutants  # noqa: E402
from ledger import HARNESS_VERSION, LedgerEntry, check_entry  # noqa: E402
from runner import _bits_equal, _mem_channel, _args_key, load_unit  # noqa: E402

BASE = 0x7FF7C4D20000
FAILURES = []


def check(name, cond, detail=""):
    if cond:
        print(f"  PASS {name}")
    else:
        print(f"  FAIL {name}  {detail}")
        FAILURES.append(name)


def make_image(funcs: dict, size: int = 0x1000) -> str:
    """funcs: {offset: bytes}; returns a temp file path."""
    img = bytearray(size)
    for off, code in funcs.items():
        img[off : off + len(code)] = code
    fd, path = tempfile.mkstemp(prefix="owimg_", suffix=".bin")
    with os.fdopen(fd, "wb") as f:
        f.write(bytes(img))
    return path


def test_basic_call():
    path = make_image({0x100: bytes.fromhex("b801000000c3")})  # mov eax,1; ret
    o = EmuOracle(path, base=BASE)
    r = o.call(BASE + 0x100, [], ret_kind="u64")
    check("oracle/basic-call", r.ok and r.ret_bits == 1 and r.fault is None,
          f"ok={r.ok} bits={r.ret_bits:#x} fault={r.fault}")
    os.unlink(path)


def test_step_limit_infinite_loop():
    path = make_image({0x100: bytes.fromhex("ebfe")})  # jmp $-2
    o = EmuOracle(path, base=BASE)
    r = o.call(BASE + 0x100, [], ret_kind="u64", steps=50_000)
    check("oracle/infinite-loop-is-fault",
          (not r.ok) and r.fault is not None and "no clean return" in r.fault,
          f"ok={r.ok} fault={r.fault}")
    os.unlink(path)


def test_abi_stack_alignment():
    # push rax; sub rsp,0x20; movaps [rsp],xmm0; add rsp,0x20; pop rax; ret
    # movaps needs RSP 16-aligned after the push, which holds only when the
    # entry RSP%16 == 8 (MS x64). The old 16-aligned entry made this #GP.
    path = make_image({0x100: bytes.fromhex("504883ec200f2904244883c42058c3")})
    o = EmuOracle(path, base=BASE)
    r = o.call(BASE + 0x100, [], ret_kind="u64")
    check("oracle/msx64-entry-alignment", r.ok and r.fault is None,
          f"ok={r.ok} fault={r.fault}")
    os.unlink(path)


def test_register_state_leak():
    path = make_image({
        0x100: bytes.fromhex("bb3412000089d8c3"),  # mov ebx,0x1234; mov eax,ebx; ret
        0x110: bytes.fromhex("89d8c3"),            # mov eax,ebx; ret
    })
    o = EmuOracle(path, base=BASE)
    r_a = o.call(BASE + 0x100, [], ret_kind="u64")
    r_b = o.call(BASE + 0x110, [], ret_kind="u64")
    check("oracle/registers-reset-between-calls",
          r_a.ok and r_a.ret_bits == 0x1234 and r_b.ok and r_b.ret_bits == 0,
          f"a={r_a.ret_bits:#x} b={r_b.ret_bits:#x}")
    os.unlink(path)


def test_image_read_detected():
    # mov eax, [rip+0] - a rip-relative read inside the image
    path = make_image({0x100: bytes.fromhex("8b0500000000c3")})
    o = EmuOracle(path, base=BASE)
    r = o.call(BASE + 0x100, [], ret_kind="u64")
    check("oracle/image-read-flagged",
          r.ok and len(r.reads_outside) == 1,
          f"ok={r.ok} reads={r.reads_outside}")
    os.unlink(path)


def test_state_mode():
    # mov eax,[rcx]; add eax,[rcx+4]; mov [rcx],eax; ret
    path = make_image({0x100: bytes.fromhex("8b010341048901c3")})
    o = EmuOracle(path, base=BASE)
    state_base = 0x0000_2000_1000_0000
    off = 0x100
    snapshot = bytearray(0x200)
    struct.pack_into("<II", snapshot, off, 3, 4)
    o.set_state(state_base, bytes(snapshot))
    args = [ArgSpec(kind="ptr", mem=struct.pack("<II", 3, 4), guest=state_base + off)]
    r1 = o.call(BASE + 0x100, args, ret_kind="u64")
    r2 = o.call(BASE + 0x100, args, ret_kind="u64")
    check("oracle/state-preload-and-compare",
          r1.ok and r1.ret_bits == 7
          and r1.state_after is not None
          and struct.unpack_from("<I", r1.state_after, off)[0] == 7,
          f"ok={r1.ok} ret={r1.ret_bits} state={r1.state_after and r1.state_after[off:off+4].hex()}")
    check("oracle/state-restored-between-calls",
          r2.ok and r2.ret_bits == 7
          and r2.state_before is not None
          and struct.unpack_from("<I", r2.state_before, off)[0] == 3,
          f"ok={r2.ok} before={r2.state_before and r2.state_before[off:off+4].hex()}")
    os.unlink(path)


def test_state_unit_parse(tmpdir):
    p = os.path.join(tmpdir, "state-unit.json")
    with open(p, "w", encoding="utf-8") as f:
        json.dump({
            "id": "fn/StateParse",
            "address": {"v2b": "0x7ff7c4d21000"},
            "entry": "x",
            "candidateLoader": "cargo",
            "args": [],
            "state": {"base": "0x200000000000", "hex": "01020304"},
        }, f)
    u = load_unit(p)
    check("unit/state-block-parsed",
          u.state is not None and u.state.base == 0x200000000000
          and u.state.data == b"\x01\x02\x03\x04",
          f"state={u.state}")


def _branch_image():
    code = (
        bytes.fromhex("8b01")            # +00 mov eax,[rcx]
        + bytes.fromhex("85c0")          # +02 test eax,eax
        + bytes.fromhex("7408")          # +04 jz +8 -> +0x0e
        + bytes.fromhex("b802000000")    # +06 mov eax,2
        + bytes.fromhex("c3")            # +0b ret
        + bytes.fromhex("9090")          # +0c nop nop
        + bytes.fromhex("b801000000")    # +0e mov eax,1
        + bytes.fromhex("c3")            # +13 ret
        + b"\xcc" * 8                    # +14 int3 padding
    )
    return make_image({0x100: code})


def test_coverage_exact_edges():
    path = _branch_image()
    o = EmuOracle(path, base=BASE)
    fn = BASE + 0x100
    zero = [ArgSpec(kind="ptr", mem=b"\x00\x00\x00\x00")]
    one = [ArgSpec(kind="ptr", mem=b"\x01\x00\x00\x00")]

    t = BranchTracker(o, fn)
    for a in (zero, one):
        t.begin_call()
        o.call(fn, a, ret_kind="u64")
    res = t.result()
    t.close()
    check("coverage/both-edges-100pct",
          res.total == 1 and res.hit == 1 and res.coverage == 100.0,
          f"{res.hit}/{res.total} missed={[hex(m) for m in res.missed]}")

    t2 = BranchTracker(o, fn)
    t2.begin_call()
    o.call(fn, zero, ret_kind="u64")  # taken edge only
    res2 = t2.result()
    t2.close()
    check("coverage/one-edge-is-a-miss",
          res2.total == 1 and res2.hit == 0 and len(res2.missed) == 1,
          f"{res2.hit}/{res2.total}")

    # Verify the per-branch edge set contains exactly the two real
    # next-addresses (fallthrough + taken target), nothing else.
    t3 = BranchTracker(o, fn)
    for a in (zero, one):
        t3.begin_call()
        o.call(fn, a, ret_kind="u64")
    edges = t3.edges[fn + 0x04]
    t3.close()
    check("coverage/exact-edge-set",
          edges == {fn + 0x06, fn + 0x0E},
          f"edges={sorted(hex(e) for e in edges)}")
    os.unlink(path)


def test_coverage_unreachable_declaration():
    path = _branch_image()
    o = EmuOracle(path, base=BASE)
    fn = BASE + 0x100
    zero = [ArgSpec(kind="ptr", mem=b"\x00\x00\x00\x00")]
    one = [ArgSpec(kind="ptr", mem=b"\x01\x00\x00\x00")]
    site = fn + 0x04
    fall = fn + 0x06
    target = fn + 0x0E

    def run(calls, decls):
        t = BranchTracker(o, fn)
        for a in calls:
            t.begin_call()
            o.call(fn, a, ret_kind="u64")
        res = t.result(unreachable=decls)
        t.close()
        return res

    r = run([zero], [(site, fall, "fall-through never taken on this corpus")])
    check("coverage/declared-dead-edge-excluded",
          r.hit == 0 and r.total == 0 and r.coverage == 100.0
          and r.declared_unreachable == 1 and not r.invalid_declarations,
          f"hit={r.hit} total={r.total} declared={r.declared_unreachable}")

    # Declaring the edge that WAS observed is a lie, not a declaration.
    r = run([zero, one], [(site, target, "wrong edge")])
    check("coverage/observed-dead-edge-rejected",
          r.invalid_declarations and not r.declared_unreachable,
          f"{r.invalid_declarations}")

    # A site that never ran cannot be excluded: that would hide an
    # unreached branch instead of documenting a dead edge.
    r = run([], [(site, fall, "never ran")])
    check("coverage/unexecuted-site-rejected", r.invalid_declarations,
          f"{r.invalid_declarations}")

    # A non-branch address and an empty rationale are both refused.
    r = run([zero], [(fn + 0x02, target, "not a branch")])
    check("coverage/non-branch-site-rejected", r.invalid_declarations,
          f"{r.invalid_declarations}")
    r = run([zero], [(site, fall, "   ")])
    check("coverage/empty-rationale-rejected", r.invalid_declarations,
          f"{r.invalid_declarations}")
    os.unlink(path)


def test_pdata_bounds():
    img = bytearray(0x400)
    img[0:2] = b"MZ"
    struct.pack_into("<I", img, 0x3C, 0x40)
    img[0x40:0x44] = b"PE\x00\x00"
    # COFF: machine, nsections=1, timestamp, symtab, nsyms, optsize=0, chars
    img[0x44:0x58] = struct.pack("<HHIIIHH", 0x8664, 1, 0, 0, 0, 0, 0x22)
    # section: name, vsize=12, va=0x200, ...
    img[0x58:0x80] = b".pdata\x00\x00" + struct.pack("<II", 12, 0x200) + b"\x00" * 16
    # RUNTIME_FUNCTION: begin=0x100, end=0x114, unwind=0
    img[0x200:0x20C] = struct.pack("<III", 0x100, 0x114, 0)
    b = _pdata_function_bounds(bytes(img), 0x100)
    check("pdata/function-bounds", b == (0x100, 0x114), f"bounds={b}")
    b2 = _pdata_function_bounds(bytes(img), 0x300)
    check("pdata/no-coverage-returns-none", b2 is None, f"bounds={b2}")
    b3 = _pdata_function_bounds(b"\x00" * 0x400, 0x100)
    check("pdata/non-pe-returns-none", b3 is None, f"bounds={b3}")


def test_gen_pools():
    specs = [UnitArg("x", "f64"), UnitArg("h", "u64")]
    a1 = generate_args(specs, 4241, 7)
    a2 = generate_args(specs, 4241, 7)
    check("gen/deterministic", a1[0].value == a2[0].value and a1[1].bits == a2[1].bits)
    check("gen/f64-pool-has-double-extremes",
          any(abs(v) >= 1e308 for v in F64_EDGE_POOL)
          and 2.2250738585072014e-308 in F64_EDGE_POOL
          and 5e-324 in F64_EDGE_POOL)
    check("gen/u64-pool-has-high-bits",
          any(v > 0xFFFFFFFF for v in U64_EDGE_VALUES))
    draws = [generate_args(specs, 4241, i)[1].bits for i in range(300)]
    check("gen/u64-draws-reach-64bit", any(v > 0xFFFFFFFF for v in draws))

    pair_cases = focused_f32_edge_cases([UnitArg("a", "f32"), UnitArg("b", "f32")])
    has_nan_pair = any(
        c[0].value != c[0].value and c[1].value == 1.0 for c in pair_cases
    )
    check("gen/pair-sweep-includes-nan-x", has_nan_pair,
          f"n_cases={len(pair_cases)}")


def test_mutate_hex_boundary():
    src = "unsigned int FUN_x(unsigned int count) {\n    if (count > 0x3f) {\n        return 1;\n    }\n    return 0;\n}\n"
    mutants = generate_mutants(src, "FUN_x", count=12)
    hex_plus = [m for m in mutants if m.kind == "boundary +1" and "0x40" in m.source]
    hex_minus = [m for m in mutants if m.kind == "boundary -1" and "0x3e" in m.source]
    check("mutate/hex-boundary-generated", bool(hex_plus) and bool(hex_minus),
          f"kinds={sorted({m.kind for m in mutants})}")


def test_mutate_bool_negate():
    src = (
        "fn FUN_x(flag: bool) {\n"
        "    if flag {\n"
        "        return 1;\n"
        "    }\n"
        "    if !flag {\n"
        "        return 2;\n"
        "    }\n"
        "    return 0;\n"
        "}\n"
    )
    mutants = generate_mutants(src, "FUN_x", count=12)
    negated = [m for m in mutants if m.kind == "bool-negate" and "if !flag {" in m.source]
    unnegated = [m for m in mutants if m.kind == "bool-negate" and "if flag {" in m.source]
    check("mutate/bool-negate-generated", len(mutants) == 2 and bool(negated) and bool(unnegated),
          f"kinds={sorted({m.kind for m in mutants})} n={len(mutants)}")


def test_ledger_version_binding():
    e = LedgerEntry(
        unit_id="fn/t", status="verified", oracle_sha256="a", candidate_sha256="b",
        options_sha256="c", surfaces_sha256="d", harness_version="1",
    )
    fresh = check_entry(e, "a", "b", "c", "d")
    check("ledger/old-harness-version-is-stale", fresh == "stale", f"got {fresh}")
    e2 = LedgerEntry(
        unit_id="fn/t", status="verified", oracle_sha256="a", candidate_sha256="b",
        options_sha256="c", surfaces_sha256="d", harness_version=HARNESS_VERSION,
    )
    check("ledger/current-version-fresh",
          check_entry(e2, "a", "b", "c", "d") == "fresh")
    e3 = LedgerEntry.from_json({**e2.to_json(), "future_key": 1})
    check("ledger/tolerant-from-json", e3.unit_id == "fn/t")


def test_runner_helpers():
    check("bits/f32-nan-equal",
          _bits_equal(0x7FC00000, 0xFFC00001, "f32"))
    check("bits/f32-zero-sign-distinct",
          not _bits_equal(0x00000000, 0x80000000, "f32"))
    check("bits/f64-nan-equal",
          _bits_equal(0x7FF8000000000000, 0xFFF8000000000042, "f64"))
    check("bits/u64-exact",
          not _bits_equal(1, 2, "u64"))
    # The client's AL-only status contract: only the low byte is compared, so
    # RAX's unrelated upper bits cannot cause a false divergence.
    check("bits/u8-masks-upper-bits",
          _bits_equal(0x1234, 0xAB34, "u8"))
    check("bits/u8-low-byte-distinct",
          not _bits_equal(0x1234, 0xAB35, "u8"))

    args = [ArgSpec(kind="ptr", mem=bytes(range(16)), relocs=(8,))]
    o_after = bytes(range(16))
    cand = {0: bytes(range(16))}
    check("mem/reloc-slot-masked",
          _mem_channel(o_after, cand, args) is None)
    cand2 = {0: bytes([9]) + bytes(range(1, 16))}
    d = _mem_channel(o_after, cand2, args)
    check("mem/first-diff-offset", d is not None and "byte 0" in d[2], str(d))

    k1 = _args_key([ArgSpec(kind="f32", value=1.0)])
    k2 = _args_key([ArgSpec(kind="f32", value=1.0)])
    k3 = _args_key([ArgSpec(kind="f32", value=2.0)])
    check("argskey/stable-and-discriminating", k1 == k2 and k1 != k3)


def test_candidate_c_f64_u64():
    import shutil

    if shutil.which("gcc") is None:
        print("  SKIP candidate_c (no gcc)")
        return
    from candidate_c import CCandidate

    src64 = (
        "double FUN_x(double a) { return a * 2.0; }\n"
        "unsigned long long FUN_y(unsigned long long a) { return a + 0x100000000ULL; }\n"
    )
    with tempfile.TemporaryDirectory() as td:
        cpath = os.path.join(td, "body.c")
        with open(cpath, "w") as f:
            f.write(src64)
        cand = CCandidate(
            c_file=cpath, entry="FUN_x", arg_kinds=["f64"], ret_kind="f64",
            work_dir=os.path.join(td, "w1"),
        )
        want = struct.unpack("<Q", struct.pack("<d", 0.2))[0]
        r = cand.call([ArgSpec(kind="f64", value=0.1)])
        check("candidate_c/f64-retbits-not-truncated",
              r.ok and r.ret_bits == want,
              f"got {r.ret_bits:#x} want {want:#x}")
        cand.close()

        cand2 = CCandidate(
            c_file=cpath, entry="FUN_y", arg_kinds=["u64"], ret_kind="u64",
            work_dir=os.path.join(td, "w2"),
        )
        r2 = cand2.call([ArgSpec(kind="u64", bits=1)])
        check("candidate_c/u64-retbits-not-truncated",
              r2.ok and r2.ret_bits == 0x100000001,
              f"got {r2.ret_bits:#x}")
        cand2.close()


def test_load_unit_validation(tmpdir):
    unit = {
        "id": "fn/t",
        "adress": {"v2b": "0x1000"},  # typo on purpose
        "address": {"v2b": "0x7ff7c58b1ef0"},
        "entry": "FUN_x",
        "args": [{"name": "p", "kind": "ptr", "ptr_len": 16}],
        "returns": "f32",
    }
    p = os.path.join(tmpdir, "u.json")
    with open(p, "w") as f:
        json.dump(unit, f)
    u = load_unit(p)  # must not raise; must warn
    check("load_unit/unknown-key-warns-but-loads", u.address_v2b == 0x7FF7C58B1EF0)
    unit["args"] = [{"name": "x", "kind": "bogus"}]
    with open(p, "w") as f:
        json.dump(unit, f)
    try:
        load_unit(p)
        check("load_unit/bad-kind-rejected", False, "no exception")
    except ValueError as e:
        check("load_unit/bad-kind-rejected", "bogus" in str(e), str(e))


def main() -> int:
    print("== verify_harness smoke tests ==")
    test_basic_call()
    test_step_limit_infinite_loop()
    test_abi_stack_alignment()
    test_register_state_leak()
    test_state_mode()
    test_image_read_detected()
    test_coverage_exact_edges()
    test_coverage_unreachable_declaration()
    test_pdata_bounds()
    test_gen_pools()
    test_mutate_hex_boundary()
    test_mutate_bool_negate()
    test_ledger_version_binding()
    test_runner_helpers()
    test_candidate_c_f64_u64()
    with tempfile.TemporaryDirectory() as td:
        test_load_unit_validation(td)
        test_state_unit_parse(td)
    print()
    if FAILURES:
        print(f"gate: FAIL ({len(FAILURES)}): {FAILURES}")
        return 1
    print("gate: PASS - all fixes behave as claimed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
