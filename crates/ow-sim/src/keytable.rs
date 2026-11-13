//! The client's keyed record search, at `0x141b331f0`.
//!
//! A flat array of `0x30`-byte records and a `0x28`-byte key. The body reads
//! five `u64` fields on each side and returns the index of a match, or `-1`:
//!
//! ```text
//! r11 = (key[8] != 0 && key[0] == 0)          ; "the key is partial"
//! best = -1
//! for i in 0..count:
//!     if rec[0x10] != key[0x10]: continue
//!     if rec[0x18] != key[0x18]: continue
//!     if require_extra && rec[0x20] != key[0x20]: continue
//!     if rec[8] != 0 && rec[0] == 0: r11 = true      ; sticky across records
//!     if key[8] == rec[8]:
//!         if rec[0] == key[0]: return i              ; exact
//!         if r11: best = i                           ; partial, last wins
//! return best
//! ```
//!
//! Three details are the whole function and each is easy to get wrong:
//!
//! 1. `r11` is **sticky**: once a record with a non-zero `+8` and a zero `+0`
//!    has been seen, every later record with the same `+8` becomes a candidate.
//! 2. The candidate index **overwrites**: the last partial match wins, not the
//!    first.
//! 3. The `+0x20` comparison is gated by the fourth argument (`r9b`), which the
//!    one caller in the image passes as `0`.
//!
//! The reads are unconditional in the client, so a slice shorter than
//! `count * 0x30` panics here rather than returning a made-up value: a short
//! slice is a fixture error, not an input the function has an answer for.

/// One record stride.
pub const STRIDE: usize = 0x30;

/// The five `u64` fields the search compares, in record/key order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Key {
    pub zero: u64,
    pub eight: u64,
    pub ten: u64,
    pub eighteen: u64,
    pub twenty: u64,
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

/// The key's fields, read the way the client reads them.
pub fn key_of(bytes: &[u8]) -> Key {
    Key {
        zero: u64_at(bytes, 0x00),
        eight: u64_at(bytes, 0x08),
        ten: u64_at(bytes, 0x10),
        eighteen: u64_at(bytes, 0x18),
        twenty: u64_at(bytes, 0x20),
    }
}

/// `find_record_by_key`: the index of the match, or `-1`.
///
/// `array` is the record block, `count` the number of records, `require_extra`
/// the caller's `r9b`.
pub fn find(array: &[u8], count: i32, key: &Key, require_extra: bool) -> i32 {
    let mut partial = key.eight != 0 && key.zero == 0;
    let mut best: i32 = -1;
    if count <= 0 {
        return best;
    }
    for index in 0..count as usize {
        let base = index * STRIDE;
        if u64_at(array, base + 0x10) != key.ten {
            continue;
        }
        if u64_at(array, base + 0x18) != key.eighteen {
            continue;
        }
        if require_extra && u64_at(array, base + 0x20) != key.twenty {
            continue;
        }
        let eight = u64_at(array, base + 0x08);
        if eight != 0 && u64_at(array, base) == 0 {
            partial = true;
        }
        if key.eight == eight {
            if u64_at(array, base) == key.zero {
                return index as i32;
            }
            if partial {
                best = index as i32;
            }
        }
    }
    best
}

/// `0x141b330c0`: the search's index as a bit mask.
///
/// The client forwards its own four arguments to the search, then does
/// `eax = 1; shl rax, cl` and `if ecx == -1 then rax = 0`. The shift
/// therefore uses `cl` - the low six bits of the index - which only matters
/// when a table has more than 64 records.
pub fn find_mask(array: &[u8], count: i32, key: &Key, require_extra: bool) -> u64 {
    let index = find(array, count, key, require_extra);
    if index == -1 {
        0
    } else {
        1u64 << ((index as u32) & 63)
    }
}

#[cfg(test)]
mod tests {
    use super::{find, key_of, Key, STRIDE};

    /// One record: (zero, eight, ten, eighteen, twenty).
    fn record(r: [u64; 5]) -> [u8; STRIDE] {
        let mut out = [0u8; STRIDE];
        for (slot, value) in [
            (0x00, r[0]),
            (0x08, r[1]),
            (0x10, r[2]),
            (0x18, r[3]),
            (0x20, r[4]),
        ] {
            out[slot..slot + 8].copy_from_slice(&value.to_le_bytes());
        }
        out
    }

    fn array(records: &[[u64; 5]]) -> Vec<u8> {
        let mut out = Vec::new();
        for r in records {
            out.extend_from_slice(&record(*r));
        }
        out
    }

    fn key(k: [u64; 5]) -> Key {
        let mut bytes = [0u8; 0x28];
        for (slot, value) in [
            (0x00, k[0]),
            (0x08, k[1]),
            (0x10, k[2]),
            (0x18, k[3]),
            (0x20, k[4]),
        ] {
            bytes[slot..slot + 8].copy_from_slice(&value.to_le_bytes());
        }
        key_of(&bytes)
    }

    #[test]
    fn an_exact_match_returns_its_index() {
        let a = array(&[[9, 1, 2, 3, 4], [8, 1, 2, 3, 4]]);
        assert_eq!(find(&a, 2, &key([9, 1, 2, 3, 4]), false), 0);
        assert_eq!(find(&a, 2, &key([8, 1, 2, 3, 4]), false), 1);
    }

    #[test]
    fn no_match_is_minus_one() {
        let a = array(&[[9, 1, 2, 3, 4]]);
        // only the *compared* fields can reject a record: +0x20 is not
        // compared unless the flag is set, so a key differing there matches
        assert_eq!(find(&a, 1, &key([9, 1, 7, 3, 4]), false), -1);
        assert_eq!(find(&a, 1, &key([9, 1, 2, 7, 4]), false), -1);
        // a *zero* key[0] makes the key partial, so a differing rec[0] is a
        // candidate rather than a rejection; the rejection needs a non-zero key[0]
        assert_eq!(find(&a, 1, &key([7, 1, 2, 3, 4]), false), -1);
    }

    #[test]
    fn a_non_positive_count_is_minus_one() {
        let a = array(&[[9, 1, 2, 3, 4]]);
        assert_eq!(find(&a, 0, &key([9, 1, 2, 3, 4]), false), -1);
        assert_eq!(find(&a, -3, &key([9, 1, 2, 3, 4]), false), -1);
    }

    #[test]
    fn a_zero_field_in_the_key_takes_the_last_candidate() {
        // key[8] matches, key[0] is zero and no record carries a zero +0, so
        // each match is a candidate - and the LAST one wins.
        let a = array(&[[1, 5, 2, 3, 4], [2, 5, 2, 3, 4], [3, 5, 2, 3, 4]]);
        assert_eq!(find(&a, 3, &key([0, 5, 2, 3, 4]), false), 2);
        // a record whose +0 is also zero is an *exact* match and returns at once
        let b = array(&[[0, 5, 2, 3, 4], [0, 5, 2, 3, 4]]);
        assert_eq!(find(&b, 2, &key([0, 5, 2, 3, 4]), false), 0);
    }

    #[test]
    fn a_candidate_with_a_different_zero_field_is_not_an_exact_match() {
        let a = array(&[[1, 5, 2, 3, 4]]);
        // key[0] != rec[0] and the key is partial, so it is a candidate, not a match
        assert_eq!(find(&a, 1, &key([0, 5, 2, 3, 4]), false), 0);
    }

    #[test]
    fn the_partial_flag_needs_a_non_zero_eight() {
        let a = array(&[[0, 0, 2, 3, 4]]);
        // key[8] == 0, so `partial` is false and a candidate needs an exact match
        assert_eq!(find(&a, 1, &key([5, 0, 2, 3, 4]), false), -1);
    }

    #[test]
    fn the_extra_field_is_only_compared_when_asked() {
        let a = array(&[[9, 1, 2, 3, 4]]);
        assert_eq!(find(&a, 1, &key([9, 1, 2, 3, 99]), false), 0);
        assert_eq!(find(&a, 1, &key([9, 1, 2, 3, 99]), true), -1);
    }

    #[test]
    fn the_mask_is_one_shifted_by_the_index_or_zero() {
        let a = array(&[[1, 1, 2, 3, 4], [9, 1, 2, 3, 4]]);
        assert_eq!(super::find_mask(&a, 2, &key([9, 1, 2, 3, 4]), false), 2);
        assert_eq!(super::find_mask(&a, 1, &key([9, 1, 2, 3, 4]), false), 0);
    }

    #[test]
    fn the_partial_flag_is_sticky_across_records() {
        // The first record sets `partial` even though its key[8] does not
        // match; the third record then qualifies as a candidate on key[8].
        let a = array(&[[0, 11, 2, 3, 4], [5, 12, 2, 3, 4], [9, 12, 2, 3, 4]]);
        assert_eq!(find(&a, 3, &key([0, 12, 2, 3, 4]), false), 2);
    }
}
