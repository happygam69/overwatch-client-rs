//! The `0x470`-byte engine object: layout and constructor.
//!
//! The world-creating step at `0x140585220` (the one the map-load entry
//! `0x14055a0f0` runs before `ConstructEngineSubsystems` `0x140554220`)
//! (`Alloc(0x470, 0)`), calls `WorldCtor` (`0x1405838c0`) on it, and stores the
//! result in the global at `0x143b7f410`. `WorldCtor` starts by calling
//! `WorldBaseCtor` (`0x141c507c0`) and then writes everything from `+0x1c8` on.
//!
//! Vtables, in the order the constructors install them: the base constructor
//! writes `0x14328c990`, the derived one overwrites it with `0x142f81090` and
//! finally with `0x142f83aa0`. So the live object's vtable is the third one and
//! the other two only exist while the object is half-built.
//!
//! ## Every field either constructor writes
//!
//! ```text
//! +0x000 vtable          0x14328c990 -> 0x142f81090 -> 0x142f83aa0
//! +0x008 flag_08         (the fifth argument == 0); see below
//! +0x00c interlocked_0c  0 (interlocked exchange)
//! +0x010 d_10            0
//! +0x018 enc_q_18        MAGIC_18 first, then overwritten: see below
//! +0x020 q_20            0
//! +0x028 d_28            0
//! +0x02c d_2c            0x400, written as the low half of a u64 at +0x2c
//! +0x030 d_30            0, the high half of that same write
//! +0x038 enc_seed_38     SEED_38(0x627dce6e1d5603a4) ^ 0xd674a580a443b55e
//! +0x040 d_40            0
//! +0x048 q_48            0
//! +0x050 q_50            0
//! +0x058 enc_q_58        INTERFACE_ARRAY(0) first, then overwritten: below
//! +0x060 d_60            0
//! +0x068 q_68            0
//! +0x070 q_70            0
//! +0x078 b_78            0 (byte)
//! +0x07c d_7c            0xffffffff
//! +0x080 d_80            0
//! +0x088 q_88            0
//! +0x090 q_90            0
//! +0x098 d_98            0
//! +0x0a0 q_a0            0
//! +0x0a8 q_a8            0
//! +0x0b0 enc_seed_b0     rol(((SEED_B0(0) + 0xcf64407ada39d02e)
//!                              ^ 0xb2b338251ec2701), 0x17)
//! +0x0b8 q_b8            0
//! +0x0c0 q_c0            0
//! +0x0c8 zero_c8         sub-constructor `0x141c23430(+0xc8, 0x20)`
//! +0x108 zero_108        sub-constructor `0x141c23430(+0x108, 0x20)`
//! +0x130 magic_130       0x389aae49b6cecf17, the same constant as the first
//!                          write to +0x018
//! +0x148 d_148           `x % 0x21`; `x` is read from a TLS-indexed table
//! +0x150 q_150           0
//! +0x158 q_158           0
//! +0x160 q_160           0x8b8bda8c7a56872b
//! +0x168 q_168           0
//! +0x170 d_170           0 (interlocked exchange)
//! +0x178 q_178           the sixth argument (`[rsp+0x98]` at entry)
//! +0x180 d_180           0
//! +0x188 iface_array     { 0, 0, 0 }, grown by `RegisterWorldInterface`
//! +0x1b8 d_1b8          0xffffffff
//! +0x1bc d_1bc          0
//! +0x1c0 b_1c0          0 (byte)
//! +0x1c8 q_1c8           0
//! +0x1d0 q_1d0           0
//! +0x1d8 sub_1d8         { 0, 0, 0 }   (helper 0x140c595e0)
//! +0x1f0 d_1f0           0x10
//! +0x1f4 f_1f4           0.25 (0x3e800000)
//! +0x1f8 b_1f8           1 while constructing, 0 before returning
//! +0x1fc d_1fc           0
//! +0x200 sub_200         { b0: 0, d10: 0, q14: 0x10, d1c: 0 }
//!                        (helper 0x141bb8e80)
//! +0x220 enc_world_id    see `World::with_a5`; the subsystem pass overwrites
//!                          it with the same expression applied to a decoded
//!                          pointer (`DecodeEnginePtr220` 0x1405437f0 + inline
//!                          chain at 0x14055532a..0x140555372)
//! +0x228 table[0x47]     0 — 71 qwords, from +0x228 up to +0x460
//! +0x460 enc_seed_460    OBJ_460(0), then overwritten by OBJ_460(the 0x1b8
//!                          object); the first form equals
//!                          SEED_B(0xf45f13eca6129815) ^ 0x689f22ff8960a2c7
//! +0x468 d_468           the u32 global at `0x1441880e8` (zero-fill: 0)
//! ```
//!
//! ## The base constructor's arguments
//!
//! Entry spills `rdx`, `r8`, `r9d` to `[rsp+0x10]`, `[rsp+0x18]`, `[rsp+0x20]`,
//! and `[rsp+0x20]` is the home slot of the **fifth** argument: the first thing
//! the function does is `cmp dword [rsp+0x90], 0` (0x141c507df), i.e. compare
//! that fifth argument against zero and store the result as the byte at `+0x08`.
//! `rdx`/`r8` are only spilled into a stack record that never reaches the
//! object; `r9` and the two stack arguments land at `+0x08` and `+0x178`.
//!
//! ## The two fields written twice
//!
//! `+0x018`: `MAGIC_18` at 0x141c5081b is a placeholder. At 0x141c50aa0 the
//! constructor overwrites it with an `Alloc`ed pointer put through the
//! `0x18317a531d116c0c` codec family (`sub`, `ror 0x1a`,
//! `add 0x8fb1b242c9b3a42d`, `xor 0x73cc01350b9bc072`), then uses the decoded
//! addresses to fill a table of `0x10000 / 0x10` entries in a loop at
//! 0x141c50ac0. So `+0x018` is an encoded heap pointer, exactly like `+0x460`.
//! [`World::encode_q_18`] reproduces the expression.
//!
//! `+0x058`: `INTERFACE_ARRAY(0)` at 0x141c5086c is likewise a placeholder. At
//! 0x141c50b76 it is replaced by
//! `((a3 ^ 0x238e31e8cd062f58) ror 0x31) + 0x34b5f414245bf6ef`, where `a3` is
//! the argument the caller passed in `r8`. [`World::encode_q_58`] reproduces
//! that expression.
//!
//! [`World::new`] stops after the first write to both fields, the same way it
//! does for `+0x460`: the values the client finally stores depend on the
//! allocator or on the caller's argument, not on the image.
//!
//! ## What the object is allocated in, and the one open byte range
//!
//! `Alloc(0x470, 0)` passes flags `0`, and flags bit 0 is the zero-fill option
//! (`Alloc` calls the `memset` helper `0x141c23430` only when it is set), so
//! the allocation is *not* zeroed by `Alloc`. Neither constructor writes
//! `+0x044`, `+0x064`, `+0x09c`, `+0x0d0..+0x0f8`, `+0x110..+0x130` or
//! `+0x0e8..+0x1b8`, yet `RegisterWorldInterface` reads `+0x188` as
//! `{ data, len, capacity }` and
//! pushes into it before anything else touches the object. The object must
//! therefore arrive zeroed: either the global allocator behind
//! `_DAT_144188200`'s `+0x30` slot zeroes (its vtable is built at runtime, so
//! the image cannot settle it) or some unread caller zeroes the range. This
//! crate zeroes the whole allocation before applying the constructor's writes
//! and records the question in `docs/formats/engine.md`.

use crate::codec::{BACK_PTR, INTERFACE_ARRAY, OBJ_460, SEED_38, SEED_A, SEED_B0};

/// The base class's vtable, installed first by `0x141c507c0`.
pub const VTABLE_BASE: u64 = 0x1_4328_c990;

/// The intermediate vtable `0x1405838c0` installs after the base constructor.
pub const VTABLE_INTERMEDIATE: u64 = 0x1_42f8_1090;

/// The live vtable of a fully constructed object.
pub const VTABLE_FINAL: u64 = 0x1_42f8_3aa0;

/// The constant `WorldBaseCtor` stores at `+0x018`.
pub const MAGIC_18: u64 = 0x389a_ae49_b6ce_cf17;

/// Number of `u64` slots the derived constructor zeroes at `+0x228`.
pub const TABLE_LEN: usize = 0x47;

/// The sub-object at `+0x1d8` (helper `0x140c595e0`: two zeroed qwords and a
/// zeroed dword at `+0x10`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Sub1d8 {
    /// `+0x1d8`
    pub q0: u64,
    /// `+0x1e0`
    pub q8: u64,
    /// `+0x1e8`
    pub d10: u32,
    /// `+0x1ec`, untouched by the helper.
    pub pad: u32,
}

/// The sub-object at `+0x200` (helper `0x141bb8e80`).
///
/// The helper writes a byte at `+0x200`, a dword at `+0x210`, a qword at
/// `+0x214` — unaligned — and a dword at `+0x21c`, which is why this is
/// packed: the client's own object has no alignment requirement inside this
/// range.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct Sub200 {
    /// `+0x200`
    pub b0: u8,
    /// `+0x201..+0x210`, untouched.
    pub pad: [u8; 0x0f],
    /// `+0x210`
    pub d10: u32,
    /// `+0x214`
    pub q14: u64,
    /// `+0x21c`
    pub d1c: u32,
}

impl core::fmt::Debug for Sub200 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Copy out of the packed fields before formatting them.
        let (b0, d10, q14, d1c) = (self.b0, self.d10, self.q14, self.d1c);
        f.debug_struct("Sub200")
            .field("b0", &b0)
            .field("d10", &d10)
            .field("q14", &q14)
            .field("d1c", &d1c)
            .finish()
    }
}

/// The engine object, `0x470` bytes, field for field.
///
/// Field names are the offsets the client uses, except where the constructor's
/// own values settle the meaning (`magic_18`, the two encoded seeds, the table
/// and the interface array at `+0x188`). Anything left as `opaque_*` or with a
/// bare `d_`/`q_` name has not been read beyond "the constructor writes zero
/// here".
#[repr(C)]
pub struct World {
    /// `+0x000`: vtable, [`VTABLE_FINAL`] once construction finishes.
    pub vtable: u64,
    /// `+0x008`: `(the fifth argument == 0)`.
    pub flag_08: u8,
    /// `+0x009`: padding.
    pub pad_09: [u8; 3],
    /// `+0x00c`: zeroed with an interlocked exchange.
    pub interlocked_0c: u32,
    /// `+0x010`: the constructor stores a zero dword here (0x141c50a55).
    pub opaque_10: [u8; 8],
    /// `+0x018`: [`MAGIC_18`], which the constructor later overwrites with an
    /// encoded heap pointer — see [`World::encode_q_18`].
    pub magic_18: u64,
    /// `+0x020`
    pub q_20: u64,
    /// `+0x028`
    pub d_28: u32,
    /// `+0x02c`: `0x400`.
    pub d_2c: u32,
    /// `+0x030`: `0`, written as the high half of a `u64 0x400` at `+0x2c`.
    pub d_30: u32,
    /// `+0x034`: untouched.
    pub d_34: u32,
    /// `+0x038`: [`SEED_38`] fold, see the module docs.
    pub enc_seed_38: u64,
    /// `+0x040`
    pub d_40: u32,
    /// `+0x044`: untouched.
    pub d_44: u32,
    /// `+0x048`
    pub q_48: u64,
    /// `+0x050`
    pub q_50: u64,
    /// `+0x058`: a transformed pointer, `INTERFACE_ARRAY(0)` at first, then
    /// overwritten from the caller's `r8` argument — see
    /// [`World::encode_q_58`].
    pub enc_ptr_58: u64,
    /// `+0x060`
    pub d_60: u32,
    /// `+0x064`: untouched.
    pub d_64: u32,
    /// `+0x068`
    pub q_68: u64,
    /// `+0x070`
    pub q_70: u64,
    /// `+0x078`: untouched.
    pub opaque_78: [u8; 0x10],
    /// `+0x088`
    pub q_88: u64,
    /// `+0x090`
    pub q_90: u64,
    /// `+0x098`
    pub d_98: u32,
    /// `+0x09c`: untouched.
    pub d_9c: u32,
    /// `+0x0a0`
    pub q_a0: u64,
    /// `+0x0a8`
    pub q_a8: u64,
    /// `+0x0b0`: [`SEED_B0`] fold, see the module docs.
    pub enc_seed_b0: u64,
    /// `+0x0b8`
    pub q_b8: u64,
    /// `+0x0c0`
    pub q_c0: u64,
    /// `+0x0c8`: `memset(0x20)` by the base constructor.
    pub zero_c8: [u8; 0x20],
    /// `+0x0e8..+0x1c8`: not written by either constructor (see the module
    /// docs).
    pub opaque_e8: [u8; 0xe0],
    /// `+0x1c8`
    pub q_1c8: u64,
    /// `+0x1d0`
    pub q_1d0: u64,
    /// `+0x1d8`
    pub sub_1d8: Sub1d8,
    /// `+0x1f0`: `0x10`.
    pub d_1f0: u32,
    /// `+0x1f4`: `0.25`.
    pub f_1f4: f32,
    /// `+0x1f8`: `1` during construction, `0` when the constructor returns.
    pub b_1f8: u8,
    /// `+0x1f9`: padding.
    pub pad_1f9: [u8; 3],
    /// `+0x1fc`
    pub d_1fc: u32,
    /// `+0x200`
    pub sub_200: Sub200,
    /// `+0x220`: the encoded world id, [`SEED_A`]-folded from a zero seed.
    pub enc_world_id: u64,
    /// `+0x228`: 71 zeroed `u64` slots up to `+0x460`.
    pub table: [u64; TABLE_LEN],
    /// `+0x460`: written twice. [`World::new`] leaves the first fold
    /// (the same address codec at 0); the constructor's last write is
    /// [`World::encode_obj_460`] of an allocation address, see the module docs.
    pub enc_seed_460: u64,
    /// `+0x468`: copied from the u32 global at `0x1441880e8`, which the image
    /// zero-fills. `0x140585220` then overwrites it with the dword at `+0x138`
    /// of the object `0x141c51cc0` returns.
    pub d_468: u32,
    /// `+0x46c`: untouched.
    pub d_46c: u32,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    /// The object as `ConstructEngineSubsystems` builds it: the bootstrap's
    /// `a5 = 0`, so `flag_08` is `1`.
    #[must_use]
    pub fn new() -> Self {
        Self::with_a5(0)
    }

    /// [`World::new`] with the base constructor's `a5` argument, which is the
    /// only one of its four arguments that reaches the object.
    #[must_use]
    pub fn with_a5(a5: i32) -> Self {
        // The client relies on the allocation being zero in the untouched
        // ranges; see the module docs.
        let mut w: Self = unsafe { core::mem::zeroed() };

        // --- WorldBaseCtor, 0x141c507c0 ---
        w.vtable = VTABLE_BASE;
        w.flag_08 = u8::from(a5 == 0);
        w.interlocked_0c = 0;
        w.magic_18 = MAGIC_18;
        w.q_20 = 0;
        w.d_28 = 0;
        w.d_2c = 0x400;
        w.d_30 = 0;
        w.enc_seed_38 = SEED_38.apply(0x627dce6e1d5603a4) ^ 0xd674a580a443b55e;
        w.d_40 = 0;
        w.q_48 = 0;
        w.q_50 = 0;
        w.enc_ptr_58 = INTERFACE_ARRAY.apply(0);
        w.d_60 = 0;
        w.q_68 = 0;
        w.q_70 = 0;
        w.q_88 = 0;
        w.q_90 = 0;
        w.d_98 = 0;
        w.q_a0 = 0;
        w.q_a8 = 0;
        w.enc_seed_b0 = ((SEED_B0.apply(0).wrapping_add(0xcf64407ada39d02e)) ^ 0xb2b338251ec2701)
            .rotate_left(0x17);
        w.q_b8 = 0;
        w.q_c0 = 0;
        w.zero_c8 = [0; 0x20];

        // --- WorldCtor, 0x1405838c0 ---
        w.vtable = VTABLE_INTERMEDIATE;
        w.q_1c8 = 0;
        w.q_1d0 = 0;
        w.sub_1d8 = Sub1d8::default();
        w.d_1f0 = 0x10;
        w.f_1f4 = 0.25;
        w.b_1f8 = 1;
        w.d_1fc = 0;
        w.sub_200 = Sub200 {
            b0: 0,
            pad: [0; 0x0f],
            d10: 0,
            q14: 0x10,
            d1c: 0,
        };
        let seeded =
            ((SEED_A.apply(0) ^ 0x6e).wrapping_add(0x6c5df24a0815916e)) ^ 0x3c1986ede8c2b44a;
        // `uVar1 >> 0x10 | uVar1 << 0x30` in the constructor.
        let ror16 = seeded.rotate_right(0x10);
        w.enc_world_id = (ror16 ^ 0xc2300451bcb9e837).wrapping_add(0x5c04429ca265e813);
        w.vtable = VTABLE_FINAL;
        w.table = [0; TABLE_LEN];
        // First write to +0x460 (0x140583bb2): the same address codec the last
        // write uses, evaluated at 0. The constructor's stored constant
        // 0xf45f13eca6129815 is exactly the codec's inner mix of 0, so
        // `SEED_B(0xf45f13eca6129815) ^ 0x689f22ff8960a2c7` is the same value.
        w.enc_seed_460 = OBJ_460.apply(0);
        // `mov eax, dword ptr [rip + 0x3c04524]` (0x140583bbe) copies the u32
        // global at 0x1441880e8. The image zero-fills it, so the constructor
        // reads 0 unless something wrote it earlier in the process.
        w.d_468 = 0;
        // Then `Alloc(0x1b8, 0)` and `0x140be33c0` on the result; that function
        // fills the object and returns its own argument, so the constructor's
        // `rbx` is the object's address. At 0x140583c2b it overwrites +0x460
        // with `OBJ_460` of that address and drops the pointer: the field is an
        // encoded heap address, which [`World::encode_obj_460`] reproduces.
        w.b_1f8 = 0;
        w
    }

    /// The constructor's **last** write to `+0x460` (0x140583c2b): the encoded
    /// address of the `0x1b8`-byte object the constructor builds and drops.
    ///
    /// [`World::new`] stops after the first write to that field, because the
    /// value the client ends up storing depends on where that allocation landed.
    /// Applying this to the address gives exactly what the client stores, and
    /// [`OBJ_460`](crate::codec::OBJ_460)'s inverse gives the address back.
    #[must_use]
    pub fn encode_obj_460(obj: u64) -> u64 {
        OBJ_460.apply(obj)
    }

    /// The constructor's **final** write to `+0x018` (0x141c50aa0): the codec of
    /// the `0x18317a531d116c0c` family applied to an `Alloc`ed pointer.
    #[must_use]
    pub fn encode_q_18(ptr: u64) -> u64 {
        ptr.wrapping_sub(0x1831_7a53_1d11_6c0c)
            .rotate_right(0x1a)
            .wrapping_add(0x8fb1_b242_c9b3_a42d)
            ^ 0x73cc_0135_0b9b_c072
    }

    /// The constructor's **final** write to `+0x058` (0x141c50b76), from the
    /// caller's third argument.
    #[must_use]
    pub fn encode_q_58(a3: u64) -> u64 {
        (a3 ^ 0x238e_31e8_cd06_2f58)
            .rotate_right(0x31)
            .wrapping_add(0x34b5_f414_245b_f6ef)
    }

    /// The encoded back pointer a registered subsystem keeps at `+0x008`, as
    /// `0x141c56fb0` writes it: `BACK_PTR(self)`.
    ///
    /// It lives in the subsystem, not in this object; it is here because the
    /// codec belongs to the world object's own either way.
    #[must_use]
    pub fn back_pointer_to(&self) -> u64 {
        BACK_PTR.apply(self as *const Self as u64)
    }

    /// The address of a *member* subsystem slot inside this object, or `None`.
    ///
    /// `ConstructEngineSubsystems` (`0x140554220`) writes 65 of the 70
    /// stride-8 offsets in `+0x220..+0x448`; the five in
    /// [`EMPTY_SLOTS`](crate::subsystems::EMPTY_SLOTS) and everything outside
    /// the window are refused, so a caller cannot reach an offset the client
    /// never writes.
    ///
    /// The pointer is to the slot itself (a `*mut u8` field), not to the
    /// subsystem object.
    #[must_use]
    pub fn subsystem_slot_address(&self, slot: u32) -> Option<*mut u8> {
        if !crate::subsystems::is_member_slot(slot) {
            return None;
        }
        // SAFETY: `slot` is one of the 65 offsets the client writes; each is
        // inside the `0x470`-byte object (the window ends at `+0x448`).
        Some(unsafe { (self as *const Self as *mut u8).add(slot as usize) })
    }

    /// Read the pointer in a member slot the way a consumer of the engine
    /// object would: `world->slot`.
    ///
    /// Every slot is null until `ConstructEngineSubsystems` runs; the five
    /// empty offsets stay part of the constructor-zeroed table below
    /// `+0x228`.
    #[must_use]
    pub fn subsystem_slot(&self, slot: u32) -> Option<*mut u8> {
        let p = self.subsystem_slot_address(slot)?;
        // SAFETY: the address is inside the object and 8-byte aligned; the
        // slot holds a pointer written by the client's own store.
        Some(unsafe { core::ptr::read_unaligned(p.cast::<*mut u8>()) })
    }

    /// Store a subsystem pointer exactly where the function's
    /// `mov qword ptr [engine + slot], reg` does.
    ///
    /// Returns `false` (and writes nothing) for a non-member offset.
    /// Registration is a separate step: the same pointer is appended to the
    /// interface array at `+0x188` by `RegisterWorldInterface`.
    ///
    /// # Safety
    ///
    /// The written value is a raw pointer that the object does not own and
    /// cannot check. `value` must be null or point to an object that stays
    /// valid for as long as anything reads the slot.
    pub unsafe fn set_subsystem_slot(&mut self, slot: u32, value: *mut u8) -> bool {
        let Some(p) = self.subsystem_slot_address(slot) else {
            return false;
        };
        // SAFETY: same as `subsystem_slot`; the caller guarantees `value`.
        unsafe { core::ptr::write_unaligned(p.cast::<*mut u8>(), value) };
        true
    }
}
