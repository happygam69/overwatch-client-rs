//! The world-interface array at `world+0x188`.
//!
//! `RegisterWorldInterface` (`0x141c52d10`) is the only writer. Its body is
//! four instructions of code around two constants:
//!
//! ```text
//! lea   rbx, [rcx+0x188]        the array header is the field itself
//! mov   eax, [rbx+0xc]          capacity word
//! mov   edx, [rbx+0x8]          length
//! xor   rbp, 0x238e31e8cd062f58 \
//! ror   rbp, 0x31               |  the element is stored transformed
//! add   rbp, 0x34b5f414245bf6ef /
//! add   eax, eax                \
//! sar   eax, 1                  |  capacity = capacity word with bit 31 cleared
//! inc   edx                     |  length + 1
//! cmp   edx, eax                |
//! jle   store                   /  fits: no growth
//! ```
//!
//! so the header is `{ data: *mut u64, len: u32, capacity_word: u32 }` — 16
//! bytes, which is exactly what `world+0x188..+0x198` holds before the
//! pointer→index map at `+0x198`.
//!
//! Bit 31 of the capacity word is a borrowing tag: the growth helper
//! `0x140767da0` adds `cap_tagged >> 31` to `0` to make the new word, skips
//! the free of the old buffer when the tag is set, and always clears the tag
//! on a successful reallocation. So a set bit means "this buffer is not mine,
//! do not free it".
//!
//! Growth, from the same helper, is `cap + cap / 2` (integer, so 1.5× rounded
//! down) and never below the requested length:
//!
//! ```text
//! cap = 0            -> newcap = min
//! cap != 0           -> newcap = max(cap + cap/2, min)
//! ```
//!
//! which for repeated single pushes walks 1, 2, 3, 4, 6, 9, 13, 19, 28, …
//! The copy count is `min(newcap, len)`, so the length can shrink back to the
//! new capacity — the client does that, and so does this.
//!
//! ## The one deviation, and why
//!
//! If the allocation fails while the array already owns its buffer, the
//! client's helper frees that buffer and stores the same pointer back
//! (`0x140767da0` frees when the tag is clear, then writes `*param_1` — a
//! use-after-free that no well-formed program can reach, because the only
//! caller grows before a store that would otherwise overrun). This model
//! returns `false` and leaves the array untouched; `push` then declines the
//! store, which is what the caller does with a `false` result anyway. Recorded
//! in `docs/formats/engine.md` under "Open".

use core::ptr;
use std::alloc::{alloc, dealloc, Layout};

use crate::codec::INTERFACE_ARRAY;

/// The capacity word's borrowing tag.
const BORROWED: u32 = 0x8000_0000;

/// The capacity a growth step asks the allocator for, from the current
/// capacity word's low 31 bits and the length that must fit.
///
/// This is `0x140767da0`'s arithmetic with the tag already removed: half again
/// on top of `cap`, raised to `min` when the half-step is not enough, and
/// simply `min` when nothing is allocated yet.
///
/// The comparison against `min` is *signed*, as the client's `(int)uVar6 <
/// (int)param_2` is: a capacity near `0x7fffffff` grows into a negative i32 and
/// falls back to `min` rather than wrapping to a smaller buffer.
#[must_use]
pub fn grow_capacity(cap: u32, min: u32) -> u32 {
    if cap == 0 {
        return min;
    }
    let grown = cap + (cap >> 1);
    if (grown as i32) < (min as i32) {
        min
    } else {
        grown
    }
}

fn layout_for(cap: u32) -> Layout {
    // `u64` is 8 bytes and 8-aligned; the client asks its allocator for
    // `cap * 8` bytes, so the length cannot overflow a usize here.
    Layout::array::<u64>(cap as usize).expect("interface array capacity fits the address space")
}

/// The `u64` array of transformed pointers at `world+0x188`.
///
/// Element `i` holds [`INTERFACE_ARRAY`]`(pointer)`; nothing else is stored in
/// it. The layout is `#[repr(C)]` on purpose: `data` at `+0x00`, `len` at
/// `+0x08`, `capacity word` at `+0x0c`, because that is where
/// `RegisterWorldInterface` reads them.
pub struct EncodedPtrArray {
    data: *mut u64,
    len: u32,
    cap: u32,
}

impl Default for EncodedPtrArray {
    fn default() -> Self {
        Self::new()
    }
}

impl EncodedPtrArray {
    /// An empty array: no buffer, no length, no borrowing tag — the state the
    /// engine object is allocated in.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            data: ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }

    /// Number of registered interfaces (`+0x08`).
    #[must_use]
    pub const fn len(&self) -> u32 {
        self.len
    }

    /// Whether nothing is registered.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Usable capacity, in elements: the capacity word without its tag
    /// (`eax = (cap << 1) >> 1` in the client).
    #[must_use]
    pub const fn capacity(&self) -> u32 {
        self.cap & !BORROWED
    }

    /// Whether the buffer belongs to somebody else and must not be freed.
    #[must_use]
    pub const fn is_borrowed(&self) -> bool {
        self.cap & BORROWED != 0
    }

    /// The raw stored word at `index`: the transformed pointer, as memory
    /// holds it. `None` past the length.
    #[must_use]
    pub fn stored(&self, index: u32) -> Option<u64> {
        if index >= self.len {
            return None;
        }
        // SAFETY: `index < len`, and `len` never exceeds the live capacity.
        Some(unsafe { *self.data.add(index as usize) })
    }

    /// The pointer registered at `index`, transformed back.
    ///
    /// The client's read of this array is not read yet, so this is the
    /// inverse of the store that *is* read; `docs/formats/engine.md` records
    /// it as the open half of the pair.
    #[must_use]
    pub fn get(&self, index: u32) -> Option<u64> {
        self.stored(index)
            .map(|stored| INTERFACE_ARRAY.revert(stored))
    }

    /// Append one pointer, transformed, exactly as `RegisterWorldInterface`
    /// does. Returns whether it was stored (the client skips the store when
    /// the growth helper reports failure).
    pub fn push(&mut self, pointer: u64) -> bool {
        let encoded = INTERFACE_ARRAY.apply(pointer);
        let need = self.len.wrapping_add(1);
        // `cmp edx, eax; jle store` — the client grows only when the new
        // length does not fit.
        if need > self.capacity() && !self.reserve(need) {
            return false;
        }
        // SAFETY: the capacity check above guarantees room at `len`.
        unsafe { self.data.add(self.len as usize).write(encoded) };
        self.len += 1;
        true
    }

    /// Make room for at least `min` elements, or report failure.
    ///
    /// The success path of `0x140767da0`, step for step: ask for a 1.5×
    /// capacity (or `min`), allocate zeroed, copy `min(newcap, len)` elements,
    /// free the old buffer unless it was borrowed, then adopt the new buffer
    /// with the tag cleared and the length truncated to the new capacity.
    fn reserve(&mut self, min: u32) -> bool {
        let cap = self.capacity();
        if self.is_borrowed() || cap < min {
            let newcap = grow_capacity(cap, min);
            let layout = layout_for(newcap);
            // SAFETY: `newcap` is non-zero whenever `min` is, and the layout is
            // the array's own element layout.
            let new = unsafe { alloc(layout) }.cast::<u64>();
            if new.is_null() {
                // The client's failure branch is destructive (see the module
                // doc); failing cleanly is the recorded deviation.
                return false;
            }
            let copied = newcap.min(self.len);
            for i in 0..copied {
                // SAFETY: both buffers hold at least `copied` elements.
                unsafe { new.add(i as usize).write(*self.data.add(i as usize)) };
            }
            if !self.is_borrowed() && !self.data.is_null() {
                // SAFETY: the current buffer was allocated with exactly this
                // layout and is owned (tag clear).
                unsafe { dealloc(self.data.cast::<u8>(), layout_for(cap)) };
            }
            self.data = new;
            self.cap = newcap;
            self.len = copied;
        }
        true
    }
}

impl Drop for EncodedPtrArray {
    fn drop(&mut self) {
        if !self.is_borrowed() && !self.data.is_null() {
            // SAFETY: owned buffer, allocated with this layout.
            unsafe { dealloc(self.data.cast::<u8>(), layout_for(self.capacity())) };
        }
    }
}

impl core::fmt::Debug for EncodedPtrArray {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EncodedPtrArray")
            .field("len", &self.len)
            .field("capacity", &self.capacity())
            .field("borrowed", &self.is_borrowed())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_the_client_layout() {
        assert_eq!(core::mem::size_of::<EncodedPtrArray>(), 0x10);
        assert_eq!(core::mem::offset_of!(EncodedPtrArray, data), 0x00);
        assert_eq!(core::mem::offset_of!(EncodedPtrArray, len), 0x08);
        assert_eq!(core::mem::offset_of!(EncodedPtrArray, cap), 0x0c);
    }

    #[test]
    fn growth_walks_the_client_sequence() {
        // Pushing one at a time, the capacity the helper asks for each time.
        let mut got = Vec::new();
        let mut cap = 0;
        for len in 1..=9 {
            cap = grow_capacity(cap, len);
            got.push(cap);
        }
        assert_eq!(got, [1, 2, 3, 4, 6, 9, 13, 19, 28]);
    }

    #[test]
    fn growth_is_half_again_and_never_below_the_request() {
        assert_eq!(grow_capacity(0, 1), 1);
        assert_eq!(grow_capacity(0, 40), 40);
        assert_eq!(grow_capacity(4, 5), 6);
        assert_eq!(grow_capacity(4, 7), 7, "min wins over the half step");
        assert_eq!(grow_capacity(0x7fff_ffff, 0x7fff_ffff), 0x7fff_ffff);
    }

    #[test]
    fn pushes_store_the_transformed_pointer() {
        let mut array = EncodedPtrArray::new();
        assert!(array.push(0x1000));
        assert_eq!(array.len(), 1);
        assert_eq!(array.capacity(), 1);
        // The raw word is the codec's output, not the pointer.
        assert_eq!(array.stored(0), Some(0x4daa_5a97_4408_08b6));
        assert_eq!(array.get(0), Some(0x1000));
        assert_eq!(array.stored(1), None);
    }

    #[test]
    fn pushing_past_capacity_keeps_every_element() {
        let mut array = EncodedPtrArray::new();
        let values: Vec<u64> = (0..64u64).map(|i| 0x1000 * (i + 1)).collect();
        for v in &values {
            assert!(array.push(*v));
        }
        assert_eq!(array.len(), 64);
        for (i, v) in values.iter().enumerate() {
            assert_eq!(array.get(i as u32), Some(*v), "element {i}");
        }
    }

    #[test]
    fn a_borrowed_buffer_is_never_freed_and_loses_its_tag_on_growth() {
        let mut raw = [0u64; 2];
        let mut array = EncodedPtrArray {
            data: raw.as_mut_ptr(),
            len: 0,
            cap: BORROWED | 2,
        };
        assert!(array.is_borrowed());
        assert_eq!(array.capacity(), 2);
        assert!(array.push(0x20));
        assert!(array.push(0x40));
        assert_eq!(array.capacity(), 2, "two pushes fit the borrowed capacity");
        // The third grows: a fresh buffer, tag cleared, old one left alone.
        assert!(array.push(0x60));
        assert!(!array.is_borrowed());
        assert_eq!(array.capacity(), 3);
        assert_eq!(array.capacity(), grow_capacity(2, 3));
        for (i, v) in [0x20u64, 0x40, 0x60].iter().enumerate() {
            assert_eq!(array.get(i as u32), Some(*v));
        }
        // `raw` must still be intact: it was never freed.
        assert_eq!(
            raw,
            [INTERFACE_ARRAY.apply(0x20), INTERFACE_ARRAY.apply(0x40)]
        );
    }

    #[test]
    fn a_short_reserve_request_changes_nothing() {
        // The client's helper is only ever called with a length that does not
        // fit (`cmp edx, eax; jle` sends the fitting case straight to the
        // store). The branch it *would* take for a short request frees the
        // buffer and keeps the pointer, so the model declines instead of
        // reproducing a use-after-free.
        let mut array = EncodedPtrArray::new();
        for v in 0..6 {
            assert!(array.push(v));
        }
        assert_eq!(array.len(), 6);
        assert!(array.reserve(2));
        assert_eq!(array.len(), 6);
        assert_eq!(array.capacity(), 6);
    }
}
