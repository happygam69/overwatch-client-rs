//! The pointer→index map at `world+0x198`.
//!
//! Four functions make it:
//!
//! * `0x141c553a0` (`WorldInterfaceMapInit`) — allocate and/or rehash for a
//!   requested bucket count;
//! * `0x141c56fb0` (`WorldInterfaceInsert`) — insert, which is also the lookup
//!   ("already present" traps);
//! * `0x140767da0` grows the `world+0x188` array, not this map;
//! * the free-list erase path has not been read (see `Open` in the crate docs).
//!
//! ## Layout
//!
//! One allocation holds three arrays behind one pointer, in this order:
//!
//! ```text
//! +0x00  buckets   bucket_count * 4 bytes, padded to 8
//!        entries   bucket_count * 16 bytes  { tag: u32, next: u32, key: u64 }
//!        values    bucket_count * 8 bytes   encoded pointers
//! ```
//!
//! `WorldInterfaceMapInit` computes the size as
//! `align8(count*4) + count*16 + count*8` (`0x141c553ea..0x141c5540b`) and
//! partitions the result at exactly those offsets (`0x141c55410..0x141c5541c`).
//! Capacity equals the bucket count: the map is full at load factor 1.
//!
//! The map header is six words at `+0x198`:
//!
//! | offset | meaning |
//! |---|---|
//! | `+0x00` | buckets pointer |
//! | `+0x08` | entries pointer |
//! | `+0x10` | values pointer |
//! | `+0x18` | bucket count (always a power of two, `0` = unallocated) |
//! | `+0x1c` | entry slots consumed, next free slot index |
//! | `+0x20` | free-list head |
//! | `+0x24` | free slots available |
//!
//! ## The two sentinels
//!
//! An entry's `tag` is its hash, and a hash always has bit 31 clear: the insert
//! computes `((key >> 4) as u32) & 0x7fff_ffff` (`0x141c57072..0x141c57076`,
//! `shr` then `btr esi,0x1f`). So `0xffff_ffff` cannot be a hash, and the same
//! value means "entry slot unused" and "bucket empty". The bucket walk follows
//! `next` while the index is non-negative, so `0xffff_ffff` also terminates a
//! chain. One value, three jobs.
//!
//! A *value* slot holds the registry codec's encoding of null until something
//! writes it: `0x4daa5a973c0808b6` (`0x141c5718d..0x141c57197`), which is
//! `INTERFACE_ARRAY.apply(0)`.
//!
//! ## Growth
//!
//! Only when a slot is needed and the free list is empty and the entries are
//! all used. The new count is `next_pow2(old + 1)`, so growth is by one bucket
//! — the table is sized exactly for what it holds. The rehash walks the *old*
//! entry count, skips unused slots, and prepends each live slot to its new
//! bucket, which reverses every chain. Values for live slots are copied by
//! index; unused slots keep whatever the allocation held.
//!
//! Evidence class: **read**. Addresses are current-image addresses; see
//! `docs/formats/engine.md`.

/// `0xffff_ffff`: entry slot unused, bucket empty, end of chain.
pub const SENTINEL: u32 = u32::MAX;

/// The value a freshly inserted slot holds: `INTERFACE_ARRAY.apply(0)`
/// (`0x4daa5a973c0808b6`).
pub const UNSET_VALUE: u64 = 0x4daa_5a97_3c08_08b6;

/// One 16-byte entry slot.
///
/// `tag` is the hash, or [`SENTINEL`] when the slot is unused. `next` is the
/// next slot in the bucket's chain, or the next free slot when this slot is on
/// the free list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Entry {
    /// Hash of [`Entry::key`], or [`SENTINEL`].
    pub tag: u32,
    /// Chain or free-list link.
    pub next: u32,
    /// The key. Only meaningful while `tag != SENTINEL`; the client never
    /// clears it, so a reused slot keeps the previous key until the insert
    /// overwrites it.
    pub key: u64,
}

/// The client's `+0x198` map.
///
/// The three arrays are separate `Vec`s here; the client keeps them in one
/// allocation. Everything observable — hashes, bucket assignment, chain order,
/// slot reuse, growth points — is reproduced exactly, which is what the tests
/// pin.
#[derive(Clone, Debug, Default)]
pub struct InterfaceMap {
    buckets: Vec<u32>,
    entries: Vec<Entry>,
    values: Vec<u64>,
    used: u32,
    free_head: u32,
    free_count: u32,
}

impl InterfaceMap {
    /// An unallocated map: `buckets == null` at `+0x198`. The first insert
    /// allocates three buckets (`0x141c5704c..0x141c57054`); the allocator
    /// rounds that up to four.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The bucket count (`+0x18`); `0` means unallocated.
    #[must_use]
    pub fn bucket_count(&self) -> u32 {
        self.buckets.len() as u32
    }

    /// Entry slots consumed (`+0x1c`).
    #[must_use]
    pub fn used(&self) -> u32 {
        self.used
    }

    /// Free slots available (`+0x24`).
    #[must_use]
    pub fn free_count(&self) -> u32 {
        self.free_count
    }

    /// Whether the map has been allocated at all.
    #[must_use]
    pub fn is_allocated(&self) -> bool {
        !self.buckets.is_empty()
    }

    /// The hash the client computes: `((key >> 4) as u32) & 0x7fff_ffff`.
    #[must_use]
    pub fn hash(key: u64) -> u32 {
        ((key >> 4) as u32) & 0x7fff_ffff
    }

    /// The allocation size `0x141c553ea..0x141c5540b` asks for: the three
    /// partitions, each rounded the way the client rounds them.
    #[must_use]
    pub fn layout_size(bucket_count: u32) -> usize {
        let buckets = Self::buckets_bytes(bucket_count);
        buckets + bucket_count as usize * 16 + bucket_count as usize * 8
    }

    /// `align8(count * 4)` — the bucket partition's size.
    #[must_use]
    pub fn buckets_bytes(bucket_count: u32) -> usize {
        // `lea edi,[7 + rsi*4]; and edi,-8`
        ((bucket_count as usize) * 4 + 7) & !7
    }

    /// `next_pow2` as `0x141c553b9..0x141c553e8` computes it: an already-power-
    /// of-two request is kept, anything else is smeared and incremented.
    ///
    /// `0` returns `0`, matching the smear's `0xffff_ffff + 1` wrap.
    #[must_use]
    pub fn next_pow2(requested: u32) -> u32 {
        if requested == 0 || requested & (requested - 1) == 0 {
            return requested;
        }
        let mut v = requested - 1;
        v |= v >> 1;
        v |= v >> 2;
        v |= v >> 4;
        v |= v >> 8;
        v |= v >> 16;
        v.wrapping_add(1)
    }

    /// `WorldInterfaceMapInit` (`0x141c553a0`): allocate for `requested`
    /// buckets, moving any existing contents across.
    ///
    /// Called with `3` for the first allocation and with `old + 1` for a grow.
    pub fn grow_to(&mut self, requested: u32) {
        let count = Self::next_pow2(requested);
        let old_count = self.bucket_count();

        let mut buckets = vec![SENTINEL; count as usize];
        let mut entries = vec![Entry::default(); count as usize];
        let mut values = vec![0u64; count as usize];
        // The unused tail is tagged; the used prefix arrives from the copy.
        for entry in entries.iter_mut().skip(old_count as usize) {
            entry.tag = SENTINEL;
        }

        if old_count > 0 {
            // Copy every old slot (used and unused), then the values of the
            // live ones — the client skips slots whose tag is the sentinel
            // (`0x141c55471..0x141c5547b`).
            for i in 0..old_count as usize {
                entries[i] = self.entries[i];
                if entries[i].tag != SENTINEL {
                    values[i] = self.values[i];
                }
            }

            // Rehash live slots into the new buckets, walking in slot order and
            // prepending — which reverses each chain.
            let mask = count - 1;
            for i in 0..old_count {
                let tag = self.entries[i as usize].tag;
                if tag != SENTINEL {
                    let bucket = (tag & mask) as usize;
                    entries[i as usize].next = buckets[bucket];
                    buckets[bucket] = i;
                }
            }
        }

        self.buckets = buckets;
        self.entries = entries;
        self.values = values;
    }

    /// `WorldInterfaceInsert` (`0x141c56fb0`) up to the point where it has found
    /// the slot to fill: allocate if unallocated, then either reuse a free slot,
    /// take the next unused slot, or grow by one bucket.
    ///
    /// The order is the client's: free list first (`0x141c570a4` tests `+0x24`
    /// and jumps to the grow/reuse split), growth only when no free slot exists
    /// and every entry slot is consumed.
    fn reserve_slot(&mut self) -> usize {
        if !self.is_allocated() {
            self.grow_to(3);
        }
        if self.free_count > 0 {
            let slot = self.free_head;
            self.free_head = self.entries[slot as usize].next;
            self.free_count -= 1;
            return slot as usize;
        }
        if self.used == self.bucket_count() {
            self.grow_to(self.bucket_count() + 1);
        }
        let slot = self.used as usize;
        self.used += 1;
        slot
    }

    /// The lookup half: the slot holding `key`, if any.
    ///
    /// The client walks the chain comparing keys (`0x141c57090..0x141c570a2`);
    /// a hit there is the assertion path, not a return.
    #[must_use]
    pub fn find(&self, key: u64) -> Option<u32> {
        if !self.is_allocated() {
            return None;
        }
        let hash = Self::hash(key);
        let mut idx = self.buckets[(hash & (self.bucket_count() - 1)) as usize];
        while idx != SENTINEL {
            let entry = self.entries[idx as usize];
            if entry.key == key {
                return Some(idx);
            }
            idx = entry.next;
        }
        None
    }

    /// Insert `key`, returning its slot index.
    ///
    /// # Panics
    ///
    /// If `key` is already present. The client does not return in that case:
    /// it runs its assertion machinery (message at `0x14328ca20`, line
    /// `0x684` = 1668) and traps.
    pub fn insert(&mut self, key: u64) -> u32 {
        assert!(
            self.find(key).is_none(),
            "RegisterWorldInterface inserted a key twice (client asserts at line 1668)"
        );
        let hash = Self::hash(key);
        let slot = self.reserve_slot();
        let bucket = (hash & (self.bucket_count() - 1)) as usize;
        let index = slot as u32;

        // `0x141c57177..0x141c571a8`: value first, then key, tag, chain splice.
        self.values[slot] = UNSET_VALUE;
        self.entries[slot].key = key;
        self.entries[slot].tag = hash;
        self.entries[slot].next = self.buckets[bucket];
        self.buckets[bucket] = index;
        index
    }

    /// The encoded value in `slot` ([`UNSET_VALUE`] until something writes it).
    #[must_use]
    pub fn value(&self, slot: u32) -> u64 {
        self.values[slot as usize]
    }

    /// Write the encoded value in `slot`.
    pub fn set_value(&mut self, slot: u32, encoded: u64) {
        self.values[slot as usize] = encoded;
    }

    /// The bucket a key lands in, for tests and diagnostics.
    #[must_use]
    pub fn bucket_of(&self, key: u64) -> Option<u32> {
        if !self.is_allocated() {
            return None;
        }
        Some(Self::hash(key) & (self.bucket_count() - 1))
    }

    /// Every slot in `bucket`'s chain, head first.
    #[must_use]
    pub fn chain(&self, bucket: u32) -> Vec<u32> {
        let mut out = Vec::new();
        if !self.is_allocated() {
            return out;
        }
        let mut idx = self.buckets[bucket as usize];
        while idx != SENTINEL {
            out.push(idx);
            idx = self.entries[idx as usize].next;
        }
        out
    }

    /// Test hook: put `slot` on the free list.
    ///
    /// The client's erase path — the only code that fills `+0x20`/`+0x24` — has
    /// not been read, so this hook transcribes just the shape the insert
    /// depends on: the slot leaves its bucket chain, the free list links
    /// through `entry.next`, and `+0x24` counts the slots available.
    ///
    /// The unlink is this crate's own, not a read expression: a slot left in
    /// its chain would still be reachable from `find`, and since its `next` now
    /// points into the free list a chain could revisit itself and never end.
    #[cfg(test)]
    pub(crate) fn push_free(&mut self, slot: u32) {
        if self.is_allocated() {
            let tag = self.entries[slot as usize].tag;
            if tag != SENTINEL {
                let bucket = (tag & (self.bucket_count() - 1)) as usize;
                let mut at = self.buckets[bucket];
                let mut prev = SENTINEL;
                while at != SENTINEL {
                    let next = self.entries[at as usize].next;
                    if at == slot {
                        if prev == SENTINEL {
                            self.buckets[bucket] = next;
                        } else {
                            self.entries[prev as usize].next = next;
                        }
                        break;
                    }
                    prev = at;
                    at = next;
                }
            }
        }
        self.entries[slot as usize].next = self.free_head;
        self.free_head = slot;
        self.free_count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pointers the client would hand in: heap addresses from the image's own
    /// allocations are never below `0x1_0000_0000`, so the shift truncates.
    const KEYS: [u64; 6] = [
        0x1_4000_0000,
        0x1_4000_0100,
        0x2_7fff_fff0,
        0x7_ffff_ffff_ffff,
        0x143b7f410,
        0x1_c0ffee00,
    ];

    #[test]
    fn hash_clears_the_sign_bit() {
        for key in KEYS {
            let h = InterfaceMap::hash(key);
            assert_eq!(h, ((key >> 4) as u32) & 0x7fff_ffff, "{key:#x}");
            assert_eq!(h & 0x8000_0000, 0, "bit 31 must be clear for {key:#x}");
        }
        // A key large enough that the shift alone would set bit 31.
        assert_eq!(
            InterfaceMap::hash(0x8_0000_0000_0000),
            0x8000_0000 & 0x7fff_ffff
        );
    }

    #[test]
    fn next_pow2_matches_the_smear() {
        assert_eq!(InterfaceMap::next_pow2(0), 0);
        assert_eq!(InterfaceMap::next_pow2(1), 1);
        assert_eq!(InterfaceMap::next_pow2(2), 2);
        assert_eq!(InterfaceMap::next_pow2(3), 4);
        assert_eq!(InterfaceMap::next_pow2(4), 4);
        assert_eq!(InterfaceMap::next_pow2(5), 8);
        assert_eq!(InterfaceMap::next_pow2(65), 128);
        assert_eq!(InterfaceMap::next_pow2(1000), 1024);
    }

    #[test]
    fn layout_size_is_the_sum_of_the_three_partitions() {
        for count in [1u32, 2, 4, 8, 16, 64, 1024] {
            // The client's own arithmetic, written the way the instructions do.
            let buckets = ((count as usize) * 4 + 7) & !7;
            let entries = count as usize * 16;
            let values = count as usize * 8;
            assert_eq!(InterfaceMap::layout_size(count), buckets + entries + values);
            assert_eq!(InterfaceMap::buckets_bytes(count), buckets);
            // The partitions must not overlap, which is what makes the offsets
            // in `grow_to` meaningful.
            assert!(buckets >= count as usize * 4);
            assert!(buckets + entries <= InterfaceMap::layout_size(count));
        }
    }

    #[test]
    fn first_insert_allocates_four_buckets() {
        let mut map = InterfaceMap::new();
        assert!(!map.is_allocated());
        let slot = map.insert(KEYS[0]);
        assert_eq!(slot, 0);
        assert_eq!(map.bucket_count(), 4);
        assert_eq!(map.used(), 1);
        assert_eq!(map.value(slot), UNSET_VALUE);
        assert_eq!(map.find(KEYS[0]), Some(0));
        assert_eq!(map.find(KEYS[1]), None);
    }

    #[test]
    fn buckets_are_masked_by_bucket_count_and_chains_prepend() {
        let mut map = InterfaceMap::new();
        // Two keys that share a bucket at four buckets.
        let a = 0x1_0000_0000;
        let b = a + 0x40;
        assert_eq!(InterfaceMap::hash(a) & 3, InterfaceMap::hash(b) & 3);

        let sa = map.insert(a);
        let sb = map.insert(b);
        let bucket = map.bucket_of(a).expect("allocated");
        assert_eq!(map.chain(bucket), vec![sb, sa], "newest first");
        // The newer entry's `next` points at the older one.
        assert_eq!(map.entries[sb as usize].next, sa);
    }

    #[test]
    fn growth_happens_on_the_insert_that_needs_a_fifth_slot() {
        let mut map = InterfaceMap::new();
        for (i, key) in (0u64..4).map(|i| 0x1_0000_0000 + i * 0x1000).enumerate() {
            assert_eq!(map.insert(key), i as u32);
        }
        assert_eq!(map.bucket_count(), 4);
        assert_eq!(map.used(), 4);

        let keys: Vec<u64> = (0u64..5).map(|i| 0x1_0000_0000 + i * 0x1000).collect();
        let fifth = keys[4];
        map.insert(fifth);
        assert_eq!(map.bucket_count(), 8, "grow to next_pow2(4 + 1)");
        assert_eq!(map.used(), 5);
        // Every earlier key still resolves, and to the same slot.
        for (i, key) in keys[..4].iter().enumerate() {
            assert_eq!(map.find(*key), Some(i as u32), "{key:#x}");
        }
        assert_eq!(map.find(fifth), Some(4));
        // Buckets now use the wider mask.
        let bucket = map.bucket_of(fifth).expect("allocated");
        assert_eq!(bucket, InterfaceMap::hash(fifth) & 7);
    }

    #[test]
    fn rehash_reverses_chains_but_keeps_lookups() {
        let mut map = InterfaceMap::new();
        let keys: Vec<u64> = (0u64..4).map(|i| 0x1_0000_0000 + i * 0x40).collect();
        for key in &keys {
            map.insert(*key);
        }
        let before: Vec<Vec<u32>> = (0..4).map(|b| map.chain(b)).collect();
        map.insert(0x1_0000_9000);
        assert_eq!(map.bucket_count(), 8);
        for (mask, key) in [(7u32, keys[0]), (7, keys[1]), (7, keys[2]), (7, keys[3])] {
            assert_eq!(map.bucket_of(key), Some(InterfaceMap::hash(key) & mask));
        }
        // Chains are rebuilt in slot order, prepending, so each new-bucket
        // chain lists its slots newest-first. Four of these five keys re-bucket
        // at mask 7, so a 4-slot chain is not expected to survive whole.
        assert!(
            before.iter().any(|chain| chain.len() > 1),
            "the fixture must start with a shared bucket"
        );
        for key in &keys {
            let slot = map.find(*key).expect("every key survives the grow");
            assert_eq!(map.entries[slot as usize].key, *key);
        }
        for bucket in 0..8u32 {
            let mut want: Vec<u32> = (0..5u32)
                .filter(|slot| map.entries[*slot as usize].tag & 7 == bucket)
                .collect();
            want.reverse();
            assert_eq!(map.chain(bucket), want, "bucket {bucket}");
        }
    }

    #[test]
    fn free_list_is_used_before_growing() {
        let mut map = InterfaceMap::new();
        let a = map.insert(0x1_0000_0000);
        let b = map.insert(0x1_0000_1000);
        let _ = b;
        // Simulate the erase pass the client has but this crate has not read.
        map.push_free(a);
        assert_eq!(map.free_count(), 1);
        let used_before = map.used();
        let reused = map.insert(0x1_0000_2000);
        assert_eq!(reused, a, "the free slot is reused");
        assert_eq!(map.used(), used_before, "no new slot consumed");
        assert_eq!(map.free_count(), 0);
        assert_eq!(map.find(0x1_0000_2000), Some(a));
        assert_eq!(map.find(0x1_0000_0000), None, "the old key left the chains");
    }

    #[test]
    #[should_panic(expected = "inserted a key twice")]
    fn duplicate_key_panics_like_the_client_asserts() {
        let mut map = InterfaceMap::new();
        map.insert(0x143b7f410);
        map.insert(0x143b7f410);
    }

    #[test]
    fn values_are_written_from_the_codec_and_can_be_replaced() {
        use crate::codec::INTERFACE_ARRAY;
        let mut map = InterfaceMap::new();
        let slot = map.insert(0x1_0000_0000);
        assert_eq!(map.value(slot), INTERFACE_ARRAY.apply(0));
        map.set_value(slot, INTERFACE_ARRAY.apply(0x1_0000_0000));
        assert_eq!(map.value(slot), INTERFACE_ARRAY.apply(0x1_0000_0000));
    }

    #[test]
    fn entries_are_two_words_and_a_key() {
        assert_eq!(std::mem::size_of::<Entry>(), 16);
        assert_eq!(std::mem::align_of::<Entry>(), 8);
        assert_eq!(std::mem::offset_of!(Entry, tag), 0);
        assert_eq!(std::mem::offset_of!(Entry, next), 4);
        assert_eq!(std::mem::offset_of!(Entry, key), 8);
    }
}
