"""Write `crates/ow-engine/src/classes.rs` from the constructor/vtable profile.

Input:  `dev/engine_classes.json` (written by `dev/eng_class_profile.py`) and
        `dev/engine_subsystems.verified.csv` (the slot schedule).
Output: `crates/ow-engine/src/classes.rs` — one row per distinct constructor,
        with the vtables it installs and the offset each lands on.

The generator is a transcription, nothing else: every value it writes is one
`dev/eng_class_profile.py` read from the image. Re-run it after re-running the
profile; `crates/ow-engine/tests/class_layout.rs` pins the result.

Usage: python dev/eng_gen_classes.py
"""

from __future__ import annotations

import csv
import json

JSON = "dev/engine_classes.json"
CSV = "dev/engine_subsystems.verified.csv"
OUT = "crates/ow-engine/src/classes.rs"


def main() -> int:
    data = json.load(open(JSON, encoding="utf-8"))
    ctors = data["ctors"]
    vtables = data["vtables"]

    rows = list(csv.DictReader(open(CSV, encoding="utf-8", newline="")))
    slots_of = {}
    size_of = {}
    for r in rows:
        slots_of.setdefault(r["producer"], []).append(int(r["slot_offset"], 16))
        if r["alloc_size"]:
            size_of.setdefault(r["producer"], int(r["alloc_size"], 16))

    lines = [
        "//! Every subsystem constructor, and the vtables it installs.",
        "//!",
        "//! Written by `dev/eng_gen_classes.py` from `dev/engine_classes.json`, which",
        "//! `dev/eng_class_profile.py` decodes from the image: for each of the 35",
        "//! producers the subsystem schedule uses, the constructor's own body is swept",
        "//! (`lea reg,[rip+X]` then `mov [this+off],reg` is a vtable store; a `call`",
        "//! ends the register tracking) and every vtable it installs is listed in store",
        "//! order at the object offset it lands on.",
        "//!",
        "//! Two things to read into the data:",
        "//!",
        "//! * **A repeated offset is a base-then-derived overwrite.** The largest class",
        "//!   (`+0x2f0`, ctor `0x14072c560`) installs `0x142f8ac68` and then its own",
        "//!   table at each of six sub-object offsets — six copies of one base, each",
        "//!   specialised. The profile keeps both stores, so the ordering is visible.",
        "//! * **`stub_base` means the shared stub is involved.** 31 of the 65 slots are",
        "//!   constructed by `0x141c578c0` itself (a 28-byte stub: vtable",
        "//!   `0x142f7d778`, cookie `0x7495174655b5d763`, return its argument); the",
        "//!   others call it from their own body. Either way the class derives from the",
        "//!   same abstract base.",
        "//!",
        "//! The 117 vtables these rows name are decoded separately; 44 of them are",
        "//! exactly 15 code entries (bounded from both sides — another known table at",
        "//! `+0x78`, or a non-code word at `+0x78`), which is what a 15-method interface",
        "//! table looks like. A table with no neighbour and no non-code word inside 128",
        "//! entries has **no read extent**; the count in `dev/engine_classes.json` is a",
        "//! lower bound, not a length. See `docs/formats/engine.md`.",
        "//!",
        "//! Three rows are producers rather than constructors: the `+0x220` decoder",
        "//! `0x1405437f0` and the two factories `0x140594090`/`0x1406cc990`. They are",
        "//! here because the schedule's producer column names them, and the profile",
        "//! shows they install no table in their own body (the decoder's 16 `mov`s are",
        "//! its fold, not field stores).",
        "//!",
        "//! The class *names* are not here: none is read yet. `ctor` is the identity,",
        "//! and `ow_engine::subsystems::SUBSYSTEMS` links a slot to its constructor.",
        "",
        "/// One vtable store the constructor performs: `vtable` lands at `offset`.",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub struct Install {",
        "    /// Object offset the store targets.",
        "    pub offset: u16,",
        "    /// Vtable address in the image.",
        "    pub vtable: u64,",
        "}",
        "",
        "/// One producer (constructor, decoder or factory), as read from its body.",
        "#[derive(Debug)]",
        "pub struct Class {",
        "    /// Constructor entry point (image address).",
        "    pub ctor: u64,",
        "    /// Bytes of body read.",
        "    pub body: u16,",
        "    /// True when the body had no `.pdata` range and was bounded by its own",
        "    /// terminating instruction instead (the stub and the `+0x220` decoder are",
        "    /// leaf functions that `.pdata` omits).",
        "    pub leaf: bool,",
        "    /// True when the constructor calls the shared stub `0x141c578c0`, or *is*",
        "    /// it.",
        "    pub stub_base: bool,",
        "    /// Vtable stores, in the order the constructor performs them.",
        "    pub installs: &'static [Install],",
        "}",
        "",
        "/// One row per distinct constructor in the schedule, ordered by address.",
        "pub const CLASSES: &[Class] = &[",
    ]

    def u32(v: int) -> str:
        return f"0x{v:x}"

    entries = sorted(ctors.values(), key=lambda c: int(c["producer"], 16))
    for c in entries:
        ctor = int(c["producer"], 16)
        installs = c.get("vtables", [])
        body = int(c["size"], 16)
        inst = ", ".join(
            f"Install {{ offset: {v['offset']}, vtable: {u32(v['vtable'])} }}" for v in installs
        )
        lines.append(
            f"    Class {{ ctor: {u32(ctor)}, body: {body}, leaf: "
            f"{'true' if c.get('bound') == 'leaf-terminator' else 'false'}, stub_base: "
            f"{'true' if c.get('stub_base') else 'false'}, installs: &[{inst}] }},"
        )
    lines += [
        "];",
        "",
        "/// The class whose constructor is `ctor`, if the schedule uses one.",
        "pub fn class_of(ctor: u64) -> Option<&'static Class> {",
        "    CLASSES.iter().find(|c| c.ctor == ctor)",
        "}",
        "",
        "/// Every vtable any class installs, in class order (duplicates kept: the same",
        "/// base is installed for several sub-objects).",
        "pub fn installed_vtables() -> impl Iterator<Item = (u64, u16, u64)> {",
        "    CLASSES",
        "        .iter()",
        "        .flat_map(|c| c.installs.iter().map(move |i| (c.ctor, i.offset, i.vtable)))",
        "}",
        "",
        "/// The vtable a class installs last at `offset` — what the object holds once",
        "/// the constructor returns.",
        "pub fn effective_vtable(ctor: u64, offset: u16) -> Option<u64> {",
        "    let c = class_of(ctor)?;",
        "    c.installs",
        "        .iter()",
        "        .filter(|i| i.offset == offset)",
        "        .map(|i| i.vtable)",
        "        .next_back()",
        "}",
        "",
    ]
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines))
    print(f"wrote {OUT}: {len(entries)} classes, {len(vtables)} vtables, "
          f"{sum(len(c['vtables']) for c in entries)} installs")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
