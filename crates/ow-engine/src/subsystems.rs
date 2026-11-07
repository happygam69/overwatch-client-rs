//! The subsystem schedule of `ConstructEngineSubsystems` (`0x140554220`).
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
pub enum Source {
    /// An inline `Alloc(size, 0)` and the constructor that follows it.
    Alloc {
        /// The size moved into ECX before the `Alloc`.
        size: u32,
    },
    /// A factory call that allocates inside itself; the slot sees its result
    /// and this function has no `Alloc` of its own.
    Factory,
    /// `[world + from_slot] + offset`, decoded in place by `0x1405437f0` and
    /// folded by the same expression the constructor uses at `+0x220`.
    Decoded {
        /// The slot whose object is decoded (`+0x330`).
        from_slot: u32,
        /// The offset into that object (`0x60`).
        offset: u32,
    },
    /// An adjustor over an object that is registered first:
    /// `object + 0x10 + (i32)[[object+0x10]+4]`.
    Adjustor {
        /// The object's `Alloc` size.
        object_size: u32,
    },
}

/// One slot of the subsystem window.
#[derive(Clone, Copy, Debug)]
pub struct Subsystem {
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
}

/// One entry of the tail attach pass.
#[derive(Clone, Copy, Debug)]
pub struct TailAttach {
    /// The `call 0x140bc7590`.
    pub site: u64,
    /// The slot it re-attaches.
    pub slot: u32,
    /// True for `+0x268`, the one behind `cmp qword ptr [rsi+0xe0], 0`.
    pub guarded: bool,
}

/// The five offsets of the window that no instruction writes.
pub const EMPTY_SLOTS: [u32; 5] = [0x228, 0x230, 0x350, 0x418, 0x440];

/// The schedule, in store order: the first entry is `+0x408`,
/// the last `+0x438`.
pub const SUBSYSTEMS: [Subsystem; MEMBERS] = [
    Subsystem {
        slot: 0x408,
        source: Source::Alloc { size: 0x100 },
        producer: 0x140662760,
        vtable: 0,
        extra: &[],
        store_site: 0x1405542c9,
        register_site: 0x1405542d3,
    },
    Subsystem {
        slot: 0x2e8,
        source: Source::Alloc { size: 0x2f8 },
        producer: 0x140651630,
        vtable: 0,
        extra: &[],
        store_site: 0x14055443e,
        register_site: 0x140554448,
    },
    Subsystem {
        slot: 0x238,
        source: Source::Alloc { size: 0x7d0 },
        producer: 0x14064f080,
        vtable: 0,
        extra: &[],
        store_site: 0x14055447c,
        register_site: 0x140554486,
    },
    Subsystem {
        slot: 0x290,
        source: Source::Alloc { size: 0x90 },
        producer: 0x14060fd30,
        vtable: 0,
        extra: &[],
        store_site: 0x1405544ac,
        register_site: 0x1405544b6,
    },
    Subsystem {
        slot: 0x250,
        source: Source::Alloc { size: 0x18 },
        producer: 0x141c578c0,
        vtable: 0x142f7fc90,
        extra: &[(0x10, 0x142f7fd08)],
        store_site: 0x1405544f4,
        register_site: 0x1405544fe,
    },
    Subsystem {
        slot: 0x240,
        source: Source::Alloc { size: 0x88 },
        producer: 0x1406c0580,
        vtable: 0,
        extra: &[],
        store_site: 0x140554524,
        register_site: 0x14055452e,
    },
    Subsystem {
        slot: 0x258,
        source: Source::Alloc { size: 0x70 },
        producer: 0x14073fc00,
        vtable: 0,
        extra: &[],
        store_site: 0x140554554,
        register_site: 0x14055455e,
    },
    Subsystem {
        slot: 0x248,
        source: Source::Alloc { size: 0xc0 },
        producer: 0x14055cc60,
        vtable: 0,
        extra: &[],
        store_site: 0x140554584,
        register_site: 0x14055458e,
    },
    Subsystem {
        slot: 0x2d8,
        source: Source::Alloc { size: 0x18 },
        producer: 0x1406b5000,
        vtable: 0,
        extra: &[],
        store_site: 0x1405545b4,
        register_site: 0x1405545be,
    },
    Subsystem {
        slot: 0x2e0,
        source: Source::Alloc { size: 0x58 },
        producer: 0x1406a4de0,
        vtable: 0,
        extra: &[],
        store_site: 0x1405545e4,
        register_site: 0x1405545ee,
    },
    Subsystem {
        slot: 0x2f0,
        source: Source::Alloc { size: 0x390 },
        producer: 0x14072c560,
        vtable: 0,
        extra: &[],
        store_site: 0x140554614,
        register_site: 0x14055461e,
    },
    Subsystem {
        slot: 0x2f8,
        source: Source::Alloc { size: 0x88 },
        producer: 0x140598900,
        vtable: 0,
        extra: &[],
        store_site: 0x140554644,
        register_site: 0x14055464e,
    },
    Subsystem {
        slot: 0x300,
        source: Source::Alloc { size: 0x18 },
        producer: 0x141c578c0,
        vtable: 0x142f7eb90,
        extra: &[(0x10, 0x142f7ec08)],
        store_site: 0x14055468c,
        register_site: 0x140554696,
    },
    Subsystem {
        slot: 0x308,
        source: Source::Alloc { size: 0x30 },
        producer: 0x140bdf740,
        vtable: 0x142f7f298,
        extra: &[],
        store_site: 0x1405546c9,
        register_site: 0x1405546d3,
    },
    Subsystem {
        slot: 0x310,
        source: Source::Alloc { size: 0x68 },
        producer: 0x140657b40,
        vtable: 0,
        extra: &[],
        store_site: 0x1405546f9,
        register_site: 0x140554703,
    },
    Subsystem {
        slot: 0x318,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f7f5c8,
        extra: &[],
        store_site: 0x140554736,
        register_site: 0x140554740,
    },
    Subsystem {
        slot: 0x320,
        source: Source::Alloc { size: 0x10 },
        producer: 0x1406928b0,
        vtable: 0,
        extra: &[],
        store_site: 0x140554766,
        register_site: 0x140554770,
    },
    Subsystem {
        slot: 0x328,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f80088,
        extra: &[],
        store_site: 0x1405547a3,
        register_site: 0x1405547ad,
    },
    Subsystem {
        slot: 0x388,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f7f640,
        extra: &[],
        store_site: 0x1405547e7,
        register_site: 0x1405547f5,
    },
    Subsystem {
        slot: 0x390,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f7f398,
        extra: &[],
        store_site: 0x140554828,
        register_site: 0x140554832,
    },
    Subsystem {
        slot: 0x398,
        source: Source::Alloc { size: 0x18 },
        producer: 0x141c578c0,
        vtable: 0x142f7eca0,
        extra: &[(0x10, 0x142f7ed18)],
        store_site: 0x140554870,
        register_site: 0x14055487a,
    },
    Subsystem {
        slot: 0x260,
        source: Source::Alloc { size: 0x40 },
        producer: 0x1406735c0,
        vtable: 0,
        extra: &[],
        store_site: 0x1405548a9,
        register_site: 0x1405548b3,
    },
    Subsystem {
        slot: 0x268,
        source: Source::Alloc { size: 0x58 },
        producer: 0x1405dac00,
        vtable: 0,
        extra: &[],
        store_site: 0x1405548d9,
        register_site: 0x1405548e3,
    },
    Subsystem {
        slot: 0x298,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f7f820,
        extra: &[],
        store_site: 0x140554916,
        register_site: 0x140554920,
    },
    Subsystem {
        slot: 0x2a0,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f7f7a8,
        extra: &[],
        store_site: 0x140554953,
        register_site: 0x14055495d,
    },
    Subsystem {
        slot: 0x2a8,
        source: Source::Alloc { size: 0x18 },
        producer: 0x141c578c0,
        vtable: 0x142f7f410,
        extra: &[(0x10, 0x142f7f488)],
        store_site: 0x14055499b,
        register_site: 0x1405549a5,
    },
    Subsystem {
        slot: 0x2b0,
        source: Source::Alloc { size: 0x18 },
        producer: 0x1406b58d0,
        vtable: 0,
        extra: &[],
        store_site: 0x1405549cb,
        register_site: 0x1405549d5,
    },
    Subsystem {
        slot: 0x2b8,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f7fc18,
        extra: &[],
        store_site: 0x140554a08,
        register_site: 0x140554a12,
    },
    Subsystem {
        slot: 0x2c0,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f7f898,
        extra: &[],
        store_site: 0x140554a45,
        register_site: 0x140554a4f,
    },
    Subsystem {
        slot: 0x2c8,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f7ee30,
        extra: &[],
        store_site: 0x140554a82,
        register_site: 0x140554a8c,
    },
    Subsystem {
        slot: 0x2d0,
        source: Source::Alloc { size: 0x10 },
        producer: 0x1407493c0,
        vtable: 0,
        extra: &[],
        store_site: 0x140554ab2,
        register_site: 0x140554abc,
    },
    Subsystem {
        slot: 0x340,
        source: Source::Factory,
        producer: 0x140594090,
        vtable: 0,
        extra: &[],
        store_site: 0x140554ac6,
        register_site: 0x140554ae3,
    },
    Subsystem {
        slot: 0x358,
        source: Source::Factory,
        producer: 0x1406cc990,
        vtable: 0,
        extra: &[],
        store_site: 0x140554adc,
        register_site: 0x140554af2,
    },
    Subsystem {
        slot: 0x348,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f7e210,
        extra: &[],
        store_site: 0x140554b2c,
        register_site: 0x140554b3a,
    },
    Subsystem {
        slot: 0x448,
        source: Source::Adjustor { object_size: 0x68 },
        producer: 0x140723890,
        vtable: 0,
        extra: &[],
        store_site: 0x140554bba,
        register_site: 0x140554b8f,
    },
    Subsystem {
        slot: 0x270,
        source: Source::Alloc { size: 0x18 },
        producer: 0x141c578c0,
        vtable: 0x142f7ef88,
        extra: &[(0x10, 0x142f7f000)],
        store_site: 0x140554c4a,
        register_site: 0x140554c54,
    },
    Subsystem {
        slot: 0x278,
        source: Source::Alloc { size: 0xb0 },
        producer: 0x141c578c0,
        vtable: 0x142f7e758,
        extra: &[
            (0x10, 0x142f7e6c8),
            (0x38, 0x142f7e6f8),
            (0x60, 0x142f7e728),
            (0x88, 0x142f7e740),
        ],
        store_site: 0x140554cff,
        register_site: 0x140554d09,
    },
    Subsystem {
        slot: 0x280,
        source: Source::Alloc { size: 0x38 },
        producer: 0x141c578c0,
        vtable: 0x142f7e638,
        extra: &[(0x10, 0x142f7e620)],
        store_site: 0x140554d56,
        register_site: 0x140554d60,
    },
    Subsystem {
        slot: 0x288,
        source: Source::Alloc { size: 0x18 },
        producer: 0x1405bc5c0,
        vtable: 0,
        extra: &[],
        store_site: 0x140554d83,
        register_site: 0x140554d9b,
    },
    Subsystem {
        slot: 0x360,
        source: Source::Alloc { size: 0x1b8 },
        producer: 0x14066b100,
        vtable: 0,
        extra: &[],
        store_site: 0x140554e99,
        register_site: 0x140554ea3,
    },
    Subsystem {
        slot: 0x368,
        source: Source::Alloc { size: 0x40 },
        producer: 0x14061c0a0,
        vtable: 0,
        extra: &[],
        store_site: 0x140554ec9,
        register_site: 0x140554ed3,
    },
    Subsystem {
        slot: 0x378,
        source: Source::Alloc { size: 0x10 },
        producer: 0x1406c5380,
        vtable: 0,
        extra: &[],
        store_site: 0x140554f86,
        register_site: 0x140554f97,
    },
    Subsystem {
        slot: 0x3a0,
        source: Source::Alloc { size: 0x18 },
        producer: 0x14061d1e0,
        vtable: 0,
        extra: &[],
        store_site: 0x14055500a,
        register_site: 0x140555014,
    },
    Subsystem {
        slot: 0x3a8,
        source: Source::Alloc { size: 0x10 },
        producer: 0x1405cf4f0,
        vtable: 0,
        extra: &[],
        store_site: 0x14055503a,
        register_site: 0x140555044,
    },
    Subsystem {
        slot: 0x3b0,
        source: Source::Alloc { size: 0x18 },
        producer: 0x1406c6d10,
        vtable: 0,
        extra: &[],
        store_site: 0x14055506a,
        register_site: 0x140555074,
    },
    Subsystem {
        slot: 0x3c8,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f80c28,
        extra: &[],
        store_site: 0x1405550a7,
        register_site: 0x1405550b1,
    },
    Subsystem {
        slot: 0x3d0,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f80b38,
        extra: &[],
        store_site: 0x1405550e4,
        register_site: 0x1405550ee,
    },
    Subsystem {
        slot: 0x3d8,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f80ca0,
        extra: &[],
        store_site: 0x140555121,
        register_site: 0x14055512b,
    },
    Subsystem {
        slot: 0x3f8,
        source: Source::Alloc { size: 0x18 },
        producer: 0x141c578c0,
        vtable: 0x142f7e0f0,
        extra: &[(0x10, 0x142f7e168)],
        store_site: 0x140555169,
        register_site: 0x140555173,
    },
    Subsystem {
        slot: 0x400,
        source: Source::Alloc { size: 0x18 },
        producer: 0x141c578c0,
        vtable: 0x142f7e300,
        extra: &[(0x10, 0x142f7e378)],
        store_site: 0x1405551b1,
        register_site: 0x1405551bb,
    },
    Subsystem {
        slot: 0x410,
        source: Source::Alloc { size: 0x18 },
        producer: 0x141c578c0,
        vtable: 0x142f7ec18,
        extra: &[(0x10, 0x142f7ec90)],
        store_site: 0x1405551f9,
        register_site: 0x140555203,
    },
    Subsystem {
        slot: 0x338,
        source: Source::Alloc { size: 0xd0 },
        producer: 0x14074a300,
        vtable: 0,
        extra: &[],
        store_site: 0x140555273,
        register_site: 0x14055527d,
    },
    Subsystem {
        slot: 0x330,
        source: Source::Alloc { size: 0x68 },
        producer: 0x14074c5a0,
        vtable: 0,
        extra: &[],
        store_site: 0x1405552d1,
        register_site: 0x1405552db,
    },
    Subsystem {
        slot: 0x220,
        source: Source::Decoded {
            from_slot: 0x330,
            offset: 0x60,
        },
        producer: 0x1405437f0,
        vtable: 0,
        extra: &[],
        store_site: 0x140555372,
        register_site: 0,
    },
    Subsystem {
        slot: 0x370,
        source: Source::Alloc { size: 0x28 },
        producer: 0x1406b4570,
        vtable: 0,
        extra: &[],
        store_site: 0x1405553b9,
        register_site: 0x1405553ca,
    },
    Subsystem {
        slot: 0x380,
        source: Source::Alloc { size: 0x10 },
        producer: 0x140741560,
        vtable: 0,
        extra: &[],
        store_site: 0x1405553ed,
        register_site: 0x1405553fe,
    },
    Subsystem {
        slot: 0x3c0,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f7f6b8,
        extra: &[],
        store_site: 0x140555431,
        register_site: 0x14055543b,
    },
    Subsystem {
        slot: 0x3b8,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f7f730,
        extra: &[],
        store_site: 0x14055546e,
        register_site: 0x140555478,
    },
    Subsystem {
        slot: 0x3e0,
        source: Source::Alloc { size: 0x30 },
        producer: 0x14061c4c0,
        vtable: 0,
        extra: &[],
        store_site: 0x14055549e,
        register_site: 0x1405554a8,
    },
    Subsystem {
        slot: 0x3e8,
        source: Source::Alloc { size: 0x18 },
        producer: 0x141c578c0,
        vtable: 0x142f7eb08,
        extra: &[(0x10, 0x142f7eb80)],
        store_site: 0x1405554e6,
        register_site: 0x1405554f0,
    },
    Subsystem {
        slot: 0x3f0,
        source: Source::Alloc { size: 0x18 },
        producer: 0x141c578c0,
        vtable: 0x142f7e388,
        extra: &[(0x10, 0x142f7e400)],
        store_site: 0x14055552e,
        register_site: 0x140555538,
    },
    Subsystem {
        slot: 0x420,
        source: Source::Alloc { size: 0x18 },
        producer: 0x141c578c0,
        vtable: 0x142f7ea90,
        extra: &[(0x10, 0x142f7e168)],
        store_site: 0x140555622,
        register_site: 0x14055562c,
    },
    Subsystem {
        slot: 0x428,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f80bb0,
        extra: &[],
        store_site: 0x14055565f,
        register_site: 0x140555669,
    },
    Subsystem {
        slot: 0x430,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f7f320,
        extra: &[],
        store_site: 0x14055569c,
        register_site: 0x1405556a6,
    },
    Subsystem {
        slot: 0x438,
        source: Source::Alloc { size: 0x10 },
        producer: 0x141c578c0,
        vtable: 0x142f7f1d0,
        extra: &[],
        store_site: 0x1405556d9,
        register_site: 0x1405556e3,
    },
];

/// The tail attach pass (`0x1405556e8..0x14055599f`), in call order.
pub const TAIL_ATTACH: [TailAttach; 21] = [
    TailAttach {
        site: 0x140555704,
        slot: 0x368,
        guarded: false,
    },
    TailAttach {
        site: 0x140555725,
        slot: 0x288,
        guarded: false,
    },
    TailAttach {
        site: 0x140555746,
        slot: 0x2e8,
        guarded: false,
    },
    TailAttach {
        site: 0x140555767,
        slot: 0x2d8,
        guarded: false,
    },
    TailAttach {
        site: 0x140555788,
        slot: 0x300,
        guarded: false,
    },
    TailAttach {
        site: 0x1405557a9,
        slot: 0x2e0,
        guarded: false,
    },
    TailAttach {
        site: 0x1405557ca,
        slot: 0x270,
        guarded: false,
    },
    TailAttach {
        site: 0x1405557eb,
        slot: 0x238,
        guarded: false,
    },
    TailAttach {
        site: 0x14055580c,
        slot: 0x3b0,
        guarded: false,
    },
    TailAttach {
        site: 0x14055582d,
        slot: 0x250,
        guarded: false,
    },
    TailAttach {
        site: 0x14055584e,
        slot: 0x240,
        guarded: false,
    },
    TailAttach {
        site: 0x14055586f,
        slot: 0x290,
        guarded: false,
    },
    TailAttach {
        site: 0x140555890,
        slot: 0x248,
        guarded: false,
    },
    TailAttach {
        site: 0x1405558b1,
        slot: 0x398,
        guarded: false,
    },
    TailAttach {
        site: 0x1405558d2,
        slot: 0x3e0,
        guarded: false,
    },
    TailAttach {
        site: 0x1405558f3,
        slot: 0x3e8,
        guarded: false,
    },
    TailAttach {
        site: 0x140555914,
        slot: 0x3f0,
        guarded: false,
    },
    TailAttach {
        site: 0x140555935,
        slot: 0x3f8,
        guarded: false,
    },
    TailAttach {
        site: 0x140555956,
        slot: 0x400,
        guarded: false,
    },
    TailAttach {
        site: 0x140555977,
        slot: 0x410,
        guarded: false,
    },
    TailAttach {
        site: 0x14055599f,
        slot: 0x268,
        guarded: true,
    },
];

impl Subsystem {
    /// True when the block calls the shared stub [`STUB_CTOR`] before
    /// overwriting the vtable.
    #[must_use]
    pub fn is_stub(&self) -> bool {
        self.producer == STUB_CTOR
    }

    /// The inline allocation size, when this slot has one.
    #[must_use]
    pub fn allocation(&self) -> Option<u32> {
        match self.source {
            Source::Alloc { size } => Some(size),
            _ => None,
        }
    }

    /// True when `RegisterWorldInterface` was called with this object.
    #[must_use]
    pub fn is_registered(&self) -> bool {
        self.register_site != 0
    }
}

/// Index of a member slot in [`SUBSYSTEMS`], or `None` for the five empty
/// offsets and anything outside the window.
///
/// [`SUBSYSTEMS`] is in store order, not slot order, so this is a scan; 65
/// comparisons is cheaper than a second index table.
#[must_use]
pub fn slot_index(slot: u32) -> Option<usize> {
    if !(SLOT_BASE..=SLOT_END).contains(&slot) || (slot - SLOT_BASE) % SLOT_STRIDE != 0 {
        return None;
    }
    SUBSYSTEMS.iter().position(|s| s.slot == slot)
}

/// The offset of a member slot by its index in [`SUBSYSTEMS`].
#[must_use]
pub fn slot_offset(index: usize) -> u32 {
    SUBSYSTEMS[index].slot
}

/// True when the function writes this offset.
#[must_use]
pub fn is_member_slot(slot: u32) -> bool {
    slot_index(slot).is_some()
}

/// The row for a slot, if the function writes it.
#[must_use]
pub fn subsystem(slot: u32) -> Option<&'static Subsystem> {
    slot_index(slot).map(|i| &SUBSYSTEMS[i])
}

/// Every member row, in store order.
pub fn members() -> impl Iterator<Item = &'static Subsystem> {
    SUBSYSTEMS.iter()
}

/// Every offset of the window paired with its row, `None` for the five the
/// function never writes. In offset order.
pub fn window() -> impl Iterator<Item = (u32, Option<&'static Subsystem>)> {
    (0..SLOT_WINDOW).map(|i| {
        let slot = SLOT_BASE + i as u32 * SLOT_STRIDE;
        (slot, subsystem(slot))
    })
}
