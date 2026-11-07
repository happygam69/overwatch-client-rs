//! The mover's jump: the state file word, its five transitions, the gate and
//! the take-off impulse, read from `Overwatch.exe`.
//!
//! Everything here is transcribed from the current image. Addresses are
//! current-image addresses; the reasoning and the open links are in
//! `docs/formats/character-controller.md`, "The jump law".
//!
//! The chain, in the client's own order:
//!
//! ```text
//! MoverUpdateFrame 0x140cbb260
//!   buttons = body+0xdc            (the packed usercmd's button dword)
//!   jump  = (buttons >> 6) & 1     bit 6, 0x40
//!   crouch= (buttons >> 7) & 1     bit 7, 0x80
//!   -> FUN_140cc1060(body, &wish, jump, crouch)
//!        -> FUN_140cc1440(body, &wish, cmd[0x40], jump, crouch, cmd[0x41])
//!             property 0x26 scales the air-jump threshold (default 0.001)
//!             if (flags & 0x80000c) != 0   -- airborne --
//!                 request && dot(body+0x1150, &wish) < threshold:
//!                     MoverOnGround 0x140ca91e0, body+0x1a7d = 1,
//!                     body+0x1ad0 = ceil(500000 / [0x143871e08])
//!             else                        -- on the ground --
//!                 no request: MoverOnLanding 0x140ca9120
//!                 request:    body+0x1ad0 -= 1 (the take-off timer)
//!             property 0x14 = jump held ? 1 : 0, then the state modifiers
//!             gate: on-ground && wants > 0.5 && !already-jumping && body
//!             impulse: rise the movement vector body+0x1150 along body+0xc0
//!             MoverOnTakeoff 0x140ca9190(alt=1), then the latch bit
//!             property 0x15 gates the returned "jumped" bool
//! ```
//!
//! **What is not here.** The impulse changes the movement vector
//! `body+0x1150` once, on the take-off frame; the per-frame decay of that
//! vector (gravity) and the contact resolve are the state-modifier machinery at
//! `body+0x960` and the sweep solver, whose record producer is unread.
//! `jump_impulse` is therefore the take-off frame only, and nothing in this
//! module claims to be a full jump arc.

use crate::movement::{sse_add, sse_mul};

/// `body+0x10d0`: the mover's state file word.
pub mod file {
    /// `0x1`: a jump is in progress (set by the take-off, cleared on landing).
    pub const JUMPING: u32 = 0x0000_0001;
    /// `0x2`: crouched. `MoverSetCrouch 0x140ca9100` owns it.
    pub const CROUCHED: u32 = 0x0000_0002;
    /// `0x4`: airborne. `MoverOnTakeoff 0x140ca9190` sets it.
    pub const AIRBORNE: u32 = 0x0000_0004;
    /// `0x8`: a landing is being resolved. `MoverOnLanding 0x140ca9120` sets it.
    pub const LANDING: u32 = 0x0000_0008;
    /// `0x400`: the second take-off bit `MoverOnTakeoff` raises with its flag.
    pub const TAKEOFF_ALT: u32 = 0x0000_0400;
    /// `0x80000`: a per-command bit, written from the jump stage's fourth
    /// argument. It is **not** part of the on-ground mask - the client's mask is
    /// `0x0080000c`, which does not include bit 19.
    pub const COMMAND_BIT: u32 = 0x0008_0000;
    /// `0x800000`: `MoverEnterAir 0x140ca9150` sets it, `MoverOnGround` clears it.
    pub const IN_AIR: u32 = 0x0080_0000;
    /// `0x1000000`: the movement vector is already world space; both transform
    /// helpers (`0x140c9f300`, `0x140ca2740`) take a direct-copy path on it.
    pub const WORLD_SPACE_VECTOR: u32 = 0x0100_0000;
    /// `0xc`: the mask the jump gate tests - anything here means "not on the
    /// ground". `MoverOnGround 0x140ca91e0` clears all of it.
    pub const NOT_ON_GROUND: u32 = IN_AIR | AIRBORNE | LANDING;
}

/// `0x140ca9100`: set or clear [`file::CROUCHED`]. Nothing else is touched.
pub fn mover_set_crouch(flags: u32, crouch: bool) -> u32 {
    if crouch {
        flags | file::CROUCHED
    } else {
        flags & !file::CROUCHED
    }
}

/// What a state-file transition wrote besides the flag word.
///
/// `stamp` is the frame id `body+0xe4` written to `body+0x10e0`. `latch` is the
/// value written to `body+0x1a76`, when the transition writes it at all. They
/// are reported separately because the client's conditions differ:
/// `MoverOnGround` clears the latch on every path but stamps only when it
/// cleared the mask, and `MoverOnTakeoff` sets the latch on every path that is
/// not the early return but stamps only when bits 2 and 3 were clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    pub flags: u32,
    pub stamp: bool,
    pub latch: Option<bool>,
}

/// `0x140ca9120`: the landing transition.
///
/// A no-op when [`file::IN_AIR`] is already set. Otherwise it clears
/// [`file::AIRBORNE`], raises [`file::LANDING`], and - only when neither
/// [`file::AIRBORNE`] nor [`file::LANDING`] was set - stamps the command id
/// `body+0xe4` into `body+0x10e0`.
pub fn mover_on_landing(flags: u32) -> Transition {
    if flags & file::IN_AIR != 0 {
        return Transition {
            flags,
            stamp: false,
            latch: None,
        };
    }
    Transition {
        flags: (flags & !file::AIRBORNE) | file::LANDING,
        stamp: flags & (file::AIRBORNE | file::LANDING) == 0,
        latch: None,
    }
}

/// `0x140ca9150`: enter the air without jumping (a ledge walk-off).
///
/// Clears [`file::AIRBORNE`] and [`file::LANDING`], raises [`file::IN_AIR`],
/// and stamps the command id when none of [`file::NOT_ON_GROUND`] was set.
pub fn mover_enter_air(flags: u32) -> Transition {
    Transition {
        flags: (flags & !(file::AIRBORNE | file::LANDING)) | file::IN_AIR,
        stamp: flags & file::NOT_ON_GROUND == 0,
        latch: None,
    }
}

/// `0x140ca9190`: the take-off transition. A no-op when [`file::IN_AIR`] is set.
///
/// `alt` is the client's `dl`: with it set [`file::TAKEOFF_ALT`] is raised,
/// with it clear [file::TAKEOFF_ALT] is cleared. The `body+0x1a76` latch - the
/// "just left the ground" byte [`mover_on_ground`] clears - is set on every
/// path that is not the early return, while the frame stamp needs bits 2 and 3
/// clear as well.
pub fn mover_on_takeoff(flags: u32, alt: bool) -> Transition {
    if flags & file::IN_AIR != 0 {
        return Transition {
            flags,
            stamp: false,
            latch: None,
        };
    }
    let mut next = (flags & !(file::LANDING | file::TAKEOFF_ALT)) | file::AIRBORNE;
    if alt {
        next |= file::TAKEOFF_ALT;
    }
    Transition {
        flags: next,
        stamp: flags & (file::AIRBORNE | file::LANDING) == 0,
        latch: Some(true),
    }
}

/// `0x140ca91e0`: the body is on the ground again.
///
/// When any of [`file::NOT_ON_GROUND`] was set it clears the whole mask and
/// stamps the command id; the `body+0x1a76` latch is cleared on every path,
/// including the one that touches nothing else.
pub fn mover_on_ground(flags: u32) -> Transition {
    let dirty = flags & file::NOT_ON_GROUND != 0;
    Transition {
        flags: if dirty {
            flags & !file::NOT_ON_GROUND
        } else {
            flags
        },
        stamp: dirty,
        latch: Some(false),
    }
}

/// `body+0xc0..0xcc`: the direction the ramp scales and the axis the take-off
/// impulse elevates along. `ConstructMoverBase` writes `(0, 1, 0, 0)`
/// (`0x140ca3acc`, the constant at `0x14328cdd0`), and the field index finds no
/// per-frame writer for it.
pub const DIRECTION: [f32; 4] = [0.0, 1.0, 0.0, 0.0];

/// `body+0x60c`: the vertical lane a standing jump takes off with.
///
/// The value is the mover component's field at class offset `0xe4`
/// (hash `EC289987`; hero 1 on this install reads `6.0`), carried into the body
/// by the component copy `CopyMoverComponentFields` / `FUN_140ca4980`, whose
/// base is `body+0x528`. The same field also caps the lateral lane of the
/// movement vector in `0x140cb2220`, which is why it is not called "jump
/// speed" here. See the doc page for the proof and for the one offset that
/// still disagrees with the ramp's verified unit.
pub mod body {
    /// The vertical lane the take-off writes (`body+0x60c`).
    pub const TAKEOFF_LANE: usize = 0x60c;
    /// The local movement vector (`body+0x1150..0x1160`).
    pub const VECTOR: usize = 0x1150;
    /// The state anchor the vector is measured from (`body+0x1510`), always
    /// zero in this build: nothing in `.text` writes it.
    pub const ANCHOR: usize = 0x1510;
    /// The physics-world slot (`body+0x1ad8`): the impulse needs a live body.
    pub const WORLD: usize = 0x1ad8;
    /// The state file word.
    pub const FLAGS: usize = 0x10d0;
}

/// The client's three-lane horizontal sum: two `SHUFPS` broadcasts and two
/// `ADDPS`, so the operand order is `(p1 + p0) + p2`. The order matters: when
/// more than one lane is a NaN, the payload that survives is the one `ADDSS`
/// keeps, not the one a left-to-right sum would keep. The fourth lane is never
/// read - the client broadcasts `0`, `0x55` and `0xaa` only.
fn sum3(p: [f32; 3]) -> f32 {
    sse_add(sse_add(p[1], p[0]), p[2])
}

/// The take-off impulse of `0x140cc15d5..0x140cc166a`.
///
/// The client reads the movement vector through the state transform
/// (`0x140c9f300`: rotate the local quad by the three basis rows, then add the
/// state origin), changes it by `rise` along [`DIRECTION`], and writes it back
/// through (`0x140ca2740`: subtract the origin, then `M^T`). With
/// [`file::WORLD_SPACE_VECTOR`] set both helpers copy instead of transforming.
///
/// `matrix` is the state record's three 16-byte row blocks as
/// `0x140c9e740` returns them; `None` is the lookup failure the client skips
/// the transform for. `anchor` is the state origin `body+0x1510` - a
/// **position**, not a bias.
///
/// `rise` is the two-branch expression of `0x140cc15ee..0x140cc1639`, and the
/// branches are not interchangeable:
///
/// ```text
/// if (flags & 0x80000c) != 0 && along > 0 :  rise = (along + lane) - along
/// else                                    :  rise = lane - along
/// ```
///
/// The first branch is written with the subtraction still in place because
/// `(along + lane) - along` is *not* `lane` in floats. Nothing in `.text`
/// writes `body+0x1510`, so the anchor is zero in this build; on the ground the
/// effect is then the world lane of the vector becoming `takeoff_lane`, and in
/// the air (rising) it is that lane gaining `takeoff_lane`.
///
/// The fourth lane of the result is the third lane's copy on the transform
/// path, which is what the client's closing two `shufps` leave; the direct-copy
/// paths keep all four lanes.
pub fn jump_impulse(
    local: [f32; 4],
    anchor: [f32; 4],
    matrix: Option<&[[f32; 4]; 3]>,
    flags: u32,
    takeoff_lane: f32,
) -> [f32; 4] {
    let world_space = flags & file::WORLD_SPACE_VECTOR != 0;
    let target = if world_space {
        local
    } else {
        let mut out = [anchor[0], anchor[1], anchor[2], anchor[3]];
        if let Some(m) = matrix {
            for lane in 0..4 {
                out[lane] += local[0] * m[0][lane] + local[1] * m[1][lane] + local[2] * m[2][lane];
            }
        } else {
            out = [
                local[0] + anchor[0],
                local[1] + anchor[1],
                local[2] + anchor[2],
                local[3] + anchor[3],
            ];
        }
        out
    };

    // `ldr xmm4,[rdi+0xc0] ; mulps xmm1,xmm3` (`0x140cc15f4`): the direction is
    // the destination operand, so it is the first of the two multiplicands.
    let along = sum3([
        sse_mul(DIRECTION[0], target[0]),
        sse_mul(DIRECTION[1], target[1]),
        sse_mul(DIRECTION[2], target[2]),
    ]);
    // `TEST [rsi],0x80000c ; JE 0x140cc1631`, then `COMISS xmm2,xmm7 ; JBE`.
    // `JBE` is taken on unordered too, so a NaN `along` takes the `lane - along`
    // branch - which is exactly what `along > 0.0` does in Rust.
    let rise = if flags & file::NOT_ON_GROUND != 0 && along > 0.0 {
        sse_add(along, takeoff_lane) - along
    } else {
        takeoff_lane - along
    };
    // `SHUFPS xmm0,xmm0,0 ; MULPS xmm0,xmm4 ; ADDPS xmm3,xmm0` `0x140cc1644`.
    let target = [
        sse_add(target[0], sse_mul(rise, DIRECTION[0])),
        sse_add(target[1], sse_mul(rise, DIRECTION[1])),
        sse_add(target[2], sse_mul(rise, DIRECTION[2])),
        sse_add(target[3], sse_mul(rise, DIRECTION[3])),
    ];

    if world_space {
        return target;
    }
    let delta = [
        target[0] - anchor[0],
        target[1] - anchor[1],
        target[2] - anchor[2],
        target[3] - anchor[3],
    ];
    match matrix {
        None => delta,
        Some(m) => {
            // The client's three dot products, then the third lane duplicated:
            // `shufps xmm4, xmm1, 0xe4` / `shufps xmm4, xmm2, 0xe8` at
            // `0x140ca27e6`/`0x140ca27ed` leave `(d0, d1, d2, d2)`.
            let d0 = dot3(delta, m[0]);
            let d1 = dot3(delta, m[1]);
            let d2 = dot3(delta, m[2]);
            [d0, d1, d2, d2]
        }
    }
}

/// `dot3` as `0x140ca2791` writes it: `MULPS v,row` per row with `v` as the
/// destination operand, then [`sum3`]. The fourth lane of neither operand is
/// read.
fn dot3(v: [f32; 4], row: [f32; 4]) -> f32 {
    sum3([
        sse_mul(v[0], row[0]),
        sse_mul(v[1], row[1]),
        sse_mul(v[2], row[2]),
    ])
}

/// The jump gate of `0x140cc15a5..0x140cc15dc`.
///
/// `wants` is the state-modifier result of property `0x14` (the jump button as
/// 1.0 or 0.0 through the modifier chain); it is not the raw button. Returns
/// the flag word after the "not wanted" clear, and whether the impulse runs.
///
/// The `>` is deliberate and is not `<=`'s complement: the client's
/// `COMISS xmm0,xmm8 ; JBE` at `0x140cc15c3` takes the branch when the compare
/// is below **or unordered**, so a NaN request must clear the latch.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn jump_gate(flags: u32, wants: f32, has_world: bool) -> (u32, bool) {
    if flags & file::NOT_ON_GROUND != 0 {
        return (flags, false);
    }
    if !(wants > 0.5) {
        // `and eax, 0xfffffffe` at `0x140cc166f`: the latch is released when the
        // request is withdrawn, which is what makes a hold a single jump.
        return (flags & !file::JUMPING, false);
    }
    if flags & file::JUMPING != 0 {
        return (flags, false);
    }
    if !has_world {
        // `0x140cc166a`: the latch is taken even without a body, but nothing
        // is published. The flag bit is the only effect.
        return (flags | file::JUMPING, false);
    }
    (flags, true)
}

/// The jump button bits of the packed usercmd's button dword, as
/// `MoverUpdateFrame` splits them at `0x140cbb627`: bit 6 is the jump, bit 7
/// the crouch request.
pub fn jump_button(buttons: u32) -> bool {
    (buttons >> 6) & 1 != 0
}

/// The crouch button bit (`0x140cbb63c`, `and eax, 0x80`).
pub fn crouch_button(buttons: u32) -> bool {
    buttons & 0x80 != 0
}

/// Read `body+0x60c`.
pub fn takeoff_lane(body: &[u8]) -> f32 {
    f32::from_le_bytes(
        body[body::TAKEOFF_LANE..body::TAKEOFF_LANE + 4]
            .try_into()
            .unwrap(),
    )
}

/// Read the local movement vector `body+0x1150..0x1160`.
pub fn local_vector(body: &[u8]) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    for (i, lane) in out.iter_mut().enumerate() {
        let at = body::VECTOR + 4 * i;
        *lane = f32::from_le_bytes(body[at..at + 4].try_into().unwrap());
    }
    out
}

/// Read the state anchor `body+0x1510..0x1520`.
pub fn anchor(body: &[u8]) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    for (i, lane) in out.iter_mut().enumerate() {
        let at = body::ANCHOR + 4 * i;
        *lane = f32::from_le_bytes(body[at..at + 4].try_into().unwrap());
    }
    out
}

/// Read the state file word.
pub fn flags(body: &[u8]) -> u32 {
    u32::from_le_bytes(body[body::FLAGS..body::FLAGS + 4].try_into().unwrap())
}

/// Write the state file word back.
pub fn set_flags(body: &mut [u8], value: u32) {
    body[body::FLAGS..body::FLAGS + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `0x140ca9100` is a plain bit-1 set/clear; the rest of the word survives.
    #[test]
    fn crouch_touches_only_bit_one() {
        let start = 0xffff_ffffu32;
        assert_eq!(mover_set_crouch(start, false), start & !2);
        assert_eq!(mover_set_crouch(0, true), 2);
        assert_eq!(mover_set_crouch(0x8000_0001, true), 0x8000_0003);
    }

    /// `0x140ca9120`: `bt edx,0x17` returns early; otherwise clear bit 2, set
    /// bit 3, and stamp only when neither bit was set.
    #[test]
    fn landing_is_a_no_op_in_the_air() {
        let t = mover_on_landing(file::IN_AIR);
        assert_eq!(t.flags, file::IN_AIR);
        assert!(!t.stamp && t.latch.is_none());
        assert!(!mover_on_landing(file::IN_AIR | file::AIRBORNE).stamp);

        let t = mover_on_landing(0);
        assert_eq!(t.flags, file::LANDING);
        assert!(t.stamp);

        // The stamp test reads the word as it arrived, not after the clear.
        let t = mover_on_landing(file::AIRBORNE);
        assert_eq!(t.flags, file::LANDING);
        assert!(!t.stamp);

        let t = mover_on_landing(1);
        assert_eq!(t.flags, 1 | file::LANDING);
        assert!(t.stamp);
    }

    /// `0x140ca9150`: clear bits 2 and 3, raise bit 23, stamp when the mask was
    /// clear. It never returns early.
    #[test]
    fn entering_the_air_sets_the_in_air_bit() {
        let t = mover_enter_air(0);
        assert_eq!(t.flags, file::IN_AIR);
        assert!(t.stamp);

        // 5 is bits 0 and 2; bit 2 is cleared with the mask, bit 0 survives.
        let t = mover_enter_air(file::AIRBORNE | file::LANDING | 5);
        assert_eq!(t.flags, file::IN_AIR | 1);
        assert!(!t.stamp);

        // A word that already has bit 23 keeps it and does not re-stamp.
        let t = mover_enter_air(file::IN_AIR | file::AIRBORNE);
        assert_eq!(t.flags, file::IN_AIR);
        assert!(!t.stamp);
    }

    /// `0x140ca9190`: bit 23 is a guard, bit 3 clears, bit 2 sets, and the
    /// caller's `dl` decides bit 10. The latch is set on every non-early path
    /// but the stamp needs bits 2 and 3 clear, so the two are independent.
    #[test]
    fn takeoff_sets_the_airborne_bit_and_the_latch() {
        assert_eq!(
            mover_on_takeoff(file::IN_AIR, true),
            Transition {
                flags: file::IN_AIR,
                stamp: false,
                latch: None
            }
        );

        let t = mover_on_takeoff(0, true);
        assert_eq!(t.flags, file::AIRBORNE | file::TAKEOFF_ALT);
        assert_eq!(t.latch, Some(true));
        assert!(t.stamp);

        let t = mover_on_takeoff(0, false);
        assert_eq!(t.flags, file::AIRBORNE);
        assert_eq!(t.latch, Some(true));

        // `btr eax, 0xa` / `cmove`: with `dl` clear bit 10 is *cleared*, not
        // left alone, so a word that arrived with it loses it.
        let t = mover_on_takeoff(file::TAKEOFF_ALT, false);
        assert_eq!(t.flags, file::AIRBORNE);
        let t = mover_on_takeoff(file::TAKEOFF_ALT, true);
        assert_eq!(t.flags, file::AIRBORNE | file::TAKEOFF_ALT);

        // Bit 3 clears, everything unrelated survives, and the latch is still
        // set - but the stamp is not, because bit 3 arrived set.
        let t = mover_on_takeoff(file::LANDING | 0x8000_0000, true);
        assert_eq!(t.flags, file::AIRBORNE | file::TAKEOFF_ALT | 0x8000_0000);
        assert_eq!(t.latch, Some(true));
        assert!(!t.stamp);
    }

    /// `0x140ca91e0`: the mask test decides the clear and the stamp, the clear
    /// is the whole three-bit mask, and the latch clears on every path.
    #[test]
    fn on_ground_clears_the_whole_not_on_ground_mask() {
        let t = mover_on_ground(0);
        assert_eq!((t.flags, t.stamp), (0, false));
        assert_eq!(t.latch, Some(false));

        let all = file::NOT_ON_GROUND;
        let t = mover_on_ground(all | 0x1234);
        // `0x1234` holds bit 2 (`file::AIRBORNE`); that bit is part of the
        // mask, so it goes with it.
        assert_eq!(t.flags, 0x1234 & !file::AIRBORNE);
        assert!(t.stamp);

        // `0x80000c` is the mask, so a word holding only `0x800000` is still
        // "not on the ground" here.
        let t = mover_on_ground(file::IN_AIR);
        assert_eq!(t.flags, 0);
        assert!(t.stamp);
        assert_eq!(t.latch, Some(false));

        // A word outside the mask leaves the flags alone but still clears the
        // latch: the client's `mov byte [rcx+0x1a76],0` is outside the branch.
        let t = mover_on_ground(file::COMMAND_BIT);
        assert_eq!(t.flags, file::COMMAND_BIT);
        assert!(!t.stamp);
        assert_eq!(t.latch, Some(false));
    }

    /// `0x140cbb627`: bit 6, and bit 7 is the crouch.
    #[test]
    fn the_button_split_is_bit_six_and_seven() {
        assert!(jump_button(0x40));
        assert!(!jump_button(0x80));
        assert!(!crouch_button(0x40));
        assert!(crouch_button(0x80));
        assert!(jump_button(0xffff_ffff));
        assert!(!jump_button(0));
    }

    /// The client's `TEST [rsi],0x80000c ; JE` / `COMISS xmm2,0 ; JBE` pair at
    /// `0x140cc15ee..0x140cc1622` chooses between `(along + lane) - along` and
    /// `lane - along`. The two differ whenever the vector is already rising.
    #[test]
    fn the_airborne_branch_adds_instead_of_replacing() {
        let identity = [
            [1.0f32, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        // Airborne and rising: the world lane gains the take-off lane, so it
        // ends at `along + lane`, not at `lane`.
        let rising = jump_impulse(
            [1.0, 2.0, 3.0, 0.0],
            [0.0; 4],
            Some(&identity),
            file::AIRBORNE,
            6.0,
        );
        assert_eq!(rising, [1.0, 2.0 + 6.0, 3.0, 3.0]);

        // Airborne but not rising: the same expression as on the ground.
        let falling = jump_impulse(
            [1.0, -2.0, 3.0, 0.0],
            [0.0; 4],
            Some(&identity),
            file::AIRBORNE,
            6.0,
        );
        assert_eq!(falling, [1.0, 6.0, 3.0, 3.0]);

        // Every bit of the mask takes the branch, not only `AIRBORNE`.
        for bit in [file::AIRBORNE, file::LANDING, file::IN_AIR] {
            let out = jump_impulse([0.0, 1.0, 0.0, 0.0], [0.0; 4], None, bit, 6.0);
            assert_eq!(out[1], 7.0, "mask bit {bit:#x} must take the add branch");
        }
    }

    /// `COMISS` is unordered on a NaN and `JBE` is taken on unordered, so a NaN
    /// `along` takes `lane - along` and the result is a NaN, not `lane`.
    #[test]
    fn a_nan_along_takes_the_subtract_branch() {
        let nan = f32::from_bits(0x7fc0_0001);
        let out = jump_impulse([nan, nan, nan, 0.0], [0.0; 4], None, file::AIRBORNE, 6.0);
        assert!(out[1].is_nan());
        assert_ne!(out[1], 7.0);
    }

    /// The fourth lane is never read by the sums: `DIRECTION[3]` is zero, and a
    /// four-lane sum would turn an infinite fourth lane into a NaN.
    #[test]
    fn the_fourth_lane_does_not_enter_the_sum() {
        let inf = f32::INFINITY;
        let out = jump_impulse([0.0, 2.0, 0.0, inf], [0.0; 4], None, 0, 6.0);
        assert_eq!(out, [0.0, 6.0, 0.0, inf]);
    }

    /// The gate: in the air nothing happens; a withdrawn request releases the
    /// latch; a held one needs the latch clear; no body still takes the latch.
    #[test]
    fn the_gate_matches_the_client_branches() {
        // `TEST EAX,0x80000c ; JNZ 0x140cc1674`.
        assert_eq!(
            jump_gate(file::AIRBORNE, 1.0, true),
            (file::AIRBORNE, false)
        );

        // `COMISS XMM0,0.5 ; JBE 0x140cc166f` clears bit 0.
        assert_eq!(jump_gate(file::JUMPING, 0.5, true), (0, false));
        assert_eq!(jump_gate(file::JUMPING, 0.25, true), (0, false));

        // `TEST AL,0x1 ; JNZ`: an in-progress jump is not restarted.
        assert_eq!(jump_gate(file::JUMPING, 1.0, true), (file::JUMPING, false));

        // `CMP qword [rdi+0x1ad8],r14 ; JZ 0x140cc166a`: latch, no impulse.
        assert_eq!(jump_gate(0, 1.0, false), (file::JUMPING, false));

        // The trigger itself.
        assert_eq!(jump_gate(0, 1.0, true), (0, true));
    }

    /// An identity state transform: the take-off lands on the direction axis,
    /// and the fourth lane is the third's copy - the client's closing two
    /// `shufps` (`0x140ca27e6`/`0x140ca27ed`) leave `(d0, d1, d2, d2)`.
    #[test]
    fn the_impulse_replaces_the_vertical_lane() {
        let identity = [
            [1.0f32, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let out = jump_impulse([1.0, 2.0, 3.0, 0.0], [0.0; 4], Some(&identity), 0, 6.0);
        assert_eq!(out, [1.0, 6.0, 3.0, 3.0]);
    }

    /// A 90-degree rotation about X makes the local up the world `-Z`; the
    /// impulse drives the **world** lane to `body+0x60c`, and the local result
    /// is that point carried back through the transform.
    #[test]
    fn a_rotated_state_sets_the_world_lane() {
        let rot = [
            [1.0f32, 0.0, 0.0, 0.0],
            [0.0, 0.0, -1.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
        ];
        let out = jump_impulse([0.0; 4], [0.0; 4], Some(&rot), 0, 6.0);
        assert_eq!(out, [0.0, 0.0, 6.0, 6.0]);
        // The world-space bit copies instead of transforming, so the same
        // request lands on the world lane directly.
        let world = jump_impulse(
            [0.0; 4],
            [0.0; 4],
            Some(&rot),
            file::WORLD_SPACE_VECTOR,
            6.0,
        );
        assert_eq!(world, [0.0, 6.0, 0.0, 0.0]);
    }

    /// The anchor is a **position**, not a bias: the impulse sets the world
    /// height of `anchor + M*local` to `body+0x60c`, so a non-zero anchor
    /// shifts the local result by `-anchor.y`. In this build nothing writes
    /// `body+0x1510`, so the anchor is always zero and the effect is
    /// `local.y := body+0x60c`.
    #[test]
    fn a_non_zero_anchor_shifts_the_local_result() {
        let identity = [
            [1.0f32, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let out = jump_impulse(
            [1.0, 2.0, 3.0, 0.0],
            [10.0, 20.0, 30.0, 1.0],
            Some(&identity),
            0,
            6.0,
        );
        assert_eq!(out, [1.0, 6.0 - 20.0, 3.0, 3.0]);
    }

    /// With the anchor zero the take-off is exactly `local.y := body+0x60c`.
    #[test]
    fn a_zero_anchor_replaces_the_vertical_lane_on_the_ground() {
        let identity = [
            [1.0f32, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let out = jump_impulse([1.0, 2.0, 3.0, 0.0], [0.0; 4], Some(&identity), 0, 6.0);
        assert_eq!(out, [1.0, 6.0, 3.0, 3.0]);
    }

    /// A missing state record skips the transform on both sides.
    #[test]
    fn a_missing_state_record_is_the_identity() {
        let out = jump_impulse([1.0, 2.0, 3.0, 0.0], [0.0; 4], None, 0, 6.0);
        assert_eq!(out, [1.0, 6.0, 3.0, 0.0]);
    }

    /// The body readers land on the client's offsets.
    #[test]
    fn the_body_readers_use_the_client_offsets() {
        let mut body = vec![0u8; 0x1c00];
        body[0x60c..0x610].copy_from_slice(&6.0f32.to_le_bytes());
        for (i, lane) in [1.0f32, 2.0, 3.0, 0.0].iter().enumerate() {
            body[0x1150 + 4 * i..0x1154 + 4 * i].copy_from_slice(&lane.to_le_bytes());
        }
        set_flags(&mut body, 0xdead_beef);
        assert_eq!(takeoff_lane(&body), 6.0);
        assert_eq!(local_vector(&body), [1.0, 2.0, 3.0, 0.0]);
        assert_eq!(anchor(&body), [0.0; 4]);
        assert_eq!(flags(&body), 0xdead_beef);
    }
}
