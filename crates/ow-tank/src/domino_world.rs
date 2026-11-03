//! The Domino world slot and the body handle: the two encodings the per-model
//! collision body uses.
//!
//! `docs/formats/static-collision.md` check 10 reads the owner side of the
//! builder: which id `0x14201ae60` hands to `OwnerCreateBody`, and what that
//! function does with it. Two encodings come out of that read, both with exact
//! constants.
//!
//! **The world slot.** `OwnerCreateBody` (`0x142cc3d90`) takes a `u16` first
//! argument, rejects it when it is `>= 0x3fff` and rejects the slot whose table
//! entry is the empty-slot sentinel, then resolves the world pointer:
//!
//! ```text
//! world = (*(u64 *)(WORLD_TABLE + slot * 8) + 0xb3269b88d44d5fb2) ^ 0x77600ba5adcc265c
//! ```
//!
//! The world is stored into that table by the allocator `0x142cc3c80`, which
//! fills the first empty slot with the inverse expression:
//!
//! ```text
//! entry = (world ^ 0x77600ba5adcc265c) + 0x4cd964772bb2a04e
//! ```
//!
//! `0x4cd964772bb2a04e + 0xb3269b88d44d5fb2 == 0` modulo `2^64`, which is why
//! the decode undoes the encode exactly; [`WORLD_SLOT_LIMIT`] is the same
//! `0x3fff` bound the character sweep's handle decode rejects at, and
//! [`WORLD_TABLE`] is the same table it indexes, so a slot in this table is a
//! world in *one* table for both users.
//!
//! **The body handle.** `OwnerCreateBody` returns
//! `((slot:u16) << 0x30) | ((generation:u16) << 0x20) | (index:u32)`, the
//! generation coming from the body entry at `[world+0x288] + index*0x1c0` field
//! `+0x198`. On refusal it returns `0xffff_0000_ffff_ffff`, and that is exactly
//! the value the builder tests `[component+0x88]` against before it builds
//! anything (`0x14201ae9f`..`0x14201aecc`: `handle >> 0x30 == 0xffff`,
//! `(u32)handle == 0xffff_ffff`, `(handle >> 0x20) as u16 == 0`).
//!
//! Nothing here converts game data; these are the client's own two bit
//! layouts, expressed so a test can pin them.

/// The global world table (`0x1442a0bc0`), indexed by a world slot. The same
/// table `RunCharacterControllerSweep`'s handle decode reads.
pub const WORLD_TABLE: u64 = 0x1442_0bc0;
/// The slot bound both users reject at: a slot `>= 0x3fff` is not a world.
pub const WORLD_SLOT_LIMIT: u16 = 0x3fff;
/// The empty-slot sentinel a free table entry holds
/// (`-0x3bc68fe326813956` in the disassembly).
pub const EMPTY_SLOT: u64 = 0xc439_701c_d97e_c6aa;
/// The XOR half of the world-pointer encoding, used by both directions.
pub const WORLD_KEY: u64 = 0x7760_0ba5_adcc_265c;
/// The addend the allocator applies: `0x142cc3c80`'s `+ 0x4cd964772bb2a04e`.
pub const WORLD_ADD_STORE: u64 = 0x4cd9_6477_2bb2_a04e;
/// The addend the reader applies: `OwnerCreateBody`'s `+ 0xb3269b88d44d5fb2`.
pub const WORLD_ADD_LOAD: u64 = 0xb326_9b88_d44d_5fb2;
/// `world+0x288`: the body pool base.
pub const WORLD_BODY_POOL: usize = 0x288;
/// The body pool's stride: one body entry per `0x1c0` bytes.
pub const WORLD_BODY_STRIDE: usize = 0x1c0;
/// `world_body + 0x198`: the body entry's generation, a `u16`.
pub const BODY_GENERATION: usize = 0x198;
/// The invalid body handle `OwnerCreateBody` returns and the builder skips.
pub const INVALID_BODY_HANDLE: u64 = 0xffff_0000_ffff_ffff;

/// The table entry the allocator writes for a world.
pub fn encode_world_pointer(world: u64) -> u64 {
    (world ^ WORLD_KEY).wrapping_add(WORLD_ADD_STORE)
}

/// The world pointer a non-empty table entry holds: `OwnerCreateBody`'s
/// expression, in the client's evaluation order (`+` before `^`).
pub fn decode_world_pointer(entry: u64) -> u64 {
    entry.wrapping_add(WORLD_ADD_LOAD) ^ WORLD_KEY
}

/// Whether a table entry is the empty slot (`OwnerCreateBody`'s refusal test).
pub fn is_empty_slot(entry: u64) -> bool {
    entry == EMPTY_SLOT
}

/// `OwnerCreateBody`'s first guard: `slot < 0x3fff`.
pub fn slot_indexes_a_world(slot: u16) -> bool {
    slot < WORLD_SLOT_LIMIT
}

/// The body handle `OwnerCreateBody` returns, packed in the client's order.
pub fn pack_body_handle(slot: u16, generation: u16, index: u32) -> u64 {
    ((slot as u64) << 0x30) | ((generation as u64) << 0x20) | index as u64
}

/// The three fields of a body handle, in the order the client's readers take
/// them: the world slot from `handle >> 0x30`, the generation from
/// `(handle >> 0x20) as u16`, the index from `handle as u32`.
pub fn body_handle_parts(handle: u64) -> (u16, u16, u32) {
    (
        (handle >> 0x30) as u16,
        (handle >> 0x20) as u16,
        handle as u32,
    )
}

/// The world slot a handle names: `handle >> 0x30`, which is the expression
/// both the builder's guard and the sweep's accessors use to pick a world.
pub fn handle_world_slot(handle: u64) -> u16 {
    (handle >> 0x30) as u16
}

/// The builder's refusal test on `[component+0x88]`: slot `0xffff`, generation
/// `0` and index `0xffff_ffff`. All three are tested separately in the client
/// (`cmp eax, 0xffff`, `cmp ecx, -1`, `test cx, cx`), so a handle with an
/// out-of-range slot but a live index is *not* the invalid one.
pub fn is_invalid_body_handle(handle: u64) -> bool {
    handle_world_slot(handle) == 0xffff
        && (handle as u32) == 0xffff_ffff
        && ((handle >> 0x20) as u16) == 0
}

/// The body entry a handle names inside a resolved world, as a byte offset from
/// the world pointer: `[world+0x288] + index*0x1c0`, generation at `+0x198`.
pub fn body_entry_offset(index: u32) -> usize {
    index as usize * WORLD_BODY_STRIDE
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The store addend and the load addend are exact inverses modulo `2^64`;
    /// that identity is what makes the allocator's encoded pointer readable by
    /// `OwnerCreateBody`. If either constant were transcribed wrongly, this
    /// fails.
    #[test]
    fn the_two_addends_are_inverses_modulo_2_pow_64() {
        assert_eq!(WORLD_ADD_STORE.wrapping_add(WORLD_ADD_LOAD), 0);
    }

    /// Encode then decode returns the world pointer bit for bit, including the
    /// extremes where the addend carries.
    #[test]
    fn the_world_pointer_round_trips_bit_exactly() {
        for world in [
            0u64,
            1,
            0xffff_ffff_ffff_ffff,
            0x0000_7fff_1234_5678,
            0xdead_beef_dead_beef,
            EMPTY_SLOT,
            WORLD_KEY,
        ] {
            assert_eq!(decode_world_pointer(encode_world_pointer(world)), world);
        }
        // The two directions are not the identity: the same value through the
        // store expression alone must differ from the input.
        assert_ne!(encode_world_pointer(0), 0);
    }

    /// A body handle packs world slot, generation and index with no overlap,
    /// and the parts come back in the order the client's readers take them.
    #[test]
    fn the_body_handle_is_slot_then_generation_then_index() {
        let handle = pack_body_handle(0x1234, 0x5678, 0x9abc_def0);
        assert_eq!(handle, 0x1234_5678_9abc_def0);
        assert_eq!(body_handle_parts(handle), (0x1234, 0x5678, 0x9abc_def0));
        // `handle >> 0x30` is the slot alone: the generation and index cannot
        // reach the top 16 bits.
        assert_eq!(handle_world_slot(handle), 0x1234);
        assert_eq!(
            handle_world_slot(pack_body_handle(0x0000, 0xffff, 0xffff_ffff)),
            0
        );
        assert_eq!(
            handle_world_slot(pack_body_handle(0x3ffe, 0xffff, 0xffff_ffff)),
            0x3ffe
        );
    }

    /// The invalid handle is exactly what `OwnerCreateBody` returns on refusal,
    /// and it is the only value the builder accepts as "no body yet".
    #[test]
    fn the_invalid_body_handle_is_ffff_0000_ffff_ffff() {
        assert_eq!(INVALID_BODY_HANDLE, 0xffff_0000_ffff_ffff);
        assert!(is_invalid_body_handle(INVALID_BODY_HANDLE));
        assert!(is_invalid_body_handle(pack_body_handle(
            0xffff,
            0,
            0xffff_ffff
        )));
        // A live handle differs in at least one of the three fields.
        assert!(!is_invalid_body_handle(pack_body_handle(
            0x0000,
            0,
            0xffff_ffff
        )));
        assert!(!is_invalid_body_handle(pack_body_handle(
            0xffff,
            1,
            0xffff_ffff
        )));
        assert!(!is_invalid_body_handle(pack_body_handle(0xffff, 0, 0)));
        // A body whose index happens to be 0xffff_ffff in world slot 0x1234 is
        // *valid*: the client tests the slot and the index separately.
        assert!(!is_invalid_body_handle(pack_body_handle(
            0x1234,
            0,
            0xffff_ffff
        )));
    }

    /// The slot bound is `< 0x3fff`, so `0x3ffe` is the last usable slot and a
    /// handle packing `0xffff` cannot name a world.
    #[test]
    fn the_slot_bound_is_strictly_below_0x3fff() {
        assert!(slot_indexes_a_world(0));
        assert!(slot_indexes_a_world(0x3ffe));
        assert!(!slot_indexes_a_world(0x3fff));
        assert!(!slot_indexes_a_world(0xffff));
    }

    /// The empty-slot sentinel is not an independent constant: it is the
    /// encoding of a **null** world pointer, so "no world" is spelled in the
    /// same alphabet as a live entry and decodes back to `0`. That is why the
    /// allocator can treat `entry == sentinel` as "this slot is free".
    #[test]
    fn the_empty_slot_is_the_encoding_of_a_null_world() {
        assert!(is_empty_slot(EMPTY_SLOT));
        assert_eq!(encode_world_pointer(0), EMPTY_SLOT);
        assert_eq!(decode_world_pointer(EMPTY_SLOT), 0);
        // A live world never encodes to the sentinel: the encoding is a
        // bijection, so only `0` maps to it.
        for world in [1u64, 0xdead_beef, 0xffff_ffff_ffff_ffff] {
            assert!(!is_empty_slot(encode_world_pointer(world)));
        }
        assert!(!is_empty_slot(0));
    }

    /// The body entry stride and the generation offset are the client's
    /// (`[world+0x288] + index*0x1c0`, generation at `+0x198`).
    #[test]
    fn the_body_pool_is_0x1c0_per_index() {
        assert_eq!(body_entry_offset(0), 0);
        assert_eq!(body_entry_offset(1), 0x1c0);
        assert_eq!(body_entry_offset(2), 0x380);
        assert_eq!(
            WORLD_BODY_POOL + body_entry_offset(2) + BODY_GENERATION,
            0x288 + 0x380 + 0x198
        );
    }
}
