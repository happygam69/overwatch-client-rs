//! The read fan solver inside the wish writer `FUN_140cb86c0`
//! (`0x140cb86c0`).
//!
//! The writer produces the input object's wish vector by searching a small
//! set of candidate directions around the mover. This module implements the
//! parts of that search that are read exactly:
//!
//! * the seed `s = dot(position - M*wish, dir)`, plus the second term
//!   `dot(query - (position - ramp), dir)` added when the state id is not the
//!   sentinel;
//! * the `+0.1` bias added to the seed before the search;
//! * the fan's speed (`min(|velocity.xyz|, max3(scale slots)) * 1.1 *
//!   (frame_time * helper)`, floored by `body+0x5cc`);
//! * the fan's radius (`max(body+0x65c, 0.35) * body+0x5b8 - 0.015`);
//! * the candidate rotation (the pentagon's 72° step and the `2π/N` step);
//! * the acceptance rules of all four call sites — the first pass's score
//!   **and** height ([`accepts_pass1`]), the fine pass's score
//!   ([`accepts_pass2`]), the ring pass's gated alignment ([`accepts_ring`]) and
//!   the refinement's facing test ([`accepts_refine`]);
//! * the back-transform's direction (`0x140cb9580`): the descriptor's `-xyz`
//!   normalized in the horizontal plane with `y` zeroed, falling back to
//!   `(0,0,1,0)`, and its `-normal` sibling that falls back to it;
//! * the refinement (`0x140cb9842..0x140cb9a20`): the `0.02` pentagon step plus
//!   the `0.005` refined-direction step, the `0.04` ground probe, and the
//!   keep-first / replace-on-facing / break-on-facing rule;
//! * the composition ([`solve`]): the `body+0x659` enable gate, the two
//!   endpoints `point + seed*dir` and `point - speed*dir` (different scalars on
//!   the two ends, `body+0xcc` scaling the `w` lane), the facing threshold
//!   `cos(turn_limit)`, the memo's `0.004` re-use test, the ring → refine →
//!   back-transform order, and the two reflection bytes (`|=` for one,
//!   assignment for the other).
//!
//! What is **not** here, because it is not read: the evaluator `0x1420328c0`
//! that scores a candidate (77 call sites, a general candidate test), the
//! **outer** walk that decides which of the four call sites runs and what the
//! query carries, pass 0's `MoverWishQueryCached` memory, the shape-ring branch
//! behind the global toggle, and `0x140cc2290`'s mesh-face refinement. Those are
//! inputs to this module, not outputs: [`accepts_pass1`],
//! [`accepts_pass2`] and [`accepts_refine`] take the evaluator's answers as
//! arguments rather than inventing them, [`refine_pentagon`] and [`solve`] take
//! solver through [`CandidateSolver`].
//!
//! Every constant below is read from the current image. The addresses in the
//! comments are the instruction that loads it.

use crate::movement::{forward_transform, maxss, minss, sse_add, sse_mul, DIRECTION};

/// `body + 0x10d0`: the flags block. Bit 1 selects the second set of scale
/// slots and the second speed cap.
pub const FLAGS_BLOCK: usize = 0x10d0;
/// The wish vector, `body + 0x1140`.
pub const WISH: usize = 0x1140;
/// `body + 0x1150`: the four-lane velocity the speed reads.
pub const VELOCITY: usize = 0x1150;
/// `body + 0x5c8`: the turn limit in degrees (`0x140ca7400`).
pub const TURN_LIMIT: usize = 0x5c8;
/// `body + 0x659`: the producer's own enable gate. Zero means the client
/// writes the zero record and never probes at all.
pub const PASS_ENABLED: usize = 0x659;
/// `body + 0x5d0..0x5d8`: the speed cap triple used when flags bit 1 is clear.
pub const SPEED_CAP_CLEAR: [usize; 3] = [0x5d0, 0x5d4, 0x5d8];
/// `body + 0x5dc..0x5e4`: the speed cap triple used when flags bit 1 is set.
pub const SPEED_CAP_SET: [usize; 3] = [0x5dc, 0x5e0, 0x5e4];
/// `body + 0xe8`: the frame counter; `* 1e-6` gives seconds.
pub const FRAME_COUNTER: usize = 0xe8;
/// `body + 0x5cc`: the floor under the fan's speed.
pub const SPEED_FLOOR: usize = 0x5cc;
/// `body + 0x5b8`: the radius scale.
pub const RADIUS_SCALE: usize = 0x5b8;
/// `body + 0x65c`: the radius input, floored at `0.35`.
pub const RADIUS_INPUT: usize = 0x65c;
/// `body + 0x5bc`: the candidate count of the fine fan.
pub const FAN_COUNT: usize = 0x5bc;
/// `body + 0x610`: the first gate term (`> 0` means active).
pub const ACTIVE: usize = 0x610;

/// `0x140cb8975`: the bias added to the seed before the search.
pub const SEED_BIAS: f32 = 0.1;
/// `0x140cb884c` (`0x3f8ccccd`): the speed gain.
pub const SPEED_GAIN: f32 = 1.1;
/// `0x142f85108` (`0x358637bd`): the frame counter's seconds scale.
pub const FRAME_SECONDS: f32 = 1e-6;
/// `0x1431102c0` (`0x3fc90f77`): the turn limit's upper clamp.
pub const TURN_LIMIT_MAX: f32 = 1.570_784_4;
/// `0x142f844b8` (`0x3c8efa35`): degrees to radians.
pub const DEG_TO_RAD: f32 = 0.017_453_292;
/// `0x143110820` (`0x3c75c28f`): the radius margin and the candidate `y`.
pub const RADIUS_MARGIN: f32 = 0.015;
/// `0x143058fac` (`0x3eb33333`): the radius input's floor.
pub const RADIUS_FLOOR: f32 = 0.35;
/// `0x142f7aad0` (`0x3c80000`): the squared-length threshold below which the
/// speed is zero.
pub const LENGTH_EPSILON: f32 = f32::from_bits(0x0080_0000);
/// `0x143110824` (`0x3e9e3779`): cos(72°), the pentagon's rotation step.
pub const PENTAGON_COS: f32 = 0.309_016_97;
/// `0x143110828` (`0x3f737871`): sin(72°).
pub const PENTAGON_SIN: f32 = 0.951_056_54;
/// `0x142f88c18` (`0x40c90fdb`): the fine fan's full turn.
pub const TWO_PI: f32 = 6.283_185_5;

#[inline]
fn f32_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[inline]
fn lanes_at(bytes: &[u8], offset: usize) -> [f32; 4] {
    [
        f32_at(bytes, offset),
        f32_at(bytes, offset + 4),
        f32_at(bytes, offset + 8),
        f32_at(bytes, offset + 12),
    ]
}

/// `0x140ca7400`: `clamp(body[0x5c8] * (π/180), 0, 1.5707844)`.
///
/// The client forms `minss(v, 1.5707844)` then `maxss(that, 0)`, so a NaN
/// takes the clamp operand.
pub fn turn_limit(body: &[u8]) -> f32 {
    let v = sse_mul(f32_at(body, TURN_LIMIT), DEG_TO_RAD);
    let v = minss(v, TURN_LIMIT_MAX);
    maxss(v, 0.0)
}

/// `0x140ca7430`: the maximum of the three cap slots, selected by flags bit 1.
///
/// The client compares `a >= b`, `a >= c`, then `b >= a`, `b >= c`, and
/// returns `c` when neither pair holds. `COMISS` + `SETNC` treats an
/// unordered comparison as false, so a NaN never wins and the last slot is
/// the fallback.
pub fn speed_cap(body: &[u8], flag_bit_set: bool) -> f32 {
    let slots = if flag_bit_set {
        SPEED_CAP_SET
    } else {
        SPEED_CAP_CLEAR
    };
    let a = f32_at(body, slots[0]);
    let b = f32_at(body, slots[1]);
    let c = f32_at(body, slots[2]);
    let ge = |x: f32, y: f32| !x.is_nan() && !y.is_nan() && x >= y;
    if ge(a, b) && ge(a, c) {
        a
    } else if ge(b, a) && ge(b, c) {
        b
    } else {
        c
    }
}

/// `0x140ca72b0`: `(f32)(i64)body[0xe8] * 1e-6` — the frame counter in
/// seconds.
pub fn frame_time(body: &[u8]) -> f32 {
    let ticks = i64::from_le_bytes(body[FRAME_COUNTER..FRAME_COUNTER + 8].try_into().unwrap());
    sse_mul(ticks as f32, FRAME_SECONDS)
}

/// The speed the fan's radius uses, read at `0x140cb87d6..0x140cb8874`.
///
/// ```text
/// len2 = (y*y + x*x) + z*z                       ; body[0x1150].xyz
/// len  = len2 > 1.17549435e-36
///        ? len2 * ((3 - len2*rcp*rcp) * (rcp*0.5))  with rcp = rsqrtss(len2)
///        : 0
/// speed = minss(len, speed_cap)
/// speed = speed * 1.1
/// speed = speed * (frame_time * helper_e)
/// speed = maxss(speed, body[0x5cc])
/// ```
///
/// The length is the client's `rsqrtss` plus one Newton step, not `sqrt`, so
/// the last bits are the approximation's.
pub fn fan_speed(body: &[u8], flag_bit_set: bool, helper_e: f32) -> f32 {
    let v = lanes_at(body, VELOCITY);
    let x = sse_mul(v[0], v[0]);
    let y = sse_mul(v[1], v[1]);
    let z = sse_mul(v[2], v[2]);
    let len2 = sse_add(sse_add(y, x), z);
    let len = if len2 > LENGTH_EPSILON {
        let rcp = 1.0f32 / len2.sqrt();
        let rcp2 = sse_mul(rcp, rcp);
        let half = sse_mul(rcp, 0.5);
        let inner = sse_mul(sse_add(3.0, -sse_mul(len2, rcp2)), half);
        sse_mul(len2, inner)
    } else {
        0.0
    };
    let capped = minss(len, speed_cap(body, flag_bit_set));
    let gained = sse_mul(capped, SPEED_GAIN);
    let scaled = sse_mul(gained, sse_mul(frame_time(body), helper_e));
    maxss(scaled, f32_at(body, SPEED_FLOOR))
}

/// The fan's radius, read at `0x140cb927a..0x140cb92e2`:
/// `maxss(body[0x65c], 0.35) * body[0x5b8] - 0.015`.
pub fn fan_radius(body: &[u8]) -> f32 {
    let base = maxss(f32_at(body, RADIUS_INPUT), RADIUS_FLOOR);
    sse_add(sse_mul(base, f32_at(body, RADIUS_SCALE)), -RADIUS_MARGIN)
}

/// The seed of the search, read at `0x140cb8766..0x140cb87ba`:
/// `((dy*diry) + (dx*dirx)) + (dz*dirz)` where `d = position - M*wish`.
///
/// `matrix` is the resolved state transform; `None` is the client's
/// no-transform path, where the wish is used as it stands.
pub fn seed(body: &[u8], position: [f32; 4], matrix: Option<&[f32; 16]>) -> f32 {
    let mut wish = lanes_at(body, WISH);
    if let Some(m) = matrix {
        forward_transform(&mut wish, m);
    }
    let dir = lanes_at(body, DIRECTION);
    let mut acc = 0.0f32;
    // The client multiplies lane-wise, then adds y, x, z in that order.
    let mut lanes = [0.0f32; 4];
    for lane in 0..4 {
        lanes[lane] = sse_mul(sse_add(position[lane], -wish[lane]), dir[lane]);
    }
    acc = sse_add(acc, sse_add(lanes[1], lanes[0]));
    acc = sse_add(acc, lanes[2]);
    acc
}

/// The second seed term, read at `0x140cb88f9..0x140cb892c`:
/// `dot(query - (position - ramp), dir)`, added to the seed when the state id
/// is not the sentinel.
pub fn seed_state_term(body: &[u8], position: [f32; 4], ramp: [f32; 4], query: [f32; 4]) -> f32 {
    let dir = lanes_at(body, DIRECTION);
    let mut lanes = [0.0f32; 4];
    for lane in 0..4 {
        let base = sse_add(position[lane], -ramp[lane]);
        lanes[lane] = sse_mul(sse_add(query[lane], -base), dir[lane]);
    }
    sse_add(sse_add(lanes[1], lanes[0]), lanes[2])
}

/// The seed as the search uses it: `s + 0.1` (`0x140cb8975`).
pub fn biased_seed(seed: f32) -> f32 {
    sse_add(seed, SEED_BIAS)
}

/// Rotate a 2D pair by `(cos, sin)`, in the client's operation order
/// (`0x140cb983c..0x140cb9863`).
///
/// ```text
/// a' = a*cos - b*sin
/// b' = a*sin + b*cos
/// ```
pub fn rotate_pair(pair: [f32; 2], cos: f32, sin: f32) -> [f32; 2] {
    let [a, b] = pair;
    let as_ = sse_mul(a, sin);
    let bc = sse_mul(b, cos);
    let ac = sse_mul(a, cos);
    let bs = sse_mul(b, sin);
    [sse_add(ac, -bs), sse_add(as_, bc)]
}

/// The pentagon fan's offsets: the start pair rotated by 72° four times, read
/// from the loop at `0x140cb9842` (five candidates, `ebx < 5`).
pub fn pentagon_offsets(start: [f32; 2]) -> [[f32; 2]; 5] {
    let mut out = [[0.0f32; 2]; 5];
    let mut pair = start;
    for slot in out.iter_mut() {
        *slot = pair;
        pair = rotate_pair(pair, PENTAGON_COS, PENTAGON_SIN);
    }
    out
}

/// The fine fan's step angle: `2π / N`, `N = body[0x5bc]`
/// (`0x140cb9322..0x140cb9332`).
///
/// Returns `None` for a non-positive count, which is the client's own
/// `jle` guard before the loop.
pub fn ring_step(body: &[u8]) -> Option<f32> {
    let n = i32::from_le_bytes(body[FAN_COUNT..FAN_COUNT + 4].try_into().unwrap());
    if n <= 0 {
        return None;
    }
    Some(TWO_PI / n as f32)
}

/// The fine fan's offsets: the start pair rotated by the step angle `n` times.
pub fn ring_offsets(start: [f32; 2], n: usize) -> Vec<[f32; 2]> {
    if n == 0 {
        return Vec::new();
    }
    let (sin, cos) = (TWO_PI / n as f32).sin_cos();
    let mut out = Vec::with_capacity(n);
    let mut pair = start;
    for _ in 0..n {
        out.push(pair);
        pair = rotate_pair(pair, cos, sin);
    }
    out
}

/// The first pass's acceptance, read at `0x140cb90f1..0x140cb91d8`:
/// `|score| < best_score` **and** `y > best_y`, both strict.
///
/// The pass starts with `best_score = +FLT_MAX` and `best_y = -FLT_MAX`
/// (`0x142f844d8` / `0x142fb8e58`), so the first candidate that passes the
/// evaluator always wins.
pub fn accepts_pass1(score: f32, y: f32, best_score: f32, best_y: f32) -> bool {
    score.abs() < best_score && y > best_y
}

/// The fine pass's acceptance, read at `0x140cb9be5..0x140cb9c16`:
/// `|score| < best_score` only — it improves the first pass's best and does
/// not test the height.
pub fn accepts_pass2(score: f32, best_score: f32) -> bool {
    score.abs() < best_score
}

/// The ring pass's acceptance, at site `0x140cb9b6e`: the record's flags byte
/// gates the test (`byte[rbp-0x52] & 1`), and the direction alignment must be
/// **strictly greater** than the threshold the pass carries.
///
/// ```text
/// gate && alignment > threshold
/// ```
///
/// The `gate` is fed from the hit record, not computed here; the alignment and
/// the threshold arrive already formed, because the alignment is a four-lane
/// product sum whose lane-add order is only partly read. This is the fan's
/// third acceptance rule and the last of them to be read.
pub fn accepts_ring(alignment: f32, threshold: f32, gate: bool) -> bool {
    gate && alignment > threshold
}

/// The refinement pass's acceptance, exactly as the client branches it.
///
/// ```text
/// hit && ( flag_bit_0x10000 || facing > best_y )
/// ```
///
/// The second term is the earlier pass's best `y` (`local_c18`), captured
/// before the loop and never updated inside it, and the comparison is strict:
/// the client's `<=` falls through to "not accepted".
pub fn accepts_refine(facing: f32, best_y: f32, flag_bit_0x10000: bool) -> bool {
    flag_bit_0x10000 || facing > best_y
}

/// `0x140cb9842..0x140cb9a20`: the refinement pass over the five pentagon
/// directions.
///
/// For each of the five rotated offsets it builds the pair [`refined_sweep`]
/// returns, asks the solver, and keeps the answer under the client's rule: the
/// **first** hit is always kept (the `!bVar25` arm), later ones replace it only
/// when they pass [`accepts_refine`], and a passing hit breaks the loop. The
/// offset pair is rotated by the pentagon step after every candidate, including
/// the last.
///
/// `dir` is the mover's direction quad (`body+0xc0..0xcc`) and `best_y` is the
/// earlier pass's `local_c18`; both are read-only here, which is why the pass
/// can be unit-tested against any solver.
pub fn refine_pentagon(
    center: [f32; 4],
    start: [f32; 2],
    refined: [f32; 4],
    dir: [f32; 4],
    best_y: f32,
    flag_bit_0x10000: bool,
    solver: &mut dyn CandidateSolver,
) -> Option<SweepHit> {
    let mut best: Option<SweepHit> = None;
    let mut pair = start;
    for _ in 0..5 {
        let (a, b) = refined_sweep(center, pair, refined);
        if let Some(hit) = solver.evaluate(a, b) {
            let passes = accepts_refine(facing(dir, hit.normal), best_y, flag_bit_0x10000);
            if best.is_none() || passes {
                best = Some(hit);
                if passes {
                    break;
                }
            }
        }
        pair = rotate_pair(pair, PENTAGON_COS, PENTAGON_SIN);
    }
    best
}

/// `1.17549435e-36` at `0x140cb953a` and `0x140cb95b6`.
pub const REFINE_EPSILON: f32 = 1.175_494_4e-36;
/// `0x140cb9844`: the pentagon step's scale on the rotated pair.
pub const REFINE_STEP: f32 = 0.02;
/// `0x140cb9850`: the refined direction's scale.
pub const REFINE_DIR: f32 = 0.005;
/// `0x140cb98a8`: the ground probe's drop below the sweep start.
pub const REFINE_GROUND_DROP: f32 = 0.04;

/// `0x140cb9580`: the horizontal unit direction of `-v`, with `y` forced to
/// zero and the `w` lane scaled from `-w`.
///
/// ```text
/// len2 = z*z + x*x
/// if len2 < 1.17549435e-36        -> (0, 0, 1, 0)
/// r    = rsqrtps(len2);  n = (3 - r*r*len2) * r * 0.5
/// out  = (x*n, n*0.5*0.0, z*n, -w*n)
/// if n*len2 < 0                   -> (0, 0, 1, 0)
/// ```
///
/// The `y` lane keeps the client's `* 0.5 * 0.0`, not a bare `0.0`, so an
/// infinite or NaN `len2` propagates the same way it does in the image. The
/// `w` lane's input is the descriptor's fourth component (`descriptor+0x460`),
/// which is what `descriptor+0x450/+0x458/+0x460` name in the caller.
///
/// Returns `None` for the degenerate and sign-failed cases, which the client
/// expresses by branching to the `(0, 0, 1, 0)` assignment.
pub fn horizontal_unit(v: [f32; 4], w: f32) -> Option<[f32; 4]> {
    let x = -v[0];
    let z = -v[2];
    let len2 = sse_add(sse_mul(z, z), sse_mul(x, x));
    if len2 < REFINE_EPSILON {
        return None;
    }
    let r = 1.0f32 / len2.sqrt();
    let n = sse_mul(sse_add(3.0, -sse_mul(sse_mul(r, r), len2)), sse_mul(r, 0.5));
    if sse_mul(n, len2) < 0.0 {
        return None;
    }
    Some([
        sse_mul(x, n),
        sse_mul(sse_mul(n, 0.5), 0.0),
        sse_mul(z, n),
        sse_mul(-w, n),
    ])
}

/// `0x140cb9580`: the back-transform's direction — `-descriptor.xyz`
/// horizontally normalized, `y` zeroed, `w` from `-descriptor.w`, with
/// `(0, 0, 1, 0)` when it is degenerate.
/// The ring pass's sweep configuration, from the writer's ring branch.
///
/// ```text
/// count  = u32 at [body+0x5bc]
/// radius = max(f32 at [body+0x65c], 0.35) * f32 at [body+0x5b8]
/// ```
///
/// A count of zero skips the whole pass, which is why this returns `None`
/// rather than an empty sweep. The per-step rotation is
/// `0x1426c40f0(6.2831855 / count)` whose **low** half is the sine and high
/// half the cosine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingSweep {
    pub count: u32,
    pub radius: f32,
}

/// Builds the ring sweep from the writer's three raw values.
pub fn ring_sweep(count: u32, configured_radius: f32, radius_scale: f32) -> Option<RingSweep> {
    if count == 0 {
        return None;
    }
    Some(RingSweep {
        count,
        radius: configured_radius.max(0.35) * radius_scale,
    })
}

/// The ring pass's per-step offset quad: `(sin*R, 0, cos*R, 0)`.
///
/// `pair` is the rotating `(cos, sin)` unit pair that starts at `(1, 0)` and is
/// rotated by the sweep angle each step. Only x and z carry the offset; y and
/// w are zero, which is why the sweep is a flat circle in the ground plane.
pub fn ring_offset(pair: [f32; 2], radius: f32) -> [f32; 4] {
    [pair[1] * radius, 0.0, pair[0] * radius, 0.0]
}

/// The alternative ring branch's candidate quad: `(sin*(seed-0.015), 0.015,
/// cos*(seed-0.015), 0)`.
///
/// The same five-way fan, but built from the **shape's own basis** instead of
/// the movement direction, with a constant `0.015` height on y and the seed
/// shortened by `0.015` (`0x3c75c28f`). `pair` is the same rotating
/// `(cos, sin)` pair; the first candidate (`pair = (0, 1)`) points along +x.
pub fn shape_ring_offset(pair: [f32; 2], seed: f32) -> [f32; 4] {
    [
        pair[1] * (seed - 0.015),
        0.015,
        pair[0] * (seed - 0.015),
        0.0,
    ]
}

pub fn back_direction(descriptor: [f32; 4]) -> [f32; 4] {
    horizontal_unit(descriptor, descriptor[3]).unwrap_or([0.0, 0.0, 1.0, 0.0])
}

/// `0x140cb95a8`: the refinement direction — `-normal.xyz` horizontally
/// normalized, `w` from `-descriptor.w`, **falling back to `fallback`** when
/// the normal is degenerate or its sign test fails.
///
/// ```text
/// d = horizontal_unit(-normal, descriptor.w)
/// refine_direction = d.is_some() ? d : fallback
/// ```
pub fn refine_direction(normal: [f32; 4], descriptor_w: f32, fallback: [f32; 4]) -> [f32; 4] {
    horizontal_unit(normal, descriptor_w).unwrap_or(fallback)
}

/// `0x140cb9850`: the refinement's offset from a candidate direction.
///
/// ```text
/// off.x = pentagon.x * 0.02 + refined.x * 0.005
/// off.y = 0.0       * 0.02 + refined.y * 0.005
/// off.z = pentagon.z * 0.02 + refined.z * 0.005
/// off.w = 0.0       * 0.02 + refined.w * 0.005
/// ```
///
/// The scale is applied to the rotated pentagon pair first and the refined
/// direction is added second, in that order, because the client multiplies
/// `fVar45`/`fVar34` by `0.02` into two temporaries and then adds the
/// `0.005` products to them.
pub fn refined_offset(pentagon: [f32; 2], refined: [f32; 4]) -> [f32; 4] {
    let sx = sse_mul(pentagon[0], REFINE_STEP);
    let sz = sse_mul(pentagon[1], REFINE_STEP);
    [
        sse_add(sx, sse_mul(refined[0], REFINE_DIR)),
        sse_add(sse_mul(0.0, REFINE_STEP), sse_mul(refined[1], REFINE_DIR)),
        sse_add(sz, sse_mul(refined[2], REFINE_DIR)),
        sse_add(sse_mul(0.0, REFINE_STEP), sse_mul(refined[3], REFINE_DIR)),
    ]
}

/// `0x140cb98a0..0x140cb98bc`: the two sweep endpoints the refinement builds
/// from `center` — the raised one and the ground probe `0.04` below it.
///
/// ```text
/// offset = refined_offset(pentagon, refined)
/// a = (center.x + off.x,  center.y + off.y,  center.z + off.z)
/// b = (center.x + off.x, (center.y - 0.04) + off.y, center.z + off.z)
/// ```
///
/// The `w` lanes are accumulators the client carries through both records
/// (`fStack_13c`, `fStack_12c`), so they are returned unchanged from `center`.
///
pub fn refined_sweep(
    center: [f32; 4],
    pentagon: [f32; 2],
    refined: [f32; 4],
) -> ([f32; 4], [f32; 4]) {
    let off = refined_offset(pentagon, refined);
    let x = sse_add(center[0], off[0]);
    let y = sse_add(center[1], off[1]);
    let z = sse_add(center[2], off[2]);
    let a = [x, y, z, center[3]];
    let b = [
        x,
        sse_add(sse_add(center[1], -REFINE_GROUND_DROP), off[1]),
        z,
        center[3],
    ];
    (a, b)
}

/// One candidate's evaluation, as the evaluator `0x1420328c0` hands it back.
///
/// The fields are the ones the fan's acceptance reads: the hit fraction
/// (`record+0x30`, which the evaluator copies from the descriptor), the
/// surface normal (`record+0x10`), and the two key words stacked the way the
/// writer keeps them in one register.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SweepHit {
    /// The hit fraction.
    pub score: f32,
    /// The surface normal, four lanes.
    pub normal: [f32; 4],
    /// `record+0x38` in the low half and `record+0x3a` in the high half.
    ///
    /// The writer keeps these two as one 32-bit value and reads its **third
    /// byte**: bit 0 of that byte (`word & 0x10000`) is the acceptance's gate,
    /// and bit 2 (`word >> 18 & 1`) is written to `out+0x42`.
    pub word: u32,
}

/// The memo's per-lane tolerance: `|cached - candidate| <= 0.004`.
pub const MEMO_TOLERANCE: f32 = 0.004;

/// Whether the cached query may be reused for these endpoints.
///
/// `MoverWishQueryCached 0x140cc1870` compares the two stored points against
/// the query's two endpoints lane by lane, **twelve** comparisons in all: the
/// x, y and z of each endpoint, never `w`. The comparison is `<=`, so an
/// exactly `0.004` move still reuses the cache.
pub fn memo_matches(cached: [[f32; 3]; 2], candidate: [[f32; 3]; 2]) -> bool {
    cached
        .iter()
        .zip(candidate.iter())
        .all(|(a, b)| (0..3).all(|i| sse_add(a[i], -b[i]).abs() <= MEMO_TOLERANCE))
}

/// `cos(turn_limit(body))`: the facing cone's cosine, `0x1426cd3d0` called
/// from the writer's prologue.
///
/// `0x140ca7400` gives the cone half-angle in degrees clamped to `[0, π/2]`,
/// and the writer takes its cosine. The client's `cosf` reduces in double
/// precision and rounds once, so the port does the same rather than calling
/// `f32::cos`.
pub fn facing_threshold(body: &[u8]) -> f32 {
    (turn_limit(body) as f64).cos() as f32
}

/// The sweep's two endpoints, in the writer's own arithmetic.
///
/// ```text
/// seed_end  = point + seed  * (dir.x, dir.y, dir.z, dir_w)
/// speed_end = point - speed * (dir.x, dir.y, dir.z, dir_w)
/// ```
///
/// The two ends carry **different** scalars, which is why they are not
/// symmetric, and the `w` lane is scaled by `body+0xcc` rather than by the
/// direction's own fourth lane.
pub fn endpoints(
    point: [f32; 4],
    dir: [f32; 4],
    dir_w: f32,
    seed: f32,
    speed: f32,
) -> ([f32; 4], [f32; 4]) {
    let lanes = [dir[0], dir[1], dir[2], dir_w];
    let mut a = [0.0f32; 4];
    let mut b = [0.0f32; 4];
    for i in 0..4 {
        a[i] = sse_add(sse_mul(seed, lanes[i]), point[i]);
        b[i] = sse_add(point[i], -sse_mul(speed, lanes[i]));
    }
    (a, b)
}

/// `|point · dir|`, accumulated in the client's y, x, z order.
///
/// This is the ring pass's distance comparison: a candidate only replaces the
/// current best when it is **strictly** closer, and the two zero terms in the
/// client's sum are the world origin, not a subtraction that can change a
/// result.
pub fn projection(point: [f32; 4], dir: [f32; 4]) -> f32 {
    let y = sse_mul(point[1], dir[1]);
    let x = sse_add(y, sse_mul(point[0], dir[0]));
    sse_add(x, sse_mul(point[2], dir[2])).abs()
}

/// The back-transform pass's direction: `s*cand_dir + cand` with
/// `s = -(cand · cand_dir)`.
///
/// The writer evaluates a candidate quad `cand`, projects it onto the mover
/// direction, and keeps the component of `cand` that is **behind** the mover:
/// the sweep's answer becomes a direction to walk back along. The result is
/// not normalised here — the caller writes it as-is, exactly as the pass does.
pub fn project_back(cand: [f32; 4], dir: [f32; 4], dir_w: f32) -> ([f32; 4], f32) {
    let y = sse_mul(cand[1], dir[1]);
    let x = sse_add(y, sse_mul(cand[0], dir[0]));
    let dot = sse_add(x, sse_mul(cand[2], dir[2]));
    let s = -dot;
    (
        [
            sse_add(sse_mul(s, dir[0]), cand[0]),
            sse_add(sse_mul(s, dir[1]), cand[1]),
            sse_add(sse_mul(s, dir[2]), cand[2]),
            sse_add(sse_mul(s, dir_w), cand[3]),
        ],
        dot,
    )
}

/// The reflection write's two bytes: `out+0x41 |= (word >> 16) & 1` and
/// `out+0x42 = (word >> 18) & 1`.
///
/// The first is an **OR** — several passes contribute to it across one solve —
/// while the second is an assignment, so the last pass to run decides it.
pub fn reflection_flags(word: u32) -> (bool, bool) {
    (((word >> 16) & 1) != 0, ((word >> 18) & 1) != 0)
}

/// Everything the composition needs that comes from outside the writer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolveContext {
    /// `MoverWishWorldPoint(body+0x10d0)`. The extrapolation term is zero:
    /// `FUN_140c9f110` writes a zero quad and returns it.
    pub point: [f32; 4],
    /// The mover's facing, `body+0xc0`.
    pub dir: [f32; 4],
    /// `body+0xcc`, the `w` lane's scale.
    pub dir_w: f32,
    /// The biased seed, already raised to `2 * [shape+0x2d2c]` if a shape is
    /// under the mover.
    pub seed: f32,
    /// `fan_speed + frame_seconds`.
    pub speed: f32,
    /// The ring sweep, or `None` when `body+0x5bc` is zero.
    pub ring: Option<RingSweep>,
}

/// What a completed solve hands the caller.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolvedWish {
    /// `out+0x00`: the direction to move, four lanes.
    pub direction: [f32; 4],
    /// `out+0x10`: the surface normal it came from.
    pub normal: [f32; 4],
    /// The hit fraction of the winning candidate.
    pub score: f32,
    /// `out+0x41` and `out+0x42`.
    pub reflection: (bool, bool),
    /// Whether the refine pass replaced the ring's pick.
    pub refined: bool,
}

/// The writer's decision, over the composition read from `0x140cb86c0`.
///
/// Pass order, exactly as the client runs it:
///
/// 1. the ring sweep about the seed endpoint, accepting on facing **and** a
///    strictly closer projection;
/// 2. the five-step refinement about the accepted direction, which may
///    replace it;
/// 3. the back-transform, which turns the winning candidate into a direction
///    to walk back along;
///
/// and then the write, with the reflection flags OR-ed in from every pass that
/// ran. `None` is the client's no-hit path: the caller zeroes the direction
/// and writes `out+0x40 = 0`.
///
/// `body` supplies the facing threshold and the enable gate; the five helpers
/// the composition needs from elsewhere (`MoverWishQueryCached`, the shape
/// basis, `0x140cc2290`) stay behind [`CandidateSolver`].
pub fn solve(
    body: &[u8],
    ctx: &SolveContext,
    solver: &mut dyn CandidateSolver,
) -> Option<SolvedWish> {
    // `body+0x659` gates the whole producer; without it the client writes the
    // zero record and never probes.
    if body[PASS_ENABLED] == 0 {
        return None;
    }
    let threshold = facing_threshold(body);
    let (seed_end, speed_end) = endpoints(ctx.point, ctx.dir, ctx.dir_w, ctx.seed, ctx.speed);
    let mut reflection = (false, false);

    // Pass 1: the ring.
    let mut best: Option<SweepHit> = None;
    let mut best_cand = [0.0f32; 4];
    let mut best_proj = f32::MAX;
    if let Some(sweep) = ctx.ring {
        let step = TWO_PI / sweep.count as f32;
        let (sin, cos) = step.sin_cos();
        let mut pair = [1.0f32, 0.0];
        for _ in 0..sweep.count {
            let off = ring_offset(pair, sweep.radius);
            let mut a = seed_end;
            let mut b = speed_end;
            for i in 0..4 {
                a[i] = sse_add(a[i], off[i]);
                b[i] = sse_add(b[i], off[i]);
            }
            if let Some(hit) = solver.evaluate(a, b) {
                let (flag_a, flag_b) = reflection_flags(hit.word);
                reflection.0 |= flag_a;
                reflection.1 = flag_b;
                let accepts = accepts_ring(facing(ctx.dir, hit.normal), threshold, !flag_a);
                let proj = projection(ctx.point, ctx.dir);
                if accepts && proj < best_proj {
                    best_proj = proj;
                    best = Some(hit);
                    best_cand = a;
                }
            }
            pair = rotate_pair(pair, cos, sin);
        }
    }

    // Pass 2: the refinement about the ring's pick.
    let best = best?;
    let (flag_a, _) = reflection_flags(best.word);
    let refined_hit = refine_pentagon(
        ctx.point,
        [1.0f32, 0.0],
        best_cand,
        ctx.dir,
        threshold,
        flag_a,
        solver,
    );
    let (hit, refined) = match refined_hit {
        Some(hit) => (hit, true),
        None => (best, false),
    };
    let (flag_a, flag_b) = reflection_flags(hit.word);
    reflection.0 |= flag_a;
    reflection.1 = flag_b;

    // Pass 3: the back-transform, over the winning candidate.
    let (direction, _) = project_back(best_cand, ctx.dir, ctx.dir_w);
    Some(SolvedWish {
        direction,
        normal: hit.normal,
        score: hit.score,
        reflection,
        refined,
    })
}

/// The seam between the producer and whatever answers "is this direction
/// reachable": one candidate in, one hit or nothing out.
///
/// In the client this is the evaluator `0x1420328c0`, which builds a sweep
/// and asks the collision query; the query's narrowphase answers through the
/// hit policy. [`crate::policy::PolicySolver`] is that chain with a synthetic
/// world behind it. The producer's own loop over the offsets, the two
/// acceptance rules and the per-candidate refinement are **not** here yet —
/// the refinement block is the fan's remaining open read (see
/// `docs/open-questions.md`), so this trait is the whole of what item 2 landed.
pub trait CandidateSolver {
    /// `a` = sweep start, `b` = sweep end.
    fn evaluate(&mut self, a: [f32; 4], b: [f32; 4]) -> Option<SweepHit>;
}

/// The acceptance's second term: the mover direction against the hit normal,
/// from the refinement pass's comparison at site `0x140cb9770` (the decompile's
/// `dir.y*n.y + dir.x*n.x + dir.z*n.z`, `body+0xc4`, `body+0xc0`, `body+0xc8`
/// against the record's `+0x10` lanes):
///
/// ```text
/// dir.y * normal.y + dir.x * normal.x + dir.z * normal.z
/// ```
///
/// The client's operand order is y, x, z (`body+0xc4`, `body+0xc0`,
/// `body+0xc8`), and the sum is accumulated left to right, so the two
/// multiplication pairs and the two additions are reproduced in that order.
pub fn facing(dir: [f32; 4], normal: [f32; 4]) -> f32 {
    let y = sse_mul(dir[1], normal[1]);
    let x = sse_add(y, sse_mul(dir[0], normal[0]));
    sse_add(x, sse_mul(dir[2], normal[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(len: usize) -> Vec<u8> {
        vec![0u8; len]
    }

    fn put(body: &mut [u8], offset: usize, value: f32) {
        body[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn turn_limit_converts_and_clamps() {
        let mut b = body(0x1100);
        put(&mut b, TURN_LIMIT, 45.0);
        assert_eq!(turn_limit(&b), 45.0 * DEG_TO_RAD);
        put(&mut b, TURN_LIMIT, 1000.0);
        assert_eq!(turn_limit(&b), TURN_LIMIT_MAX);
        put(&mut b, TURN_LIMIT, -5.0);
        assert_eq!(turn_limit(&b), 0.0);
    }

    #[test]
    fn speed_cap_takes_the_maximum_of_three() {
        let mut b = body(0x1100);
        put(&mut b, SPEED_CAP_CLEAR[0], 1.0);
        put(&mut b, SPEED_CAP_CLEAR[1], 3.0);
        put(&mut b, SPEED_CAP_CLEAR[2], 2.0);
        assert_eq!(speed_cap(&b, false), 3.0);
        put(&mut b, SPEED_CAP_SET[0], 4.0);
        put(&mut b, SPEED_CAP_SET[1], 5.0);
        put(&mut b, SPEED_CAP_SET[2], 6.0);
        assert_eq!(speed_cap(&b, true), 6.0);
    }

    #[test]
    fn speed_cap_falls_back_to_the_last_slot_on_nan() {
        let mut b = body(0x1100);
        put(&mut b, SPEED_CAP_CLEAR[0], f32::NAN);
        put(&mut b, SPEED_CAP_CLEAR[1], 3.0);
        put(&mut b, SPEED_CAP_CLEAR[2], 2.0);
        // a is unordered, b wins both comparisons: the client's second test
        // is `b >= a` (false for NaN) so it falls through to c.
        assert_eq!(speed_cap(&b, false), 2.0);
    }

    #[test]
    fn frame_time_is_ticks_times_a_microsecond() {
        let mut b = body(0x1100);
        b[FRAME_COUNTER..FRAME_COUNTER + 8].copy_from_slice(&16_666i64.to_le_bytes());
        assert_eq!(frame_time(&b), 16_666.0 * FRAME_SECONDS);
    }

    #[test]
    fn fan_speed_uses_the_velocity_length_and_the_floor() {
        let mut b = body(0x1200);
        put(&mut b, VELOCITY, 3.0);
        put(&mut b, SPEED_CAP_CLEAR[0], 10.0);
        put(&mut b, SPEED_CAP_CLEAR[1], 10.0);
        put(&mut b, SPEED_CAP_CLEAR[2], 10.0);
        b[FRAME_COUNTER..FRAME_COUNTER + 8].copy_from_slice(&1_000_000i64.to_le_bytes());
        // len = 3, cap = 10, gain 1.1, frame 1.0 s, helper 1.0 -> 3.3
        let speed = fan_speed(&b, false, 1.0);
        assert!((speed - 3.3).abs() < 1e-5, "{speed}");
        // A zero velocity falls to the floor.
        put(&mut b, VELOCITY, 0.0);
        put(&mut b, VELOCITY + 4, 0.0);
        put(&mut b, VELOCITY + 8, 0.0);
        put(&mut b, SPEED_FLOOR, 0.5);
        assert_eq!(fan_speed(&b, false, 1.0), 0.5);
    }

    #[test]
    fn fan_radius_floors_scales_and_shrinks() {
        let mut b = body(0x1100);
        put(&mut b, RADIUS_INPUT, 0.1);
        put(&mut b, RADIUS_SCALE, 2.0);
        // max(0.1, 0.35) * 2 - 0.015
        assert_eq!(fan_radius(&b), 0.35 * 2.0 - RADIUS_MARGIN);
        put(&mut b, RADIUS_INPUT, 1.0);
        assert_eq!(fan_radius(&b), 2.0 - RADIUS_MARGIN);
    }

    #[test]
    fn seed_projects_the_delta_onto_the_direction() {
        let mut b = body(0x1200);
        put(&mut b, DIRECTION, 1.0);
        put(&mut b, DIRECTION + 4, 1.0);
        put(&mut b, DIRECTION + 8, 1.0);
        put(&mut b, WISH, 1.0);
        put(&mut b, WISH + 4, 2.0);
        put(&mut b, WISH + 8, 3.0);
        let position = [10.0, 10.0, 10.0, 0.0];
        // delta = (9, 8, 7); dot with (1,1,1) = 24
        assert_eq!(seed(&b, position, None), 24.0);
    }

    #[test]
    fn seed_uses_the_transformed_wish() {
        let mut b = body(0x1200);
        put(&mut b, DIRECTION, 1.0);
        put(&mut b, WISH, 1.0);
        let mut m = [0.0f32; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[12] = 100.0;
        let position = [5.0, 0.0, 0.0, 0.0];
        // M*wish = 101; delta.x = -96
        assert_eq!(seed(&b, position, Some(&m)), -96.0);
    }

    #[test]
    fn state_term_subtracts_the_ramp_before_projecting() {
        let mut b = body(0x1200);
        put(&mut b, DIRECTION, 1.0);
        let position = [1.0, 0.0, 0.0, 0.0];
        let ramp = [0.5, 0.0, 0.0, 0.0];
        let query = [2.0, 0.0, 0.0, 0.0];
        // 2 - (1 - 0.5) = 1.5
        assert_eq!(seed_state_term(&b, position, ramp, query), 1.5);
    }

    #[test]
    fn biased_seed_adds_a_tenth() {
        assert_eq!(biased_seed(0.0), 0.1);
    }

    #[test]
    fn rotate_pair_is_the_client_rotation() {
        let rotated = rotate_pair([1.0, 0.0], 0.0, 1.0);
        assert_eq!(rotated, [0.0, 1.0]);
        let rotated = rotate_pair([1.0, 0.0], 1.0, 0.0);
        assert_eq!(rotated, [1.0, 0.0]);
    }

    #[test]
    fn pentagon_rotates_by_72_degrees_five_times() {
        let offsets = pentagon_offsets([1.0, 0.0]);
        assert_eq!(offsets[0], [1.0, 0.0]);
        // One 72° step from (1, 0) is (cos 72°, sin 72°).
        assert!((offsets[1][0] - PENTAGON_COS).abs() < 1e-6);
        assert!((offsets[1][1] - PENTAGON_SIN).abs() < 1e-6);
        // Five 72° steps return to the start.
        let back = rotate_pair(offsets[4], PENTAGON_COS, PENTAGON_SIN);
        assert!((back[0] - 1.0).abs() < 1e-5);
        assert!(back[1].abs() < 1e-5);
    }

    #[test]
    fn ring_step_is_two_pi_over_the_count() {
        let mut b = body(0x1100);
        b[FAN_COUNT..FAN_COUNT + 4].copy_from_slice(&8i32.to_le_bytes());
        assert_eq!(ring_step(&b), Some(TWO_PI / 8.0));
        b[FAN_COUNT..FAN_COUNT + 4].copy_from_slice(&0i32.to_le_bytes());
        assert_eq!(ring_step(&b), None);
    }

    #[test]
    fn ring_offsets_wrap_after_a_full_turn() {
        let offsets = ring_offsets([1.0, 0.0], 4);
        assert_eq!(offsets.len(), 4);
        let back = rotate_pair(offsets[3], (TWO_PI / 4.0).cos(), (TWO_PI / 4.0).sin());
        assert!((back[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_first_pass_needs_a_better_score_and_a_higher_y() {
        assert!(accepts_pass1(1.0, 0.0, f32::MAX, -f32::MAX));
        assert!(accepts_pass1(-0.5, 1.0, 1.0, 0.0));
        assert!(!accepts_pass1(1.0, 1.0, 1.0, 0.0)); // score not strictly lower
        assert!(!accepts_pass1(0.5, 0.0, 1.0, 0.0)); // y not strictly higher
    }

    #[test]
    fn the_fine_pass_only_needs_a_better_score() {
        assert!(accepts_pass2(-0.5, 1.0));
        assert!(!accepts_pass2(1.0, 1.0));
    }

    #[test]
    fn the_back_direction_normalizes_horizontally_and_drops_y() {
        // (-3, ., -4) has horizontal length 5, so each lane is scaled by 0.2
        // and the y lane is `n * 0.5 * 0.0`, which is zero.
        let d = back_direction([3.0, 5.0, 4.0, 2.0]);
        assert!((d[0] - -0.6).abs() < 1e-6, "x {}", d[0]);
        assert_eq!(d[1], 0.0);
        assert!((d[2] - -0.8).abs() < 1e-6, "z {}", d[2]);
        assert!((d[3] - -0.4).abs() < 1e-6, "w {}", d[3]);
    }

    #[test]
    fn a_flat_descriptor_falls_back_to_positive_z() {
        assert_eq!(back_direction([0.0, 1.0, 0.0, 0.0]), [0.0, 0.0, 1.0, 0.0]);
    }

    #[test]
    fn the_refinement_direction_falls_back_when_the_normal_is_degenerate() {
        let fallback = [0.0, 0.0, 1.0, 0.0];
        assert_eq!(
            refine_direction([0.0, 0.0, 0.0, 0.0], 0.0, fallback),
            fallback
        );
        // A real normal wins over the fallback and is normalized the same way.
        let d = refine_direction([0.0, 1.0, -2.0, 0.0], 0.0, fallback);
        assert!((d[2] - 1.0).abs() < 1e-6, "z {}", d[2]);
        assert_eq!(d[1], 0.0);
    }

    #[test]
    fn the_refined_offset_scales_both_terms() {
        let off = refined_offset([1.0, 0.0], [0.0; 4]);
        assert_eq!(off, [0.02, 0.0, 0.0, 0.0]);
        let off = refined_offset([0.0, 0.0], [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(off[0], 0.005);
        let off = refined_offset([1.0, 2.0], [3.0, 4.0, 5.0, 6.0]);
        assert_eq!(off[0], 0.02f32 + 0.015);
        assert_eq!(off[1], 0.02);
        assert_eq!(off[2], 0.04f32 + 0.025);
        assert_eq!(off[3], 0.03);
    }

    #[test]
    fn the_refinement_breaks_on_the_first_facing_hit() {
        struct Counting {
            calls: usize,
            hit: Option<SweepHit>,
        }
        impl CandidateSolver for Counting {
            fn evaluate(&mut self, _a: [f32; 4], _b: [f32; 4]) -> Option<SweepHit> {
                self.calls += 1;
                self.hit.map(|mut h| {
                    h.score = self.calls as f32;
                    h
                })
            }
        }
        let normal = [0.0, 0.0, 1.0, 0.0];
        let dir = [0.0, 0.0, 1.0, 0.0];

        // facing is 1.0 > 0.0, so the first candidate is kept and breaks.
        let mut solver = Counting {
            calls: 0,
            hit: Some(SweepHit {
                score: 0.0,
                normal,
                word: 0,
            }),
        };
        let best = refine_pentagon([0.0; 4], [1.0, 0.0], [0.0; 4], dir, 0.0, false, &mut solver)
            .expect("a hit");
        assert_eq!(solver.calls, 1);
        assert_eq!(best.score, 1.0);

        // best_y above the facing keeps the first hit but walks all five.
        let mut solver = Counting {
            calls: 0,
            hit: Some(SweepHit {
                score: 0.0,
                normal,
                word: 0,
            }),
        };
        let best = refine_pentagon([0.0; 4], [1.0, 0.0], [0.0; 4], dir, 2.0, false, &mut solver)
            .expect("a hit");
        assert_eq!(solver.calls, 5);
        assert_eq!(best.score, 1.0);

        // The flag bit bypasses the facing test entirely.
        let mut solver = Counting {
            calls: 0,
            hit: Some(SweepHit {
                score: 0.0,
                normal,
                word: 0,
            }),
        };
        refine_pentagon([0.0; 4], [1.0, 0.0], [0.0; 4], dir, 2.0, true, &mut solver);
        assert_eq!(solver.calls, 1);

        // No hit at all walks all five and returns nothing.
        let mut solver = Counting {
            calls: 0,
            hit: None,
        };
        assert!(
            refine_pentagon([0.0; 4], [1.0, 0.0], [0.0; 4], dir, 0.0, false, &mut solver,)
                .is_none()
        );
        assert_eq!(solver.calls, 5);
    }

    #[test]
    fn the_ring_acceptance_is_gated_and_strict() {
        assert!(accepts_ring(1.0, 0.5, true));
        assert!(!accepts_ring(0.5, 0.5, true)); // equal does not pass
        assert!(!accepts_ring(1.0, 0.5, false)); // the record's flag gates it
    }

    #[test]
    fn the_ring_sweep_radius_is_floored_at_0_35() {
        let sweep = ring_sweep(8, 0.1, 2.0).expect("a non-zero count sweeps");
        assert_eq!(sweep.count, 8);
        assert_eq!(sweep.radius, 0.35 * 2.0); // the 0.35 floor is applied first
        assert!(ring_sweep(0, 1.0, 1.0).is_none());
        // and the offset quad puts sine on x and cosine on z
        assert_eq!(ring_offset([0.0, 1.0], 2.0), [2.0, 0.0, 0.0, 0.0]);
        assert_eq!(ring_offset([1.0, 0.0], 2.0), [0.0, 0.0, 2.0, 0.0]);
    }

    #[test]
    fn the_shape_ring_shortens_the_seed_and_floats_at_0_015() {
        // pair = (0, 1) is the first candidate and points along +x
        assert_eq!(shape_ring_offset([0.0, 1.0], 1.0), [0.985, 0.015, 0.0, 0.0]);
        assert_eq!(shape_ring_offset([1.0, 0.0], 1.0), [0.0, 0.015, 0.985, 0.0]);
    }

    #[test]
    fn the_refinement_acceptance_is_strict_and_flag_bypassable() {
        assert!(accepts_refine(1.0, 0.0, false));
        assert!(!accepts_refine(0.0, 0.0, false)); // equal is not "higher"
        assert!(!accepts_refine(-1.0, 0.0, false));
        assert!(accepts_refine(-1.0, 0.0, true));
    }

    #[test]
    fn the_refined_sweep_drops_the_ground_probe_by_four_centimetres() {
        let center = [10.0, 20.0, 30.0, 7.0];
        let (a, b) = refined_sweep(center, [1.0, 0.0], [0.0; 4]);
        assert_eq!(a, [sse_add(10.0, 0.02), 20.0, 30.0, 7.0]);
        assert_eq!(b[1], sse_add(sse_add(20.0, -0.04), 0.0));
        assert_eq!(a[0], b[0]);
        assert_eq!(a[2], b[2]);
    }
}

#[cfg(test)]
mod composition_tests {
    use super::*;

    const DIR: [f32; 4] = [0.0, 0.0, 1.0, 0.0];

    fn body_with(turn_degrees: f32, enabled: bool) -> Vec<u8> {
        let mut body = vec![0u8; 0x700];
        body[TURN_LIMIT..TURN_LIMIT + 4].copy_from_slice(&turn_degrees.to_le_bytes());
        body[PASS_ENABLED] = u8::from(enabled);
        body
    }

    struct Fixed(Option<SweepHit>);
    impl CandidateSolver for Fixed {
        fn evaluate(&mut self, _a: [f32; 4], _b: [f32; 4]) -> Option<SweepHit> {
            self.0
        }
    }

    #[test]
    fn the_two_endpoints_carry_different_scalars() {
        let point = [1.0, 2.0, 3.0, 4.0];
        let (a, b) = endpoints(point, DIR, 7.0, 0.5, 2.0);
        assert_eq!(a, [1.0, 2.0, 3.0 + 0.5, 4.0 + 0.5 * 7.0]);
        assert_eq!(b, [1.0, 2.0, 3.0 - 2.0, 4.0 - 7.0 * 2.0]);
        // the w lane is scaled by dir_w, not by dir[3]
        let (_, b) = endpoints([0.0; 4], DIR, 3.0, 1.0, 1.0);
        assert_eq!(b[3], -3.0);
    }

    #[test]
    fn the_memo_tolerance_is_inclusive_and_ignores_w() {
        let cached = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]];
        assert!(memo_matches(cached, cached));
        let nudged = [[1.0, 2.0, 3.0 + 0.004], [4.0, 5.0, 6.0]];
        assert!(memo_matches(cached, nudged), "0.004 exactly still reuses");
        let past = [[1.0, 2.0, 3.0 + 0.005], [4.0, 5.0, 6.0]];
        assert!(!memo_matches(cached, past));
    }

    #[test]
    fn the_facing_threshold_is_the_turn_limits_cosine() {
        assert_eq!(facing_threshold(&body_with(0.0, true)), 1.0);
        let t = facing_threshold(&body_with(60.0, true));
        assert!((t - 0.5).abs() < 1e-6, "cos 60 == 0.5, got {t}");
    }

    #[test]
    fn the_projection_is_absolute_and_ordered() {
        assert_eq!(projection([0.0, 0.0, 2.0, 9.0], DIR), 2.0);
        assert_eq!(projection([0.0, 0.0, -2.0, 9.0], DIR), 2.0);
        assert_eq!(projection([1.0, 0.0, 0.0, 9.0], DIR), 0.0);
    }

    #[test]
    fn the_back_transform_removes_the_component_along_the_mover() {
        // out = cand - (cand . dir) * dir: the along-fan part cancels, the
        // sideways part survives.
        let (out, dot) = project_back([3.0, 0.0, 2.0, 0.0], DIR, 0.0);
        assert_eq!(dot, 2.0);
        assert_eq!(out, [3.0, 0.0, 0.0, 0.0]);
        // already perpendicular: nothing to project
        let (out, dot) = project_back([5.0, 0.0, 0.0, 0.0], DIR, 0.0);
        assert_eq!(dot, 0.0);
        assert_eq!(out, [5.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn the_reflection_flags_read_the_third_byte() {
        assert_eq!(reflection_flags(0), (false, false));
        assert_eq!(reflection_flags(0x1_0000), (true, false));
        assert_eq!(reflection_flags(0x4_0000), (false, true));
        assert_eq!(reflection_flags(0x5_0000), (true, true));
        // bit 0 is the gate; bit 1 is not read by either output
        assert_eq!(reflection_flags(0x3_0000), (true, false));
    }

    #[test]
    fn the_gate_stops_the_producer_before_any_probe() {
        struct Panic;
        impl CandidateSolver for Panic {
            fn evaluate(&mut self, _a: [f32; 4], _b: [f32; 4]) -> Option<SweepHit> {
                panic!("the gated producer must not probe")
            }
        }
        let ctx = SolveContext {
            point: [0.0; 4],
            dir: DIR,
            dir_w: 0.0,
            seed: 1.0,
            speed: 1.0,
            ring: ring_sweep(8, 1.0, 1.0),
        };
        assert_eq!(solve(&body_with(30.0, false), &ctx, &mut Panic), None);
    }

    #[test]
    fn a_misaligned_ring_hit_is_rejected() {
        // facing 0.0 against a cos(30) threshold: the ring's gate fails
        let ctx = SolveContext {
            point: [0.0; 4],
            dir: DIR,
            dir_w: 0.0,
            seed: 1.0,
            speed: 1.0,
            ring: ring_sweep(8, 1.0, 1.0),
        };
        let mut solver = Fixed(Some(SweepHit {
            score: 1.0,
            normal: [1.0, 0.0, 0.0, 0.0],
            word: 0,
        }));
        assert_eq!(solve(&body_with(30.0, true), &ctx, &mut solver), None);
    }

    #[test]
    fn an_aligned_ring_hit_flows_through_refine_and_the_back_transform() {
        let ctx = SolveContext {
            point: [0.0, 0.0, 10.0, 0.0],
            dir: DIR,
            dir_w: 0.0,
            seed: 0.5,
            speed: 2.0,
            ring: ring_sweep(4, 1.0, 1.0),
        };
        let mut solver = Fixed(Some(SweepHit {
            score: 0.25,
            normal: DIR,
            word: 0,
        }));
        let solved = solve(&body_with(30.0, true), &ctx, &mut solver).expect("a hit");
        // the ring's first candidate is the unrotated pair, so the endpoint is
        // point + seed*dir + ring_offset([1,0], radius)
        let sweep = ring_sweep(4, 1.0, 1.0).unwrap();
        let off = ring_offset([1.0, 0.0], sweep.radius);
        let cand = [
            0.0 + off[0],
            0.0 + off[1],
            10.0 + 0.5 + off[2],
            0.0 + off[3],
        ];
        let (want, _) = project_back(cand, DIR, 0.0);
        assert_eq!(solved.direction, want);
        assert_eq!(solved.normal, DIR);
        assert!(solved.refined, "the refine's first candidate always wins");
    }
}
