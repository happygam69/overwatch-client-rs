//! The shape-bound leaves the controller's move queries, read from
//! `Overwatch.exe`.
//!
//! `0x142cb9030` dispatches on a shape record's type byte and copies the
//! 0x20-byte result. The type-0 sphere (`0x142ccd410`), type-1 capsule
//! (`0x142ccd380`), type-2 point cloud (`0x142cd3720`) and type-4 box
//! (`0x142ccf460`) leaves are here. Type 3 (`0x142cae5a0`) is still open.
//!
//! All transforms use the controller's column-major matrix:
//! `v' = x*m[0..4] + y*m[4..8] + z*m[8..12] + m[12..16]`. The additive order
//! is the client's, not a re-associated one: the type-0/1/4 leaves add the
//! translation after the x term and before the y and z terms, while the
//! type-2 loop accumulates the linear part only and adds the translation
//! after the min/max fold.

use crate::movement::{maxss, minss};

fn f32_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn point3(record: &[u8], offset: usize) -> [f32; 3] {
    [
        f32_at(record, offset),
        f32_at(record, offset + 4),
        f32_at(record, offset + 8),
    ]
}

fn transform_with_translation(p: [f32; 3], matrix: &[f32; 16]) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    for (lane, value) in out.iter_mut().enumerate() {
        let x = p[0] * matrix[lane];
        let t = x + matrix[12 + lane];
        let y = t + p[1] * matrix[4 + lane];
        *value = y + p[2] * matrix[8 + lane];
    }
    out
}

fn transform_linear(p: [f32; 3], matrix: &[f32; 16]) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    for (lane, value) in out.iter_mut().enumerate() {
        let y = p[1] * matrix[4 + lane];
        let x = y + p[0] * matrix[lane];
        *value = x + p[2] * matrix[8 + lane];
    }
    out
}

/// `0x142ccd410`: type 0, a sphere (`center +0x00`, `radius +0x10`). The
/// output is the lower and upper bounds, eight floats.
pub fn sphere_bounds(record: &[u8], matrix: &[f32; 16]) -> [f32; 8] {
    let center = transform_with_translation(point3(record, 0), matrix);
    let radius = f32_at(record, 0x10);
    let mut out = [0.0f32; 8];
    for lane in 0..4 {
        out[lane] = center[lane] - radius;
        out[4 + lane] = center[lane] + radius;
    }
    out
}

/// `0x142ccd380`: type 1, a capsule (`point0 +0x00`, `point1 +0x10`,
/// `radius +0x20`). The output is `min(p0,p1)-r` then `max(p0,p1)+r`.
pub fn capsule_bounds(record: &[u8], matrix: &[f32; 16]) -> [f32; 8] {
    let a = transform_with_translation(point3(record, 0), matrix);
    let b = transform_with_translation(point3(record, 0x10), matrix);
    let radius = f32_at(record, 0x20);
    let mut out = [0.0f32; 8];
    for lane in 0..4 {
        out[lane] = minss(a[lane], b[lane]) - radius;
        out[4 + lane] = maxss(a[lane], b[lane]) + radius;
    }
    out
}

/// `0x142ccf460`: type 4, a box (`point0 +0x10`, `point1 +0x20`). The output
/// is the min and max, eight floats, with no expansion.
pub fn box_bounds(record: &[u8], matrix: &[f32; 16]) -> [f32; 8] {
    let a = transform_with_translation(point3(record, 0x10), matrix);
    let b = transform_with_translation(point3(record, 0x20), matrix);
    let mut out = [0.0f32; 8];
    for lane in 0..4 {
        out[lane] = minss(a[lane], b[lane]);
        out[4 + lane] = maxss(a[lane], b[lane]);
    }
    out
}

/// `0x142cd3720`: type 2, a point cloud (`points +0x10`, `count +0x30`).
/// Each point is three floats at stride 0x10. The linear part is folded into
/// min/max from `+FLT_MAX` / `-FLT_MAX`, expanded by 0.01 and then translated:
/// `lower = (min - 0.01) + t`, `upper = (t + max) + 0.01`.
pub fn point_cloud_bounds(record: &[u8], points: &[u8], matrix: &[f32; 16]) -> [f32; 8] {
    let count = i32::from_le_bytes(record[0x30..0x34].try_into().unwrap());
    let mut lo = [f32::from_bits(0x7f7f_ffff); 4];
    let mut hi = [f32::from_bits(0xff7f_ffff); 4];
    if count > 0 {
        for i in 0..count as usize {
            let p = point3(points, i * 0x10);
            let v = transform_linear(p, matrix);
            for lane in 0..4 {
                lo[lane] = minss(lo[lane], v[lane]);
                hi[lane] = maxss(hi[lane], v[lane]);
            }
        }
    }
    let mut out = [0.0f32; 8];
    for lane in 0..4 {
        let t = matrix[12 + lane];
        out[lane] = (lo[lane] - 0.01) + t;
        out[4 + lane] = (t + hi[lane]) + 0.01;
    }
    out
}

/// `0x142cb9030`: select a shape record from the world array named by the
/// type byte at `record+0x20`, compute its bounds, and copy the eight floats
/// into `record+0x00..0x20`.
///
/// Types: 0 sphere (`world[0x2d0]`, stride 0x20), 1 capsule (`world[0x2e8]`,
/// stride 0x30), 2 point cloud (`world[0x300]`, stride 0x50), 3 mesh (open),
/// 4 box (`world[0x330]`, stride 0x50). Any other type follows the client's
/// final box path.
pub fn shape_bounds(record: &mut [u8], world: &[u8], base: u64, matrix: &[f32; 16]) -> bool {
    if record.len() < 0x28 || world.len() < 0x338 {
        return false;
    }
    let kind = record[0x20] as i8;
    let index = i32::from_le_bytes(record[0x24..0x28].try_into().unwrap()) as i64;
    let (slot, stride, bounds) = match kind {
        0 => (0x2d0usize, 0x20i64, 0),
        1 => (0x2e8, 0x30, 1),
        2 => (0x300, 0x50, 2),
        3 => return false,
        _ => (0x330, 0x50, 4),
    };
    let array = u64::from_le_bytes(world[slot..slot + 8].try_into().unwrap());
    let selected = array.wrapping_add((index as u64).wrapping_mul(stride as u64));
    let Some(offset) = selected.checked_sub(base).map(|v| v as usize) else {
        return false;
    };
    let out = match bounds {
        0 => {
            if offset + 0x20 > world.len() {
                return false;
            }
            sphere_bounds(&world[offset..offset + 0x20], matrix)
        }
        1 => {
            if offset + 0x24 > world.len() {
                return false;
            }
            capsule_bounds(&world[offset..offset + 0x24], matrix)
        }
        2 => {
            if offset + 0x34 > world.len() {
                return false;
            }
            let count = i32::from_le_bytes(world[offset + 0x30..offset + 0x34].try_into().unwrap());
            let need = (count.max(0) as usize).saturating_mul(0x10);
            let points: &[u8] = if need == 0 {
                &[]
            } else {
                let points =
                    u64::from_le_bytes(world[offset + 0x10..offset + 0x18].try_into().unwrap());
                let Some(point_offset) = points.checked_sub(base).map(|v| v as usize) else {
                    return false;
                };
                if point_offset + need > world.len() {
                    return false;
                }
                &world[point_offset..point_offset + need]
            };
            point_cloud_bounds(&world[offset..offset + 0x34], points, matrix)
        }
        _ => {
            if offset + 0x30 > world.len() {
                return false;
            }
            box_bounds(&world[offset..offset + 0x30], matrix)
        }
    };
    for (lane, value) in out.iter().enumerate() {
        record[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identity_sphere_bounds_are_its_radius() {
        let mut record = [0.0f32; 5];
        record[0] = 1.0;
        record[1] = 2.0;
        record[2] = 3.0;
        record[4] = 0.5;
        let record: Vec<u8> = record.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut matrix = [0.0f32; 16];
        matrix[0] = 1.0;
        matrix[5] = 1.0;
        matrix[10] = 1.0;
        matrix[15] = 1.0;
        let out = sphere_bounds(&record, &matrix);
        assert_eq!(&out[0..4], &[0.5, 1.5, 2.5, 0.5]);
        assert_eq!(&out[4..8], &[1.5, 2.5, 3.5, 1.5]);
    }
}
