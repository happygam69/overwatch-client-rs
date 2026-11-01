//! The world-handle lookups the mover uses to read and write the character
//! controller's world state, read from `Overwatch.exe`.
//!
//! The body stores a pointer at `+0x1ad8`; that slot's pointee is `V`, the
//! variant object: `[V]` is the tag (`1` = rigid, else character), `[V+8]`
//! the character handle and `[V+0x10]` the rigid handle. `0x142cbcec0` reads
//! the slot and `0x142cf5630` dispatches on the tag.
//!
//! Both leaves resolve a handle through the same global table at
//! `0x1442a0bc0`: `record = ([table[space]] - 0x4cd964772bb2a04e) ^
//! 0x77600ba5adcc265c`. The character layout is array `[record+0x2a0]`,
//! count `[record+0x2ac]`, stride `0x50`, index `+0x20`, generation `+0x34`;
//! the rigid layout is array `[record+0x288]`, count `[record+0x294]`, stride
//! `0x1c0`, index `+0x00`, generation `+0x198`. Both put the position at
//! entry `+0x10`. Every failing test returns the zero vec4.

use crate::shapes::shape_bounds;
use crate::state::MemoryView;

/// The handle space limit: `space >= 0x3fff` is rejected before the table
/// lookup.
pub const MAX_HANDLE_SPACE: u16 = 0x3fff;

/// The global table base both handle kinds resolve through.
pub const WORLD_GLOBAL: u64 = 0x1442a0bc0;

const RECORD_XOR: u64 = 0x77600ba5adcc265c;
const RECORD_SUB: u64 = 0x4cd964772bb2a04e;

#[derive(Clone, Copy)]
struct Layout {
    count: u64,
    array: u64,
    stride: u64,
    index: u64,
    generation: u64,
}

const CHARACTER: Layout = Layout {
    count: 0x2ac,
    array: 0x2a0,
    stride: 0x50,
    index: 0x20,
    generation: 0x34,
};

const RIGID: Layout = Layout {
    count: 0x294,
    array: 0x288,
    stride: 0x1c0,
    index: 0x00,
    generation: 0x198,
};

/// The three fields the handle encodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle {
    pub space: u16,
    pub generation: u16,
    pub index: u32,
}

impl Handle {
    pub fn decode(raw: u64) -> Handle {
        Handle {
            space: (raw >> 0x30) as u16,
            generation: (raw >> 0x20) as u16,
            index: raw as u32,
        }
    }
}

/// `0x142cd4ff0`: the character position, or the zero vec4 when the handle
/// does not resolve to a live entry.
pub fn character_position(raw: u64, mem: &MemoryView<'_>, global: u64) -> [f32; 4] {
    position(raw, mem, global, CHARACTER)
}

/// `0x142cb4770`: the rigid-body position, or the zero vec4.
pub fn rigid_position(raw: u64, mem: &MemoryView<'_>, global: u64) -> [f32; 4] {
    position(raw, mem, global, RIGID)
}

/// `0x142cbcec0`: read the body's world slot, then the variant it points at,
/// then dispatch on the variant tag. The slot is the qword at `body+0x1ad8`.
pub fn world_position(slot: u64, mem: &MemoryView<'_>, global: u64) -> [f32; 4] {
    if slot == 0 {
        return [0.0; 4];
    }
    let Some(variant) = mem.read_u64(slot) else {
        return [0.0; 4];
    };
    variant_position(variant, mem, global)
}

/// `0x142cf5630`: dispatch the variant (`1` = rigid, anything else =
/// character) to its position leaf.
pub fn variant_position(variant: u64, mem: &MemoryView<'_>, global: u64) -> [f32; 4] {
    if variant == 0 {
        return [0.0; 4];
    }
    let Some(tag) = mem.read_u32(variant) else {
        return [0.0; 4];
    };
    let (handle_at, layout) = if tag == 1 {
        (variant + 0x10, RIGID)
    } else {
        (variant + 0x8, CHARACTER)
    };
    let Some(raw) = mem.read_u64(handle_at) else {
        return [0.0; 4];
    };
    position(raw, mem, global, layout)
}

fn position(raw: u64, mem: &MemoryView<'_>, global: u64, layout: Layout) -> [f32; 4] {
    let Some(entry) = entry(raw, mem, global, layout) else {
        return [0.0; 4];
    };
    let Some(bytes) = mem.read_bytes(entry + 0x10, 16) else {
        return [0.0; 4];
    };
    let mut out = [0.0f32; 4];
    for (lane, value) in out.iter_mut().enumerate() {
        *value = f32::from_le_bytes(bytes[lane * 4..lane * 4 + 4].try_into().unwrap());
    }
    out
}

/// The resolved entry address, or `None` where the client returns the zero
/// vec4.
///
/// The sequence is the client's: space bound, not `0xffffffff`, non-zero
/// record, non-negative index, index below the signed count, entry index and
/// generation equality.
fn entry(raw: u64, mem: &MemoryView<'_>, global: u64, layout: Layout) -> Option<u64> {
    let handle = Handle::decode(raw);
    if handle.space >= MAX_HANDLE_SPACE {
        return None;
    }
    if handle.index == 0xffff_ffff {
        return None;
    }
    let table = mem.read_u64(global)?;
    let slot = table.checked_add(u64::from(handle.space) * 8)?;
    let encoded = mem.read_u64(slot)?;
    let record = encoded.wrapping_sub(RECORD_SUB) ^ RECORD_XOR;
    if record == 0 {
        return None;
    }
    if (handle.index as i32) < 0 {
        return None;
    }
    let count = mem.read_u32(record + layout.count)? as i32;
    if (handle.index as i32) >= count {
        return None;
    }
    let array = mem.read_u64(record + layout.array)?;
    let entry = array.wrapping_add(u64::from(handle.index) * layout.stride);
    if mem.read_u32(entry + layout.index)? != handle.index {
        return None;
    }
    if mem.read_u16(entry + layout.generation)? != handle.generation {
        return None;
    }
    Some(entry)
}

/// The public character-entry helper retained for callers that only need the
/// resolved address.
pub fn character_entry(raw: u64, mem: &MemoryView<'_>, global: u64) -> Option<u64> {
    entry(raw, mem, global, CHARACTER)
}

/// The character record and its selected entry, both resolved by the same
/// checks as the position leaf.
pub fn character_lookup(raw: u64, mem: &MemoryView<'_>, global: u64) -> Option<(u64, u64)> {
    let selected = entry(raw, mem, global, CHARACTER)?;
    let record = record_for(raw, mem, global)?;
    Some((record, selected))
}

fn record_for(raw: u64, mem: &MemoryView<'_>, global: u64) -> Option<u64> {
    let handle = Handle::decode(raw);
    let table = mem.read_u64(global)?;
    let slot = table.checked_add(u64::from(handle.space) * 8)?;
    let encoded = mem.read_u64(slot)?;
    let record = encoded.wrapping_sub(RECORD_SUB) ^ RECORD_XOR;
    (record != 0).then_some(record)
}

fn f32_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn put_f32(bytes: &mut [u8], offset: usize, value: f32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// `MoveCharacterController` (`0x142cd4bc0`), the no-shape path.
///
/// The client normalises the entry's quaternion by
/// `1 / sqrt((q0^2 + q1^2) + (q2^2 + q3^2))`, writes the normalised quat to
/// entry `+0x00`, copies the new velocity to `+0x10` and sets the record's
/// `+0x190` flag. If the new velocity's xyz already equals the entry's
/// current velocity, the client returns without a write when the entry quat
/// is unchanged (or already normalised).
///
/// The shape-query loop (`entry+0x24` shape index, record `+0x2b8` array) is
/// **open**: this function returns `false` for a non-`-1` index before any
/// write, rather than guessing the query.
pub fn move_character_controller(
    entry: &mut [u8],
    record: &mut [u8],
    new_velocity: [f32; 4],
    old_quat: [f32; 4],
) -> bool {
    if i32::from_le_bytes(entry[0x24..0x28].try_into().unwrap()) != -1 {
        return false;
    }
    if !controller_writes(entry, new_velocity, old_quat) {
        return true;
    }
    record[0x190] = 1;
    true
}

/// The entry write shared by the no-shape and shape paths. Returns `true`
/// when the quaternion/velocity were written, `false` on the unchanged
/// early-out.
fn controller_writes(entry: &mut [u8], new_velocity: [f32; 4], old_quat: [f32; 4]) -> bool {
    let current = [
        f32_at(entry, 0x10),
        f32_at(entry, 0x14),
        f32_at(entry, 0x18),
        f32_at(entry, 0x1c),
    ];
    let same_velocity = new_velocity[0] == current[0]
        && new_velocity[1] == current[1]
        && new_velocity[2] == current[2];

    let q0 = old_quat[0];
    let q1 = old_quat[1];
    let q2 = old_quat[2];
    let q3 = old_quat[3];
    let sq = (q0 * q0 + q1 * q1) + (q2 * q2 + q3 * q3);
    let inv = 1.0f32 / sq.sqrt();
    let normalized = [q0 * inv, q1 * inv, q2 * inv, q3 * inv];

    if same_velocity {
        let entry_quat = [
            f32_at(entry, 0x00),
            f32_at(entry, 0x04),
            f32_at(entry, 0x08),
            f32_at(entry, 0x0c),
        ];
        let quat_equal =
            |a: [f32; 4], b: [f32; 4]| a[0] == b[0] && a[1] == b[1] && a[2] == b[2] && a[3] == b[3];
        if quat_equal(entry_quat, old_quat) || quat_equal(entry_quat, normalized) {
            return false;
        }
    }

    for lane in 0..4 {
        put_f32(entry, lane * 4, normalized[lane]);
        put_f32(entry, 0x10 + lane * 4, new_velocity[lane]);
    }
    true
}

fn quat_matrix(q: [f32; 4], translation: [f32; 4]) -> [f32; 16] {
    let x = q[0];
    let y = q[1];
    let z = q[2];
    let w = q[3];
    let xx = (x + x) * x;
    let yy = (y + y) * y;
    let zz = (z + z) * z;
    [
        (1.0 - yy) - zz,
        (z + z) * w + (y + y) * x,
        (z + z) * x - (y + y) * w,
        0.0,
        (y + y) * x - (z + z) * w,
        (1.0 - xx) - zz,
        (x + x) * w + (z + z) * y,
        0.0,
        (y + y) * w + (z + z) * x,
        (z + z) * y - (x + x) * w,
        (1.0 - xx) - yy,
        0.0,
        translation[0],
        translation[1],
        translation[2],
        translation[3],
    ]
}

/// `MoveCharacterController` (`0x142cd4bc0`) with the shape-query loop.
///
/// `record` must cover the record fields and the shape arrays it points at
/// (`+0x2b8`, `+0x2d0`, `+0x2e8`, `+0x300`, `+0x330`); `record_addr` is the
/// absolute address of `record[0]`. The contact event (`0x142cf32d0`) is
/// open: a shape with `+0xa4 != -1` returns `false` after its bounds are
/// written, rather than guessing the event.
pub fn move_character_controller_world(
    entry: &mut [u8],
    record: &mut [u8],
    record_addr: u64,
    new_velocity: [f32; 4],
    old_quat: [f32; 4],
) -> bool {
    if entry.len() < 0x28 || record.len() < 0x338 {
        return false;
    }
    let shape_index = i32::from_le_bytes(entry[0x24..0x28].try_into().unwrap());
    if shape_index == -1 {
        if controller_writes(entry, new_velocity, old_quat) {
            record[0x190] = 1;
        }
        return true;
    }
    if !controller_writes(entry, new_velocity, old_quat) {
        return true;
    }
    let matrix = quat_matrix(old_quat, new_velocity);
    let mut index = shape_index;
    while index != -1 {
        let list = u64::from_le_bytes(record[0x2b8..0x2c0].try_into().unwrap());
        let Some(list_off) = list.checked_sub(record_addr).map(|v| v as usize) else {
            return false;
        };
        let shape_off = list_off + (index as usize) * 0xc0;
        if shape_off + 0xb0 > record.len() {
            return false;
        }
        let next = i32::from_le_bytes(
            record[shape_off + 0x54..shape_off + 0x58]
                .try_into()
                .unwrap(),
        );
        let contact = i32::from_le_bytes(
            record[shape_off + 0xa4..shape_off + 0xa8]
                .try_into()
                .unwrap(),
        );
        let mut bounds_record = record[shape_off..shape_off + 0x28].to_vec();
        if !shape_bounds(&mut bounds_record, record, record_addr, &matrix) {
            return false;
        }
        record[shape_off..shape_off + 0x20].copy_from_slice(&bounds_record[..0x20]);
        if contact != -1 {
            // 0x142cf32d0 is the contact event; it is open.
            return false;
        }
        index = next;
    }
    record[0x190] = 1;
    true
}

/// `ApplyCharacterControllerMove` (`0x142cd5370`), the no-shape path.
///
/// The handle resolves through the character table; xyz must not have an
/// all-ones exponent (inf or NaN), and record `+0x15c` must be zero. The
/// entry quaternion is passed as the old quat. Returns `false` where the
/// client would decline or where the shape path is open.
pub fn apply_character_controller_move(
    raw: u64,
    velocity: [f32; 4],
    state: &mut [u8],
    base: u64,
    global: u64,
) -> bool {
    let lookup = {
        let mem = MemoryView::new(base, state);
        character_lookup(raw, &mem, global)
    };
    let Some((record, selected)) = lookup else {
        return false;
    };
    for lane in 0..3 {
        if velocity[lane].to_bits() & 0x7f80_0000 == 0x7f80_0000 {
            return false;
        }
    }
    let Some(record_off) = record.checked_sub(base).map(|v| v as usize) else {
        return false;
    };
    let Some(entry_off) = selected.checked_sub(base).map(|v| v as usize) else {
        return false;
    };
    if record_off + 0x194 > state.len() || entry_off + 0x28 > state.len() {
        return false;
    }
    if i32::from_le_bytes(
        state[record_off + 0x15c..record_off + 0x160]
            .try_into()
            .unwrap(),
    ) != 0
    {
        return false;
    }
    let old_quat = [
        f32_at(state, entry_off),
        f32_at(state, entry_off + 4),
        f32_at(state, entry_off + 8),
        f32_at(state, entry_off + 12),
    ];
    let mut entry_buf = state[entry_off..entry_off + 0x28].to_vec();
    let mut record_buf = state[record_off..record_off + 0x194].to_vec();
    let applied = move_character_controller(&mut entry_buf, &mut record_buf, velocity, old_quat);
    if applied {
        state[entry_off..entry_off + 0x28].copy_from_slice(&entry_buf);
        state[record_off..record_off + 0x194].copy_from_slice(&record_buf);
    }
    applied
}

/// `FUN_142cf5810` + `SetCharacterControllerVelocity` (`0x142cbd1b0`) for the
/// character variant.
///
/// The slot's pointee is the variant; tag `1` selects the rigid path, which
/// is **open** and returns `false`. Any other tag uses `[variant+8]` as the
/// character handle.
pub fn set_character_controller_velocity(
    slot: u64,
    velocity: [f32; 4],
    state: &mut [u8],
    base: u64,
    global: u64,
) -> bool {
    if slot == 0 {
        return false;
    }
    let raw = {
        let mem = MemoryView::new(base, state);
        let Some(variant) = mem.read_u64(slot) else {
            return false;
        };
        if variant == 0 {
            return false;
        }
        let Some(tag) = mem.read_u32(variant) else {
            return false;
        };
        if tag == 1 {
            // The rigid path (0x142cb5310) is a separate open stage.
            return false;
        }
        let Some(raw) = mem.read_u64(variant + 8) else {
            return false;
        };
        raw
    };
    apply_character_controller_move(raw, velocity, state, base, global)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u64 = WORLD_GLOBAL;
    const TABLE: u64 = BASE + 0x40;
    const RECORD: u64 = BASE + 0x80;
    const ARRAY: u64 = BASE + 0x200;

    fn state() -> Vec<u8> {
        let mut b = vec![0u8; 0x800];
        let put = |b: &mut Vec<u8>, addr: u64, bytes: &[u8]| {
            let off = (addr - BASE) as usize;
            b[off..off + bytes.len()].copy_from_slice(bytes);
        };
        put(&mut b, BASE, &TABLE.to_le_bytes());
        put(
            &mut b,
            TABLE,
            &((RECORD ^ RECORD_XOR).wrapping_add(RECORD_SUB)).to_le_bytes(),
        );
        put(&mut b, RECORD + 0x2a0, &ARRAY.to_le_bytes());
        put(&mut b, RECORD + 0x2ac, &2i32.to_le_bytes());
        put(&mut b, ARRAY + 0x20, &0u32.to_le_bytes());
        put(&mut b, ARRAY + 0x34, &7u16.to_le_bytes());
        put(
            &mut b,
            ARRAY + 0x10,
            &[
                1.0f32.to_le_bytes(),
                2.0f32.to_le_bytes(),
                3.0f32.to_le_bytes(),
                4.0f32.to_le_bytes(),
            ]
            .concat(),
        );
        put(&mut b, ARRAY + 0x50 + 0x20, &1u32.to_le_bytes());
        put(&mut b, ARRAY + 0x50 + 0x34, &7u16.to_le_bytes());
        b
    }

    fn handle(space: u16, generation: u16, index: u32) -> u64 {
        (u64::from(space) << 0x30) | (u64::from(generation) << 0x20) | u64::from(index)
    }

    #[test]
    fn a_live_handle_returns_the_position() {
        let state = state();
        let mem = MemoryView::new(BASE, &state);
        assert_eq!(
            character_position(handle(0, 7, 0), &mem, BASE),
            [1.0, 2.0, 3.0, 4.0]
        );
    }

    #[test]
    fn the_controller_move_normalises_and_publishes() {
        let mut entry = vec![0u8; 0x28];
        for (lane, value) in [0.0f32, 0.0, 0.0, 2.0].iter().enumerate() {
            entry[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        entry[0x24..0x28].copy_from_slice(&(-1i32).to_le_bytes());
        let mut record = vec![0u8; 0x194];
        assert!(move_character_controller(
            &mut entry,
            &mut record,
            [1.0, 2.0, 3.0, 4.0],
            [0.0, 0.0, 0.0, 2.0],
        ));
        assert_eq!(f32_at(&entry, 0x0c).to_bits(), 1.0f32.to_bits());
        assert_eq!(f32_at(&entry, 0x10), 1.0);
        assert_eq!(record[0x190], 1);
    }

    #[test]
    fn the_controller_move_keeps_an_unchanged_velocity() {
        let mut entry = vec![0u8; 0x28];
        for lane in 0..4 {
            let current = [1.0f32, 2.0, 3.0, 4.0][lane];
            entry[0x10 + lane * 4..0x14 + lane * 4].copy_from_slice(&current.to_le_bytes());
            let quat = [0.0f32, 0.0, 0.0, 1.0][lane];
            entry[lane * 4..lane * 4 + 4].copy_from_slice(&quat.to_le_bytes());
        }
        entry[0x24..0x28].copy_from_slice(&(-1i32).to_le_bytes());
        let mut record = vec![0u8; 0x194];
        assert!(move_character_controller(
            &mut entry,
            &mut record,
            [1.0, 2.0, 3.0, 9.0],
            [0.0, 0.0, 0.0, 1.0],
        ));
        assert_eq!(record[0x190], 0);
        assert_eq!(entry[0x10..0x14], 1.0f32.to_le_bytes());
    }

    #[test]
    fn each_client_test_falls_back_to_zero() {
        let state = state();
        let mem = MemoryView::new(BASE, &state);
        assert_eq!(
            character_position(handle(0x3fff, 7, 0), &mem, BASE),
            [0.0; 4]
        );
        assert_eq!(
            character_position(handle(0, 7, 0xffff_ffff), &mem, BASE),
            [0.0; 4]
        );
        assert_eq!(character_position(handle(0, 7, 5), &mem, BASE), [0.0; 4]);
        assert_eq!(character_position(handle(0, 8, 0), &mem, BASE), [0.0; 4]);
        assert_eq!(character_position(handle(1, 7, 0), &mem, BASE), [0.0; 4]);
    }
}
