//! The client's engine object, rebuilt from the current image.
//!
//! "Engine object" here means the one allocation the client makes at
//! `0x140585220`: `0x470` bytes from `Alloc`, constructed by `0x1405838c0`
//! (`WorldCtor`), whose base constructor is `0x141c507c0` (`WorldBaseCtor`).
//! The same step publishes it in the global at `0x143b7f410`; `0x14058526e` is
//! the only store to that global in the image, and 5,486 rip-relative reads in
//! 3,496 `.pdata` functions read it.
//! The map-load entry `0x14055a0f0` runs this step and then
//! `ConstructEngineSubsystems` (`0x140554220`), which fills `+0x220..+0x448`
//! with subsystem interfaces; that schedule is [`subsystems`].
//!
//! The object has four pieces that this crate models, in the order they were
//! read:
//!
//! * [`codec`] — every handle the object stores is transformed on the way in
//!   and out. The transforms are the client's own out-of-line helpers
//!   (`0x140543a40`, `0x1405674e0`, `0x1405437f0`, `0x1405838a0`) plus one
//!   inline pair in the registry. A `Codec` is that transform, exactly;
//!   `revert` is its inverse, so a test can prove the round trip.
//! * [`registry`] — `world+0x188` is a `u64` array of transformed pointers
//!   with a tagged capacity (bit 31 of the capacity word). `0x141c52d10`
//!   (`RegisterWorldInterface`) appends one; `0x140767da0` grows it by half.
//! * [`interface_map`] — `world+0x198` is a chained hash map whose bucket and
//!   entry sentinel is `0xffff_ffff`, whose hash is `((key >> 4) & 0x7fff_ffff)`
//!   and whose capacity equals its bucket count. `0x141c553a0` allocates and
//!   rehashes it, `0x141c56fb0` inserts.
//! * [`world`] — the `0x470`-byte layout and the constructor's field values,
//!   including the two fields that are only ever written as constants
//!   (`+0x058` holds `encode(null)`, `+0x228..+0x460` is a 71-slot table the
//!   constructor zeroes).
//! * [`subsystems`] — `ConstructEngineSubsystems`' schedule: the 65 member
//!   slots of `+0x220..+0x448`, each with its producer, allocation size,
//!   vtables, store site and registration call, plus the 21-entry tail attach
//!   pass. Generated from a decode of the instruction stream.
//! * [`classes`] — the 35 constructors behind those slots and the vtables each
//!   one installs, in store order and at the object offset it lands on. A class
//!   that installs the same base six times and then six specialised tables is
//!   an aggregation, and the ordering makes that readable.
//!
//! ## What is *not* modelled yet
//!
//! * the free-list erase path of the `+0x198` map — the insert consumes
//!   `+0x20`/`+0x24`, but nothing read so far fills them.
//! * what each subsystem class *is*: [`classes`] carries the constructor and
//!   the vtables as identity, but no class has been read far enough to name it.
//! * the `0x1b8`-byte object `Alloc(0x1b8, 0)` constructs at the end of
//!   `WorldCtor`: the constructor drops the pointer, so only the object's own
//!   global registration is observable.
//!
//! Every constant below is a read expression, cited with the instruction that
//! reads it. `docs/formats/engine.md` carries the reasoning and the evidence
//! class for each one.

pub mod classes;
pub mod codec;
pub mod interface_map;
pub mod registry;
pub mod subsystems;
pub mod world;

pub use codec::Codec;
pub use registry::EncodedPtrArray;
pub use subsystems::Subsystem;
pub use world::World;
