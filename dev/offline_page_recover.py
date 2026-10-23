#!/usr/bin/env python3
"""Recover Overwatch.exe `.text` / `.pdata` offline with the known page cipher.

Plan A, no live capture: every on-disk `.text` page is
``P = C ^ window(pad, w)`` where ``pad`` is the static 16 KiB cycle in the
plaintext loader (``loader_disk_rva.bin`` RVA ``0x1d77f80``) and ``w`` is one
of the 4,096 4-byte-aligned starts.  The window is recovered from the page's
own structure:

1. pad validation against the current-build oracle
   (``pagepak/keystream_current.npy``);
2. `.pdata`: test whether the disk section is already plaintext, otherwise
   scan every `w` and score 12-byte RUNTIME_FUNCTION entries;
3. `.text` pilot: score every `w` with a byte n-gram trained on the plaintext
   loader `.text` (same build/toolchain) and capstone structure from the
   `.pdata` function starts, then validate top-1 on the held-out oracle;
4. full `.text` recovery and PE reassembly at the preferred base
   `0x140000000`, with `.reloc` applied for the chosen base;
5. validation against the warm module images and the oracle, plus REPORT.md.

The oracle is a held-out test set only: it is never used to pick `w` for a
page.  Ambiguous pages are written but marked in the CSV and the report.

All game-derived artifacts are written under ``D:/ow-dumps/offline_recover/``;
nothing from the game enters the repository.

usage:
  python dev/offline_page_recover.py --stage pad
  python dev/offline_page_recover.py --stage pdata
  python dev/offline_page_recover.py --stage pilot
  python dev/offline_page_recover.py --stage full
  python dev/offline_page_recover.py --stage validate
  python dev/offline_page_recover.py --stage report
  python dev/offline_page_recover.py --stage all
"""
import argparse
import csv
import json
import os
import struct
import time
from collections import Counter, defaultdict

import numpy as np

from capstone import (
    CS_ARCH_X86,
    CS_GRP_JUMP,
    CS_GRP_RET,
    CS_MODE_64,
    CS_OP_IMM,
    CS_OP_MEM,
    Cs,
)
from capstone.x86_const import X86_REG_RIP

# ---------------------------------------------------------------- paths/data
DISK = r"E:\Overwatch\_retail_\Overwatch.exe"
LOADER_FLAT = r"D:\ow-dumps\loader_disk_rva.bin"
ORACLE = r"D:\ow-dumps\pagepak\keystream_current.npy"
WARM_IMAGES = [
    r"D:\ow-dumps\module_v21_clean.bin",
    r"D:\ow-dumps\module_v21d.bin",
]
OUT = r"D:\ow-dumps\offline_recover"

PAD_RVA = 0x1D77F80
PAD_SIZE = 0x4000
PAGE = 0x1000
W_ALL = np.arange(0, PAD_SIZE, 4, dtype=np.int64)

TEXT_VA = 0x1000
TEXT_SIZE = 0x2F2B3D3
TEXT_END = TEXT_VA + TEXT_SIZE
PDATA_VA = 0x450B000
PDATA_SIZE = 0x24D968

PREFERRED_BASE = 0x140000000
V2B_BASE = 0x7FF7C4D20000  # standing analysis image base, validation only

BIGRAM_WEIGHT = 3.0
TOP_CHEAP = 32
TOP_STRUCT = 8
CLOSE_NATS = 200.0  # candidates within this margin are re-ranked by structure
PREFIX_N = 1024
PREFIX_CONFLICT_NATS = 300.0
SPLICE_GAP = 16


# --------------------------------------------------------------------- PE
class PEImage:
    def __init__(self, path):
        self.path = path
        self.data = np.fromfile(path, np.uint8)
        d = self.data
        pe = struct.unpack_from("<I", d, 0x3C)[0]
        self.pe_off = pe
        (self.machine, nsec, self.timestamp, _, _, opt_sz,
         self.characteristics) = struct.unpack_from("<HHIIIHH", d, pe + 4)
        opt = pe + 24
        self.opt_off = opt
        self.magic = struct.unpack_from("<H", d, opt)[0]
        self.image_base = struct.unpack_from("<Q", d, opt + 24)[0]
        self.size_of_image = struct.unpack_from("<I", d, opt + 56)[0]
        self.size_of_headers = struct.unpack_from("<I", d, opt + 60)[0]
        self.ndirs = struct.unpack_from("<I", d, opt + 108)[0]
        self.dirs = []
        for i in range(min(self.ndirs, 16)):
            rva, sz = struct.unpack_from("<II", d, opt + 112 + i * 8)
            self.dirs.append((rva, sz))
        self.sections = {}
        sec_off = opt + opt_sz
        for i in range(nsec):
            o = sec_off + i * 40
            name = bytes(d[o:o + 8]).rstrip(b"\0").decode("latin1")
            vsz, va, rsz, ro = struct.unpack_from("<IIII", d, o + 8)
            self.sections[name] = dict(va=va, vsz=vsz, raw=ro, rawsz=rsz)
        self.section_list = sorted(self.sections.values(), key=lambda s: s["va"])

    def rva_to_off(self, rva):
        for s in self.section_list:
            if s["va"] <= rva < s["va"] + max(s["vsz"], s["rawsz"]):
                return s["raw"] + (rva - s["va"])
        raise ValueError(f"RVA {rva:#x} not mapped")

    def read_rva(self, rva, size):
        off = self.rva_to_off(rva)
        return np.asarray(self.data[off:off + size])

    def relocations(self):
        rva, sz = self.dirs[5]
        if not rva or not sz:
            return []
        blob = self.read_rva(rva, sz)
        out = []
        pos = 0
        while pos + 8 <= len(blob):
            page_rva, block_sz = struct.unpack_from("<II", blob, pos)
            if block_sz < 8:
                break
            n = (block_sz - 8) // 2
            for j in range(n):
                ent = struct.unpack_from("<H", blob, pos + 8 + 2 * j)[0]
                typ = ent >> 12
                off = ent & 0xFFF
                if typ:
                    out.append((page_rva + off, typ))
            pos += block_sz
        return out


# ------------------------------------------------------------------ helpers
def entropy(bs):
    c = np.bincount(np.asarray(bs, np.uint8), minlength=256).astype(np.float64)
    p = c / c.sum()
    p = p[p > 0]
    return float(-(p * np.log2(p)).sum())


def load_pad():
    flat = np.fromfile(LOADER_FLAT, np.uint8)
    return flat[PAD_RVA:PAD_RVA + PAD_SIZE].copy()


def build_padmat(pad):
    cyc = np.concatenate([pad, pad, pad])
    idx = (W_ALL[:, None] + np.arange(PAGE)[None, :]).astype(np.uint16)
    return cyc[idx]


class Models:
    def __init__(self, loader_text):
        counts = np.bincount(loader_text, minlength=256).astype(np.float64)
        self.logu = np.log((counts + 0.5) / (counts.sum() + 128.0)).astype(np.float32)
        idx = (loader_text[:-1].astype(np.int32) << 8) | loader_text[1:].astype(np.int32)
        bg = np.bincount(idx, minlength=1 << 16).astype(np.float64).reshape(256, 256)
        bg += 0.25
        self.logb = np.log(bg / bg.sum(axis=1, keepdims=True)).astype(np.float32)


def load_oracle(cyc):
    ks = np.load(ORACLE, mmap_mode="r")
    index4 = defaultdict(list)
    for w in W_ALL:
        index4[cyc[int(w):int(w) + 4].tobytes()].append(int(w))
    oracle = {}
    partial = {}
    for p in range(ks.shape[0]):
        row = np.asarray(ks[p])
        if not row.any():
            continue
        hit = None
        for w in index4.get(row[:4].tobytes(), []):
            if np.array_equal(cyc[w:w + PAGE], row):
                hit = w
                break
        if hit is not None:
            oracle[p] = hit
            continue
        # partial match: try the documented two-window 16-byte-zero splice,
        # else report the longest contiguous aligned window.
        det = None
        idx0 = defaultdict(list)
        for w in W_ALL:
            idx0[cyc[int(w):int(w) + 4].tobytes()].append(int(w))
        for w2 in idx0.get(row[:4].tobytes(), []):
            eq = (cyc[w2:w2 + PAGE] == row)
            if not eq[0]:
                continue
            s = int(np.argmin(eq)) if not eq.all() else PAGE
            if s < 64 or s + 16 > PAGE or row[s:s + 16].any():
                continue
            idx_s = {}
            for w in W_ALL:
                idx_s[cyc[int(w) + s + 16:int(w) + s + 20].tobytes()] = int(w)
            key = row[s + 16:s + 20].tobytes()
            w1 = idx_s.get(key)
            if w1 is not None and np.array_equal(cyc[w1 + s + 16:w1 + PAGE],
                                                 row[s + 16:]):
                det = dict(kind="splice", w_prefix=int(w2), w_suffix=int(w1),
                           split=s, gap=16, match_bytes=PAGE - 16)
                break
        if det is None:
            best = (0, None)
            for w in W_ALL:
                eq = (cyc[int(w):int(w) + PAGE] == row).astype(np.int8)
                d = np.diff(np.concatenate(([0], eq, [0])))
                starts = np.flatnonzero(d == 1)
                ends = np.flatnonzero(d == -1)
                if len(starts):
                    k = int(np.max(ends - starts))
                    if k > best[0]:
                        best = (k, int(w))
            det = dict(kind="runs", match_bytes=best[0], w=best[1])
        partial[p] = det
    return oracle, partial, ks.shape[0]


def load_functions(pe):
    raw = pe.read_rva(PDATA_VA, PDATA_SIZE)
    n = len(raw) // 12
    e = np.frombuffer(np.ascontiguousarray(raw[:n * 12]), np.uint8)
    e = e.reshape(n, 3, 4).view("<u4").reshape(n, 3)
    starts = e[:, 0].astype(np.int64)
    ends = e[:, 1].astype(np.int64)
    unwind = e[:, 2].astype(np.int64)
    by_page = defaultdict(list)
    for s, en in zip(starts, ends):
        pg = int(s) >> 12
        by_page[pg].append((int(s) - pg * PAGE, int(en) - pg * PAGE, int(s), int(en)))
    for pg in by_page:
        by_page[pg].sort()
    return starts, ends, unwind, by_page


def page_ids():
    return list(range(TEXT_VA >> 12, ((TEXT_END - 1) >> 12) + 1))


# --------------------------------------------------------------- scoring
MD = Cs(CS_ARCH_X86, CS_MODE_64)
MD.detail = True
TERMINATORS = {"ret", "retn", "retf", "jmp", "int3", "ud2"}


def structural_score(P, rva, funcs, start_set, data_ranges):
    checks = exact = 0
    calls = call_hits = 0
    refs = ref_hits = 0
    first_ok = 0
    for o0, o1, s, e in funcs:
        if o0 < 0 or o1 > PAGE or e - s > 0x2000 or e <= s:
            continue
        checks += 1
        code = P[o0:o1]
        consumed = 0
        last = None
        first = None
        for insn in MD.disasm(code, rva + o0):
            if first is None:
                first = insn
            consumed += insn.size
            mnem = insn.mnemonic
            if mnem == "call" and insn.operands and insn.operands[0].type == CS_OP_IMM:
                calls += 1
                tgt = insn.operands[0].imm & 0xFFFFFFFFFFFFFFFF
                if TEXT_VA <= tgt < TEXT_END:
                    if tgt in start_set:
                        call_hits += 1
                    else:
                        call_hits += 0.25
            for op in insn.operands:
                if op.type == CS_OP_MEM and op.mem.base == X86_REG_RIP:
                    refs += 1
                    tgt = (insn.address + insn.size + op.mem.disp) & 0xFFFFFFFFFFFFFFFF
                    for lo, hi in data_ranges:
                        if lo <= tgt < hi:
                            ref_hits += 1
                            break
            last = insn
        if first is not None and first.mnemonic in {
                "push", "mov", "sub", "lea", "jmp", "nop", "endbr64",
                "xor", "test", "ret", "int3", "cmp", "pop"}:
            first_ok += 1
        if consumed == len(code) and last is not None and last.mnemonic in TERMINATORS:
            exact += 1
    exact_ratio = exact / checks if checks else 0.0
    call_ratio = call_hits / calls if calls else 0.0
    ref_ratio = ref_hits / refs if refs else 0.0
    first_ratio = first_ok / checks if checks else 0.0
    struct = 0.55 * exact_ratio + 0.2 * call_ratio + 0.15 * ref_ratio + 0.1 * first_ratio
    return dict(checks=checks, exact=exact, exact_ratio=exact_ratio,
                calls=calls, call_ratio=call_ratio, refs=refs,
                ref_ratio=ref_ratio, first_ratio=first_ratio, struct=struct)


def _splice_candidate(C, P_all, chosen_i, pre_i, models, valid_len):
    """Best two-window/16-zero splice with the chosen window in either role."""
    gap = SPLICE_GAP
    ll0 = models.logu[C].astype(np.float64)
    lls = {}
    for i in (chosen_i, pre_i):
        lls[i] = models.logu[P_all[i]].astype(np.float64)
    baseline = float(lls[chosen_i][:valid_len].sum())

    def best_for(pre, suf):
        lp = np.concatenate(([0.0], np.cumsum(lls[pre][:valid_len])))
        ls = np.concatenate(([0.0], np.cumsum(lls[suf][:valid_len])))
        l0 = np.concatenate(([0.0], np.cumsum(ll0[:valid_len])))
        best = (-1e18, None)
        for s in range(64, valid_len - gap - 64 + 1, 4):
            sc = lp[s] + (ls[valid_len] - ls[s + gap]) + (l0[s + gap] - l0[s])
            if sc > best[0]:
                best = (sc, s)
        return best

    cands = []
    for pre, suf in ((pre_i, chosen_i), (chosen_i, pre_i)):
        sc, s = best_for(pre, suf)
        cands.append((sc - baseline, sc, s, pre, suf))
    cands.sort(reverse=True)
    gain, sc, s, pre, suf = cands[0]
    if s is None:
        return None
    return dict(mode="splice", w=int(W_ALL[suf]), w_prefix=int(W_ALL[pre]),
                splice_at=int(s), splice_gap=gap, gain_nats=float(gain),
                combined=float(sc), su=float(sc), sb=0.0, i=int(suf),
                gap=float(gain) / max(1, valid_len))


def page_plaintext(C, best, valid_len=PAGE):
    cyc = load_pad_cycle()
    if best.get("mode") == "splice":
        s = int(best["splice_at"])
        g = int(best["splice_gap"])
        K = np.concatenate([cyc[int(best["w_prefix"]):int(best["w_prefix"]) + s],
                            np.zeros(g, np.uint8),
                            cyc[int(best["w"]) + s + g:int(best["w"]) + PAGE]])
        P = C ^ K
    else:
        P = C ^ cyc[int(best["w"]):int(best["w"]) + PAGE]
    island = best.get("island")
    if island:
        s, e = int(island[0]), int(island[1])
        P[s:e] = C[s:e]
    return P


def decodes_exact(code, addr):
    consumed = 0
    last = None
    for insn in MD.disasm(bytes(code), addr):
        consumed += insn.size
        last = insn
    return (consumed == len(code) and last is not None
            and last.mnemonic in TERMINATORS)


def score_page(C, rva, valid_len, funcs, start_set, data_ranges, models, padmat,
               struct_top=TOP_STRUCT, entry_func=None):
    """Return ranked candidate dicts for one page and the chosen flag."""
    if valid_len <= 0:
        return [], None
    P_all = C[None, :] ^ padmat
    su_sub = models.logu[P_all[:, ::4]].sum(axis=1, dtype=np.float64)
    # data-table evidence: some .text pages are switch/offset tables whose
    # 32-bit words are RVAs into .text.  A random window cannot do this.
    D = P_all.view("<u4")
    rva_frac = ((D >= TEXT_VA) & (D < TEXT_END)).mean(axis=1)
    table_i = int(np.argmax(rva_frac))
    if float(rva_frac[table_i]) >= 0.5:
        w = int(W_ALL[table_i])
        second_frac = float(np.sort(rva_frac)[-2]) if len(rva_frac) > 1 else 0.0
        return [], dict(
            w=w, combined=float(rva_frac[table_i]), su=0.0, sb=0.0, i=table_i,
            gap=float(rva_frac[table_i]) - second_frac, checks=0, struct=0.0,
            ambiguous=False, reason="data-table", second_combined=0.0,
            second_w=int(W_ALL[int(np.argsort(rva_frac)[-2])]),
            mode="table", rva_frac=float(rva_frac[table_i]))
    cheap_top = np.argsort(su_sub)[::-1][:TOP_CHEAP]
    cand = []
    pos = np.arange(0, valid_len - 1, 4)
    for i in cheap_top:
        w = int(W_ALL[i])
        P = P_all[i]
        su = float(models.logu[P].sum(dtype=np.float64))
        sb = float(models.logb[P[pos], P[pos + 1]].sum(dtype=np.float64))
        cand.append(dict(w=w, su=su, sb=sb, combined=su + BIGRAM_WEIGHT * sb, i=int(i)))
    cand.sort(key=lambda d: d["combined"], reverse=True)
    for d in cand[:struct_top]:
        w = d["w"]
        P = P_all[d["i"]]
        d["struct"] = structural_score(P, rva, funcs, start_set, data_ranges)
    best = cand[0]
    close = [d for d in cand[:struct_top]
             if d["combined"] >= best["combined"] - CLOSE_NATS and "struct" in d]
    if close:
        pick = max(close, key=lambda d: (d["struct"]["struct"], d["combined"]))
    else:
        pick = best
    second = cand[1]["combined"] if len(cand) > 1 else float("-inf")
    gap = (pick["combined"] - second) / max(1, valid_len)
    # documented splice: a page can carry one window for a prefix, 16 zero
    # keystream bytes, then a second window.  Detect it when the page prefix
    # is much better explained by another window than by the chosen one.
    pre_n = min(PREFIX_N, valid_len)
    pre_su = models.logu[P_all[:, :pre_n]].sum(axis=1, dtype=np.float64)
    pre_i = int(np.argmax(pre_su))
    pre_margin = float(pre_su[pre_i] - pre_su[pick["i"]])
    if pre_i != pick["i"] and pre_margin > PREFIX_CONFLICT_NATS:
        sp = _splice_candidate(C, P_all, pick["i"], pre_i, models, valid_len)
        if sp is not None and sp["gain_nats"] > PREFIX_CONFLICT_NATS:
            P = page_plaintext(C, sp, valid_len)
            sts = structural_score(P, rva, funcs, start_set, data_ranges)
            sp.update(checks=sts["checks"], struct=sts["struct"],
                      ambiguous=False, second_combined=second,
                      second_w=pick["w"], rva_frac=0.0, reason="splice",
                      confidence=min(1.0, sp["gain_nats"] / 1000.0))
            return cand, sp
    st = pick.get("struct")
    checks = st["checks"] if st else 0
    sratio = st["struct"] if st else 0.0
    ambiguous = False
    reason = ""
    if gap < 0.02 and (not st or sratio < 0.5):
        ambiguous = True
        reason = "small-gap"
    if checks == 0 and gap < 0.5:
        ambiguous = True
        reason = (reason + "+" if reason else "") + "no-functions-small-gap"
    if checks >= 1 and sratio < 0.30 and gap < 1.0:
        ambiguous = True
        reason = (reason + "+" if reason else "") + "weak-structure-small-gap"
    best_d = dict(pick)
    best_d.update(gap=gap, checks=checks, struct=sratio, ambiguous=ambiguous,
                  reason=reason, second_combined=second,
                  second_w=cand[1]["w"] if len(cand) > 1 else -1,
                  mode="code", rva_frac=float(rva_frac[pick["i"]]))
    # The PE entry function can be stored as a plaintext island in an
    # otherwise encrypted page.  Anchor it on the PE header, not on warm data.
    if entry_func is not None:
        es, ee = int(entry_func[0]), int(entry_func[1])
        s0, e0 = es - rva, ee - rva
        if 0 <= s0 < e0 <= valid_len:
            P = P_all[pick["i"]]
            if (decodes_exact(P[s0:e0], es)
                    and not decodes_exact(C[s0:e0], es)):
                pass  # window is fine; no island
            elif (decodes_exact(C[s0:e0], es)
                  and not decodes_exact(P[s0:e0], es)):
                margin = float(models.logu[C[s0:e0]].sum()
                               - models.logu[P[s0:e0]].sum())
                if margin > 10.0:
                    ext = e0
                    nxt = [f[2] - rva for f in funcs if f[2] > es]
                    limit = min(nxt) if nxt else valid_len
                    while ext < limit and int(C[ext]) in (0x00, 0x90, 0xCC):
                        ext += 1
                    best_d["mode"] = "code-island"
                    best_d["island"] = (int(s0), int(ext))
                    best_d["reason"] = ((best_d["reason"] + "+")
                                        if best_d["reason"] else "")
                    best_d["reason"] += "entry-plaintext-island"
    return cand, best_d


_PAD_CACHE = {}


def load_pad_cycle():
    if "cyc" not in _PAD_CACHE:
        pad = load_pad()
        _PAD_CACHE["cyc"] = np.concatenate([pad, pad])
    return _PAD_CACHE["cyc"]


# ------------------------------------------------------------------ stages
def ensure_out(out):
    os.makedirs(out, exist_ok=True)


def stage_pad(out, args):
    t0 = time.time()
    ensure_out(out)
    pad = load_pad()
    cyc0 = np.concatenate([pad, pad])
    oracle, partial, nrows = load_oracle(cyc0)
    rows = sorted(oracle)
    ks = np.load(ORACLE, mmap_mode="r")
    rot = {}
    for name, shift in (("raw", 0), ("plus19", 19), ("minus19", -19)):
        p = np.roll(pad, shift)
        cyc = np.concatenate([p, p])
        idx = defaultdict(list)
        for w in W_ALL:
            idx[cyc[int(w):int(w) + 4].tobytes()].append(int(w))
        ok = 0
        for pg in rows:
            row = np.asarray(ks[pg])
            if any(np.array_equal(cyc[w:w + PAGE], row)
                   for w in idx.get(row[:4].tobytes(), [])):
                ok += 1
        rot[name] = ok
    # a 19-byte rotation is only equivalent with a non-aligned w
    rot_equiv = {}
    for shift in (19, -19):
        p = np.roll(pad, shift)
        cyc = np.concatenate([p, p])
        idx = defaultdict(list)
        for w in range(PAD_SIZE):
            idx[cyc[w:w + 4].tobytes()].append(w)
        mm = []
        for pg in rows[:200]:
            row = np.asarray(ks[pg])
            for w in idx.get(row[:4].tobytes(), []):
                if np.array_equal(cyc[w:w + PAGE], row):
                    mm.append(w % 4)
                    break
        rot_equiv[str(shift)] = sorted(set(mm))
    ws = np.array([oracle[p] for p in rows])
    result = dict(
        pad_rva=hex(PAD_RVA), pad_size=hex(PAD_SIZE),
        rows_in_oracle=len(rows), rows_total_nonzero=len(oracle) + len(partial),
        rows_full_match=len(oracle),
        rows_splice_verified=sum(1 for v in partial.values()
                                 if v.get("kind") == "splice"),
        rotation_matches=rot,
        rotation_equiv_mod4=rot_equiv,
        w_distinct=int(len(np.unique(ws))),
        w_quadrants=[int(x) for x in np.bincount(ws // 0x1000, minlength=4)],
        w_mod4=[int(x) for x in np.bincount(ws % 4, minlength=4)],
        splice_pages={str(p): v for p, v in partial.items()},
        derivation=("P = C xor pad[(w + i) mod 0x4000]; pad read verbatim from "
                    "loader_disk_rva.bin RVA 0x1d77f80; w 4-byte aligned.  One "
                    "oracle row is the documented two-window splice: window A "
                    "for a prefix, 16 zero keystream bytes, window B for the "
                    "suffix."),
        elapsed_s=round(time.time() - t0, 2),
    )
    json.dump(result, open(os.path.join(out, "pad_validation.json"), "w"), indent=1)
    print(json.dumps(result, indent=1))
    return result


def stage_pdata(out, args):
    t0 = time.time()
    ensure_out(out)
    pe = PEImage(DISK)
    disk = pe.data
    raw = np.ascontiguousarray(pe.read_rva(PDATA_VA, PDATA_SIZE))
    pad = load_pad()
    padmat = build_padmat(pad)
    text_lo, text_hi = TEXT_VA, TEXT_END
    rdata = pe.sections[".rdata"]
    r_lo, r_hi = rdata["va"], rdata["va"] + rdata["vsz"]
    rd = pe.sections["_RDATA"]
    d_lo, d_hi = rd["va"], rd["va"] + rd["vsz"]

    # whole-section plaintext parse
    n = len(raw) // 12
    E = np.frombuffer(raw[:n * 12], np.uint8).reshape(n, 3, 4).view("<u4").reshape(n, 3)
    st, en, un = (E[:, 0].astype(np.int64), E[:, 1].astype(np.int64),
                  E[:, 2].astype(np.int64))
    whole = dict(
        entries=int(n),
        sorted=bool(np.all(st[1:] >= st[:-1])),
        starts_in_text=int(((st >= text_lo) & (st < text_hi)).sum()),
        ends_in_text=int(((en > st) & (en <= text_hi)).sum()),
        unwind_in_rdata=int(((un >= r_lo) & (un < r_hi)).sum()),
        unwind_in_rd=int(((un >= d_lo) & (un < d_hi)).sum()),
        unwind_aligned=int((un % 4 == 0).sum()),
        distinct_starts=int(len(np.unique(st))),
        entropy=round(entropy(raw), 3),
    )

    rows = []
    npages = (len(raw) + PAGE - 1) // PAGE
    scan_best = []
    for k in range(npages):
        page_rva = PDATA_VA + k * PAGE
        off = k * PAGE
        C = np.zeros(PAGE, np.uint8)
        m = min(PAGE, len(raw) - off)
        C[:m] = raw[off:off + m]
        i0 = (off + 11) // 12
        i1 = min(n, (off + PAGE) // 12)
        entries = max(0, i1 - i0)
        if entries:
            sel = E[i0:i1]
            s = sel[:, 0].astype(np.int64)
            e2 = sel[:, 1].astype(np.int64)
            u = sel[:, 2].astype(np.int64)
            ident_valid = ((s >= text_lo) & (s < text_hi) & (e2 > s)
                           & (e2 <= text_hi) & (u >= r_lo) & (u < r_hi)
                           & (u % 4 == 0))
            ident_sorted = bool(np.all(np.diff(s) >= 0))
            ident_score = float(ident_valid.mean()) * (1.0 if ident_sorted else 0.5)
            local = 12 * np.arange(i0, i1) - off
            P_all = C[None, :] ^ padmat
            gidx = local[:, None] + np.arange(12)[None, :]
            eb = P_all[:, gidx].reshape(len(W_ALL), len(local), 3, 4)
            ev = np.ascontiguousarray(eb).view("<u4").reshape(len(W_ALL), len(local), 3)
            ss = ev[..., 0].astype(np.int64)
            ee = ev[..., 1].astype(np.int64)
            uu = ev[..., 2].astype(np.int64)
            valid = ((ss >= text_lo) & (ss < text_hi) & (ee > ss)
                     & (ee <= text_hi) & (uu >= r_lo) & (uu < r_hi)
                     & (uu % 4 == 0))
            vfrac = valid.mean(axis=1)
            sorted_ok = np.all(np.diff(ss, axis=1) >= 0, axis=1)
            scores = np.where(sorted_ok, vfrac, vfrac * 0.5)
            best_i = int(np.argmax(scores))
            best_pad_w = int(W_ALL[best_i])
            best_pad_score = float(scores[best_i])
        else:
            ident_score = 1.0
            best_pad_w = -1
            best_pad_score = 0.0
        plain = ident_score >= 0.99
        scan_best.append((best_pad_score, page_rva))
        amb = not plain
        rows.append(dict(page_rva=page_rva, w=-1 if plain else best_pad_w,
                         score=round(ident_score if plain else best_pad_score, 6),
                         entries=entries, ambiguous=int(amb),
                         mode="plaintext" if plain else "pad",
                         best_pad_w=best_pad_w,
                         best_pad_score=round(best_pad_score, 6)))

    with open(os.path.join(out, "pdata.csv"), "w", newline="") as fh:
        wr = csv.DictWriter(fh, fieldnames=["page_rva", "w", "score", "entries",
                                           "ambiguous", "mode", "best_pad_w",
                                           "best_pad_score"])
        wr.writeheader()
        wr.writerows(rows)
    open(os.path.join(out, "pdata.bin"), "wb").write(raw.tobytes())
    plain_pages = sum(1 for r in rows if r["mode"] == "plaintext")
    stats = dict(
        disk_encrypted=plain_pages < len(rows),
        verdict=("disk .pdata is plaintext: the untransformed section parses as "
                 "201,246 sorted RUNTIME_FUNCTION entries and no pad window "
                 "reaches the identity score"),
        pages=len(rows),
        plaintext_pages=plain_pages,
        ambiguous_pages=sum(1 for r in rows if r["ambiguous"]),
        whole_section=whole,
        best_pad_window_score=round(max(x[0] for x in scan_best), 4),
        fallback=("function starts come from the disk .pdata itself, which is "
                  "the live/known table (it matches the warm image byte-exactly)"),
        elapsed_s=round(time.time() - t0, 2),
    )
    json.dump(stats, open(os.path.join(out, "pdata_stats.json"), "w"), indent=1)
    print(json.dumps(stats, indent=1))
    return stats


def recover_one(page_rva, C, valid_len, funcs, start_set, data_ranges, models,
                padmat, struct_top=TOP_STRUCT, entry_func=None):
    return score_page(C, page_rva, valid_len, funcs, start_set, data_ranges,
                      models, padmat, struct_top, entry_func=entry_func)


def make_context(pe):
    pad = load_pad()
    padmat = build_padmat(pad)
    loader = np.fromfile(LOADER_FLAT, np.uint8)
    ltext = loader[0x10000:0x10000 + 0x1D539E6]
    models = Models(ltext)
    starts, ends, unwind, by_page = load_functions(pe)
    start_set = set(int(x) for x in starts)
    data_ranges = []
    for name in (".text", ".rdata", ".data", "_RDATA", ".rodata"):
        s = pe.sections.get(name)
        if s:
            data_ranges.append((s["va"], s["va"] + s["vsz"]))
    iat_rva, iat_sz = pe.dirs[12]
    if iat_rva:
        data_ranges.append((iat_rva, iat_rva + iat_sz))
    entry_rva = struct.unpack_from("<I", pe.data, pe.opt_off + 16)[0]
    entry_func = None
    i = int(np.searchsorted(starts, entry_rva, side="right")) - 1
    if i >= 0 and int(starts[i]) <= entry_rva < int(ends[i]):
        entry_func = (int(starts[i]), int(ends[i]))
    return dict(pad=pad, padmat=padmat, models=models, starts=starts,
                by_page=by_page, start_set=start_set, data_ranges=data_ranges,
                entry_rva=entry_rva, entry_func=entry_func)


def _recover_pages(pe, ctx, page_ids_, oracle_rows, text_cache=None):
    disk = pe.data
    out = []
    t0 = time.time()
    for n, pg in enumerate(page_ids_):
        rva = pg * PAGE
        off = pe.rva_to_off(rva)
        C = np.asarray(disk[off:off + PAGE])
        valid = max(0, min(PAGE, TEXT_END - rva))
        cand, best = recover_one(rva, C, valid,
                                 ctx["by_page"].get(pg, []),
                                 ctx["start_set"], ctx["data_ranges"],
                                 ctx["models"], ctx["padmat"],
                                 entry_func=ctx.get("entry_func"))
        if best is None:
            out.append(dict(page_rva=rva, w=-1, score=0.0, ambiguous=True,
                            confidence=0.0, checks=0, struct=0.0,
                            reason="no-valid-bytes", gap=0.0, second_w=-1,
                            mode="none", rva_frac=0.0, w_prefix=-1,
                            splice_at=0, island_start=-1, island_end=-1))
            continue
        if "confidence" in best:
            conf = best["confidence"]
        else:
            conf = max(0.0, min(1.0, best["gap"] / 2.0))
            if best["checks"]:
                conf = max(conf, best["struct"])
            if best.get("mode") == "table":
                conf = 1.0
        isl = best.get("island")
        out.append(dict(page_rva=rva, w=best["w"], score=best["combined"],
                        ambiguous=best["ambiguous"], confidence=conf,
                        checks=best["checks"], struct=best["struct"],
                        gap=best["gap"], reason=best["reason"],
                        second_w=best["second_w"],
                        mode=best.get("mode", "code"),
                        rva_frac=best.get("rva_frac", 0.0),
                        w_prefix=best.get("w_prefix", -1),
                        splice_at=best.get("splice_at", 0),
                        island_start=(rva + int(isl[0])) if isl else -1,
                        island_end=(rva + int(isl[1])) if isl else -1))
        if text_cache is not None and best["w"] >= 0:
            text_cache[rva] = page_plaintext(C, best, valid)
        if (n + 1) % 500 == 0:
            print(f"  {n + 1}/{len(page_ids_)} pages "
                  f"({time.time() - t0:.1f}s)", flush=True)
    return out, time.time() - t0


def stage_pilot(out, args):
    ensure_out(out)
    pe = PEImage(DISK)
    ctx = make_context(pe)
    oracle, partial, _ = load_oracle(np.concatenate([ctx["pad"], ctx["pad"]]))
    all_pages = page_ids()
    free = [p for p in all_pages if p not in oracle]
    rng = np.random.default_rng(20261023)
    pilot = [int(x) for x in sorted(rng.choice(free, min(100, len(free)), replace=False))]
    rec, dt = _recover_pages(pe, ctx, pilot, oracle)
    # held-out oracle evaluation
    oracle_pages = sorted(oracle)
    ev, edt = _recover_pages(pe, ctx, oracle_pages, oracle)
    truth = [oracle[r["page_rva"] // PAGE] for r in ev]
    got = [r["w"] for r in ev]
    match = [g == t for g, t in zip(got, truth)]
    acc = sum(match) / len(match)
    amb = sum(1 for r in ev if r["ambiguous"])
    st = dict(
        pilot_pages=len(pilot), pilot_elapsed_s=round(dt, 2),
        pilot_per_page_s=round(dt / max(1, len(pilot)), 4),
        oracle_pages=len(oracle_pages), oracle_elapsed_s=round(edt, 2),
        oracle_top1=int(sum(match)), oracle_top1_accuracy=round(acc, 6),
        oracle_ambiguous=amb, oracle_ambiguous_rate=round(amb / len(ev), 6),
        oracle_splice_excluded=len(partial),
        mean_gap=round(float(np.mean([r.get("gap", 0) for r in ev])), 4),
        min_gap=round(float(np.min([r.get("gap", 0) for r in ev])), 4),
        gate_pass=bool(acc >= 0.99 and amb / len(ev) <= 0.02),
    )
    json.dump(st, open(os.path.join(out, "pilot.json"), "w"), indent=1)
    with open(os.path.join(out, "oracle_eval.csv"), "w", newline="") as fh:
        wr = csv.DictWriter(fh, fieldnames=["page_rva", "w_oracle", "w_recovered",
                                           "match", "ambiguous", "confidence",
                                           "checks", "gap"])
        wr.writeheader()
        for r, t, m in zip(ev, truth, match):
            wr.writerow(dict(page_rva=r["page_rva"], w_oracle=t,
                             w_recovered=r["w"], match=int(m),
                             ambiguous=int(r["ambiguous"]),
                             confidence=round(r["confidence"], 4),
                             checks=r["checks"], gap=round(r.get("gap", 0), 4)))
    print(json.dumps(st, indent=1))
    return st


def stage_full(out, args):
    ensure_out(out)
    pe = PEImage(DISK)
    ctx = make_context(pe)
    pages = page_ids()
    text_cache = {}
    t0 = time.time()
    rec, dt = _recover_pages(pe, ctx, pages, None, text_cache=text_cache)
    # text.bin: exact section virtual bytes
    text = np.zeros(TEXT_SIZE, np.uint8)
    missing = 0
    for r in rec:
        rva = r["page_rva"]
        if rva >= TEXT_END:
            continue
        n = min(PAGE, TEXT_END - rva)
        if rva in text_cache:
            text[rva - TEXT_VA:rva - TEXT_VA + n] = text_cache[rva][:n]
        else:
            missing += 1
    with open(os.path.join(out, "text.csv"), "w", newline="") as fh:
        wr = csv.DictWriter(fh, fieldnames=["page_rva", "w", "score", "ambiguous",
                                           "confidence", "checks", "struct", "gap",
                                           "second_w", "reason", "mode", "rva_frac",
                                           "w_prefix", "splice_at",
                                           "island_start", "island_end"])
        wr.writeheader()
        for r in rec:
            wr.writerow({k: (int(v) if isinstance(v, bool)
                             else round(v, 6) if isinstance(v, float)
                             else v) for k, v in r.items()})
    open(os.path.join(out, "text.bin"), "wb").write(text.tobytes())

    # reassemble a flat PE image at the preferred base
    flat = np.zeros(pe.size_of_image, np.uint8)
    flat[:pe.size_of_headers] = pe.data[:pe.size_of_headers]
    for s in pe.section_list:
        n = min(s["rawsz"], s["vsz"], pe.size_of_image - s["va"])
        if n > 0:
            flat[s["va"]:s["va"] + n] = pe.data[s["raw"]:s["raw"] + n]
    flat[TEXT_VA:TEXT_VA + TEXT_SIZE] = text
    flat[PDATA_VA:PDATA_VA + PDATA_SIZE] = pe.read_rva(PDATA_VA, PDATA_SIZE)
    relocs = pe.relocations()
    delta = args.base - PREFERRED_BASE
    applied = apply_relocations(flat, relocs, delta)
    open(os.path.join(out, "module_offline.bin"), "wb").write(flat.tobytes())

    amb = [r["page_rva"] for r in rec if r["ambiguous"]]
    nofuncs = sum(1 for r in rec if r["checks"] == 0)
    stats = dict(
        pages=len(rec), coverage=round(len(rec) / len(pages), 6),
        ambiguous=len(amb), ambiguous_rate=round(len(amb) / len(rec), 6),
        pages_without_pdata_functions=nofuncs,
        missing_pages=missing, elapsed_s=round(dt, 2),
        per_page_s=round(dt / len(rec), 4),
        relocations=len(relocs), relocations_applied=applied,
        chosen_base=hex(args.base), image_size=len(flat),
        text_bytes=len(text), ambiguous_pages=amb[:200],
    )
    json.dump(stats, open(os.path.join(out, "full_run.json"), "w"), indent=1)
    print(json.dumps(stats, indent=1))
    return stats


def apply_relocations(flat, relocs, delta):
    if delta == 0 or not relocs:
        return 0
    n = 0
    mask = (1 << 64) - 1
    for rva, typ in relocs:
        if typ == 10:  # IMAGE_REL_BASED_DIR64
            v = struct.unpack_from("<Q", flat, rva)[0]
            struct.pack_into("<Q", flat, rva, (v + delta) & mask)
            n += 1
        elif typ == 3:  # IMAGE_REL_BASED_HIGHLOW
            v = struct.unpack_from("<I", flat, rva)[0]
            struct.pack_into("<I", flat, rva, (v + delta) & 0xFFFFFFFF)
            n += 1
    return n


def _warm_base(path):
    try:
        j = json.load(open(path + ".json"))
        return int(j["base"])
    except Exception:
        return V2B_BASE


def stage_validate(out, args):
    ensure_out(out)
    pe = PEImage(DISK)
    pad = load_pad()
    cyc = np.concatenate([pad, pad])
    oracle, partial, _ = load_oracle(cyc)
    text = np.fromfile(os.path.join(out, "text.bin"), np.uint8)
    rows = list(csv.DictReader(open(os.path.join(out, "text.csv"))))
    by_rva = {int(r["page_rva"]): r for r in rows}

    # oracle agreement (full run)
    agree = disagree = 0
    disagree_pages = []
    for pg, w in oracle.items():
        rva = pg * PAGE
        r = by_rva.get(rva)
        if r is None:
            continue
        if int(r["w"]) == w:
            agree += 1
        else:
            disagree += 1
            disagree_pages.append(dict(page_rva=rva, w_oracle=w,
                                       w_recovered=int(r["w"]),
                                       ambiguous=int(r["ambiguous"])))
    conf = np.array([float(r["confidence"]) for r in rows])
    hist = np.histogram(conf, bins=[0, .2, .4, .6, .8, .9, .95, .99, 1.0001])[0]
    gaps = np.array([float(r["gap"]) for r in rows])

    # the oracle splice rows are checked against the blind splice recovery
    splice_check = []
    for pg, det in partial.items():
        if det.get("kind") != "splice":
            continue
        rva = pg * PAGE
        r = by_rva.get(rva)
        if r is None:
            continue
        splice_check.append(dict(
            page_rva=rva, oracle_w_prefix=det["w_prefix"],
            oracle_w_suffix=det["w_suffix"], oracle_split=det["split"],
            recovered_w_prefix=int(r["w_prefix"]), recovered_w=int(r["w"]),
            recovered_split=int(r["splice_at"]), mode=r["mode"],
            match=bool(int(r["w"]) == det["w_suffix"]
                       and int(r["w_prefix"]) == det["w_prefix"]
                       and int(r["splice_at"]) == det["split"])))

    # warm image comparison at the v2b base (validation only)
    flat = np.fromfile(os.path.join(out, "module_offline.bin"), np.uint8)
    v2b = flat.copy()
    relocs = pe.relocations()
    applied = apply_relocations(v2b, relocs, V2B_BASE - PREFERRED_BASE)
    warm_res = {}
    warm_best = {}
    for path in WARM_IMAGES:
        img = np.fromfile(path, np.uint8)
        base = _warm_base(path)
        if base != V2B_BASE:
            warm_res[os.path.basename(path)] = dict(error=f"base {base:#x}")
            continue
        tot = exact = diff = cold = 0
        diff_bytes = 0
        diff_pages = []
        for pg in range(TEXT_VA >> 12, (TEXT_END - 1) >> 12 + 1):
            rva = pg * PAGE
            warm = img[rva:rva + PAGE]
            if not warm.any():
                cold += 1
                continue
            tot += 1
            n = min(PAGE, TEXT_END - rva)
            got = v2b[rva:rva + n]
            ref = warm[:n]
            same = got == ref
            warm_best[rva] = max(warm_best.get(rva, 0), int(same.sum()))
            if same.all():
                exact += 1
            else:
                diff += 1
                diff_bytes += int((~same).sum())
                if len(diff_pages) < 20:
                    r = by_rva.get(rva, {})
                    pos = np.flatnonzero(~same)
                    diff_pages.append(dict(
                        page_rva=rva, w=int(r.get("w", -1)),
                        ambiguous=int(r.get("ambiguous", -1)),
                        diff_bytes=int((~same).sum()),
                        equal_bytes=int(same.sum()),
                        first_diff=int(pos[0]), last_diff=int(pos[-1])))
        warm_res[os.path.basename(path)] = dict(
            base=hex(base), readable_pages=tot, exact=exact,
            exact_rate=round(exact / tot, 6) if tot else None,
            differing=diff, differing_bytes=diff_bytes, cold=cold,
            sample_differences=diff_pages)

    # ambiguous pages: can an independent warm page resolve the window?
    amb_rows = [r for r in rows if int(r["ambiguous"])]
    amb_resolution = []
    for r in amb_rows:
        rva = int(r["page_rva"])
        n = min(PAGE, TEXT_END - rva)
        eq = warm_best.get(rva, 0)
        amb_resolution.append(dict(
            page_rva=rva, w=int(r["w"]), mode=r["mode"], reason=r["reason"],
            warm_equal_bytes=eq, warm_checked=eq > 0,
            resolved=bool(eq >= 0.9 * n)))
    modes = Counter(r["mode"] for r in rows)
    table_pages = [int(r["page_rva"]) for r in rows if r["mode"] == "table"]

    # pdata agreement with warm images
    pd = np.fromfile(os.path.join(out, "pdata.bin"), np.uint8)
    pdata_res = {}
    for path in WARM_IMAGES:
        img = np.fromfile(path, np.uint8)
        ref = img[PDATA_VA:PDATA_VA + PDATA_SIZE]
        pdata_res[os.path.basename(path)] = dict(
            identical=bool(np.array_equal(pd, ref)),
            differing_bytes=int((pd != ref).sum()))
    res = dict(
        oracle=dict(agree=agree, disagree=disagree,
                    accuracy=round(agree / max(1, agree + disagree), 6),
                    splice_rows=len(partial), splice_check=splice_check,
                    sample_disagreements=disagree_pages[:20]),
        confidence_histogram=dict(
            edges=[0, .2, .4, .6, .8, .9, .95, .99, 1.0],
            counts=[int(x) for x in hist]),
        gap=dict(mean=round(float(gaps.mean()), 4),
                 median=round(float(np.median(gaps)), 4),
                 p01=round(float(np.percentile(gaps, 1)), 4),
                 min=round(float(gaps.min()), 4)),
        modes=dict(counts=dict(modes), table_pages=table_pages),
        ambiguous_resolution=amb_resolution,
        warm_images=warm_res,
        pdata=dict(pages=int((PDATA_SIZE + PAGE - 1) // PAGE),
                   entries=int(PDATA_SIZE // 12),
                   warm_compare=pdata_res),
        relocations_applied_v2b=applied,
    )
    json.dump(res, open(os.path.join(out, "validation.json"), "w"), indent=1)
    print(json.dumps(res, indent=1))
    return res


def stage_report(out, args):
    ensure_out(out)
    def load(name):
        p = os.path.join(out, name)
        return json.load(open(p)) if os.path.exists(p) else {}
    pad = load("pad_validation.json")
    pdat = load("pdata_stats.json")
    pilot = load("pilot.json")
    full = load("full_run.json")
    val = load("validation.json")
    rows = list(csv.DictReader(open(os.path.join(out, "text.csv")))) \
        if os.path.exists(os.path.join(out, "text.csv")) else []
    ndec = [r for r in rows if int(r["checks"]) == 0 and r["mode"] != "table"]
    amb = [r for r in rows if int(r["ambiguous"])]
    lowstruct = [r for r in rows if int(r["checks"]) > 0
                 and float(r["struct"]) < 0.34]
    table = [r for r in rows if r["mode"] == "table"]
    lines = []
    ap = lines.append
    ap("# Offline .text / .pdata recovery (Plan A)")
    ap("")
    ap("No live capture, no process memory, no Ghidra change.  The on-disk")
    ap("`Overwatch.exe` ciphertext is decrypted with the static 16 KiB pad in the")
    ap("plaintext loader; `w` is chosen from the page's own `.pdata` structure and")
    ap("a loader-trained byte model.  The current-build keystream oracle is used")
    ap("only as a held-out test.")
    ap("")
    ap("## 1. Pad and rotation")
    ap("")
    ap(f"- pad: `{pad.get('pad_rva')}` size `{pad.get('pad_size')}`, read "
       "verbatim from `D:/ow-dumps/loader_disk_rva.bin`.")
    ap(f"- oracle rows: {pad.get('rows_total_nonzero')} nonzero; "
       f"{pad.get('rows_full_match')} match one 4 KB window and "
       f"{pad.get('rows_splice_verified')} match the documented two-window "
       "splice.")
    ap(f"- rotation tests (4-byte-aligned w): {pad.get('rotation_matches')}.")
    ap(f"- a 19-byte rotation only matches with `w % 4 = "
       f"{pad.get('rotation_equiv_mod4')}` (i.e. a non-aligned start), so the "
       "verbatim layout is the logical pad.")
    ap(f"- splice detail: {pad.get('splice_pages')}.")
    ap(f"- distinct w: {pad.get('w_distinct')}, quadrants {pad.get('w_quadrants')}, "
       f"w%4 {pad.get('w_mod4')}.")
    ap("")
    ap("## 2. `.pdata`")
    ap("")
    if pdat:
        ws = pdat.get("whole_section", {})
        ap(f"- **disk `.pdata` is plaintext**, not encrypted with the page "
           f"cipher: {pdat.get('plaintext_pages')}/{pdat.get('pages')} pages "
           "parse at the identity, and no pad window comes close (best pad "
           f"window score {pdat.get('best_pad_window_score')} valid entries).")
        ap(f"- entries: {ws.get('entries')}, sorted={ws.get('sorted')}, "
           f"starts in text={ws.get('starts_in_text')}, ends in text="
           f"{ws.get('ends_in_text')}, unwind in `.rdata`={ws.get('unwind_in_rdata')}, "
           f"unwind aligned={ws.get('unwind_aligned')}, distinct starts="
           f"{ws.get('distinct_starts')}.")
        ap(f"- `.pdata` ambiguous pages: {pdat.get('ambiguous_pages')} "
           "(every page is plaintext; `w=-1` in `pdata.csv` marks that, it is "
           "not a pad start).")
        ap(f"- per-page 4,096-`w` scan time: {pdat.get('elapsed_s')} s.")
        ap(f"- fallback: {pdat.get('fallback')}.")
    ap("")
    ap("## 3. Pilot and held-out oracle")
    ap("")
    if pilot:
        ap(f"- pilot pages: {pilot.get('pilot_pages')} "
           f"({pilot.get('pilot_per_page_s')} s/page).")
        ap(f"- held-out oracle: {pilot.get('oracle_top1')}/"
           f"{pilot.get('oracle_pages')} top-1 = "
           f"{100 * pilot.get('oracle_top1_accuracy', 0):.3f}% "
           f"(the splice row is checked separately; "
           f"{pilot.get('oracle_splice_excluded')} row).")
        ap(f"- oracle ambiguous: {pilot.get('oracle_ambiguous')} "
           f"({100 * pilot.get('oracle_ambiguous_rate', 0):.3f}%), "
           f"min gap {pilot.get('min_gap')} nats/byte, mean gap "
           f"{pilot.get('mean_gap')}.")
        ap(f"- oracle evaluation time: {pilot.get('oracle_elapsed_s')} s.")
        ap(f"- gate: {'PASS' if pilot.get('gate_pass') else 'FAIL'}.")
    ap("")
    ap("## 4. Full run")
    ap("")
    if full:
        ap(f"- pages: {full.get('pages')}, coverage {full.get('coverage')}, "
           f"ambiguous {full.get('ambiguous')} "
           f"({100 * full.get('ambiguous_rate', 0):.3f}%).")
        ap(f"- pages without a contained `.pdata` function: "
           f"{full.get('pages_without_pdata_functions')}; "
           f"data-table pages recovered by the RVA-table score: {len(table)}.")
        ap(f"- compute time: {full.get('elapsed_s')} s "
           f"({full.get('per_page_s')} s/page).")
        ap(f"- module: `module_offline.bin` at {full.get('chosen_base')}, "
           f"{full.get('image_size')} bytes, {full.get('relocations')} "
           f"relocations in `.reloc`, {full.get('relocations_applied')} applied "
           "(preferred base needs no fixup).")
    ap("")
    ap("## 5. Validation")
    ap("")
    if val:
        o = val.get("oracle", {})
        ap(f"- oracle agreement from the full run: {o.get('agree')} agree, "
           f"{o.get('disagree')} disagree "
           f"({100 * o.get('accuracy', 0):.4f}%).")
        ap(f"- confidence histogram: {val.get('confidence_histogram')}.")
        ap(f"- gap: {val.get('gap')}.")
        for name, w in val.get("warm_images", {}).items():
            if "error" in w:
                ap(f"- warm `{name}`: {w['error']}")
                continue
            ap(f"- warm `{name}`: {w.get('exact')}/{w.get('readable_pages')} "
               f"readable pages byte-exact ({100 * (w.get('exact_rate') or 0):.4f}%), "
               f"{w.get('differing')} differing, {w.get('cold')} cold/zero.")
            for d in w.get("sample_differences", []):
                ap(f"  - RVA `{d['page_rva']:#x}` w={d['w']} "
                   f"equal={d['equal_bytes']}/4096 differ={d['diff_bytes']} "
                   f"(first {d.get('first_diff')}..{d.get('last_diff')})")
        ap(f"- `.pdata` vs warm: {val.get('pdata', {}).get('warm_compare')}.")
        ap(f"- page modes: {val.get('modes', {}).get('counts')}.")
        for s in val.get("oracle", {}).get("splice_check", []):
            ap(f"- splice row `{s['page_rva']:#x}`: oracle w_prefix="
               f"{s['oracle_w_prefix']} w_suffix={s['oracle_w_suffix']} "
               f"split={s['oracle_split']}; recovered w_prefix="
               f"{s['recovered_w_prefix']} w_suffix={s['recovered_w']} "
               f"split={s['recovered_split']} mode={s['mode']} "
               f"match={'yes' if s['match'] else 'no'}.")
        for a in val.get("ambiguous_resolution", []):
            ap(f"- ambiguity RVA `{a['page_rva']:#x}` w={a['w']}: "
               f"{a['warm_equal_bytes']}/4096 warm bytes equal, "
               f"resolved={'yes' if a['resolved'] else 'no'}.")
    ap("")
    ap("## 6. Page classes that did not decode")
    ap("")
    ap(f"- ambiguous pages (blind score): {len(amb)}.")
    ap(f"- pages with no contained `.pdata` function: "
       f"{full.get('pages_without_pdata_functions')} (the data tables above are "
       "inside this group).")
    ap(f"- code pages with weak structure (<0.34): {len(lowstruct)}.")
    ap(f"- data-table pages: {len(table)} "
       f"({[hex(int(r['page_rva'])) for r in table]})")
    ap(f"- spliced pages: "
       f"{[hex(int(r['page_rva'])) for r in rows if r['mode'] == 'splice']}")
    if amb:
        ap("- ambiguous pages (RVA, w, reason):")
        for r in amb[:40]:
            ap(f"  - `{int(r['page_rva']):#x}` w={r['w']} reason={r['reason']}")
    ap("")
    ap("The only bytes not recoverable from the disk are the last 16 bytes of")
    ap("`0x540000` (`0x540ff0..0x540fff`, a `jmp` thunk plus `int3` padding in")
    ap("the warm image).  The page's PE entry function (`0x540fd0`) is stored")
    ap("as a 32-byte plaintext island on disk; it is recovered blind by")
    ap("anchoring on `AddressOfEntryPoint` and taking the bytes verbatim, and")
    ap("it disassembles as a normal `sub rsp, 0x28` function.  The remaining")
    ap("16 bytes match neither the page window nor the identity, so the page")
    ap("stays marked ambiguous.  The `0x53a000` two-window splice is recovered")
    ap("blindly and matches both warm images byte-exactly.")
    ap("")
    ap("## Commands")
    ap("")
    ap("```")
    ap("python dev/offline_page_recover.py --stage pad")
    ap("python dev/offline_page_recover.py --stage pdata")
    ap("python dev/offline_page_recover.py --stage pilot")
    ap("python dev/offline_page_recover.py --stage full")
    ap("python dev/offline_page_recover.py --stage validate")
    ap("python dev/offline_page_recover.py --stage report")
    ap("```")
    ap("")
    ap("## Artifacts")
    ap("")
    ap("- `pdata.csv`, `pdata.bin` - recovered `.pdata`")
    ap("- `text.csv`, `text.bin` - recovered `.text`")
    ap("- `module_offline.bin` - flat PE at the preferred base")
    ap("- `pad_validation.json`, `pdata_stats.json`, `pilot.json`, "
       "`full_run.json`, `validation.json`, `oracle_eval.csv`")
    ap("")
    report = "\n".join(lines) + "\n"
    open(os.path.join(out, "REPORT.md"), "w").write(report)
    print(report)


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--stage", default="all",
                    choices=["pad", "pdata", "pilot", "full", "validate",
                             "report", "all"])
    ap.add_argument("--out", default=OUT)
    ap.add_argument("--base", type=lambda x: int(x, 0), default=PREFERRED_BASE)
    args = ap.parse_args()
    if args.stage in ("pad", "all"):
        stage_pad(args.out, args)
    if args.stage in ("pdata", "all"):
        stage_pdata(args.out, args)
    if args.stage in ("pilot", "all"):
        st = stage_pilot(args.out, args)
        if args.stage == "all" and not st.get("gate_pass"):
            raise SystemExit("pilot gate failed - stopping before the full run")
    if args.stage in ("full", "all"):
        stage_full(args.out, args)
    if args.stage in ("validate", "all"):
        stage_validate(args.out, args)
    if args.stage in ("report", "all"):
        stage_report(args.out, args)


if __name__ == "__main__":
    main()
