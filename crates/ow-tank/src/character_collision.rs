//! The character controller's collision record, and the two descriptors
//! `0x142cf35a0` builds from it.
//!
//! This is the input half of the character's swept shape - it replaces the
//! `0.35` m sphere proxy with the client's own **capsule**. Every offset below
//! is quoted from the instruction stream; the capsule triplet is pinned by three
//! independent places (the object copy-out, the degeneracy nudge, and
//! `ShapeEntryFromDescriptor`'s type-1 element fill).
//!
//! # The chain
//!
//! ```text
//! 0x142cbcd40                      builds the query record  (formats/static-collision.md, check 11)
//!   0x142cf5440(record)            Alloc(0x590) -> 0x142cf35a0(object, record)
//!     0x142cf35a0                  copies the record, builds two descriptors
//!       0x142cc3f50(slot, owner)   owner pool (world+0x2a0, stride 0x50); 0x142cd4290 fills it
//!       0x142cd4ef0(handle, shape) ShapeAddToOwner -> 0x142cb7f10 (ShapeEntryFromDescriptor)
//! ```
//!
//! # The record
//!
//! | offset | type | destination |
//! |---|---|---|
//! | `+0x00` | `u32` | the mode: `0` -> shape path, `1` -> body path, else nothing |
//! | `+0x10` | `vec4` | **the capsule's first point** - `object+0x560`, shape descriptor `+0x70` |
//! | `+0x20` | `vec4` | **the capsule's second point** - `object+0x570`, shape descriptor `+0x80` |
//! | `+0x30` | `f32` | **the capsule's radius** - `object+0x580`, shape descriptor `+0x90` |
//! | `+0x40` | `vec4` | the **owner** descriptor's first quad (`+0x10`), validated finite |
//! | `+0x50` | `vec4` | the owner descriptor's second quad (`+0x20`), validated finite and non-zero |
//! | `+0x60` | `vec4` | the mode-1 body descriptor (`0x142cb50e0`) |
//! | `+0x70` | `vec4` | the mode-1 body descriptor |
//! | `+0x80` | `u32` | shape descriptor `+0x58` |
//! | `+0x84` | `u16` | shape descriptor `+0x5c` |
//! | `+0x86` | `u16` | **the world slot**: `object+0x04`, first argument of `0x142cc3f50` and `OwnerCreateBody` |
//! | `+0x88` | `f32` | *(not copied by the mode-0 path)* |
//! | `+0x8c` | `u16` | owner descriptor `+0x30` |
//! | `+0x8e` | `u16` | owner descriptor `+0x32` |
//! | `+0x90` | `u16` | shape descriptor `+0x60` |
//! | `+0x98` | `u64` | owner descriptor `+0x00` |
//!
//! Nothing above `+0x98` is read.
//!
//! # The two descriptors are different structs
//!
//! An earlier note in this file's history called them one struct with one layout.
//! They are two: the owner descriptor is `{+0x00 qword, +0x10 quad, +0x20 quad,
//! +0x30 u16, +0x32 u16}`, and the shape descriptor is the generic
//! `ShapeEntryFromDescriptor` source - the same layout the map's geometry
//! descriptor uses, with **the type byte at `+0x67`**, the per-type geometry at
//! `+0x70`/`+0x80`/`+0x90`, and the constants the builder writes.
//!
//! # The shape descriptor (`ShapeEntryFromDescriptor`'s source)
//!
//! Type 1 (capsule), from the builder's stores and the consumer's reads:
//!
//! | field | value |
//! |---|---|
//! | `+0x00` | `0` |
//! | `+0x08` | `100.0` |
//! | `+0x10` | `1` (u16) |
//! | `+0x14` | `-1` (qword) |
//! | `+0x1c` | `-1` (dword) |
//! | `+0x20` | the quad at `0x143365490` |
//! | `+0x30` | the quad at `0x1431102d0` |
//! | `+0x40` | `1.0`, `+0x44` = `0.0` |
//! | `+0x48` | `0.1` |
//! | `+0x50` | `10.0` |
//! | `+0x54` | `0.7` |
//! | `+0x58` | `record+0x80` |
//! | `+0x5c` | `record+0x84` (u16) |
//! | `+0x5e` | `0` |
//! | `+0x60` | `record+0x90` (u16) |
//! | `+0x62` | `0xffff` (u16) |
//! | `+0x64` | `0` (`+0x65` = `1`) |
//! | `+0x66` | `0x0100` - so **`+0x67` = the type byte = 1** |
//! | `+0x70` | the capsule's first point (`record+0x10`) |
//! | `+0x80` | the capsule's second point (`record+0x20`, nudged) |
//! | `+0x90` | the capsule's radius (`record+0x30`) |
//!
//! `ShapeEntryFromDescriptor` reads `+0x67`, allocates the type-1 element from
//! `world+0x2e8` (stride `0x30`), writes `{P0, P1, radius}` from
//! `+0x70`/`+0x80`/`+0x90`, stores the element index at `entry+0x24`, and sets
//! `entry+0x5c` = the radius and `entry+0x60` = `max(|P0|, |P1|)`.
//!
//! # The degeneracy nudge
//!
//! `COMISS |B-A|², <0x143110818>` + `JA`, so a NaN or a value at or below the
//! threshold takes the nudge. The threshold is the f32 `0x057a0000` =
//! **1.1754943508222875e-35**, exactly `FLT_MIN * 1000` (and **not**
//! `f32::MIN_POSITIVE`; the same `1e-35` guard appears in `0x142cc3f50` and
//! `0x142cd75d0`). The nudge is
//! `A + (2.0,2.0,2.0,2.0) * (0.005,...) * (1.0,0,0,0)` - three multiplies by the
//! constants at `0x1435ed210`, `0x1435ed220`, `0x1435ec8a0` - which is
//! **`0.01` in lane 0 only**. The squared length is `dx² + dy² + dz²` in that
//! addition order (`w` never summed), so a degenerate pair becomes a `0.01`-long
//! segment along `x`: a sphere expressed as a capsule.
//!
//! # Where the numbers come from (closed 2026-11-07)
//!
//! `0x142cbcd40` builds the record from a `0x60`-byte source that
//! `0x140ca5db0` fills from three floats on the mover, and those three floats
//! are copies of **`STUCharacterMoverComponent` fields** (`0x6EFDBD73`, id
//! `0x26`): class offsets `+0x78` `0x1972C687`, `+0x7c` `0x7A70E3E3`,
//! `+0x80` `0x4A576BC2`. `ConstructMoverBody` (`0x140ca9730`) first maxes the
//! first against `2.1 * radius + offset`, and `0x140ca5db0` then builds
//!
//! ```text
//! r = max(0.02, radius)
//! d = max(0.1 * r, extent - (offset + 2 * r))
//! A = (0, r, 0)      B = (0, r + d, 0)
//! ```
//!
//! so the shape is a **vertical capsule** in the character's local space,
//! spanning `y` in `[0, 2r + d]`. [`capsule_from_mover`] is that construction;
//! the three fields are per-hero install data (`ow_tank::entity::mover_capsule_of`
//! reads them). What is still open is the driver: `0x142cf3f80` walks the
//! entry's already-paired shapes (`0x142cd4d90`, `0x142cbb260`) rather than
//! re-querying, so `ow_sim::walk`'s grid remains a declared deviation.

/// The shortest record `0x142cf35a0` can read: its highest read is `+0x98..+0xa0`.
pub const RECORD_LEN: usize = 0xa0;

/// `record+0x00`: the shape path (`0x142cc3f50` + `0x142cd4ef0`).
pub const MODE_SHAPE: u32 = 0;
/// `record+0x00`: the body path (`OwnerCreateBody` + `0x142cb50e0`).
pub const MODE_BODY: u32 = 1;

/// The shape descriptor's type byte (`+0x67` in the shape path): capsule, the
/// `entry+0x20` kind that dispatches to `0x142ccd7b0`.
pub const KIND_CAPSULE: u8 = 1;

/// Shape descriptor `+0x08`.
pub const DESC_08: f32 = 100.0;
/// Shape descriptor `+0x10` (a u16).
pub const DESC_10: u16 = 1;
/// Shape descriptor `+0x40`.
pub const DESC_40: f32 = 1.0;
/// Shape descriptor `+0x48`.
pub const DESC_48: f32 = 0.1;
/// Shape descriptor `+0x50`.
pub const DESC_50: f32 = 10.0;
/// Shape descriptor `+0x54`.
pub const DESC_54: f32 = 0.7;
/// Shape descriptor `+0x62`.
pub const DESC_62: u16 = 0xffff;
/// Shape descriptor `+0x66`: its high byte, `+0x67`, is the type byte.
pub const DESC_66: u16 = 0x0100;

/// The nudge's lane-0 delta: `2.0 * 0.005 * 1.0`, computed as the client does.
pub fn degenerate_nudge() -> f32 {
    let a = 2.0f32;
    let b = 0.005f32;
    let c = 1.0f32;
    a * b * c
}

/// The `COMISS` threshold the pair's squared length is compared against: the
/// f32 at `0x143110818`, bits `0x057a0000` = `1.1754943508222875e-35`
/// (`FLT_MIN * 1000`). At or below it - and for NaN, because `JA` is not taken -
/// the pair is nudged.
pub const NON_DEGENERATE: f32 = f32::from_bits(0x057A_0000);

/// Why a record could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The slice is shorter than [`RECORD_LEN`].
    Truncated { len: usize },
    /// `record+0x00` is neither [`MODE_SHAPE`] nor [`MODE_BODY`]; the client's
    /// two `if` arms both fail and it builds nothing.
    UnknownMode(u32),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Truncated { len } => {
                write!(
                    f,
                    "the character collision record is {len} bytes, needs {RECORD_LEN}"
                )
            }
            Error::UnknownMode(mode) => write!(
                f,
                "the character collision record's mode {mode} is neither {MODE_SHAPE} nor {MODE_BODY}"
            ),
        }
    }
}

impl std::error::Error for Error {}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes([
        b[at],
        b[at + 1],
        b[at + 2],
        b[at + 3],
        b[at + 4],
        b[at + 5],
        b[at + 6],
        b[at + 7],
    ])
}

fn quad_at(b: &[u8], at: usize) -> [f32; 4] {
    [
        f32_at(b, at),
        f32_at(b, at + 4),
        f32_at(b, at + 8),
        f32_at(b, at + 12),
    ]
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// The character's collision record, field for field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharacterCollision {
    /// `+0x00`: [`MODE_SHAPE`] or [`MODE_BODY`].
    pub mode: u32,
    /// `+0x86`: the Domino world slot both owner and body calls take.
    pub world_slot: u16,
    /// `+0x10`/`+0x20`: the capsule's two points.
    pub capsule: ([f32; 4], [f32; 4]),
    /// `+0x30`: the capsule's radius.
    pub radius: f32,
    /// `+0x40`/`+0x50`: the owner descriptor's validated pair.
    pub owner: ([f32; 4], [f32; 4]),
    /// `+0x60`/`+0x70`: the mode-1 body descriptor's two quads.
    pub body: ([f32; 4], [f32; 4]),
    /// `+0x80`: shape descriptor `+0x58`.
    pub u80: u32,
    /// `+0x84`: shape descriptor `+0x5c`.
    pub u84: u16,
    /// `+0x88`: not copied by the mode-0 path.
    pub f88: f32,
    /// `+0x8c`: owner descriptor `+0x30`.
    pub u8c: u16,
    /// `+0x8e`: owner descriptor `+0x32`.
    pub u8e: u16,
    /// `+0x90`: shape descriptor `+0x60`.
    pub u90: u16,
    /// `+0x98`: owner descriptor `+0x00`.
    pub w98: u64,
}

impl CharacterCollision {
    /// Read the record. The client reads no byte above `+0x98`, so a shorter
    /// slice is refused; an unknown mode is refused because both of the
    /// client's arms miss and nothing is built.
    pub fn parse(bytes: &[u8]) -> Result<CharacterCollision, Error> {
        if bytes.len() < RECORD_LEN {
            return Err(Error::Truncated { len: bytes.len() });
        }
        let mode = u32_at(bytes, 0x00);
        if mode != MODE_SHAPE && mode != MODE_BODY {
            return Err(Error::UnknownMode(mode));
        }
        Ok(CharacterCollision {
            mode,
            world_slot: u16_at(bytes, 0x86),
            capsule: (quad_at(bytes, 0x10), quad_at(bytes, 0x20)),
            radius: f32_at(bytes, 0x30),
            owner: (quad_at(bytes, 0x40), quad_at(bytes, 0x50)),
            body: (quad_at(bytes, 0x60), quad_at(bytes, 0x70)),
            u80: u32_at(bytes, 0x80),
            u84: u16_at(bytes, 0x84),
            f88: f32_at(bytes, 0x88),
            u8c: u16_at(bytes, 0x8c),
            u8e: u16_at(bytes, 0x8e),
            u90: u16_at(bytes, 0x90),
            w98: u64_at(bytes, 0x98),
        })
    }
}

/// The owner descriptor `0x142cc3f50` validates and `0x142cd4290` copies.
///
/// Only the fields this path writes are modelled; `0x142cc3f50` reads `+0x10`
/// and `+0x20` (finiteness, and a non-zero squared length on the second).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OwnerDescriptor {
    /// `+0x00`: `record+0x98`.
    pub word: u64,
    /// `+0x10`: `record+0x40`, copied to the owner record's `+0x10`.
    pub first: [f32; 4],
    /// `+0x20`: `record+0x50`, copied to the owner record's `+0x00`.
    pub second: [f32; 4],
    /// `+0x30`/`+0x32`: `record+0x8c`/`+0x8e`.
    pub u30: u16,
    pub u32_: u16,
}

/// The shape descriptor `ShapeEntryFromDescriptor` (`0x142cb7f10`) reads.
///
/// The type is at `+0x67`; for the character's shape path it is
/// [`KIND_CAPSULE`], and the type-1 element is built from `+0x70`/`+0x80`/`+0x90`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeDescriptor {
    /// `+0x58`: `record+0x80`.
    pub u58: u32,
    /// `+0x5c`: `record+0x84`.
    pub u5c: u16,
    /// `+0x5e`: `0`.
    pub u5e: u16,
    /// `+0x60`: `record+0x90`.
    pub u60: u16,
    /// `+0x62`: `0xffff`.
    pub u62: u16,
    /// `+0x64`: `0`.
    pub u64_: u8,
    /// `+0x65`: `1`.
    pub u65: u8,
    /// `+0x66`: `0x0100`, so `+0x67` is [`KIND_CAPSULE`].
    pub u66: u16,
    /// `+0x70`: the capsule's first point (`record+0x10`).
    pub point_a: [f32; 4],
    /// `+0x80`: the capsule's second point (`record+0x20`), after the nudge.
    pub point_b: [f32; 4],
    /// `+0x90`: the capsule's radius (`record+0x30`).
    pub radius: f32,
}

/// `ShapeEntryFromDescriptor`'s type byte for this descriptor.
pub fn shape_kind() -> u8 {
    (DESC_66 >> 8) as u8
}

/// Build the character's owner descriptor exactly as `0x142cf35a0` writes it.
pub fn owner_descriptor(record: &CharacterCollision) -> OwnerDescriptor {
    OwnerDescriptor {
        word: record.w98,
        first: record.owner.0,
        second: record.owner.1,
        u30: record.u8c,
        u32_: record.u8e,
    }
}

/// Build the character's shape descriptor exactly as `0x142cf35a0` writes it,
/// including the degenerate-pair nudge.
pub fn shape_descriptor(record: &CharacterCollision) -> ShapeDescriptor {
    let (a, mut b) = record.capsule;
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let sum3 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    // `COMISS` sets the unordered flags too, so `JA` is not taken for NaN: the
    // predicate is `is_nan || <=`, not `<=`.
    if sum3.is_nan() || sum3 <= NON_DEGENERATE {
        let nudge = degenerate_nudge();
        b = [a[0] + nudge, a[1], a[2], a[3]];
    }
    ShapeDescriptor {
        u58: record.u80,
        u5c: record.u84,
        u5e: 0,
        u60: record.u90,
        u62: DESC_62,
        u64_: 0,
        u65: 1,
        u66: DESC_66,
        point_a: a,
        point_b: b,
        radius: record.radius,
    }
}

/// The smallest the client's radius clamp allows (`COMISS`-guarded `0.02`).
pub const MIN_RADIUS: f32 = 0.02;

/// The fraction of the radius the capsule's segment keeps when the mover's
/// extent is too small: `fVar11 * 0.1`.
pub const SEGMENT_FRACTION: f32 = 0.1;

/// The factor `ConstructMoverBody` uses when it maxes the mover's extent:
/// `[mover+0x2e0] = max([+0x2e0], 2.1 * [+0x2e4] + [+0x2e8])`.
pub const EXTENT_SLACK: f32 = 2.1;

/// The mover's vertical step limits, `[+0x688]` and `[+0x68c]`.
///
/// `min` is the **negative** downward limit (a step down deeper than this is a
/// fall) and `max` the upward limit, both exactly as `ConstructMoverBody`
/// stores them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepLimits {
    pub min: f32,
    pub max: f32,
}

impl StepLimits {
    /// The downward reach as a positive distance, `-min`.
    pub fn reach(&self) -> f32 {
        -self.min
    }
}

/// The client's step limits, from the mover component's seven fields.
///
/// `ConstructMoverBody` (`0x140ca9730`) maxes both capsules' extents first -
/// `c78 = max(c78, 2.1*c7c + c80)` and `c84 = max(c84, 2.1*c88 + c8c)` - then:
///
/// ```text
/// down = max(c84 + c8c, c78 + c80)      // note: the c84 pair is the left one
/// min  = -down, clamped so that -down <= 0
/// max  = max(0, c_a4 + c_a4)
/// ```
///
/// The `MOVSS`/`MAXSS` order is the disassembly's: `[+0x454] + [+0x44c]` is
/// computed first and is the value the `MAXSS` on the second sum compares
/// against.
pub fn step_limits_from_mover(
    extent: f32,
    radius: f32,
    offset: f32,
    second_extent: f32,
    second_radius: f32,
    second_offset: f32,
    up_field: f32,
) -> StepLimits {
    let raised_extent = {
        let candidate = EXTENT_SLACK * radius + offset;
        if extent <= candidate {
            candidate
        } else {
            extent
        }
    };
    let raised_second = {
        let candidate = EXTENT_SLACK * second_radius + second_offset;
        if second_extent <= candidate {
            candidate
        } else {
            second_extent
        }
    };
    // `fVar45 = [+0x454] + [+0x44c]`, then `fVar46 = [+0x448] + [+0x440]` and
    // `MAXSS XMM0(c84-sum), XMM2(c78-sum)` when `c78-sum <= c84-sum`.
    let left = second_extent_add(raised_second, second_offset);
    let right = raised_extent + offset;
    let down = if right <= left { left } else { right };
    let min = if -down <= 0.0 { -down } else { 0.0 };
    let doubled = up_field + up_field;
    let max = if doubled <= 0.0 { 0.0 } else { doubled };
    StepLimits { min, max }
}

fn second_extent_add(a: f32, b: f32) -> f32 {
    a + b
}

/// The character's collision capsule, in its own local space.
///
/// Read by `0x142cf35a0` as the shape record's `+0x10`/`+0x20`/`+0x30` and
/// consumed by `0x142ccd7b0` through the type-1 pool element.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Capsule {
    /// The lower sphere's centre: `(0, r, 0)`.
    pub point_a: [f32; 3],
    /// The upper sphere's centre: `(0, r + d, 0)`.
    pub point_b: [f32; 3],
    /// The sphere radius `r`.
    pub radius: f32,
}

impl Capsule {
    /// The segment's length, `d`.
    pub fn segment(&self) -> f32 {
        self.point_b[1] - self.point_a[1]
    }

    /// The capsule's total height, `2r + d`.
    pub fn height(&self) -> f32 {
        2.0 * self.radius + self.segment()
    }
}

/// The client's capsule, from the three `STUCharacterMoverComponent` fields.
///
/// `extent` is the class's `+0x78` (`0x1972C687`), `radius` its `+0x7c`
/// (`0x7A70E3E3`), `offset` its `+0x80` (`0x4A576BC2`).
///
/// `ConstructMoverBody` (`0x140ca9730`) maxes `extent` against
/// `2.1 * radius + offset` **first**, using the raw radius; `0x140ca5db0` then
/// clamps the radius to `0.02` and derives the segment - with the **raw** radius
/// again, in the client's own addition order:
///
/// ```text
/// raised = max(extent, 2.1 * radius + offset)
/// r      = max(0.02, radius)
/// d      = max(r * 0.1, raised - (offset + radius + radius))
/// ```
///
/// The comparisons are the client's `COMISS`/`<=` forms, so a `NaN` takes the
/// left-hand branch (`extent`, `0.02`, `r * 0.1` respectively).
pub fn capsule_from_mover(extent: f32, radius: f32, offset: f32) -> Capsule {
    let candidate = EXTENT_SLACK * radius + offset;
    let raised = if extent <= candidate {
        candidate
    } else {
        extent
    };
    let r = if MIN_RADIUS <= radius {
        radius
    } else {
        MIN_RADIUS
    };
    let raw = raised - (offset + radius + radius);
    let floor = r * SEGMENT_FRACTION;
    let segment = if floor <= raw { raw } else { floor };
    Capsule {
        point_a: [0.0, r, 0.0],
        point_b: [0.0, r + segment, 0.0],
        radius: r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record with a distinct sentinel in every field, so a wrong offset shows.
    fn record_bytes() -> Vec<u8> {
        let mut b = vec![0u8; RECORD_LEN];
        let put_u32 =
            |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        let put_u16 =
            |b: &mut Vec<u8>, at: usize, v: u16| b[at..at + 2].copy_from_slice(&v.to_le_bytes());
        let put_quad = |b: &mut Vec<u8>, at: usize, v: [f32; 4]| {
            for (i, x) in v.iter().enumerate() {
                b[at + i * 4..at + i * 4 + 4].copy_from_slice(&x.to_le_bytes());
            }
        };
        put_u32(&mut b, 0x00, MODE_SHAPE);
        put_quad(&mut b, 0x10, [0.0, 0.0, 1.0, 0.0]);
        put_quad(&mut b, 0x20, [0.0, 0.0, 0.6, 0.0]);
        b[0x30..0x34].copy_from_slice(&0.35f32.to_le_bytes());
        put_quad(&mut b, 0x40, [9.0, 10.0, 11.0, 12.0]);
        put_quad(&mut b, 0x50, [13.0, 14.0, 15.0, 16.0]);
        put_quad(&mut b, 0x60, [17.0, 18.0, 19.0, 20.0]);
        put_quad(&mut b, 0x70, [21.0, 22.0, 23.0, 24.0]);
        put_u32(&mut b, 0x80, 0x1122_3344);
        put_u16(&mut b, 0x84, 0x5566);
        put_u16(&mut b, 0x86, 0x7788);
        b[0x88..0x8c].copy_from_slice(&(2.25f32).to_le_bytes());
        put_u16(&mut b, 0x8c, 0x99aa);
        put_u16(&mut b, 0x8e, 0xbbcc);
        put_u16(&mut b, 0x90, 0xddee);
        b[0x98..0xa0].copy_from_slice(&0x0123_4567_89ab_cdefu64.to_le_bytes());
        b
    }

    #[test]
    fn every_field_is_read_at_its_own_offset() {
        let r = CharacterCollision::parse(&record_bytes()).expect("the sentinel record parses");
        assert_eq!(r.mode, MODE_SHAPE);
        assert_eq!(r.capsule, ([0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.6, 0.0]));
        assert_eq!(r.radius, 0.35);
        assert_eq!(r.owner, ([9.0, 10.0, 11.0, 12.0], [13.0, 14.0, 15.0, 16.0]));
        assert_eq!(r.body, ([17.0, 18.0, 19.0, 20.0], [21.0, 22.0, 23.0, 24.0]));
        assert_eq!(r.u80, 0x1122_3344);
        assert_eq!(r.u84, 0x5566);
        assert_eq!(r.world_slot, 0x7788);
        assert_eq!(r.f88, 2.25);
        assert_eq!(r.u8c, 0x99aa);
        assert_eq!(r.u8e, 0xbbcc);
        assert_eq!(r.u90, 0xddee);
        assert_eq!(r.w98, 0x0123_4567_89ab_cdef);
    }

    #[test]
    fn a_short_record_is_refused() {
        let bytes = record_bytes();
        assert_eq!(
            CharacterCollision::parse(&bytes[..RECORD_LEN - 1]),
            Err(Error::Truncated {
                len: RECORD_LEN - 1
            })
        );
    }

    #[test]
    fn an_unknown_mode_builds_nothing() {
        let mut bytes = record_bytes();
        bytes[0..4].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(
            CharacterCollision::parse(&bytes),
            Err(Error::UnknownMode(2))
        );
    }

    #[test]
    fn the_shape_descriptor_is_a_type_1_capsule_from_the_record() {
        let r = CharacterCollision::parse(&record_bytes()).expect("parses");
        let d = shape_descriptor(&r);
        // The type byte `ShapeEntryFromDescriptor` reads at `+0x67`.
        assert_eq!(shape_kind(), KIND_CAPSULE);
        assert_eq!((d.u66 >> 8) as u8, KIND_CAPSULE);
        assert_eq!(d.u62, 0xffff);
        assert_eq!((d.u64_, d.u65), (0, 1));
        // The capsule: the record's own triplet.
        assert_eq!(d.point_a, r.capsule.0);
        assert_eq!(d.point_b, r.capsule.1);
        assert_eq!(d.radius, 0.35);
        // The record's remaining fields, at their descriptor offsets.
        assert_eq!(d.u58, r.u80);
        assert_eq!(d.u5c, r.u84);
        assert_eq!(d.u60, r.u90);
        assert_eq!(d.u5e, 0);
    }

    #[test]
    fn the_owner_descriptor_carries_the_validated_pair() {
        let r = CharacterCollision::parse(&record_bytes()).expect("parses");
        let o = owner_descriptor(&r);
        assert_eq!(o.word, r.w98);
        assert_eq!(o.first, r.owner.0);
        assert_eq!(o.second, r.owner.1);
        assert_eq!((o.u30, o.u32_), (r.u8c, r.u8e));
    }

    #[test]
    fn a_degenerate_capsule_is_nudged_in_lane_zero_only() {
        let mut bytes = record_bytes();
        let a = bytes[0x10..0x20].to_vec();
        bytes[0x20..0x30].copy_from_slice(&a);
        let r = CharacterCollision::parse(&bytes).expect("parses");
        let d = shape_descriptor(&r);
        assert_eq!(d.point_a, [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(d.point_b, [degenerate_nudge(), 0.0, 1.0, 0.0]);
        assert_eq!(degenerate_nudge(), 0.01);
        // The radius is untouched by the nudge.
        assert_eq!(d.radius, 0.35);
    }

    #[test]
    fn a_nan_capsule_takes_the_nudge() {
        let mut bytes = record_bytes();
        bytes[0x20..0x24].copy_from_slice(&f32::NAN.to_le_bytes());
        let r = CharacterCollision::parse(&bytes).expect("parses");
        let d = shape_descriptor(&r);
        assert_eq!(d.point_b, [degenerate_nudge(), 0.0, 1.0, 0.0]);
    }

    #[test]
    fn the_threshold_is_the_images_1e_35_not_float_min() {
        // `0x057a0000` = 1.1754943508222875e-35, exactly `FLT_MIN * 1000`.
        assert_eq!(NON_DEGENERATE.to_bits(), 0x057A_0000);
        assert_eq!(NON_DEGENERATE, f32::MIN_POSITIVE * 1000.0);
    }

    #[test]
    fn a_pair_between_float_min_and_the_threshold_still_nudges() {
        // The points differ only in lane 0, by 1e-18, so |d|² = 1e-36: above
        // FLT_MIN, at or below the client's 1e-35 guard, so the nudge runs.
        let mut bytes = record_bytes();
        let a = bytes[0x10..0x20].to_vec();
        bytes[0x20..0x30].copy_from_slice(&a);
        bytes[0x10..0x14].copy_from_slice(&0.0f32.to_le_bytes());
        bytes[0x20..0x24].copy_from_slice(&1e-18f32.to_le_bytes());
        let r = CharacterCollision::parse(&bytes).expect("parses");
        let d = shape_descriptor(&r);
        assert_eq!(d.point_b, [degenerate_nudge(), 0.0, 1.0, 0.0]);
    }

    #[test]
    fn the_common_hero_capsule_is_half_a_metre_wide_and_one_and_a_half_tall() {
        // Every hero measured with `0x1972C687 = 2.0`, `0x7A70E3E3 = 0.5`,
        // `0x4A576BC2 = 0.5` (heroes 2, 3, 4, 8, 10 on this install).
        let c = capsule_from_mover(2.0, 0.5, 0.5);
        assert_eq!(c.radius, 0.5);
        assert_eq!(c.point_a, [0.0, 0.5, 0.0]);
        assert_eq!(c.point_b, [0.0, 1.0, 0.0]);
        assert_eq!(c.segment(), 0.5);
        assert_eq!(c.height(), 1.5);
    }

    #[test]
    fn the_large_hero_capsule_is_seventy_centimetres_wide_and_two_metres_tall() {
        // Hero 7: `0x1972C687 = 2.3`, `0x7A70E3E3 = 0.7`, `0x4A576BC2 = 0.5`.
        let c = capsule_from_mover(2.3, 0.7, 0.5);
        assert_eq!(c.radius, 0.7);
        assert_eq!(c.point_a, [0.0, 0.7, 0.0]);
        // 0.4 = 2.3 - (0.5 + 0.7 + 0.7); the 0.1 * r floor is 0.07, below it.
        assert!((c.segment() - 0.4).abs() < 1e-6, "segment {}", c.segment());
        // The capsule is 2r + d tall, not the extent field's 2.3.
        assert!((c.height() - 1.8).abs() < 1e-6, "height {}", c.height());
    }

    #[test]
    fn the_extent_is_raised_before_the_segment_is_derived() {
        // 1.0 < 2.1 * 0.5 + 0.5 = 1.55, so the raise applies; the segment is then
        // 1.55 - (0.5 + 0.5 + 0.5) = 0.05, which lands on the 0.1 * r floor.
        let c = capsule_from_mover(1.0, 0.5, 0.5);
        assert!((c.segment() - 0.05).abs() < 1e-6, "segment {}", c.segment());
        assert!((c.point_b[1] - 0.55).abs() < 1e-6, "top {}", c.point_b[1]);
    }

    #[test]
    fn a_small_radius_is_clamped_after_the_raise() {
        // The raise and the segment both use the raw 0.001; only `r` is clamped.
        let c = capsule_from_mover(2.0, 0.001, 0.5);
        assert_eq!(c.radius, MIN_RADIUS);
        assert_eq!(c.point_a, [0.0, 0.02, 0.0]);
        // d = max(0.002, 2.0 - (0.5 + 0.001 + 0.001)) = 1.498.
        assert!(
            (c.segment() - 1.498).abs() < 1e-5,
            "segment {}",
            c.segment()
        );
    }

    #[test]
    fn the_common_heros_step_limits_are_two_and_a_half_down_and_one_up() {
        // Hero 2: extent 2.0, radius 0.5, offset 0.5, second triple 1.25 / 0.5 /
        // 0.25, up field 0.5.
        let l = step_limits_from_mover(2.0, 0.5, 0.5, 1.25, 0.5, 0.25, 0.5);
        assert_eq!(l.min, -2.5);
        assert_eq!(l.max, 1.0);
        assert_eq!(l.reach(), 2.5);
    }

    #[test]
    fn the_large_heros_step_limits_track_its_taller_capsule() {
        // Hero 7: extent 2.3, radius 0.7, offset 0.5, second triple unchanged.
        let l = step_limits_from_mover(2.3, 0.7, 0.5, 1.25, 0.5, 0.25, 0.5);
        assert!((l.reach() - 2.8).abs() < 1e-6, "{}", l.reach());
        assert_eq!(l.max, 1.0);
    }

    #[test]
    fn the_second_triple_wins_when_its_sum_is_larger() {
        // second_extent 1.25 raised to 2.1*0.5 + 0.25 = 1.3 -> 1.55, against
        // extent + offset 1.0: the second pair's sum is the left operand the
        // MAXSS keeps.
        let l = step_limits_from_mover(0.5, 0.25, 0.5, 1.25, 0.5, 0.25, 0.0);
        assert!((l.reach() - 1.55).abs() < 1e-6, "{}", l.reach());
        assert_eq!(l.max, 0.0);
    }

    #[test]
    fn a_negative_up_field_clamps_to_zero() {
        let l = step_limits_from_mover(2.0, 0.5, 0.5, 1.25, 0.5, 0.25, -0.4);
        assert_eq!(l.max, 0.0);
    }

    #[test]
    fn a_pair_above_the_threshold_is_left_alone() {
        // The same pair by 1e-17, so |d|² = 1e-34, above the guard.
        let mut bytes = record_bytes();
        let a = bytes[0x10..0x20].to_vec();
        bytes[0x20..0x30].copy_from_slice(&a);
        bytes[0x10..0x14].copy_from_slice(&0.0f32.to_le_bytes());
        bytes[0x20..0x24].copy_from_slice(&1e-17f32.to_le_bytes());
        let r = CharacterCollision::parse(&bytes).expect("parses");
        assert_eq!(shape_descriptor(&r).point_b, [1e-17f32, 0.0, 1.0, 0.0]);
    }
}
