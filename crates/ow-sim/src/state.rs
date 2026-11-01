//! The mover's state-record transform lookup, read from `Overwatch.exe`.
//!
//! `FUN_140cb83b0` gates the state id and delegates to `FUN_140c71e10`, which
//! resolves a dense-table entry to a transform object and copies its 4x4
//! matrix. The record layout and the entry decode were read on 2026-11-01:
//! table pointer `+0x80`, presence bitmap `+0x110`, group base `+0x130`, id
//! `+0x138`; the entry decodes through the leaf at `0x140543480` and
//! `ROL64(x, 0x2b)` gives the object. The object's matrix is at `+0x60` and
//! `+0x298` must be zero for this leaf path.
//!
//! The parent/pose paths (`[object+0x298] != 0`, `FUN_140c71e10`'s
//! `param_3 != 0`) and the second key-4 lookup are not read, so this
//! implementation returns `false` on those inputs rather than guessing.

/// A flat window over the client's state block.
pub struct MemoryView<'a> {
    base: u64,
    bytes: &'a [u8],
}

impl<'a> MemoryView<'a> {
    pub fn new(base: u64, bytes: &'a [u8]) -> Self {
        Self { base, bytes }
    }

    fn slice(&self, addr: u64, len: usize) -> Option<&'a [u8]> {
        let off = addr.checked_sub(self.base)? as usize;
        let end = off.checked_add(len)?;
        self.bytes.get(off..end)
    }

    pub fn read_u8(&self, addr: u64) -> Option<u8> {
        self.slice(addr, 1).map(|b| b[0])
    }

    pub fn read_u16(&self, addr: u64) -> Option<u16> {
        self.slice(addr, 2)
            .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
    }

    pub fn read_u32(&self, addr: u64) -> Option<u32> {
        self.slice(addr, 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    }

    pub fn read_u64(&self, addr: u64) -> Option<u64> {
        self.slice(addr, 8)
            .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
    }

    pub fn read_bytes(&self, addr: u64, len: usize) -> Option<&'a [u8]> {
        self.slice(addr, len)
    }

    fn read_matrix(&self, addr: u64) -> Option<[f32; 16]> {
        let b = self.slice(addr, 64)?;
        let mut m = [0.0f32; 16];
        for (i, value) in m.iter_mut().enumerate() {
            *value = f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
        }
        Some(m)
    }
}

/// The runtime constants the decode reads: `Q` (the `[0x143b79598]+0x198`
/// qword), the build byte at `0x1438480f3`, and the state-id sentinel global
/// at `0x1441880e8`.
#[derive(Clone, Copy)]
pub struct StateLookup {
    pub q: u64,
    pub decode_byte: u8,
    pub sentinel_id: u32,
}

/// The leaf of `FUN_140cb83b0` / `FUN_140c71e10`: resolve `state_id` through
/// the state record at `record` and copy the 4x4 matrix into `out`.
///
/// Returns `false` where the client declines (sentinel id) or where the read
/// path is still open (null/mismatched handle, invalid bitmap, non-zero
/// parent at `+0x298`, missing memory).
pub fn state_transform(
    out: &mut [f32; 16],
    record: u64,
    state_id: u32,
    handle: u64,
    mem: &MemoryView<'_>,
    lookup: StateLookup,
) -> bool {
    if state_id == lookup.sentinel_id || (state_id & 0x1fff_ffff) == 0x1fff_ffff {
        return false;
    }
    if handle == 0 || mem.read_u32(handle + 0x138) != Some(state_id) {
        // The client's mismatch path is a global registry lookup guarded by
        // a TLS generation check; it is not the leaf path and is still open.
        return false;
    }

    let Some(matrix) = record_transform(record, mem, lookup) else {
        return false;
    };
    out.copy_from_slice(&matrix);
    true
}

/// The record's table walk without the state-id gate: `FUN_140c71e10`'s
/// bitmap/rank/table index, the `0x140543480` decode and the object matrix
/// copy. `0x140c862e0` calls the same walk after the fixed setter at
/// `0x142001f80` (index 0, mask bit 1, shift 1).
///
/// Returns `None` where the client would take the parent path
/// (`[object+0x298] != 0`), where the presence bit is clear, or where memory
/// is missing.
pub fn record_transform(
    record: u64,
    mem: &MemoryView<'_>,
    lookup: StateLookup,
) -> Option<[f32; 16]> {
    let word = mem.read_u64(record + 0x110)?;
    if word & 2 == 0 {
        return None;
    }
    let rank = u64::from((word & 1).count_ones());
    let base = mem.read_u8(record + 0x130)?;
    let table = mem.read_u64(record + 0x80)?;
    let slot = table.wrapping_add((u64::from(base).wrapping_add(rank)).wrapping_mul(8));
    let entry = mem.read_u64(slot)?;

    let object = decode_entry(entry, lookup.q, lookup.decode_byte);
    if mem.read_u64(object + 0x298) != Some(0) {
        return None;
    }
    mem.read_matrix(object + 0x60)
}

/// `FUN_140c71e10`'s table-entry decode for the leaf path.
fn decode_entry(entry: u64, q: u64, decode_byte: u8) -> u64 {
    const A: u64 = 0x4d79_452d_486f_9431;
    const C: u64 = 0xec99_e783_d64e_4c83;
    const D: u64 = 0x327a_ecc3_f519_246f;
    const H1: u64 = 0x8b08_4bd9_71b9_46cc;
    const H2: u64 = 0x32c1_c6a5_a278_ca6f;

    let mut x = entry.rotate_right(0x38);
    x = x.wrapping_add(A);
    x ^= q;
    x ^= C;
    x = x.wrapping_sub(D);
    x ^= u64::from(decode_byte);
    // The leaf at 0x140543480.
    x = x.rotate_right(0x14);
    x ^= H1;
    x = x.wrapping_sub(H2);
    x = x.rotate_right(0x26);
    x.rotate_left(0x2b)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u64 = 0x1000;
    const RECORD: u64 = 0x1100;
    const TABLE: u64 = 0x1200;
    const OBJECT: u64 = 0x1400;

    fn block() -> Vec<u8> {
        let mut b = vec![0u8; 0x700];
        let put = |b: &mut Vec<u8>, addr: u64, bytes: &[u8]| {
            let off = (addr - BASE) as usize;
            b[off..off + bytes.len()].copy_from_slice(bytes);
        };
        put(&mut b, RECORD + 0x80, &TABLE.to_le_bytes());
        put(&mut b, RECORD + 0x110, &2u64.to_le_bytes());
        put(&mut b, RECORD + 0x130, &[0]);
        put(&mut b, RECORD + 0x138, &1u32.to_le_bytes());
        let entry = 0x0000_0000_0000_0000u64; // replaced below
        put(&mut b, TABLE, &entry.to_le_bytes());
        let mut m = [0.0f32; 16];
        for (i, v) in m.iter_mut().enumerate() {
            *v = (i + 1) as f32;
        }
        let mut bytes = [0u8; 64];
        for (i, v) in m.iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        put(&mut b, OBJECT + 0x60, &bytes);
        b
    }

    #[test]
    fn the_leaf_decodes_the_table_entry() {
        let mut b = block();
        // encode_entry(OBJECT) with Q = 0 and the 0x42 decode byte.
        let off = (TABLE - BASE) as usize;
        b[off..off + 8].copy_from_slice(&0x9999_588a_31be_aaf9u64.to_le_bytes());
        let mem = MemoryView::new(BASE, &b);
        let mut out = [9.0f32; 16];
        assert!(state_transform(
            &mut out,
            RECORD,
            1,
            RECORD,
            &mem,
            StateLookup {
                q: 0,
                decode_byte: 0x42,
                sentinel_id: 0,
            },
        ));
        assert_eq!(out, std::array::from_fn(|i| (i + 1) as f32));
    }

    #[test]
    fn the_sentinel_declines_without_writing() {
        let b = block();
        let mem = MemoryView::new(BASE, &b);
        let mut out = [9.0f32; 16];
        assert!(!state_transform(
            &mut out,
            RECORD,
            0,
            RECORD,
            &mem,
            StateLookup {
                q: 0,
                decode_byte: 0x42,
                sentinel_id: 0,
            },
        ));
        assert_eq!(out, [9.0; 16]);
    }
}
