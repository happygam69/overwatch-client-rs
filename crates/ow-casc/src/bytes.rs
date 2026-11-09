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
