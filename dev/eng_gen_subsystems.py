"""Generate `crates/ow-engine/src/subsystems.rs` from the verified census.

Input: `dev/engine_subsystems.verified.csv`, produced by
`dev/eng_verify_schedule.py` (which re-derives every row from the function's
instruction stream and diffs it against `dev/engine_subsystems.csv`).

The generated table is the schedule of `ConstructEngineSubsystems`
(`0x140554220`): one row per `mov qword ptr [engine + slot], reg` in the
window `+0x220..+0x448`, with the producer, the allocation size, the vtables
the block stores and the `RegisterWorldInterface` call that registers it.

Usage: python dev/eng_gen_subsystems.py [--check]
`--check` writes nothing and fails if the generated file is out of date.
"""

from __future__ import annotations

import csv
import pathlib
import sys

CSV = pathlib.Path("dev/engine_subsystems.verified.csv")
OUT = pathlib.Path("crates/ow-engine/src/subsystems.rs")

ADJUSTOR_SLOT = 0x448
ADJUSTOR_OBJECT = 0x68  # `mov ecx, 0x68` at 0x140554b6a
DECODED = (0x220, 0x330, 0x60)  # slot <- [[world+0x330] + 0x60] through 0x1405437f0

# The tail attach pass, 0x1405556e8..0x14055599f: (call site, slot it re-attaches).
TAIL = [
    (0x140555704, 0x368),
    (0x140555725, 0x288),
    (0x140555746, 0x2E8),
    (0x140555767, 0x2D8),
    (0x140555788, 0x300),
    (0x1405557A9, 0x2E0),
    (0x1405557CA, 0x270),
    (0x1405557EB, 0x238),
    (0x14055580C, 0x3B0),
    (0x14055582D, 0x250),
    (0x14055584E, 0x240),
    (0x14055586F, 0x290),
    (0x140555890, 0x248),
    (0x1405558B1, 0x398),
    (0x1405558D2, 0x3E0),
    (0x1405558F3, 0x3E8),
    (0x140555914, 0x3F0),
    (0x140555935, 0x3F8),
    (0x140555956, 0x400),
    (0x140555977, 0x410),
    (0x14055599F, 0x268),  # guarded by `cmp qword ptr [rsi+0xe0], 0`
]


def gen() -> str:
    rows = list(csv.DictReader(open(CSV, encoding="utf-8")))
    assert len(rows) == 65, len(rows)
    out = []
    for r in rows:
        slot = int(r["slot_offset"], 16)
        producer = int(r["producer"], 16)
        if slot == DECODED[0]:
            source = f"Source::Decoded {{ from_slot: 0x{DECODED[1]:x}, offset: 0x{DECODED[2]:x} }}"
        elif slot == ADJUSTOR_SLOT:
            source = f"Source::Adjustor {{ object_size: 0x{ADJUSTOR_OBJECT:x} }}"
        elif r["alloc_size"]:
            source = f"Source::Alloc {{ size: 0x{int(r['alloc_size'], 16):x} }}"
        else:
            source = "Source::Factory"
        extra = [
            f"(0x{int(part.split('=')[0][1:], 16):x}, 0x{int(part.split('=')[1], 16):x})"
            for part in r["extra_vtables"].split(";")
            if part
        ]
        extra_s = "&[]" if not extra else "&[" + ", ".join(extra) + "]"
        vtable = f"0x{int(r['vtable'], 16):x}" if r["vtable"] else "0"
        register = f"0x{int(r['register_insn'], 16):x}" if r["register_insn"] else "0"
        out.append(
            "    Subsystem {\n"
            f"        slot: 0x{slot:x},\n"
            f"        source: {source},\n"
            f"        producer: 0x{producer:x},\n"
            f"        vtable: {vtable},\n"
            f"        extra: {extra_s},\n"
            f"        store_site: 0x{int(r['store_insn'], 16):x},\n"
            f"        register_site: {register},\n"
            "    },"
        )
    table = "\n".join(out)

    tail = "\n".join(
        f"    TailAttach {{ site: 0x{site:x}, slot: 0x{slot:x}, guarded: {str(slot == 0x268).lower()} }},"
        for site, slot in TAIL
    )

    return f'''//! The subsystem schedule of `ConstructEngineSubsystems` (`0x140554220`).
//!
//! The function allocates each of the engine object's subsystems, constructs
//! it, stores the pointer into a slot of the window `+0x220..+0x448` and
//! registers it through `RegisterWorldInterface` (`0x141c52d10`). This module
//! is that schedule as data: [`SUBSYSTEMS`] has one row per slot store, and
//! [`TAIL_ATTACH`] is the pass at the end of the function.
//!
//! Everything here is a decode of the instruction stream, not a measurement:
//! `dev/eng_verify_schedule.py` re-derives each row from the function's
//! instructions (producer call, `Alloc` size, vtable stores, slot store, the
//! `RegisterWorldInterface` call that carries the same object in RDX) and
//! fails if it disagrees with the census in `dev/engine_subsystems.csv`.
//! `dev/eng_gen_subsystems.py` writes this file from the verified result.
//!
//! ## The window
//!
//! `+0x220..+0x448` is 70 stride-8 offsets (the engine object is `0x470`
//! bytes; `+0x220` is the encoded world id and `+0x228..+0x460` is the
//! 71-qword table the derived constructor zeroes). **65 are written** and
//! five are not: [`EMPTY_SLOTS`]. The function contains no indexed store into
//! the window, so those five are plain fields or another function's, not a
//! missed member.
//!
//! ## What each row records
//!
//! * `slot` — the byte offset in the engine object.
//! * `source` — where the stored pointer came from. 61 rows are an inline
//!   `Alloc(size, 0)` followed by the constructor; two are factory calls that
//!   allocate inside themselves (`+0x340`, `+0x358`); one is a decoded pointer
//!   (`+0x220`, `[[world+0x330] + 0x60]` through the in-place decoder
//!   `0x1405437f0` and the same five-step fold the constructor uses); and one
//!   is an adjustor over a `0x68`-byte object (`+0x448`).
//! * `producer` — the constructor or factory that made the object. For
//!   `+0x220` it is the decoder `0x1405437f0`.
//! * `vtable` / `extra` — the vtable stores **the block** performs in place
//!   (a producer may install its own inside its body: slot `+0x288` does, see
//!   `docs/formats/engine.md`), so an empty field means "this block stores
//!   none", not "the class has none":
//!   primary at `+0x00`, then further bases at the offsets listed. The 31
//!   rows whose `producer` is [`STUB_CTOR`] call the shared 28-byte stub
//!   `0x141c578c0`, which installs `0x142f7d778` and a cookie and is then
//!   overwritten by the block's own `lea`/`mov`. That vtable is a real one:
//!   three methods (`0x14054c370`, `0x14054ee60`, `0x14054d540`) and
//!   `_purecall` at `+0x18`, i.e. the abstract base the 31 slots derive
//!   from, not a placeholder.
//! * `store_site` — the `mov qword ptr [engine + slot], reg`.
//! * `register_site` — the `RegisterWorldInterface` call whose RDX carries
//!   this object; `0` where the slot is not registered (`+0x220`; its decoded
//!   pointer is never registered — the follow-on `Alloc(0x980)` object is,
//!   but that object never reaches a slot).
//!
//! ## Counts that reconcile
//!
//! The function makes 80 `Alloc` calls and 84 registrations but only 65 slot
//! stores: 20 registrations belong to objects that are constructed and
//! dropped, and one block (`+0x340`/`+0x358`) stores two slots. One block
//! (`0x140554301..0x140554418`) is anti-tamper code: it reads a cookie from
//! the context object, compares it against a constant and calls junk targets
//! plus one registration with a garbage interface. It allocates nothing and
//! stores no slot.
//!
//! The order of [`SUBSYSTEMS`] is the order of the store instructions, which
//! is not slot order: the first store is `+0x408` (`0x1405542c9`) and the
//! last is `+0x438` (`0x1405556d9`).
//!
//! ## The tail attach pass
//!
//! After the last registration the function walks 21 slots and re-attaches
//! each object's secondary base (`object + 0x10`) through `0x140bc7590`,
//! passing zero when the slot is null. [`TAIL_ATTACH`] is that list in call
//! order; the last entry (`+0x268`) is the only one guarded by a context
//! condition (`cmp qword ptr [rsi+0xe0], 0` at `0x14055597c`).
//!
//! ## Open
//!
//! The rows carry the class identity of each subsystem as its vtable and
//! constructor address; what each class *is* is not read yet, and neither are
//! the 20 registered objects that never reach a slot. `docs/formats/engine.md`
//! has the current state.

/// First offset of the subsystem window (`+0x220`).
pub const SLOT_BASE: u32 = 0x220;

/// Last offset of the subsystem window (`+0x448`, inclusive).
pub const SLOT_END: u32 = 0x448;

/// Stride of the window.
pub const SLOT_STRIDE: u32 = 8;

/// Number of offsets in the window: `(0x448 - 0x220) / 8 + 1`.
pub const SLOT_WINDOW: usize = 70;

/// Number of slots the function writes.
pub const MEMBERS: usize = 65;

/// The shared 28-byte constructor stub: `[obj] = 0x142f7d778`,
/// `[obj+8] = 0x7495174655b5d763`, returns `obj`.
pub const STUB_CTOR: u64 = 0x1_41c5_78c0;

/// `Alloc(0x470, 0)` at `0x140585243` sets the object's size; the window ends
/// `0x28` bytes before the end.
pub const OBJECT_SIZE: u32 = 0x470;

/// `call 0x141baff30` sites in the function: 61 stored, 1 adjustor and 18
/// registered-and-dropped.
pub const ALLOC_CALLS: usize = 80;

/// `call 0x141c52d10` sites: 64 that carry a member slot's object, 20 that
/// register an object never stored.
pub const REGISTER_CALLS: usize = 84;

/// Registrations whose object never reaches a slot.
pub const REGISTER_ONLY: usize = 20;

/// Where the stored pointer comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {{
    /// An inline `Alloc(size, 0)` and the constructor that follows it.
    Alloc {{
        /// The size moved into ECX before the `Alloc`.
        size: u32,
    }},
    /// A factory call that allocates inside itself; the slot sees its result
    /// and this function has no `Alloc` of its own.
    Factory,
    /// `[world + from_slot] + offset`, decoded in place by `0x1405437f0` and
    /// folded by the same expression the constructor uses at `+0x220`.
    Decoded {{
        /// The slot whose object is decoded (`+0x330`).
        from_slot: u32,
        /// The offset into that object (`0x60`).
        offset: u32,
    }},
    /// An adjustor over an object that is registered first:
    /// `object + 0x10 + (i32)[[object+0x10]+4]`.
    Adjustor {{
        /// The object's `Alloc` size.
        object_size: u32,
    }},
}}

/// One slot of the subsystem window.
#[derive(Clone, Copy, Debug)]
pub struct Subsystem {{
    /// Byte offset in the engine object.
    pub slot: u32,
    /// Where the stored pointer came from.
    pub source: Source,
    /// Constructor, factory or decoder that produced it.
    pub producer: u64,
    /// Primary vtable the block stores at `+0x00`, or `0` when the producer
    /// sets its own and the block stores none inline.
    pub vtable: u64,
    /// Further vtable stores in the block: `(offset, vtable)`.
    pub extra: &'static [(u32, u64)],
    /// The `mov qword ptr [engine + slot], reg` instruction.
    pub store_site: u64,
    /// The `RegisterWorldInterface` call carrying this object in RDX, or `0`.
    pub register_site: u64,
}}

/// One entry of the tail attach pass.
#[derive(Clone, Copy, Debug)]
pub struct TailAttach {{
    /// The `call 0x140bc7590`.
    pub site: u64,
    /// The slot it re-attaches.
    pub slot: u32,
    /// True for `+0x268`, the one behind `cmp qword ptr [rsi+0xe0], 0`.
    pub guarded: bool,
}}

/// The five offsets of the window that no instruction writes.
pub const EMPTY_SLOTS: [u32; 5] = [0x228, 0x230, 0x350, 0x418, 0x440];

/// The schedule, in store order: the first entry is `+0x408`,
/// the last `+0x438`.
pub const SUBSYSTEMS: [Subsystem; MEMBERS] = [
{table}
];

/// The tail attach pass (`0x1405556e8..0x14055599f`), in call order.
pub const TAIL_ATTACH: [TailAttach; 21] = [
{tail}
];

impl Subsystem {{
    /// True when the block calls the shared stub [`STUB_CTOR`] before
    /// overwriting the vtable.
    #[must_use]
    pub fn is_stub(&self) -> bool {{
        self.producer == STUB_CTOR
    }}

    /// The inline allocation size, when this slot has one.
    #[must_use]
    pub fn allocation(&self) -> Option<u32> {{
        match self.source {{
            Source::Alloc {{ size }} => Some(size),
            _ => None,
        }}
    }}

    /// True when `RegisterWorldInterface` was called with this object.
    #[must_use]
    pub fn is_registered(&self) -> bool {{
        self.register_site != 0
    }}
}}

/// Index of a member slot in [`SUBSYSTEMS`], or `None` for the five empty
/// offsets and anything outside the window.
///
/// [`SUBSYSTEMS`] is in store order, not slot order, so this is a scan; 65
/// comparisons is cheaper than a second index table.
#[must_use]
pub fn slot_index(slot: u32) -> Option<usize> {{
    if !(SLOT_BASE..=SLOT_END).contains(&slot) || (slot - SLOT_BASE) % SLOT_STRIDE != 0 {{
        return None;
    }}
    SUBSYSTEMS.iter().position(|s| s.slot == slot)
}}

/// The offset of a member slot by its index in [`SUBSYSTEMS`].
#[must_use]
pub fn slot_offset(index: usize) -> u32 {{
    SUBSYSTEMS[index].slot
}}

/// True when the function writes this offset.
#[must_use]
pub fn is_member_slot(slot: u32) -> bool {{
    slot_index(slot).is_some()
}}

/// The row for a slot, if the function writes it.
#[must_use]
pub fn subsystem(slot: u32) -> Option<&'static Subsystem> {{
    slot_index(slot).map(|i| &SUBSYSTEMS[i])
}}

/// Every member row, in store order.
pub fn members() -> impl Iterator<Item = &'static Subsystem> {{
    SUBSYSTEMS.iter()
}}

/// Every offset of the window paired with its row, `None` for the five the
/// function never writes. In offset order.
pub fn window() -> impl Iterator<Item = (u32, Option<&'static Subsystem>)> {{
    (0..SLOT_WINDOW).map(|i| {{
        let slot = SLOT_BASE + i as u32 * SLOT_STRIDE;
        (slot, subsystem(slot))
    }})
}}
'''


def main() -> int:
    text = gen()
    if "--check" in sys.argv:
        current = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if current != text:
            print(f"{OUT} is out of date; run python dev/eng_gen_subsystems.py")
            return 1
        print(f"{OUT} is current")
        return 0
    OUT.write_text(text, encoding="utf-8")
    print(f"wrote {OUT} ({len(text)} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
