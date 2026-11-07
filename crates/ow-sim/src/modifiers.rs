//! The mover body's per-frame state-modifier evaluator, read from
//! `Overwatch.exe`.
//!
//! `ApplyStateModifiers` (`0x140cbcbe0`) walks the 0x38-byte modifier records
//! at `body+0x870` (count `+0x878`), applies the six operations to one
//! accumulator per kind, then folds the accumulators into the movement
//! parameter block `+0x528..` (the block copied from `+0x3c8`).
//!
//! The 37 live kinds are `0x00..0x10`, `0x16..0x21`, `0x27..0x2e`; the holes
//! `0x11..0x15` and `0x22..0x26` and any kind outside the table are ignored.
//! A record's window is `[+0x1c, +0x20)` in the frame id `body+0xe4`.
//!
//! The class's two virtual calls (`[vptr+0x68]` and `[vptr+0x70]`) are base
//! stubs: the first is `ret 0`, the second returns 1. The `+0x70` gate is
//! constant true, so the only early return is unreachable for this class. The
//! end-of-function events (`0x11`/`0x12`) and the `0x142cbd190` notify are
//! external side effects and are not modelled; they do not touch the body.

use crate::movement::{maxss, minss, sse_add, sse_mul};

/// The body's live byte span.
pub const BODY_LEN: usize = 0x1c00;

const FLT_EPSILON: f32 = f32::from_bits(0x3748_0000);
const DEG_TO_RAD: f32 = f32::from_bits(0x3c8e_fa35);
const ROUND_HALF: f32 = 0.5;
const DASH_MULTIPLIER: f32 = 0.01;
const DASH_SHORT_TICKS: u32 = 20;
const DASH_LONG_TICKS: u32 = 63;
const DASH_FLAGS: u32 = 0x0080_000c;
const CLAMP_CAP: f32 = 100.0;

#[inline]
fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

#[inline]
fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[inline]
fn i32_at(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[inline]
fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

#[inline]
fn f32_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[inline]
fn put_u8(bytes: &mut [u8], offset: usize, value: u8) {
    bytes[offset] = value;
}

#[inline]
fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

#[inline]
fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[inline]
fn put_f32(bytes: &mut [u8], offset: usize, value: f32) {
    put_u32(bytes, offset, value.to_bits());
}

/// The block assignment at `0x140ca4980` / `0x140ca4680`: a field-wise copy
/// that skips `+0x00..+0x0f` (vptr member), `+0x48..+0x4f` (non-trivial
/// member) and `+0x132..+0x133` (the two bytes between the pair at
/// `+0x130/+0x131` and the dword at `+0x134`).
fn copy_block(body: &mut [u8], dst: usize, src: usize) {
    for (lo, hi) in [
        (0x10usize, 0x48usize),
        (0x50, 0x130),
        (0x130, 0x132),
        (0x134, 0x15c),
    ] {
        body.copy_within(src + lo..src + hi, dst + lo);
    }
}

/// The SSE `maxss(0.0, x)` fold the client applies before using an
/// accumulator. A NaN accumulator yields the destination `0.0`, because the
/// source is the NaN operand.
#[inline]
fn saturate(value: f32) -> f32 {
    maxss(0.0, value)
}

/// `comiss` + conditional swap: ordered `min`, unordered takes `b`.
#[inline]
fn minf(a: f32, b: f32) -> f32 {
    if b >= a {
        a
    } else {
        b
    }
}

/// `comiss` + conditional swap: ordered `max`, unordered takes `a`.
#[inline]
fn maxf(a: f32, b: f32) -> f32 {
    if b >= a {
        b
    } else {
        a
    }
}

/// `CVTTSS2SI` then the low 16 bits. Out-of-range and NaN produce
/// `0x80000000`, whose low half is zero.
#[inline]
fn truncate_to_u16(value: f32) -> u16 {
    if value.is_nan() || value >= 2_147_483_648.0 || value < -2_147_483_648.0 {
        0
    } else {
        (value as i32) as u16
    }
}

/// `0x140cbcbe0`: apply the body's modifier records to its movement block.
///
/// `records` is the raw array at `[body+0x870]` (0x38 bytes each).
/// `gate` is the base vtable's `+0x70` result (always true for this class).
/// `feature_bit` is bit 93 of the obfuscated service global reached from
/// `body+8`; `g1`/`g2` are the two mutable doubles at `0x1439b1230` /
/// `0x1439b1270`.
pub fn apply_state_modifiers(
    body: &mut [u8],
    base: u64,
    records: &[u8],
    gate: bool,
    feature_bit: bool,
    g1: f64,
    g2: f64,
) {
    let id = u32_at(body, 0xe4);

    // ---- dash prologue: may expire the dash and clear +0x10e4 ----
    let start_raw = i32_at(body, 0x10e4);
    if start_raw != -1 {
        let start = start_raw as u32;
        if u32_at(body, 0x10d8) >= start && id >= start {
            let t = id.wrapping_sub(start);
            let expired = t >= DASH_LONG_TICKS
                || (t >= DASH_SHORT_TICKS && (u32_at(body, 0x10d0) & DASH_FLAGS) == 0);
            if expired {
                put_u32(body, 0x10e4, 0xffff_ffff);
            }
            // t < 20 and (20 <= t < 63 with the flags set) keep the value.
        }
    }
    if !gate {
        return;
    }

    // ---- the 0xa70 array scan: restore or clear the +0x3c8 block ----
    let mut found = None;
    let last = i32_at(body, 0xa78) - 1;
    if last >= 0 {
        let table = u64_at(body, 0xa70);
        if let Some(offset) = table.checked_sub(base) {
            let offset = offset as usize;
            for i in (0..=last as usize).rev() {
                let entry = offset + i * 0x188;
                if entry + 0x24 > body.len() {
                    break;
                }
                if id >= u32_at(body, entry + 0x1c) && id < u32_at(body, entry + 0x20) {
                    found = Some(entry);
                    break;
                }
            }
        }
    }
    match found {
        Some(entry) => {
            copy_block(body, 0x3c8, entry + 0x28);
            put_u8(body, 0x1a81, 1);
        }
        None => {
            if body[0x1a81] == 1 {
                copy_block(body, 0x3c8, 0x268);
                put_u8(body, 0x1a81, 0);
            }
        }
    }

    let old_660 = f32_at(body, 0x660);
    let old_650 = u16_at(body, 0x650);
    let old_664 = f32_at(body, 0x664);
    copy_block(body, 0x528, 0x3c8);

    // ---- accumulator seeds ----
    let mut slots = [0.0f32; 0x2f];
    slots[0x00] = 1.0;
    slots[0x01] = 1.0;
    slots[0x02] = 1.0;
    slots[0x03] = 1.0;
    slots[0x04] = 1.0;
    slots[0x05] = 1.0;
    slots[0x06] = 1.0;
    slots[0x07] = 1.0;
    slots[0x08] = 1.0;
    slots[0x09] = 1.0;
    slots[0x0a] = f32_at(body, 0x60c);
    slots[0x0b] = 1.0;
    slots[0x0c] = 1.0;
    slots[0x0d] = f32_at(body, 0x614);
    slots[0x0e] = f32_at(body, 0x618);
    slots[0x0f] = f32_at(body, 0x61c);
    slots[0x10] = f32_at(body, 0x620);
    slots[0x16] = f32::from(body[0x652]);
    slots[0x17] = 1.0;
    slots[0x18] = 1.0;
    slots[0x19] = 0.0;
    slots[0x1a] = 1.0;
    slots[0x1b] = 1.0;
    slots[0x1c] = 0.0;
    slots[0x1d] = -10000.0;
    slots[0x1e] = 10000.0;
    slots[0x1f] = 0.0;
    slots[0x20] = -10000.0;
    slots[0x21] = 10000.0;
    slots[0x27] = 0.0;
    slots[0x28] = 1.0;
    slots[0x29] = 1.0;
    slots[0x2a] = 1.0;
    slots[0x2b] = 1.0;
    slots[0x2c] = 1.0;
    slots[0x2d] = 1.0;
    slots[0x2e] = 1.0;

    // ---- record loop ----
    let count = records.len() / 0x38;
    for i in 0..count {
        let record = &records[i * 0x38..(i + 1) * 0x38];
        if id < u32_at(record, 0x1c) || id >= u32_at(record, 0x20) {
            continue;
        }
        let kind = i32_at(record, 0x28);
        if (kind as u32) > 0x2e {
            continue;
        }
        let op = i32_at(record, 0x2c);
        if (op as u32) > 5 {
            continue;
        }
        let value = f32_at(record, 0x30);
        let bound = f32_at(record, 0x34);
        let slot = &mut slots[kind as usize];
        match op {
            0 => *slot = sse_add(*slot, value),
            1 => *slot = sse_mul(*slot, value),
            2 => *slot = value,
            3 => *slot = minss(*slot, value),
            4 => *slot = maxss(*slot, value),
            5 => *slot = maxss(minss(*slot, maxss(value, bound)), value),
            _ => unreachable!(),
        }
    }

    // ---- final combination ----
    let clamp_low = g1 as f32;
    let clamp_high = g2 as f32;
    let x_65c = maxss(minss(slots[0x0c], clamp_low), clamp_high);
    slots[0x0e] = saturate(slots[0x0e]);
    let k_old = saturate(slots[0x0d]);
    slots[0x0d] = saturate(slots[0x1a]);
    slots[0x1b] = saturate(slots[0x1b]);

    let a = saturate(slots[0x00]);
    let b = saturate(slots[0x01]);
    let c = saturate(slots[0x02]);
    let d = saturate(slots[0x03]);
    let e = saturate(slots[0x04]);
    let f = saturate(slots[0x05]);
    let g = saturate(slots[0x07]);
    let h = saturate(slots[0x08]);
    let i_slot = saturate(slots[0x0a]);
    let j = saturate(slots[0x28]);
    let raw30 = slots[0x06];
    let raw40 = slots[0x09];

    let start = i32_at(body, 0x10e4);
    let mut e2 = e;
    let mut g2v = g;
    let mut h2 = h;
    let mut copy_5e8 = false;
    if start != -1 {
        let start = start as u32;
        if id >= start {
            let t = id.wrapping_sub(start);
            if t < DASH_SHORT_TICKS {
                copy_5e8 = true;
                e2 = DASH_MULTIPLIER;
                g2v = 1.0;
                h2 = 0.0;
            } else if t < DASH_LONG_TICKS && (u32_at(body, 0x10d0) & DASH_FLAGS) != 0 {
                g2v = 1.0;
                h2 = 0.0;
            }
        }
    }

    if copy_5e8 {
        put_u32(body, 0x5e8, u32_at(body, 0x5ec));
    }
    for lane in 0..4 {
        let offset = 0x5d0 + lane * 4;
        put_f32(body, offset, sse_mul(f32_at(body, offset), a));
    }
    for lane in 0..4 {
        let offset = 0x638 + lane * 4;
        put_f32(body, offset, sse_mul(f32_at(body, offset), a));
    }
    put_f32(body, 0x5e0, sse_mul(a, f32_at(body, 0x5e0)));
    put_f32(body, 0x5e4, sse_mul(a, f32_at(body, 0x5e4)));
    put_f32(body, 0x5e8, sse_mul(b, f32_at(body, 0x5e8)));
    put_f32(body, 0x5ec, sse_mul(c, f32_at(body, 0x5ec)));
    put_f32(body, 0x5f0, sse_mul(d, f32_at(body, 0x5f0)));
    put_f32(body, 0x5f4, sse_mul(sse_mul(f, e2), f32_at(body, 0x5f4)));
    put_f32(body, 0x5f8, sse_mul(sse_mul(raw30, a), f32_at(body, 0x5f8)));
    put_f32(body, 0x5fc, sse_mul(e2, f32_at(body, 0x5fc)));
    put_f32(body, 0x600, sse_mul(sse_mul(h2, g2v), f32_at(body, 0x600)));
    put_f32(body, 0x604, sse_mul(sse_mul(raw40, a), f32_at(body, 0x604)));
    put_f32(body, 0x608, sse_mul(g2v, f32_at(body, 0x608)));

    put_f32(body, 0x5cc, sse_mul(j, f32_at(body, 0x5cc)));
    put_f32(body, 0x60c, i_slot);
    put_f32(body, 0x610, sse_mul(slots[0x0b], f32_at(body, 0x610)));
    put_f32(body, 0x614, k_old);
    put_f32(body, 0x618, saturate(slots[0x0e]));
    put_f32(body, 0x61c, slots[0x0f]);
    put_f32(body, 0x620, slots[0x10]);

    let rounded = slots[0x27];
    let adjusted = if rounded >= 0.0 {
        rounded + ROUND_HALF
    } else {
        rounded - ROUND_HALF
    };
    put_u16(body, 0x650, truncate_to_u16(adjusted));
    put_u8(body, 0x652, u8::from(slots[0x16] > FLT_EPSILON));
    put_u8(body, 0x658, u8::from(slots[0x17] > FLT_EPSILON));
    put_u8(body, 0x659, u8::from(slots[0x18] > FLT_EPSILON));
    put_f32(body, 0x65c, x_65c);

    put_f32(body, 0x660, a);
    put_f32(body, 0x664, saturate(slots[0x1a]));
    put_f32(body, 0x668, maxss(minss(slots[0x1b], CLAMP_CAP), 0.0));
    put_f32(body, 0x66c, sse_mul(slots[0x1c], DEG_TO_RAD));
    put_f32(
        body,
        0x670,
        sse_mul(minf(slots[0x1d], slots[0x1e]), DEG_TO_RAD),
    );
    put_f32(
        body,
        0x674,
        sse_mul(maxf(slots[0x1d], slots[0x1e]), DEG_TO_RAD),
    );
    put_f32(body, 0x678, sse_mul(slots[0x1f], DEG_TO_RAD));
    put_f32(
        body,
        0x67c,
        sse_mul(minf(slots[0x20], slots[0x21]), DEG_TO_RAD),
    );
    put_f32(
        body,
        0x680,
        sse_mul(maxf(slots[0x20], slots[0x21]), DEG_TO_RAD),
    );

    put_f32(body, 0x578, sse_mul(slots[0x29], f32_at(body, 0x578)));
    put_f32(body, 0x57c, sse_mul(slots[0x2a], f32_at(body, 0x57c)));
    put_f32(body, 0x580, sse_mul(slots[0x2b], f32_at(body, 0x580)));
    put_f32(body, 0x584, sse_mul(slots[0x2c], f32_at(body, 0x584)));
    put_f32(body, 0x588, sse_mul(slots[0x2d], f32_at(body, 0x588)));
    put_f32(body, 0x58c, sse_mul(slots[0x2e], f32_at(body, 0x58c)));

    // ---- guarded crouch clamps ----
    if feature_bit && body[0x654] != 0 {
        put_f32(body, 0x5a0, minss(2.0, f32_at(body, 0x440)));
        put_f32(body, 0x5a4, minss(0.5, f32_at(body, 0x444)));
        put_f32(body, 0x5ac, minss(2.0, f32_at(body, 0x44c)));
        put_f32(body, 0x5b0, minss(0.5, f32_at(body, 0x450)));
    }

    // ---- post writes ----
    put_u8(body, 0x1a7b, u8::from(slots[0x19] > FLT_EPSILON));
    put_f32(body, 0x10e8, f32_at(body, 0x610));
    // The 0x11/0x12 events and the 0x142cbd190 notify are external side
    // effects; they do not write the body.
    let _ = (old_660, old_650, old_664);
}

/// `FUN_140cb3990` (`0x140cb3990`): the single-property query over the
/// state-modifier records.
///
/// This is the function every "property" read in the mover goes through - the
/// jump stage asks it for property `0x14` (jump requested) and `0x15` (crouch
/// requested) and for the `0x22`-`0x26` wish properties. It walks the same
/// 0x38-byte records as [`apply_state_modifiers`] (`body+0x870`, count
/// `body+0x878`), keeps those whose window `[+0x1c, +0x20)` holds the frame id
/// `body+0xe4`, and applies the op of every record whose `+0x28` equals
/// `property`, in table order:
///
/// | op (`+0x2c`) | effect on the value |
/// |---|---|
/// | 0 | `value + current` (the record's float is the destination) |
/// | 1 | `value * current` |
/// | 2 | `value` |
/// | 3 | `minss(current, value)` |
/// | 4 | `maxss(current, value)` |
/// | 5 | `maxss(minss(current, maxss(value, bound)), value)` |
///
/// `bound` is `+0x34`. An op outside `0..=5` leaves the value alone but still
/// counts as applied (`bl = 1` at `0x140cb3a8a` is outside the jump table).
/// Returns the value and whether any record matched.
pub fn apply_property_records(
    records: &[u8],
    frame_id: u32,
    property: i32,
    value: f32,
) -> (f32, bool) {
    let mut value = value;
    let mut applied = false;
    let count = records.len() / 0x38;
    for i in 0..count {
        let record = &records[i * 0x38..(i + 1) * 0x38];
        if frame_id < u32_at(record, 0x1c) || frame_id >= u32_at(record, 0x20) {
            continue;
        }
        if i32_at(record, 0x28) != property {
            continue;
        }
        let op = i32_at(record, 0x2c);
        let arg = f32_at(record, 0x30);
        let bound = f32_at(record, 0x34);
        match op {
            0 => value = sse_add(arg, value),
            1 => value = sse_mul(arg, value),
            2 => value = arg,
            3 => value = minss(value, arg),
            4 => value = maxss(value, arg),
            5 => value = maxss(minss(value, maxss(arg, bound)), arg),
            _ => {}
        }
        applied = true;
    }
    (value, applied)
}

/// The three-lane squared length `FUN_140cb3b10` compares: `(y*y + x*x) + z*z`,
/// in the client's own add order, so a NaN lane behaves as it does there. The
/// fourth lane is not part of the length.
fn quad_length_sq(q: [f32; 4]) -> f32 {
    sse_add(
        sse_add(sse_mul(q[1], q[1]), sse_mul(q[0], q[0])),
        sse_mul(q[2], q[2]),
    )
}

/// `FUN_140cb3b10` (`0x140cb3b10`): the four-lane record applier.
///
/// The companion of [`apply_property_records`] for the 0x40-byte records at
/// `body+0x960` (count `body+0x968`). A record is live when its window
/// `[+0x1c, +0x20)` holds the frame id `body+0xe4` and its `+0x28` equals
/// `mode`; its op is `+0x2c`:
///
/// | op | effect on the quad |
/// |---|---|
/// | 0 | `quad + record` (lane by lane) |
/// | 1 | `quad * record` |
/// | 2 | `record` |
/// | 3 | `record` when `|quad| > |record|` (the shorter wins) |
/// | 4 | `record` when `|quad| < |record|` (the longer wins) |
///
/// The length is [`quad_length_sq`]. With `mode == 1` the `y` lane is put back
/// the way it arrived, per record - which is why the lateral lanes are the ones
/// the movement vector's state modifiers act on.
///
/// The two comparisons are `COMISS` + `JBE` (op 3) and `COMISS` + `JAE` (op 4),
/// so an unordered pair differs between them: op 3 keeps the quad, op 4 takes
/// the record. Both are reproduced as the client's branch, not as a
/// `min`/`max`.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn apply_vector_records(
    records: &[u8],
    frame_id: u32,
    mode: i32,
    quad: [f32; 4],
) -> ([f32; 4], bool) {
    let mut quad = quad;
    let mut applied = false;
    let count = records.len() / 0x40;
    for i in 0..count {
        let record = &records[i * 0x40..(i + 1) * 0x40];
        if frame_id < u32_at(record, 0x1c) || frame_id >= u32_at(record, 0x20) {
            continue;
        }
        if i32_at(record, 0x28) != mode {
            continue;
        }
        let lane_y = quad[1];
        let mut v = [0.0f32; 4];
        for (k, lane) in v.iter_mut().enumerate() {
            *lane = f32_at(record, 0x30 + 4 * k);
        }
        match i32_at(record, 0x2c) {
            0 => {
                for k in 0..4 {
                    quad[k] = sse_add(quad[k], v[k]);
                }
            }
            1 => {
                for k in 0..4 {
                    quad[k] = sse_mul(quad[k], v[k]);
                }
            }
            2 => quad = v,
            // `comiss s2,s1 ; jbe` skips when `s2 <= s1` or unordered.
            3 if quad_length_sq(quad) > quad_length_sq(v) => {
                quad = v;
            }
            // `comiss s2,s1 ; jae` skips when `s2 >= s1`, so unordered copies.
            4 if !(quad_length_sq(quad) >= quad_length_sq(v)) => {
                quad = v;
            }
            _ => {}
        }
        if mode == 1 {
            quad[1] = lane_y;
        }
        applied = true;
    }
    (quad, applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> Vec<u8> {
        let mut body = vec![0u8; BODY_LEN];
        put_f32(&mut body, 0x660, 1.0);
        put_f32(&mut body, 0x664, 1.0);
        put_u32(&mut body, 0xe4, 0);
        put_u32(&mut body, 0x10e4, 0xffff_ffff);
        body
    }

    #[test]
    fn the_default_path_writes_the_expected_fingerprint() {
        let mut body = body();
        apply_state_modifiers(&mut body, 0, &[], true, false, 0.0, 0.0);
        assert_eq!(f32_at(&body, 0x660), 1.0);
        assert_eq!(f32_at(&body, 0x664), 1.0);
        assert_eq!(f32_at(&body, 0x668), 1.0);
        assert_eq!(f32_at(&body, 0x65c), 0.0);
        assert_eq!(body[0x658], 1);
        assert_eq!(body[0x659], 1);
        assert_eq!(u16_at(&body, 0x650), 0);
    }

    #[test]
    fn a_set_record_writes_its_kind_slot() {
        let mut body = body();
        let mut record = [0u8; 0x38];
        put_u32(&mut record, 0x1c, 0);
        put_u32(&mut record, 0x20, 1);
        put_u32(&mut record, 0x28, 0x1b);
        put_u32(&mut record, 0x2c, 2);
        put_f32(&mut record, 0x30, 7.5);
        apply_state_modifiers(&mut body, 0, &record, true, false, 0.0, 0.0);
        assert_eq!(f32_at(&body, 0x668), 7.5);
    }
}
