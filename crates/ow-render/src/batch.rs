//! The engine's per-frame command records and the two batch buffers.
//!
//! These are **not** the `pm_dx11` replay stream. The engine records into a
//! segment ring at `cmdlist+0x58`; the replay consumes a flat list whose data
//! pointer is `cmdlist+0x18` and whose offset is `cmdlist+0x20`. The two
//! formats differ (id at `+0x00` with no size field versus opcode `+0x10` and
//! size `+0x14`), so there is no record-id -> replay-opcode translation.
//!
//! The read leaves are:
//!
//! | leaf | id | size | payload |
//! |---|---|---|---|
//! | `FUN_141fa7b70` (`0x141fa7b70`) | `0x30` | 4 | none |
//! | `FUN_141f9fe30` (`0x141f9fe30`) | `0x1be` | 4 | none |
//! | `FUN_141f02b10` (`0x141f02b10`) | `0x387` | `0x28` | batch base, frame slot, count |
//! | `FUN_141f03a80` (`0x141f03a80`) | `0x60` | `0x18` | object reference plus a tag |
//! | leaf in `0x141f99dd0` region | `0x352` | `0x18` | one batch item |
//!
//! `0x387` is a **batch submission**, not a draw. Batch A is a CPU→GPU upload
//! (`FUN_141f0edd0`) of two qwords per item; batch B is a GPU→CPU readback
//! (`FUN_141f0e5f0`) of one qword per item, gated by a per-item state.
//!
//! The item bytes are opaque: the read code never interprets the two A qwords
//! or the one B qword. A B value's visible/culled meaning is **inferred** from
//! the only consumer, `FUN_141efbcb0`, which returns `(*item == 0) + 1`.
//!
//! The record layout is bit-exact; the two padding dwords of `0x387`
//! (`+0x04`/`+0x24`) and of `0x60` (`+0x04`/`+0x12`) are uninitialised stack
//! bytes in the client and are deliberately not exposed here.

use std::fmt;

/// The `0x30` ring-toggle record id.
pub const RING_TOGGLE_ID: u32 = 0x30;
/// The `0x1be` ring-detach record id.
pub const RING_DETACH_ID: u32 = 0x1be;
/// The `0x387` batch record id.
pub const BATCH_ID: u32 = 0x387;
/// The `0x60` object-reference record id.
pub const VIEW_REFERENCE_ID: u32 = 0x60;
/// The `0x352` per-item record id.
pub const BATCH_ITEM_ID: u32 = 0x352;

/// The `-1` item state: not ready / never submitted.
pub const STATE_NOT_READY: u32 = 0xffff_ffff;
/// The `-2` item state: waiting for the GPU.
pub const STATE_PENDING: u32 = 0xffff_fffe;
/// The `-3` item state: resolved by an upload or a readback.
pub const STATE_READY: u32 = 0xffff_fffd;

/// A decode error. The client has no bounds check in the read path; the Rust
/// model rejects rather than reading outside the record or the batch array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchError {
    /// The byte slice is shorter than the record size.
    Truncated { len: usize, need: usize },
    /// The record id at `+0x00` is not the expected one.
    WrongRecordId { expected: u32, found: u32 },
    /// A batch count exceeds the read capacity.
    CountOverCapacity { count: u32, capacity: usize },
    /// A frame index is outside the three-frame slot set.
    FrameOutOfRange { frame: usize },
    /// The item slice is shorter than the count.
    ItemCountMismatch { count: u32, items: usize },
    /// The slot byte slice is shorter than `count * item_size`.
    SlotTooShort { count: u32, len: usize, need: usize },
    /// An item index is outside the batch capacity.
    ItemOutOfRange { index: usize, capacity: usize },
}

impl fmt::Display for BatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { len, need } => {
                write!(f, "record is {len} bytes, needs {need}")
            }
            Self::WrongRecordId { expected, found } => {
                write!(f, "record id 0x{found:08x}, expected 0x{expected:08x}")
            }
            Self::CountOverCapacity { count, capacity } => {
                write!(f, "batch count {count} exceeds capacity {capacity}")
            }
            Self::FrameOutOfRange { frame } => {
                write!(f, "frame index {frame} is outside 0..3")
            }
            Self::ItemCountMismatch { count, items } => {
                write!(f, "batch count {count} needs {count} items, got {items}")
            }
            Self::SlotTooShort { count, len, need } => {
                write!(
                    f,
                    "batch slot is {len} bytes, needs {need} for {count} items"
                )
            }
            Self::ItemOutOfRange { index, capacity } => {
                write!(f, "item index {index} is outside capacity {capacity}")
            }
        }
    }
}

impl std::error::Error for BatchError {}

/// One of the two per-frame batch buffers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Batch {
    /// `renderer+0x1a0`: CPU→GPU upload, 16 bytes per item, 2048 slots.
    A,
    /// `renderer+0xc1d0`: GPU→CPU readback, 8 bytes per item, 10240 slots.
    B,
}

impl Batch {
    /// The three frame slots.
    pub const FRAME_COUNT: usize = 3;

    /// The batch's base-object offset in the renderer.
    pub const fn base_offset(self) -> usize {
        match self {
            Self::A => 0x1a0,
            Self::B => 0xc1d0,
        }
    }

    /// The first frame-slot qword offset.
    pub const fn slot_offset(self) -> usize {
        match self {
            Self::A => 0x1a8,
            Self::B => 0xc1d8,
        }
    }

    /// The first frame-count dword offset.
    pub const fn count_offset(self) -> usize {
        match self {
            Self::A => 0x1c0,
            Self::B => 0xc1f0,
        }
    }

    /// The first item-pointer array qword offset.
    pub const fn item_array_offset(self) -> usize {
        match self {
            Self::A => 0x1d0,
            Self::B => 0xc200,
        }
    }

    /// The per-frame item capacity.
    pub const fn capacity(self) -> usize {
        match self {
            Self::A => 0x800,
            Self::B => 0x2800,
        }
    }

    /// The GPU item stride in bytes.
    pub const fn item_size(self) -> usize {
        match self {
            Self::A => 0x10,
            Self::B => 0x08,
        }
    }

    /// The byte offset of a frame slot.
    pub fn slot(self, frame: usize) -> Result<usize, BatchError> {
        self.check_frame(frame)?;
        Ok(self.slot_offset() + frame * 8)
    }

    /// The byte offset of a frame count.
    pub fn count(self, frame: usize) -> Result<usize, BatchError> {
        self.check_frame(frame)?;
        Ok(self.count_offset() + frame * 4)
    }

    /// The byte offset of the CPU pointer for one item in one frame.
    pub fn item_pointer(self, index: usize, frame: usize) -> Result<usize, BatchError> {
        self.check_frame(frame)?;
        if index >= self.capacity() {
            return Err(BatchError::ItemOutOfRange {
                index,
                capacity: self.capacity(),
            });
        }
        Ok(self.item_array_offset() + (index + frame * self.capacity()) * 8)
    }

    /// Validate a count against this batch's read capacity.
    pub fn check_count(self, count: u32) -> Result<(), BatchError> {
        if count as usize > self.capacity() {
            return Err(BatchError::CountOverCapacity {
                count,
                capacity: self.capacity(),
            });
        }
        Ok(())
    }

    fn check_frame(self, frame: usize) -> Result<(), BatchError> {
        if frame >= Self::FRAME_COUNT {
            return Err(BatchError::FrameOutOfRange { frame });
        }
        Ok(())
    }
}

/// The `0x387` batch record. The client writes six fields; the two padding
/// dwords are stack garbage and are not exposed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Batch387 {
    /// `+0x08`: the batch base object.
    pub base: u64,
    /// `+0x10`: the frame-slot resource.
    pub frame_slot: u64,
    /// `+0x18`: always zero at the two read call sites; meaning open.
    pub zero: u32,
    /// `+0x1c`: the number of items in this frame.
    pub count: u32,
    /// `+0x20`: always zero at the two read call sites; meaning open.
    pub zero2: u32,
}

impl Batch387 {
    /// The record size in bytes.
    pub const SIZE: usize = 0x28;

    /// Decode the six written fields. The padding dwords at `+0x04` and
    /// `+0x24` are ignored.
    pub fn decode(bytes: &[u8]) -> Result<Self, BatchError> {
        if bytes.len() < Self::SIZE {
            return Err(BatchError::Truncated {
                len: bytes.len(),
                need: Self::SIZE,
            });
        }
        let id = u32::from_le_bytes(bytes[0..4].try_into().expect("four bytes"));
        if id != BATCH_ID {
            return Err(BatchError::WrongRecordId {
                expected: BATCH_ID,
                found: id,
            });
        }
        Ok(Self {
            base: u64::from_le_bytes(bytes[0x08..0x10].try_into().expect("eight bytes")),
            frame_slot: u64::from_le_bytes(bytes[0x10..0x18].try_into().expect("eight bytes")),
            zero: u32::from_le_bytes(bytes[0x18..0x1c].try_into().expect("four bytes")),
            count: u32::from_le_bytes(bytes[0x1c..0x20].try_into().expect("four bytes")),
            zero2: u32::from_le_bytes(bytes[0x20..0x24].try_into().expect("four bytes")),
        })
    }

    /// Validate the count against the batch it names.
    pub fn validate_count(&self, batch: Batch) -> Result<(), BatchError> {
        batch.check_count(self.count)
    }
}

/// The `0x60` object-reference record. The record is generic: the frame-end
/// call site uses a view object, tag `0x0d` and value `0xffffffff`, but the
/// leaf has about twenty call sites.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewReference60 {
    /// `+0x08`: the referenced object.
    pub object: u64,
    /// `+0x10`: the 5-bit tag at the frame-end call site.
    pub tag: u8,
    /// `+0x11`: the second byte, `0x0d` at the frame-end call site.
    pub kind: u8,
    /// `+0x14`: the 32-bit value, `0xffffffff` at the frame-end call site.
    pub value: u32,
}

impl ViewReference60 {
    /// The record size in bytes.
    pub const SIZE: usize = 0x18;

    /// Decode the written fields. The padding bytes at `+0x04` and `+0x12`
    /// are ignored.
    pub fn decode(bytes: &[u8]) -> Result<Self, BatchError> {
        if bytes.len() < Self::SIZE {
            return Err(BatchError::Truncated {
                len: bytes.len(),
                need: Self::SIZE,
            });
        }
        let id = u32::from_le_bytes(bytes[0..4].try_into().expect("four bytes"));
        if id != VIEW_REFERENCE_ID {
            return Err(BatchError::WrongRecordId {
                expected: VIEW_REFERENCE_ID,
                found: id,
            });
        }
        Ok(Self {
            object: u64::from_le_bytes(bytes[0x08..0x10].try_into().expect("eight bytes")),
            tag: bytes[0x10],
            kind: bytes[0x11],
            value: u32::from_le_bytes(bytes[0x14..0x18].try_into().expect("four bytes")),
        })
    }
}

/// The `0x352` per-item record. The record carries the batch-B base and one
/// item field; the consumer that turns it into GPU work is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchItem352 {
    /// `+0x08`: the batch base, `renderer+0xc1d0` at the read leaf.
    pub base: u64,
    /// `+0x10`: zero at the read leaf.
    pub zero: u8,
    /// `+0x14`: the item field copied to the record.
    pub value: u32,
}

impl BatchItem352 {
    /// The record size in bytes.
    pub const SIZE: usize = 0x18;

    /// Decode the written fields. The padding dword at `+0x04` is ignored.
    pub fn decode(bytes: &[u8]) -> Result<Self, BatchError> {
        if bytes.len() < Self::SIZE {
            return Err(BatchError::Truncated {
                len: bytes.len(),
                need: Self::SIZE,
            });
        }
        let id = u32::from_le_bytes(bytes[0..4].try_into().expect("four bytes"));
        if id != BATCH_ITEM_ID {
            return Err(BatchError::WrongRecordId {
                expected: BATCH_ITEM_ID,
                found: id,
            });
        }
        Ok(Self {
            base: u64::from_le_bytes(bytes[0x08..0x10].try_into().expect("eight bytes")),
            zero: bytes[0x10],
            value: u32::from_le_bytes(bytes[0x14..0x18].try_into().expect("four bytes")),
        })
    }
}

/// One batch-A item: two opaque qwords and the `+0x18` state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchAItem {
    /// The two qwords copied verbatim to the GPU.
    pub bytes: [u8; 16],
    /// The item state at `+0x18`; `STATE_READY` after an upload.
    pub state: u32,
}

/// One batch-B item: one opaque qword and the `+0x10` state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchBItem {
    /// The one qword copied back from the GPU.
    pub value: u64,
    /// The item state at `+0x10`; `STATE_PENDING` before a readback.
    pub state: u32,
}

/// Model `FUN_141f0edd0`: upload `count` A items into the frame slot.
///
/// A null item pointer is skipped (the slot keeps its previous bytes) but is
/// still counted. Every copied item's state becomes `STATE_READY`.
pub fn upload_a(
    count: u32,
    items: &mut [Option<BatchAItem>],
    slot: &mut [u8],
) -> Result<(), BatchError> {
    let batch = Batch::A;
    batch.check_count(count)?;
    let count = count as usize;
    if items.len() < count {
        return Err(BatchError::ItemCountMismatch {
            count: count as u32,
            items: items.len(),
        });
    }
    let need = count * batch.item_size();
    if slot.len() < need {
        return Err(BatchError::SlotTooShort {
            count: count as u32,
            len: slot.len(),
            need,
        });
    }
    for (index, item) in items.iter_mut().take(count).enumerate() {
        if let Some(item) = item.as_mut() {
            let offset = index * batch.item_size();
            slot[offset..offset + batch.item_size()].copy_from_slice(&item.bytes);
            item.state = STATE_READY;
        }
    }
    Ok(())
}

/// Model `FUN_141f0e5f0`: read `count` B items back from the frame slot.
///
/// Only a non-null item in `STATE_PENDING` is copied; its state becomes
/// `STATE_READY`. Every other item is left untouched.
pub fn readback_b(
    count: u32,
    items: &mut [Option<BatchBItem>],
    slot: &[u8],
) -> Result<(), BatchError> {
    let batch = Batch::B;
    batch.check_count(count)?;
    let count = count as usize;
    if items.len() < count {
        return Err(BatchError::ItemCountMismatch {
            count: count as u32,
            items: items.len(),
        });
    }
    let need = count * batch.item_size();
    if slot.len() < need {
        return Err(BatchError::SlotTooShort {
            count: count as u32,
            len: slot.len(),
            need,
        });
    }
    for (index, item) in items.iter_mut().take(count).enumerate() {
        if let Some(item) = item.as_mut() {
            if item.state == STATE_PENDING {
                let offset = index * batch.item_size();
                item.value =
                    u64::from_le_bytes(slot[offset..offset + 8].try_into().expect("eight bytes"));
                item.state = STATE_READY;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch_record() -> Vec<u8> {
        let mut bytes = vec![0u8; Batch387::SIZE];
        bytes[0x00..0x04].copy_from_slice(&BATCH_ID.to_le_bytes());
        bytes[0x04..0x08].copy_from_slice(&0xdead_beefu32.to_le_bytes());
        bytes[0x08..0x10].copy_from_slice(&0x1111u64.to_le_bytes());
        bytes[0x10..0x18].copy_from_slice(&0x2222u64.to_le_bytes());
        bytes[0x18..0x1c].copy_from_slice(&0u32.to_le_bytes());
        bytes[0x1c..0x20].copy_from_slice(&3u32.to_le_bytes());
        bytes[0x20..0x24].copy_from_slice(&0u32.to_le_bytes());
        bytes[0x24..0x28].copy_from_slice(&0xcafe_babeu32.to_le_bytes());
        bytes
    }

    #[test]
    fn batch387_fields_are_exact_and_padding_is_ignored() {
        let decoded = Batch387::decode(&batch_record()).expect("valid record");
        assert_eq!(
            decoded,
            Batch387 {
                base: 0x1111,
                frame_slot: 0x2222,
                zero: 0,
                count: 3,
                zero2: 0,
            }
        );
        assert_eq!(Batch387::SIZE, 0x28);
    }

    #[test]
    fn batch387_rejects_short_and_wrong_id() {
        assert_eq!(
            Batch387::decode(&[0u8; 0x27]),
            Err(BatchError::Truncated {
                len: 0x27,
                need: 0x28,
            })
        );
        let mut bytes = batch_record();
        bytes[0x00..0x04].copy_from_slice(&0x386u32.to_le_bytes());
        assert_eq!(
            Batch387::decode(&bytes),
            Err(BatchError::WrongRecordId {
                expected: BATCH_ID,
                found: 0x386,
            })
        );
    }

    #[test]
    fn batch_layout_offsets_are_pinned() {
        assert_eq!(Batch::A.base_offset(), 0x1a0);
        assert_eq!(Batch::A.slot_offset(), 0x1a8);
        assert_eq!(Batch::A.count_offset(), 0x1c0);
        assert_eq!(Batch::A.item_array_offset(), 0x1d0);
        assert_eq!(Batch::A.capacity(), 0x800);
        assert_eq!(Batch::A.item_size(), 0x10);

        assert_eq!(Batch::B.base_offset(), 0xc1d0);
        assert_eq!(Batch::B.slot_offset(), 0xc1d8);
        assert_eq!(Batch::B.count_offset(), 0xc1f0);
        assert_eq!(Batch::B.item_array_offset(), 0xc200);
        assert_eq!(Batch::B.capacity(), 0x2800);
        assert_eq!(Batch::B.item_size(), 0x08);

        assert_eq!(Batch::A.slot(2).unwrap(), 0x1b8);
        assert_eq!(Batch::B.slot(2).unwrap(), 0xc1e8);
        assert_eq!(Batch::A.count(2).unwrap(), 0x1c8);
        assert_eq!(Batch::B.count(2).unwrap(), 0xc1f8);
        assert_eq!(Batch::A.item_pointer(0, 0).unwrap(), 0x1d0);
        assert_eq!(Batch::A.item_pointer(1, 0).unwrap(), 0x1d8);
        assert_eq!(Batch::A.item_pointer(0, 1).unwrap(), 0x41d0);
        assert_eq!(Batch::B.item_pointer(0, 0).unwrap(), 0xc200);
        assert_eq!(Batch::B.item_pointer(0, 1).unwrap(), 0x20200);
        assert_eq!(Batch::B.item_pointer(0, 2).unwrap(), 0x34200);
    }

    #[test]
    fn batch_frame_and_item_bounds_are_rejected() {
        assert_eq!(
            Batch::A.slot(3),
            Err(BatchError::FrameOutOfRange { frame: 3 })
        );
        assert_eq!(
            Batch::B.item_pointer(0x2800, 0),
            Err(BatchError::ItemOutOfRange {
                index: 0x2800,
                capacity: 0x2800,
            })
        );
    }

    #[test]
    fn batch_a_upload_copies_16_bytes_and_marks_ready() {
        let mut items = [
            Some(BatchAItem {
                bytes: [0x11; 16],
                state: STATE_PENDING,
            }),
            None,
            Some(BatchAItem {
                bytes: [0x33; 16],
                state: STATE_PENDING,
            }),
        ];
        let mut slot = [0xaau8; 48];
        upload_a(3, &mut items, &mut slot).expect("upload");
        assert_eq!(&slot[0..16], &[0x11; 16]);
        assert_eq!(&slot[16..32], &[0xaa; 16]);
        assert_eq!(&slot[32..48], &[0x33; 16]);
        assert_eq!(items[0].as_ref().unwrap().state, STATE_READY);
        assert_eq!(items[2].as_ref().unwrap().state, STATE_READY);
    }

    #[test]
    fn batch_b_readback_copies_only_pending_items() {
        let mut items = [
            Some(BatchBItem {
                value: 0,
                state: STATE_PENDING,
            }),
            Some(BatchBItem {
                value: 0x55,
                state: STATE_READY,
            }),
            Some(BatchBItem {
                value: 0,
                state: STATE_PENDING,
            }),
        ];
        let mut slot = [0u8; 24];
        slot[0..8].copy_from_slice(&0x1111u64.to_le_bytes());
        slot[8..16].copy_from_slice(&0x2222u64.to_le_bytes());
        slot[16..24].copy_from_slice(&0x3333u64.to_le_bytes());
        readback_b(3, &mut items, &slot).expect("readback");
        assert_eq!(items[0].as_ref().unwrap().value, 0x1111);
        assert_eq!(items[0].as_ref().unwrap().state, STATE_READY);
        assert_eq!(items[1].as_ref().unwrap().value, 0x55);
        assert_eq!(items[1].as_ref().unwrap().state, STATE_READY);
        assert_eq!(items[2].as_ref().unwrap().value, 0x3333);
        assert_eq!(items[2].as_ref().unwrap().state, STATE_READY);
    }

    #[test]
    fn over_capacity_counts_are_rejected_not_truncated() {
        let mut a_items = [];
        let mut b_items = [];
        let mut slot = [];
        assert_eq!(
            upload_a(0x801, &mut a_items, &mut slot),
            Err(BatchError::CountOverCapacity {
                count: 0x801,
                capacity: 0x800,
            })
        );
        assert_eq!(
            readback_b(0x2801, &mut b_items, &slot),
            Err(BatchError::CountOverCapacity {
                count: 0x2801,
                capacity: 0x2800,
            })
        );
    }

    #[test]
    fn view_reference_and_item_records_decode_their_read_fields() {
        let mut view = vec![0u8; ViewReference60::SIZE];
        view[0x00..0x04].copy_from_slice(&VIEW_REFERENCE_ID.to_le_bytes());
        view[0x08..0x10].copy_from_slice(&0x4444u64.to_le_bytes());
        view[0x10] = 5;
        view[0x11] = 0x0d;
        view[0x14..0x18].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        assert_eq!(
            ViewReference60::decode(&view),
            Ok(ViewReference60 {
                object: 0x4444,
                tag: 5,
                kind: 0x0d,
                value: 0xffff_ffff,
            })
        );

        let mut item = vec![0u8; BatchItem352::SIZE];
        item[0x00..0x04].copy_from_slice(&BATCH_ITEM_ID.to_le_bytes());
        item[0x08..0x10].copy_from_slice(&0x5555u64.to_le_bytes());
        item[0x10] = 0;
        item[0x14..0x18].copy_from_slice(&7u32.to_le_bytes());
        assert_eq!(
            BatchItem352::decode(&item),
            Ok(BatchItem352 {
                base: 0x5555,
                zero: 0,
                value: 7,
            })
        );
    }
}
