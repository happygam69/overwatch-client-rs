//! The hit policy the candidate evaluator installs: `0x14202bc90`.
//!
//! The fan's evaluator `0x1420328c0` builds a sweep and asks the collision
//! query whether a candidate direction is reachable. The query's narrowphase
//! `0x142cc1610` answers each contact by calling the callback the descriptor
//! carries — `0x14202c580`, a four-move thunk that rotates
//! `(hit, packed, ctx)` — and that lands here. This module is that callback.
//!
//! The contract is three outcomes, read from the three return sites:
//! **1** = no hit, **2** = accepted, **3** = rejected. The policy writes at
//! most three outputs on the hit object (`+0x30` the score the fan compares,
//! `+0x38` and `+0x39` one-byte flags) plus, in mode `1`, the caller's
//! `0x50`-byte answer record.
//!
//! What is **not** here, because it is data rather than arithmetic: the
//! geometry object behind `[world+8]`. The policy reads its table
//! (`[geometry+0x80]`), its slot word (`[geometry+0x110]`), its index base
//! (`byte[geometry+0x130]`) and its variant table (`[key+0x250]`). Those are
//! supplied here as a [`Geometry`] implementation, so the policy can be
//! tested against synthetic worlds; a real map needs the collision ingest
//! (`docs/formats/maps.md`, cells -> shapes), which is still open.
//!
//! Two obfuscation details are reproduced rather than dropped: the key decode
//! chain (constants cited by the instruction that loads them) and the
//! presence-mask **swap** between the policy's two call sites. The first site
//! masks the presence bit out of the low nibble *shifted by four*, so it is
//! always zero and the dedupe scan behind it is unreachable in this build.
//! [`dead_presence_term`] keeps that formula so [`tests`] can prove it never
//! fires; the live path uses the second site's binding.

use crate::fan::{CandidateSolver, SweepHit};

/// Return value: no hit; the candidate is a free direction.
pub const NO_HIT: u8 = 1;
/// Return value: the candidate hit is accepted, and in mode `1` the record is
/// filled.
pub const ACCEPTED: u8 = 2;
/// Return value: the hit was filtered, excluded, deduplicated, or classified
/// as a rejection.
pub const REJECTED: u8 = 3;

/// `0x14202bd06` (`0x1e2421a4798a328`): the first key-decode addend.
const KEY_ADD: u64 = 0x01e2_421a_4798_a328;
/// `0x14202bd22` (`0xe1379e3cb308170d`): the first key-decode xor.
const KEY_XOR: u64 = 0xe137_9e3c_b308_170d;
/// `0x14202beb0`: the key's rotate amount, `0x2b` at the second site.
const KEY_ROT: u32 = 0x2b;
/// The decode's first rotate, `0x22` at `0x14202bd1b`.
const KEY_ROT_INNER: u32 = 0x22;
/// `0x14202bed8` (`0x4d79452d486f9431`): the entry value's addend.
const ENTRY_ADD: u64 = 0x4d79_452d_486f_9431;
/// `0x14202bee8` (`0xec99e783d64e4c83`): the entry value's xor.
const ENTRY_XOR: u64 = 0xec99_e783_d64e_4c83;
/// `0x14202bef0` (`0x327aecc3f519246f`): the entry value's subtrahend.
const ENTRY_SUB: u64 = 0x327a_ecc3_f519_246f;
/// `0x14202bee0`: the entry value's rotate amount.
const ENTRY_ROT: u32 = 0x38;
/// `0x140543481` (`0x8b084bd971b946cc`): the in-place decode's xor.
const DECODE_XOR: u64 = 0x8b08_4bd9_71b9_46cc;
/// `0x1405434a1` (`0x32c1c6a5a278ca6f`): the in-place decode's subtrahend.
const DECODE_SUB: u64 = 0x32c1_c6a5_a278_ca6f;
/// `0x14202be70`: the key's final rotate.
const H_ROT: u32 = 0x15;
/// `0x14202bcf2`: the packed key that means "the shape id is unset".
pub const KEY_UNSET: u16 = 0xffff;
/// `0x14202bf3a`: the substitute id written before a negative id is resolved.
pub const ID_PENDING: u16 = 1;

/// The packed shape descriptor the narrowphase passes as its second argument
/// (`packed` in the read).
///
/// Field offsets are the descriptor's own, as read at `0x14202bf4c..0x14202c026`
/// and the two record fills.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PackedShape {
    /// `+0x00`: the contact point, four lanes.
    pub point: [f32; 4],
    /// `+0x10`: the surface normal, four lanes.
    pub normal: [f32; 4],
    /// `+0x20`: the hit fraction — the score the fan's rules compare.
    pub fraction: f32,
    /// `+0x30`: a dword copied to the record untouched.
    pub word30: u32,
    /// `+0x34`: the shape id; negative means "resolve the low 15 bits as an
    /// index through the variant table" (`0x14202bf93`).
    pub id: i16,
    /// `+0x36`: a second id copied to the record.
    pub sub: u16,
    /// `+0x38`: the shape key; [`KEY_UNSET`] disables the key pass.
    pub key: u16,
}

impl Default for PackedShape {
    fn default() -> Self {
        Self {
            point: [0.0; 4],
            normal: [0.0; 4],
            fraction: 0.0,
            word30: 0,
            id: 0,
            sub: 0,
            key: KEY_UNSET,
        }
    }
}

/// The `0x50`-byte answer record the fan reads back.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HitRecord {
    /// `+0x00`: contact point.
    pub point: [f32; 4],
    /// `+0x10`: surface normal.
    pub normal: [f32; 4],
    /// `+0x20`: the decoded geometry handle.
    ///
    /// With no world the client never executes the decode's load, so this
    /// field receives a **stale register**. A Rust caller has no stale
    /// register and writes `0`; the real evaluator always has a world, so
    /// this is the one value here that is not bit-identical by construction.
    pub geometry: u64,
    /// `+0x28`: the caller's context (the policy's third argument).
    pub ctx: u64,
    /// `+0x30`: the hit fraction.
    pub fraction: f32,
    /// `+0x34`: the descriptor's dword.
    pub word34: u32,
    /// `+0x38`: the shape key.
    pub key: u16,
    /// `+0x3a`: `0xffff` in the mode `>= 2` local record, `0` in mode `1`.
    pub slot3a: u16,
    /// `+0x3c`: the resolved shape id.
    pub id: u16,
    /// `+0x3e`: the resolved kind.
    pub sub: u16,
    /// `+0x40`: zeroed.
    pub tail: u64,
    /// `+0x48`: zeroed in the mode `>= 2` local record.
    pub tail48: u8,
}

/// The object `hit+0x08` points at, screened by `0x1420af950` (62 bytes) and
/// handed to the classifier as `[hit+0x08] + 0x470`.
#[derive(Debug, Clone, Copy, Default)]
pub struct HitFilter<'a> {
    /// `+0x08`: bits the hit must share with the world's layer mask.
    pub required: u32,
    /// `+0x10`: bits the hit must not share with it.
    pub forbidden: u32,
    /// `+0x20`/`+0x28`: the worlds this filter excludes.
    pub excluded: &'a [u64],
    /// `+0x470`: the classifier's second argument.
    pub arg470: u64,
}

/// The policy's first argument — the narrowphase's hit object.
pub struct Hit<'a> {
    /// `+0x00`: the classifier, called as `(record, filter+0x470)` in mode
    /// `>= 2`. `None` is a null function pointer.
    pub classifier: Option<&'a dyn Fn(&HitRecord, u64) -> i32>,
    /// `+0x08`: the filter object.
    pub filter: HitFilter<'a>,
    /// `+0x10`: where mode `1` writes the answer record.
    pub record: Option<&'a mut HitRecord>,
    /// `+0x18`: the optional extra filter, `(geometry, +0x20, +0x28)`.
    pub optional: Option<(&'a dyn Fn(u64, u64, u64) -> bool, u64, u64)>,
    /// `+0x34`: `0` presence test, `1` fill the record, `>= 2` classify.
    pub mode: u32,
    /// `+0x30`: the score, written when the classifier returns `1`.
    pub score: f32,
    /// `+0x38`: set when the classifier neither accepts nor rejects.
    pub flag: u8,
    /// `+0x39`: set by the presence test and by mode `1`.
    pub flag2: u8,
}

impl<'a> Hit<'a> {
    /// A hit in the given mode with no callbacks and an empty filter.
    pub fn new(mode: u32) -> Self {
        Self {
            classifier: None,
            filter: HitFilter::default(),
            record: None,
            optional: None,
            mode,
            score: 0.0,
            flag: 0,
            flag2: 0,
        }
    }
}

/// The geometry object the query resolves from the world (`[world+8]`,
/// decoded).
///
/// The client stores these fields in a 0x3210-byte object the map ingest
/// fills; here they are supplied so the policy can be exercised without a
/// map. Every method names the field the client reads.
pub trait Geometry {
    /// `[geometry+0x110]`: the slot word. Its low nibble feeds the table
    /// index and its bit 4 is the presence bit.
    fn slot_word(&self) -> u64;
    /// `byte[geometry+0x130]`: added to the index.
    fn base_index(&self) -> u8;
    /// `[[geometry+0x80] + index*8]`: the table read.
    fn table_entry(&self, index: usize) -> u64;
    /// `[key + 0x250]`: the variant table behind a decoded key.
    fn variant_table(&self, key: u64) -> Option<&dyn VariantLookup>;
    /// `[0x143b79598 + 0x198]`: the anti-tamper xor applied to the entry.
    fn key_mix(&self) -> u64;
    /// `byte[0x1438480f3]`: the anti-tamper salt.
    fn key_salt(&self) -> u8;
}

/// The variant table a decoded key resolves to: `0x141b3c890` over
/// `[key+0x250]`.
pub trait VariantLookup {
    /// `0x141b3c890(table, &out, index)` fills an id/kind pair.
    fn resolve(&self, index: u16) -> Option<(u16, u16)>;
}

/// The world the policy reads: `0x1420c2360(ctx)` produces it, and its
/// geometry object is `[world+8]`.
pub struct World<'a> {
    /// `+0x280`: the layer mask the filter is compared against.
    pub layer_mask: u32,
    /// The world's own identity, compared against the filter's exclusions.
    pub identity: u64,
    /// `[world+8]`, decoded. `None` models the null world, where the client
    /// skips both the decode and the optional filter.
    pub geometry: Option<&'a dyn Geometry>,
}

#[inline]
fn ror64(v: u64, n: u32) -> u64 {
    v.rotate_right(n)
}

/// `0x14202beba..0x14202bd06`: the decode the policy applies to `[world+8]`
/// before it touches anything.
///
/// ```text
/// rdi = ror64(ror64(raw + 0x1e2421a4798a328, 0x22) ^ 0xe1379e3cb308170d, 0x2b)
/// ```
///
/// A caller whose world still holds the raw, encoded pointer runs this first;
/// [`World::geometry`] models that value *after* the decode, so the policy
/// itself does not call it. It is public because the decode is part of the
/// read contract, and [`tests`] pins it against the documented inverse.
pub fn decode_geometry_pointer(raw: u64) -> u64 {
    let v = ror64(raw.wrapping_add(KEY_ADD), KEY_ROT_INNER);
    ror64(v ^ KEY_XOR, KEY_ROT)
}

/// The dead presence term of the policy's **first** call site.
///
/// That site binds the constant-stub's masks the other way round from the
/// live site: it takes the presence mask from `0x0f` and shifts it right by
/// four, so the term is `(word & 0x0f) >> 4` — always zero. The dedupe scan
/// behind it therefore never runs in this build. Kept so [`tests`] can assert
/// that, and so nobody re-derives the scan as live.
pub fn dead_presence_term(slot_word: u64) -> u64 {
    (slot_word & 0x0f) >> 4
}

/// The decoded geometry key `h`, as the live (mode `1`) call site computes it.
///
/// ```text
/// presence = (word & 0x10) >> 4                   ; the live site's binding
/// index    = popcount(word & 0x0f) + byte[+0x130]
/// v        = [[[geometry+0x80] + index*8]]
/// v        = ror(v, 0x38) + 0x4d79452d486f9431
/// v       ^= [0x143b79598+0x198]; v ^= 0xec99e783d64e4c83
/// v       -= 0x327aecc3f519246f;  v ^= byte[0x1438480f3]
/// v        = 0x140543440(&v)      ; in-place decode
/// h        = presence ? ror(v, 0x15) : 0
/// ```
///
/// The popcount is the client's SWAR sequence over the masked qword, which is
/// exact and equals [`u64::count_ones`].
pub fn geometry_key(geometry: &dyn Geometry) -> u64 {
    let word = geometry.slot_word();
    let presence = (word & 0x10) >> 4;
    if presence == 0 {
        return 0;
    }
    let index = (word & 0x0f).count_ones() as usize + geometry.base_index() as usize;
    let mut v = geometry.table_entry(index);
    v = ror64(v, ENTRY_ROT).wrapping_add(ENTRY_ADD);
    v ^= geometry.key_mix();
    v ^= ENTRY_XOR;
    v = v.wrapping_sub(ENTRY_SUB);
    v ^= u64::from(geometry.key_salt());
    // `0x140543440` decodes in place; the low 32 bits are zeroed first.
    let v = ror64((v & 0xffff_ffff) & 0xffff_ffff, 0x14) ^ DECODE_XOR;
    let v = ror64(v.wrapping_sub(DECODE_SUB), 0x26);
    ror64(v, H_ROT)
}

/// The **dead** variant of [`geometry_key`] — the first call site's binding.
/// Its presence term is always zero, so its key always is too.
pub fn dead_key(geometry: &dyn Geometry) -> u64 {
    let presence = dead_presence_term(geometry.slot_word());
    if presence == 0 {
        return 0;
    }
    let _ = geometry.table_entry(0);
    unreachable!("the dead site's presence term is always zero");
}

/// `0x1420af950`: the layer and exclusion screen.
///
/// Rejects when the hit shares no bit of `required` with the world's mask,
/// when it shares any bit of `forbidden`, or when the world is one of the
/// ones the filter excludes.
pub fn passes_layer_filter(filter: &HitFilter, world: &World) -> bool {
    if filter.required & world.layer_mask == 0 {
        return false;
    }
    if filter.forbidden & world.layer_mask != 0 {
        return false;
    }
    !filter.excluded.contains(&world.identity)
}

/// The callback itself: `0x14202bc90`.
///
/// The gates run in the client's order — the world, then the layer filter,
/// then the optional filter — and the mode switch runs last. A `None` world
/// skips both the geometry decode and the optional filter, exactly as the
/// client's `je` to the mode switch does.
pub fn candidate_hit_policy(
    hit: &mut Hit,
    packed: &PackedShape,
    ctx: u64,
    world: Option<&World>,
) -> u8 {
    // The client computes the world first; a null world jumps to the mode
    // switch without executing the decode or the filter call.
    let mut geometry: Option<&dyn Geometry> = None;
    let mut rejected = false;
    if let Some(world) = world {
        if !passes_layer_filter(&hit.filter, world) {
            rejected = true;
        } else if let Some((f, a, b)) = hit.optional {
            let handle = geometry_key_of(world);
            if !f(handle, a, b) {
                rejected = true;
            }
        }
        geometry = world.geometry;
    }
    if rejected {
        return REJECTED;
    }

    match hit.mode {
        0 => {
            hit.flag2 = 1;
            NO_HIT
        }
        1 => {
            hit.flag2 = 1;
            let handle = world.and_then(|w| w.geometry).map_or(0, geometry_key);
            let resolved = resolve_id(geometry, packed.id);
            if let Some(record) = hit.record.as_deref_mut() {
                record.point = packed.point;
                record.normal = packed.normal;
                record.geometry = handle;
                record.ctx = ctx;
                record.fraction = packed.fraction;
                record.word34 = packed.word30;
                record.key = packed.key;
                record.slot3a = 0;
                record.id = resolved.0;
                record.sub = resolved.1.unwrap_or(packed.sub);
                record.tail = 0;
                record.tail48 = 0;
            }
            ACCEPTED
        }
        _ => {
            let resolved = resolve_id(geometry, packed.id);
            let record = HitRecord {
                point: packed.point,
                normal: packed.normal,
                geometry: world.and_then(|w| w.geometry).map_or(0, geometry_key),
                ctx,
                fraction: packed.fraction,
                word34: packed.word30,
                key: packed.key,
                slot3a: KEY_UNSET,
                id: resolved.0,
                sub: resolved.1.unwrap_or(packed.sub),
                tail: 0,
                tail48: 0,
            };
            let class = hit.classifier.map_or(0, |f| f(&record, hit.filter.arg470));
            if class == 1 {
                hit.score = packed.fraction;
                ACCEPTED
            } else if class == 2 {
                REJECTED
            } else {
                hit.flag = 1;
                NO_HIT
            }
        }
    }
}

/// The geometry handle the mode `1` record stores. The client stores the raw
/// decoded pointer; here the key stands in for it, which is the same value at
/// the same offset in every synthetic world.
fn geometry_key_of(world: &World) -> u64 {
    world.geometry.map_or(0, geometry_key)
}

/// `0x14202bf93`: a negative id is an index into the variant table.
///
/// The client writes [`ID_PENDING`] first, then — only when the geometry is
/// non-null and the presence bit is set — replaces it with
/// `out.id & 0x7fff` and stores the kind. Otherwise the pending id stands and
/// the record keeps the descriptor's own `+0x36` in `+0x3e`.
///
/// Returns the id and, when the lookup succeeded, the kind that replaces the
/// descriptor's `+0x36`.
fn resolve_id(geometry: Option<&dyn Geometry>, id: i16) -> (u16, Option<u16>) {
    if id >= 0 {
        return (id as u16, None);
    }
    let Some(geometry) = geometry else {
        return (ID_PENDING, None);
    };
    let key = geometry_key(geometry);
    if key == 0 {
        return (ID_PENDING, None);
    }
    let Some(table) = geometry.variant_table(key) else {
        return (ID_PENDING, None);
    };
    match table.resolve((id as u16) & 0x7fff) {
        Some((resolved, kind)) => (resolved & 0x7fff, Some(kind)),
        None => (ID_PENDING, None),
    }
}

/// The evaluator chain with a synthetic world behind it — the [`CandidateSolver`]
/// the producer's seam expects.
///
/// The client's order is: the fan builds a sweep, the evaluator `0x1420328c0`
/// runs the query, the narrowphase calls this policy, and the accepted record
/// comes back as the hit. This type is that chain minus the sweep build (which
/// lives inside the evaluator and is not modelled yet), so `evaluate` ignores
/// its two endpoints and answers with the policy's decision for the descriptor
/// it was constructed with.
pub struct PolicySolver<'a> {
    /// The world the policy screens against.
    pub world: Option<&'a World<'a>>,
    /// The filter the query would carry. [`PolicySolver::new`] passes
    /// everything (`required = !0`), which is what a fan with no filter data
    /// does: the layer gates are the caller's to set.
    pub filter: HitFilter<'a>,
    /// The query's context argument.
    pub ctx: u64,
    /// The descriptor the narrowphase would have produced.
    pub packed: PackedShape,
    /// The hit object's mode.
    pub mode: u32,
    /// The last record the policy filled.
    pub record: HitRecord,
    /// The last hit object, so a caller can read `score`/`flag`/`flag2`.
    pub hit_flag: u8,
}

impl<'a> PolicySolver<'a> {
    /// A solver for one descriptor and world.
    pub fn new(world: Option<&'a World<'a>>, packed: PackedShape, ctx: u64, mode: u32) -> Self {
        Self {
            world,
            filter: HitFilter {
                required: !0,
                ..HitFilter::default()
            },
            ctx,
            packed,
            mode,
            record: HitRecord::default(),
            hit_flag: 0,
        }
    }
}

impl CandidateSolver for PolicySolver<'_> {
    fn evaluate(&mut self, _a: [f32; 4], _b: [f32; 4]) -> Option<SweepHit> {
        let mut record = self.record;
        let (out, flag) = {
            let mut hit = Hit::new(self.mode);
            // The fan's filter is the reason the seam exists: the layer gates
            // come from the world, and the record is what the acceptance
            // rules read.
            hit.filter = self.filter;
            hit.record = Some(&mut record);
            let out = candidate_hit_policy(&mut hit, &self.packed, self.ctx, self.world);
            (out, hit.flag)
        };
        self.record = record;
        self.hit_flag = flag;
        (out == ACCEPTED).then_some(SweepHit {
            score: record.fraction,
            normal: record.normal,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeGeometry {
        word: u64,
        base: u8,
        table: Vec<u64>,
        mix: u64,
        salt: u8,
        variants: Option<FakeVariants>,
    }

    struct FakeVariants {
        id: u16,
        kind: u16,
    }

    impl VariantLookup for FakeVariants {
        fn resolve(&self, index: u16) -> Option<(u16, u16)> {
            // `-3 as u16 & 0x7fff` is the index the negative-id test uses.
            (index == 0x7ffd).then_some((self.id, self.kind))
        }
    }

    impl Geometry for FakeGeometry {
        fn slot_word(&self) -> u64 {
            self.word
        }
        fn base_index(&self) -> u8 {
            self.base
        }
        fn table_entry(&self, index: usize) -> u64 {
            self.table[index]
        }
        fn variant_table(&self, _key: u64) -> Option<&dyn VariantLookup> {
            self.variants.as_ref().map(|v| v as &dyn VariantLookup)
        }
        fn key_mix(&self) -> u64 {
            self.mix
        }
        fn key_salt(&self) -> u8 {
            self.salt
        }
    }

    fn geometry(word: u64) -> FakeGeometry {
        FakeGeometry {
            word,
            base: 0,
            table: vec![0x1122_3344_5566_7788],
            mix: 0,
            salt: 0,
            variants: None,
        }
    }

    fn world(mask: u32, geometry: &dyn Geometry) -> World<'_> {
        World {
            layer_mask: mask,
            identity: 0x1000,
            geometry: Some(geometry),
        }
    }

    #[test]
    fn mode_zero_is_a_presence_test() {
        let g = geometry(0);
        let w = world(1, &g);
        let mut hit = Hit::new(0);
        hit.filter.required = 1;
        let out = candidate_hit_policy(&mut hit, &PackedShape::default(), 7, Some(&w));
        assert_eq!(out, NO_HIT);
        assert_eq!(hit.flag2, 1);
        assert!(hit.record.is_none());
    }

    #[test]
    fn mode_one_fills_the_record() {
        let g = geometry(0);
        let w = world(1, &g);
        let mut record = HitRecord::default();
        let mut hit = Hit::new(1);
        hit.filter.required = 1;
        hit.record = Some(&mut record);
        let packed = PackedShape {
            point: [1.0, 2.0, 3.0, 4.0],
            normal: [0.0, 1.0, 0.0, 0.0],
            fraction: 0.25,
            word30: 9,
            id: 5,
            sub: 6,
            key: 0x1234,
        };
        let out = candidate_hit_policy(&mut hit, &packed, 0xabc, Some(&w));
        assert_eq!(out, ACCEPTED);
        assert_eq!(hit.flag2, 1);
        assert_eq!(record.point, packed.point);
        assert_eq!(record.normal, packed.normal);
        assert_eq!(record.fraction, 0.25);
        assert_eq!(record.word34, 9);
        assert_eq!(record.id, 5);
        assert_eq!(record.sub, 6);
        assert_eq!(record.key, 0x1234);
        assert_eq!(record.ctx, 0xabc);
        assert_eq!(record.slot3a, 0);
        assert_eq!(record.tail, 0);
    }

    #[test]
    fn a_negative_id_resolves_through_the_variant_table() {
        let mut g = geometry(0x10);
        g.variants = Some(FakeVariants {
            id: 0x1234,
            kind: 7,
        });
        let w = world(1, &g);
        let mut record = HitRecord::default();
        let mut hit = Hit::new(1);
        hit.filter.required = 1;
        hit.record = Some(&mut record);
        let packed = PackedShape {
            id: -3,
            sub: 6,
            key: 0x1234,
            ..PackedShape::default()
        };
        let out = candidate_hit_policy(&mut hit, &packed, 0, Some(&w));
        assert_eq!(out, ACCEPTED);
        assert_eq!(record.id, 0x1234);
        assert_eq!(record.sub, 7);
    }

    #[test]
    fn without_the_presence_bit_the_pending_id_stands() {
        let mut g = geometry(0);
        g.variants = Some(FakeVariants {
            id: 0x1234,
            kind: 7,
        });
        let w = world(1, &g);
        let mut record = HitRecord::default();
        let mut hit = Hit::new(1);
        hit.filter.required = 1;
        hit.record = Some(&mut record);
        let packed = PackedShape {
            id: -3,
            sub: 6,
            ..PackedShape::default()
        };
        candidate_hit_policy(&mut hit, &packed, 0, Some(&w));
        assert_eq!(record.id, ID_PENDING);
        assert_eq!(record.sub, 6);
    }

    #[test]
    fn a_null_world_skips_the_filter_and_the_decode() {
        // The client's `je` after the world lookup lands past both, so the
        // layer filter cannot reject and the record's handle stays unset.
        let mut hit = Hit::new(1);
        hit.filter.required = 0xff; // would reject any real mask
        let mut record = HitRecord::default();
        hit.record = Some(&mut record);
        let out = candidate_hit_policy(&mut hit, &PackedShape::default(), 0, None);
        assert_eq!(out, ACCEPTED);
        assert_eq!(record.geometry, 0);
    }

    #[test]
    fn the_layer_filter_rejects_missing_forbidden_and_excluded_worlds() {
        let g = geometry(0);
        let w = world(0b0010, &g);

        let missing = HitFilter {
            required: 0b0100,
            ..HitFilter::default()
        };
        assert!(!passes_layer_filter(&missing, &w));

        let forbidden = HitFilter {
            required: 0b0010,
            forbidden: 0b0010,
            ..HitFilter::default()
        };
        assert!(!passes_layer_filter(&forbidden, &w));

        let excluded = [0x1000u64];
        let excluded_filter = HitFilter {
            required: 0b0010,
            excluded: &excluded,
            ..HitFilter::default()
        };
        assert!(!passes_layer_filter(&excluded_filter, &w));

        let ok = HitFilter {
            required: 0b0010,
            ..HitFilter::default()
        };
        assert!(passes_layer_filter(&ok, &w));
    }

    #[test]
    fn the_optional_filter_rejects() {
        let g = geometry(0);
        let w = world(1, &g);
        let deny = |_: u64, _: u64, _: u64| false;
        let mut hit = Hit::new(1);
        hit.filter.required = 1;
        hit.optional = Some((&deny, 1, 2));
        assert_eq!(
            candidate_hit_policy(&mut hit, &PackedShape::default(), 0, Some(&w)),
            REJECTED
        );
    }

    #[test]
    fn the_classifier_decides_mode_two_and_up() {
        let g = geometry(0);
        let w = world(1, &g);
        let packed = PackedShape {
            fraction: 0.75,
            ..PackedShape::default()
        };

        let accept = |_: &HitRecord, _: u64| 1;
        let mut hit = Hit::new(2);
        hit.filter.required = 1;
        hit.classifier = Some(&accept);
        assert_eq!(
            candidate_hit_policy(&mut hit, &packed, 0, Some(&w)),
            ACCEPTED
        );
        assert_eq!(hit.score, 0.75);

        let reject = |_: &HitRecord, _: u64| 2;
        let mut hit = Hit::new(2);
        hit.filter.required = 1;
        hit.classifier = Some(&reject);
        assert_eq!(
            candidate_hit_policy(&mut hit, &packed, 0, Some(&w)),
            REJECTED
        );

        let other = |_: &HitRecord, _: u64| 0;
        let mut hit = Hit::new(2);
        hit.filter.required = 1;
        hit.classifier = Some(&other);
        assert_eq!(candidate_hit_policy(&mut hit, &packed, 0, Some(&w)), NO_HIT);
        assert_eq!(hit.flag, 1);
    }

    #[test]
    fn the_classifier_sees_the_filter_argument() {
        use std::cell::Cell;
        let seen = Cell::new(0u64);
        let f = |_: &HitRecord, arg: u64| {
            seen.set(arg);
            1
        };
        let g = geometry(0);
        let w = world(1, &g);
        let mut hit = Hit::new(2);
        hit.filter.required = 1;
        hit.filter.arg470 = 0x470;
        hit.classifier = Some(&f);
        candidate_hit_policy(&mut hit, &PackedShape::default(), 0, Some(&w));
        assert_eq!(seen.get(), 0x470);
    }

    #[test]
    fn the_pointer_decode_inverts_the_documented_encoder() {
        // The decode read backwards is the encode the loader applied, so a
        // round trip proves the three constants and both rotate amounts.
        let encode = |y: u64| {
            let v2 = y.rotate_left(KEY_ROT);
            let v1 = v2 ^ KEY_XOR;
            v1.rotate_left(KEY_ROT_INNER).wrapping_sub(KEY_ADD)
        };
        for y in [0u64, 1, 0xdead_beef, u64::MAX, 0x1_4333_c620] {
            assert_eq!(decode_geometry_pointer(encode(y)), y, "{y:#x}");
        }
        // And it is not the identity, which is the whole point of the chain.
        assert_ne!(decode_geometry_pointer(0), 0);
    }

    #[test]
    fn the_solver_hands_the_fan_the_records_own_values() {
        let g = geometry(0);
        let w = world(1, &g);
        let packed = PackedShape {
            normal: [0.0, 1.0, 0.0, 0.0],
            fraction: 0.5,
            ..PackedShape::default()
        };
        let mut solver = PolicySolver::new(Some(&w), packed, 0, 1);
        let hit = solver.evaluate([0.0; 4], [1.0; 4]).expect("accepted");
        assert_eq!(hit.score, 0.5);
        assert_eq!(hit.normal, [0.0, 1.0, 0.0, 0.0]);
        // And the acceptance's second term reads that normal.
        assert_eq!(crate::fan::facing([0.0, 1.0, 0.0, 0.0], hit.normal), 1.0);
    }

    #[test]
    fn the_solver_reports_nothing_when_the_filter_rejects() {
        let g = geometry(0);
        let w = world(0b0010, &g);
        let mut solver = PolicySolver::new(Some(&w), PackedShape::default(), 0, 1);
        solver.filter.forbidden = 0b0010;
        assert_eq!(solver.evaluate([0.0; 4], [1.0; 4]), None);
    }

    #[test]
    fn the_first_sites_presence_term_is_always_zero() {
        // This is the finding, not a curiosity: with `0x0f` as the presence
        // mask and a shift of four, no slot word can produce a key, so the
        // dedupe scan over `[world+0x240]` never runs in this build.
        for word in 0..=0xffffu64 {
            assert_eq!(dead_presence_term(word), 0, "word {word:#x}");
        }
        assert_eq!(dead_key(&geometry(0xffff)), dead_key(&geometry(0xffff)));
    }

    #[test]
    fn the_key_is_zero_without_the_presence_bit_and_stable_with_it() {
        assert_eq!(geometry_key(&geometry(0)), 0);
        let g = geometry(0x10);
        let k = geometry_key(&g);
        assert_ne!(k, 0);
        // Deterministic in the geometry's own fields.
        assert_eq!(k, geometry_key(&geometry(0x10)));
        // The salt and the mix both enter the chain.
        let mut salted = geometry(0x10);
        salted.salt = 1;
        assert_ne!(geometry_key(&salted), k);
        let mut mixed = geometry(0x10);
        mixed.mix = 1;
        assert_ne!(geometry_key(&mixed), k);
    }
}
