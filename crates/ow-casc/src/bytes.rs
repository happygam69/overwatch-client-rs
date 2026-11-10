//! Byte primitives read from the client image.
//!
//! `memcmp` at exe `0x142efecf0` (199 bytes, `0x142efecf0..0x142efedb7`) is the
//! comparison the index searches and the CDN page checks go through. It is a
//! leaf: no calls, no image constants, no writes.
//!
//! The read, instruction for instruction:
//!
//! - `sub rdx, rcx` turns the second pointer into a displacement from the
//!   first, so one cursor walks both buffers.
//! - a length below 8 skips the aligned walk (`cmp r8, 8` / `jb`); an
//!   unaligned first pointer is walked one byte at a time until `cl & 7 == 0`
//!   (`test cl, 7` / `jne`), so every wide compare is 8-byte aligned.
//! - the wide walk compares four qwords in 32-byte blocks (`cmp rax, [rcx+rdx]`
//!   and its three siblings), then single qwords, then the 0..7 byte tail.
//! - every mismatch lands on one result site. The head and tail byte walks
//!   compare the byte pair itself (`cmp al, [rcx+rdx]` + `sbb eax, eax` /
//!   `sbb eax, -1`). The wide walk reloads the failing qword, byte-swaps both
//!   words (`bswap rax` / `bswap rcx`) and compares unsigned, which orders
//!   exactly like the big-endian, i.e. lexicographic, view of the eight bytes -
//!   the first differing byte decides. The result is `-1` (first below),
//!   `1` (first above) or `0` (all equal), written through `eax` so RAX holds
//!   `0x0000_0000_ffff_ffff`, `1` or `0`.
//!
//! The wide compares read exactly the bytes a byte-by-byte scan reads (they
//! compare equality) and the ordering site is the first differing byte, so the
//! observable result is the byte scan below: same read set, same `-1`/`0`/`1`.

/// `memcmp` (exe `0x142efecf0`): unsigned comparison of `count` bytes of `a`
/// and `b`, returning the client's RAX value: `0xffff_ffff` when `a` orders
/// below `b`, `1` when above, `0` when the `count` bytes are equal.
///
/// # Panics
///
/// Panics if either slice is shorter than `count`; the client reads with no
/// bounds check, so a short buffer is a caller bug, not a value.
pub fn client_memcmp(a: &[u8], b: &[u8], count: usize) -> u64 {
    let mut i = 0;
    while i < count {
        let left = a[i];
        let right = b[i];
        if left < right {
            return 0xffff_ffff;
        }
        if right < left {
            return 1;
        }
        i += 1;
    }
    0
}

/// `strlen` (exe `0x1426cce30`): the length of the NUL-terminated string at
/// `s`, in the client's own SWAR form. It is the length helper the encoding
/// espec build calls (`0x142498a70`), and 1,628 other call sites reach it.
///
/// The read, instruction for instruction:
///
/// - `neg rcx` turns the start pointer into `-s`, so the return is a length
///   (`end - s`), not the terminator's address.
/// - the aligned walk reads one qword at a time and applies the client's
///   zero-byte test: `r9 = 0x7efefefefefefeff + x`, `t = (~x) ^ r9`,
///   `t &= 0x8101010101010100`; a zero `t` means no NUL in the qword.
/// - on a hit the qword is reloaded and the first zero byte is found from the
///   low byte up; the return is `qword_address + 8 - s - (8 - j)`, i.e. the
///   byte offset of the NUL. A qword the test flags but that holds no zero
///   byte falls through to the next qword (`test dh, dh` / `jne`).
///
/// The client's byte walk aligns the qword reads to the absolute address
/// (`test al, 7` / `jne`). That walk is a performance path only: the test is a
/// fast zero-byte detector (it has false positives, which the byte scan's
/// fall-through handles) and the byte scan finds the first NUL, so the
/// returned length is the same for every alignment. The Rust reads from index
/// 0 and records that equivalence here, the same way `client_memcmp` records
/// its byte scan. The fixture varies the oracle's alignment through
/// `arena_offset` to exercise the client's walk; the differential compares the
/// length only.
///
/// # Panics
///
/// Never panics: a slice with no NUL returns the offset it reached, which is
/// the caller-bug case the client would run off its buffer on.
pub fn client_strlen(s: &[u8]) -> u64 {
    let mut i = 0usize;
    loop {
        let x = match s.get(i..i + 8) {
            Some(c) => u64::from_le_bytes(c.try_into().unwrap()),
            None => return i as u64,
        };
        let r9 = 0x7efefefefefefeffu64.wrapping_add(x);
        let t = (!x) ^ r9;
        let t = t & 0x8101010101010100u64;
        i += 8;
        if t == 0 {
            continue;
        }
        // The client's byte scan, from the low byte up. No zero byte means
        // the test was a false positive: the client loops to the next qword.
        let mut shift = 0usize;
        while shift < 64 {
            if (x >> shift) & 0xff == 0 {
                return (i - 8 + shift / 8) as u64;
            }
            shift += 8;
        }
    }
}
