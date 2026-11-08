//! The client's fixed-point scaler.
//!
//! `FUN_141c6c830` (`0x141c6c830`) converts a run of `f32` to an integer with a
//! 12-bit fraction (`value * 4096`, truncated toward zero) by integer bit
//! manipulation only - no `cvtss2si`, so its rounding and its saturation are
//! its own, not the FPU's:
//!
//! ```text
//!   exponent >= 138 : (mantissa | 0x800000) << (exponent - 138)   0x7fffffff if that shift >= 8
//!   exponent <  138 : (mantissa | 0x800000) >> (138 - exponent)   0 if that shift > 23
//!   sign            : (magnitude ^ -sign) - -sign     (two's-complement negate)
//! ```
//!
//! So `1.0` becomes `4096`, `1/60` becomes `68` (not `68.27` rounded), a value
//! below `2^-12` becomes `0`, a value at or above `2^19` saturates at
//! `i32::MAX`, and `-inf`/negative `NaN` become **`-i32::MAX`**, not `i32::MIN`:
//! the sign mask is applied to the saturated magnitude, so the extra `-1` that
//! `i32::MIN` would need is never produced (`i32::MIN` is unreachable). The
//! three constants are read from the body, not chosen: `138`, `8`, `23`.
//!
//! Verified against the client's own bytes by the `fn/FloatToQ12` unit
//! (`dev/verify_harness`).

/// Convert `count` f32 values from `src` into Q12 integers in `dst`.
///
/// The client's loop is `for (i = 0; i < count; i++) dst[i] = scale(src[i])`,
/// with `dst: *mut i32`, `src: *const f32`, `count: u32`.
pub fn quantize_q12(dst: &mut [i32], src: &[f32], count: u32) {
    for i in 0..count as usize {
        dst[i] = scale_q12(src[i]);
    }
}

/// One value through the client's integer path.
pub fn scale_q12(value: f32) -> i32 {
    let bits = value.to_bits();
    let sign = (bits as i32) >> 31; // 0 or -1
    let exponent = ((bits >> 23) & 0xFF) as i32;
    let mantissa = bits & 0x007F_FFFF;
    let magnitude = if exponent >= 138 {
        let shift = exponent - 138;
        if shift >= 8 {
            0x7FFF_FFFF
        } else {
            (mantissa + 0x0080_0000) << shift
        }
    } else {
        let shift = 138 - exponent;
        if shift > 23 {
            0
        } else {
            (mantissa + 0x0080_0000) >> shift
        }
    };
    let magnitude = magnitude as i32;
    (magnitude ^ sign).wrapping_sub(sign)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_anchors_of_the_path() {
        assert_eq!(scale_q12(1.0), 4096);
        assert_eq!(scale_q12(-1.0), -4096);
        assert_eq!(scale_q12(0.0), 0);
        assert_eq!(scale_q12(-0.0), 0);
        // 1/60 exactly as the client's own call site writes it.
        assert_eq!(scale_q12(f32::from_bits(0x3C88_8889)), 68);
        // Below 2^-12 the mantissa shift exceeds 23 and the result is zero.
        assert_eq!(scale_q12(1.0e-5), 0);
        // At 2^19 the left shift reaches 8 and the value saturates.
        assert_eq!(scale_q12(1.0e30), i32::MAX);
        // The negation is applied after saturation, so the magnitude is what
        // saturates: -i32::MAX, never i32::MIN.
        assert_eq!(scale_q12(f32::NEG_INFINITY), -i32::MAX);
        assert_eq!(scale_q12(f32::NAN), i32::MAX);
        assert_eq!(scale_q12(-f32::NAN), -i32::MAX);
    }

    #[test]
    fn a_run_keeps_order_and_uses_the_count() {
        let src = [0.0, 1.0, -1.0, 2.0, 0.5];
        let mut dst = [-7; 5];
        quantize_q12(&mut dst, &src, 3);
        assert_eq!(dst, [0, 4096, -4096, -7, -7]);
    }
}
