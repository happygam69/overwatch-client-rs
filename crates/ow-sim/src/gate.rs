//! The mover's movement gate, read from `Overwatch.exe`.
//!
//! `0x140ca7d80(body)`: false when `body+0x1a78` or `body+0x658` is zero.
//! Otherwise it walks the 0x30-byte records at `body+0x690` (count
//! `body+0x698`), keeps those whose window `[+0x1c, +0x20)` contains the
//! frame id `body+0xe4`, calls the record object's `vtable+0x78` for each and
//! ANDs the results.
//!
//! The virtual results are the record object's business (a gameplay-state
//! predicate); this module takes them as `results`, one byte per record, in
//! array order.

/// `0x140ca7d80`: whether the mover may move this frame.
pub fn movement_gate(body: &[u8], results: &[u8], base: u64) -> bool {
    if body[0x1a78] == 0 {
        return false;
    }
    if body[0x658] == 0 {
        return false;
    }
    let count = i32::from_le_bytes(body[0x698..0x69c].try_into().unwrap());
    if count <= 0 {
        return true;
    }
    let id = u32::from_le_bytes(body[0xe4..0xe8].try_into().unwrap());
    let array = u64::from_le_bytes(body[0x690..0x698].try_into().unwrap());
    let Some(offset) = array.checked_sub(base) else {
        return true;
    };
    let offset = offset as usize;
    let mut result = true;
    for i in 0..count as usize {
        let record = offset + i * 0x30;
        let Some(entry) = body.get(record..record + 0x24) else {
            break;
        };
        let lo = u32::from_le_bytes(entry[0x1c..0x20].try_into().unwrap());
        let hi = u32::from_le_bytes(entry[0x20..0x24].try_into().unwrap());
        if id >= lo && id < hi {
            result &= results.get(i).copied().unwrap_or(0) != 0;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> Vec<u8> {
        let mut body = vec![0u8; 0x1c00];
        body[0x1a78] = 1;
        body[0x658] = 1;
        body
    }

    #[test]
    fn the_gate_is_closed_when_either_flag_is_zero() {
        let mut b = body();
        b[0x1a78] = 0;
        assert!(!movement_gate(&b, &[], 0));
        let mut b = body();
        b[0x658] = 0;
        assert!(!movement_gate(&b, &[], 0));
    }

    #[test]
    fn an_empty_record_array_is_open() {
        let b = body();
        assert!(movement_gate(&b, &[], 0));
    }

    #[test]
    fn only_matching_records_are_anded() {
        let mut b = body();
        b[0xe4..0xe8].copy_from_slice(&3u32.to_le_bytes());
        b[0x698..0x69c].copy_from_slice(&2i32.to_le_bytes());
        b[0x690..0x698].copy_from_slice(&0x1000u64.to_le_bytes());
        // record 0 matches and is false; record 1 is out of window.
        b[0x1000 + 0x1c..0x1000 + 0x20].copy_from_slice(&0u32.to_le_bytes());
        b[0x1000 + 0x20..0x1000 + 0x24].copy_from_slice(&4u32.to_le_bytes());
        b[0x1030 + 0x1c..0x1030 + 0x20].copy_from_slice(&5u32.to_le_bytes());
        b[0x1030 + 0x20..0x1030 + 0x24].copy_from_slice(&6u32.to_le_bytes());
        assert!(!movement_gate(&b, &[0, 1], 0));
        assert!(movement_gate(&b, &[1, 1], 0));
    }
}
