//! The mover's movement-settings block, read from `Overwatch.exe`.
//!
//! `0x1405a87d0` default-constructs the settings object. It writes the named
//! fields of the leading 0x5e8 bytes and leaves everything else as the caller
//! left it. Four fields are pointers into the object itself (`+0x10`,
//! `+0x70`, `+0x280`, `+0x558`), so the constructor needs the object's
//! address; `base` is that address in the client's address space and `global`
//! is the value of the sentinel global at `0x1441880e8`.
//!
//! This is the config path the verified ramp and spring read:
//! `+0x5a8/+0x5ac` (the clear/set scale pair, both 1.0), `+0x5b4` (the
//! alternate scale, 1.0), `+0x5c0` (0.0, the spring frequency slot written by
//! the qword store at `+0x5bc`) and `+0x5c4..+0x5c6` (three bools, all 1).
//! The parameter block `+0x5d0..+0x5e6` is initialised here too: `-2` at
//! `+0x5d0`, `1.0` at `+0x5d4`, `0x10000` at `+0x5dc`.
//!
//! The client's settings apply (`0x140c862e0`) later overwrites the scale
//! pair, `+0x5b0`, `+0x5c0`, the three bools and `+0x5c8` from the movement
//! asset. `+0x5b4` has no writer outside this constructor and the copy
//! routines, so its value stays at this default unless the object is copied.

/// The highest byte the default constructor writes, plus one.
pub const DEFAULT_SETTINGS_SPAN: usize = 0x5e8;

#[inline]
fn put_u8(out: &mut [u8], offset: usize, value: u8) {
    out[offset] = value;
}

#[inline]
fn put_u16(out: &mut [u8], offset: usize, value: u16) {
    out[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

#[inline]
fn put_u32(out: &mut [u8], offset: usize, value: u32) {
    out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[inline]
fn put_u64(out: &mut [u8], offset: usize, value: u64) {
    out[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

/// `0x1405a87d0`: default-construct the movement-settings object.
///
/// `out` is the object (at least [`DEFAULT_SETTINGS_SPAN`] bytes), `base` is
/// its address and `global` is `[0x1441880e8]`. The self-referential pointer
/// fields are written as `base + offset`, exactly as the client's `LEA`s do.
pub fn default_settings(out: &mut [u8], base: u64, global: u32) {
    let address = base as usize;

    put_u32(out, 0x000, 0);
    put_u32(out, 0x004, global);
    put_u32(out, 0x008, global);
    put_u64(out, 0x010, (address + 0x20) as u64);
    put_u32(out, 0x018, 0);
    put_u32(out, 0x01c, 0x8000_0008);
    put_u32(out, 0x060, 0);
    put_u32(out, 0x064, 0);
    put_u64(out, 0x068, 0);
    put_u64(out, 0x070, (address + 0x80) as u64);
    put_u32(out, 0x078, 0);
    put_u32(out, 0x07c, 0x8000_0008);
    put_u64(out, 0x280, (address + 0x290) as u64);
    put_u32(out, 0x288, 0);
    put_u32(out, 0x28c, 0x8000_0004);
    for offset in (0x490..0x510).step_by(0x10) {
        put_u64(out, offset, 0);
        put_u64(out, offset + 8, 0);
    }
    put_u32(out, 0x510, global);
    put_u32(out, 0x514, global);
    put_u32(out, 0x518, global);
    put_u32(out, 0x51c, global);
    put_u64(out, 0x520, 0);
    put_u64(out, 0x528, 1);
    put_u64(out, 0x530, 0);
    put_u64(out, 0x538, 0);
    put_u32(out, 0x540, 0);
    put_u64(out, 0x548, 0);
    put_u64(out, 0x550, 0);
    put_u64(out, 0x558, (address + 0x568) as u64);
    put_u32(out, 0x560, 0);
    put_u32(out, 0x564, 0x8000_0002);
    put_u8(out, 0x598, 1);
    put_u32(out, 0x59c, 0);
    put_u64(out, 0x5a0, 0);
    put_u32(out, 0x5a8, 0x3f80_0000);
    put_u32(out, 0x5ac, 0x3f80_0000);
    put_u32(out, 0x5b0, 0);
    put_u64(out, 0x5b4, 0x3f80_0000);
    put_u64(out, 0x5bc, 0);
    put_u16(out, 0x5c4, 0x0101);
    put_u8(out, 0x5c6, 1);
    put_u64(out, 0x5c8, 0);
    put_u32(out, 0x5d0, 0xffff_fffe);
    put_u64(out, 0x5d4, 0x3f80_0000);
    put_u32(out, 0x5dc, 0x0001_0000);
    put_u8(out, 0x5e0, 0);
    put_u8(out, 0x5e1, 0);
    put_u32(out, 0x5e2, 0);
    put_u16(out, 0x5e6, 0);
}

/// The three blocks a movement asset points at.
pub struct AssetPointees<'a> {
    /// `[asset + 0x100]`: the two scale floats copied to `+0x5a8/+0x5ac`.
    pub scales: &'a [u8],
    /// `[asset + 0xd0]`: the three gate bytes copied to `+0x5c4..+0x5c6`.
    pub gates: &'a [u8],
    /// `[asset + 0x118]`: the `u32` ids appended to the `+0x10` vector.
    /// `asset + 0x120` gives the count.
    pub ids: &'a [u8],
}

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
fn put_i32(out: &mut [u8], offset: usize, value: i32) {
    put_u32(out, offset, value as u32);
}

#[inline]
fn put_f32(out: &mut [u8], offset: usize, value: f32) {
    put_u32(out, offset, value.to_bits());
}

/// `0x140c862e0`: apply a movement asset to the settings object.
///
/// The three asset pointer fields are supplied as [`AssetPointees`] rather
/// than dereferenced, because the candidate has no address space. `state` is
/// `(record, block base, block bytes)`; `None` takes the client's direct-copy
/// path (`+0x490..+0x498` from `asset+0x90..0x98`). A record whose presence
/// bit is clear, or whose decode yields null, also takes the direct-copy path,
/// exactly as the client falls through when the masked decode is zero.
///
/// Returns the client's `EAX` (`asset+0xc8`), or `None` when the `+0x10`
/// vector would have to grow past its capacity (that allocator path is open).
pub fn apply_settings(
    out: &mut [u8],
    base: u64,
    asset: &[u8],
    pointees: &AssetPointees<'_>,
    state: Option<(u64, u64, &[u8])>,
    lookup: crate::state::StateLookup,
) -> Option<u32> {
    put_u32(out, 0x060, u32::from(asset[0x78]));
    put_u32(out, 0x5a4, u32_at(asset, 0xe8));
    put_u32(out, 0x5a8, u32_at(pointees.scales, 0));
    put_u32(out, 0x5ac, u32_at(pointees.scales, 4));
    put_u32(out, 0x008, u32_at(asset, 0x7c));
    put_u32(out, 0x51c, u32_at(asset, 0x80));
    put_u32(out, 0x4a0, u32_at(asset, 0x84));
    put_u32(out, 0x4a4, u32_at(asset, 0x88));
    put_u32(out, 0x4a8, u32_at(asset, 0x8c));
    put_u32(out, 0x4ac, 0);
    let tag = u16_at(asset, 0xf2);
    let tagged = if tag != 0 {
        u64::from(tag) | 0x0120_0000_0000_0000
    } else {
        0
    };
    put_u64(out, 0x530, tagged);
    put_u32(out, 0x064, 0);

    let matrix = state.and_then(|(record, state_base, block)| {
        let mem = crate::state::MemoryView::new(state_base, block);
        crate::state::record_transform(record, &mem, lookup)
    });
    match matrix {
        Some(matrix) => {
            let mut v = [
                f32_at(asset, 0x90),
                f32_at(asset, 0x94),
                f32_at(asset, 0x98),
                0.0,
            ];
            crate::movement::forward_transform(&mut v, &matrix);
            for (lane, value) in v.iter().enumerate() {
                put_f32(out, 0x490 + lane * 4, *value);
            }
        }
        None => {
            for lane in 0..3 {
                put_u32(out, 0x490 + lane * 4, u32_at(asset, 0x90 + lane * 4));
            }
            put_u32(out, 0x49c, 0);
        }
    }

    put_u8(out, 0x5c4, u8::from(pointees.gates[0] != 0));
    put_u8(out, 0x5c5, u8::from(pointees.gates[1] != 0));
    put_u8(out, 0x5c6, u8::from(pointees.gates[2] != 0));
    put_u32(out, 0x5c0, u32_at(asset, 0xf4));
    put_u32(out, 0x4b0, u32_at(asset, 0x9c));
    put_u32(out, 0x4b4, u32_at(asset, 0xa0));
    put_u32(out, 0x4b8, u32_at(asset, 0xa4));
    put_u32(out, 0x4bc, 0);
    put_u32(out, 0x5b0, u32::from(u16_at(asset, 0xf0)));
    put_u8(out, 0x5e2, u8::from(asset[0xf8] != 0));

    let count = i32_at(asset, 0x120);
    if count < 0 {
        return None;
    }
    let pointer = u64_at(out, 0x10);
    let size = i32_at(out, 0x18);
    let capacity = (i32_at(out, 0x1c)).wrapping_mul(2) >> 1;
    if count > size {
        if count > capacity {
            return None;
        }
        let start = pointer.checked_sub(base)? as usize + size.max(0) as usize * 8;
        let bytes = (count - size.max(0)) as usize * 8;
        out[start..start + bytes].fill(0);
    }
    put_i32(out, 0x18, count);
    let start = pointer.checked_sub(base)? as usize;
    for i in 0..count as usize {
        let id = u32_at(pointees.ids, i * 4);
        put_u64(out, start + i * 8, u64::from(id) | 0x0d80_0000_0000_0000);
    }

    put_u8(out, 0x5dd, u8::from(asset[0x130] != 0));
    put_u32(out, 0x5c8, u32_at(asset, 0xec));
    put_u32(out, 0x4e0, u32_at(asset, 0xa8));
    put_u32(out, 0x4e4, u32_at(asset, 0xac));
    put_u32(out, 0x4e8, u32_at(asset, 0xb0));
    put_u32(out, 0x4ec, 0);
    put_u32(out, 0x4f0, u32_at(asset, 0xb4));
    put_u32(out, 0x4f4, u32_at(asset, 0xb8));
    put_u32(out, 0x4f8, u32_at(asset, 0xbc));
    put_u32(out, 0x4fc, 0);
    put_u32(out, 0x500, u32_at(asset, 0xc0));
    put_u32(out, 0x504, u32_at(asset, 0xc4));
    let last = u32_at(asset, 0xc8);
    put_u32(out, 0x508, last);
    put_u32(out, 0x50c, 0);
    Some(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ramp_and_spring_defaults_are_pinned() {
        let mut out = [0xffu8; 0x600];
        default_settings(&mut out, 0x1000_0000, 0);
        let at = |offset: usize| f32::from_le_bytes(out[offset..offset + 4].try_into().unwrap());
        assert_eq!(at(0x5a8), 1.0);
        assert_eq!(at(0x5ac), 1.0);
        assert_eq!(at(0x5b4), 1.0);
        assert_eq!(at(0x5c0), 0.0);
        assert_eq!(out[0x5c4], 1);
        assert_eq!(out[0x5c5], 1);
        assert_eq!(out[0x5c6], 1);
        assert_eq!(out[0x5e7], 0, "the last written byte is 0x5e7");
        assert_eq!(out[0x5e8], 0xff, "the constructor stops at 0x5e8");
    }

    #[test]
    fn the_self_pointers_are_relative_to_the_object() {
        let mut out = [0u8; DEFAULT_SETTINGS_SPAN];
        default_settings(&mut out, 0x1000_0000, 0);
        let at = |offset: usize| u64::from_le_bytes(out[offset..offset + 8].try_into().unwrap());
        assert_eq!(at(0x10), 0x1000_0020);
        assert_eq!(at(0x70), 0x1000_0080);
        assert_eq!(at(0x280), 0x1000_0290);
        assert_eq!(at(0x558), 0x1000_0568);
    }
}
