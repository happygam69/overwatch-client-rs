//! The archive page cache behind `DynamicContainerReadPage` (`0x1424fcd20`).
//!
//! `DynamicContainerRead` (`0x1424f48d0`) turns a stored-record read into the
//! client's key-state request `(offset + 0x1e, len)`, then calls
//! `DynamicContainerReadPage`, which selects the archive, opens it through the
//! archive-handle cache, and hands the archive-relative range to the reader
//! object at `container+0xb38`. That object's cache lives at `reader+8` and is
//! built by `DynamicContainerReaderInit` (`0x1424fbf00`) and
//! `PageCacheInit` (`0x1424f8790`).
//!
//! ## Geometry
//!
//! The geometry is not read from the index header. The `.cmf` database open
//! params set the reader's slot count at `+0x78` and its page size at `+0x7c`
//! (`0x1424a6df0` copies the 0xa0-byte params struct to `reader+0x58`); the
//! open path at `0x141fd20f0` sets `+0x78 = 0x10` and `+0x7c = 0` ("use the
//! default"), and `0x1424a7d70` substitutes `0x10000` for a zero page size. So
//! this install runs **16 slots of 0x10000 bytes** (1 MiB).
//!
//! ## Key and entry
//!
//! A page is keyed by `(archive number, page-aligned archive offset)`
//! (`page_table+0xc` is the page size). The archive number is the qword at
//! `handle+0x18` that `DynamicContainerOpenArchive` (`0x14250c8f0`) stores when
//! it opens `data.NNN`; it is not a new per-open id. The slot carries the page
//! buffer, the number of bytes actually read from it, and a last-use stamp.
//!
//! ## Read plan
//!
//! `PageReadPlan` (`0x1424faa20`) splits a range into a head fragment (from the
//! offset to its page end), a run of whole pages, and a tail fragment. The
//! reader (`PageCacheRead`, `0x1424fb1c0`):
//!
//! 1. looks the head page up in the cache and copies the fragment on a hit;
//! 2. on a head miss loads the whole page (`0x1424fa540`), which copies the
//!    fragment and zero-fills any part the archive did not supply;
//! 3. reads the whole-page run with one **direct** read because the cache's
//!    non-blocking flag (`cache+0x10`, set to 1 by the open path) is set;
//! 4. looks the tail page up and loads it the same way, but only when the
//!    whole-page run was read completely.
//!
//! A range whose offset and length are both page-aligned never enters the
//! cache: `ArchiveRead` (`0x1424f9fe0`) sends it straight to the archive
//! handle. A zero length returns 0 before any I/O.
//!
//! ## Eviction
//!
//! `PageSlotFind` (`0x1424fa440`) scans the fixed 16-slot table. A usable slot
//! whose key matches *and* whose stored length covers the fragment is a hit;
//! its stamp is set to the incremented table clock (`page_table+0x20`). A miss
//! (including a key match whose stored length is too short) takes the usable
//! slot with the smallest stamp — the smallest `clock - stamp`, ties to the
//! lowest index — bumps its stamp, and reloads it. Slots marked busy/failed
//! (`+0x2e != 0`) are never evicted; with the non-blocking flag set, no usable
//! slot means the caller falls back to a direct read instead of waiting.
//!
//! The client's per-table lock, per-slot event and in-flight bit are real, but
//! a single-threaded reader cannot observe them. This module is synchronous and
//! keeps the state that survives a load: key, buffer, stored length and stamp.
//!
//! ## Statuses
//!
//! A page or direct read reports 0 with the number of bytes actually read; a
//! short read at the archive's end is not an error. A read failure is status 8
//! at `DynamicContainerReadPage` and the slot is returned to the empty usable
//! state. `DynamicContainerReadPage` itself returns 10 for an archive above
//! `0x3fe` or a range that crosses a 2^30 archive boundary; the caller owns
//! that check and this module only sees an archive-local range.

use std::io;

/// Page size used by this build's `.cmf` database open params.
pub const PAGE_SIZE: u32 = 0x10000;
/// Number of page slots (`params+0x78`).
pub const SLOT_COUNT: usize = 16;

/// One underlying archive read issued by the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadOp {
    /// `true` for a whole-page load, `false` for the direct whole-page run.
    pub page: bool,
    pub archive: u32,
    /// Archive-relative offset.
    pub offset: u64,
    pub len: u32,
}

/// What `PageCache::read` decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The cache handled the range; the value is the number of bytes written.
    Served(u32),
    /// The cache did not run; the caller must direct-read the whole range.
    Bypass,
}

/// One page slot (`0x70` bytes in the client; only the surviving state here).
#[derive(Debug)]
struct Slot {
    archive: u32,
    page: u64,
    data: Vec<u8>,
    /// `slot+0x28`: bytes actually read for this page.
    stored: u32,
    /// `slot+0x20`: last-use stamp.
    stamp: u64,
    /// `slot+0x2c`: a page has been loaded into this slot.
    populated: bool,
    /// `slot+0x2d`: invalidated while a load was in flight.
    invalid: bool,
    /// `slot+0x2e`: busy or failed; `false` is the usable state.
    failed: bool,
}

impl Slot {
    fn new() -> Slot {
        Slot {
            archive: 0,
            page: 0,
            data: vec![0u8; PAGE_SIZE as usize],
            stored: 0,
            stamp: 0,
            populated: false,
            invalid: false,
            failed: false,
        }
    }

    fn usable(&self) -> bool {
        !self.failed
    }

    fn reset(&mut self) {
        self.archive = 0;
        self.page = 0;
        self.stored = 0;
        self.populated = false;
        self.invalid = false;
        self.failed = false;
    }
}

/// The 16-slot page cache for one container reader.
pub struct PageCache {
    slots: Vec<Slot>,
    /// `page_table+0x20`: the monotonic last-use clock.
    clock: u64,
    /// `page_table+0x30`: the table is closed; `ArchiveRead` bypasses it.
    closed: bool,
    trace: Vec<ReadOp>,
    tracing: bool,
}

impl Default for PageCache {
    fn default() -> Self {
        PageCache::new()
    }
}

impl PageCache {
    /// A cache with the install's geometry and the database's non-blocking
    /// flag. All slots start empty and usable, exactly as
    /// `PageCacheInit` leaves them (`+0x2c = +0x2d = +0x2e = 0`).
    pub fn new() -> PageCache {
        PageCache {
            slots: (0..SLOT_COUNT).map(|_| Slot::new()).collect(),
            clock: 0,
            closed: false,
            trace: Vec::new(),
            tracing: false,
        }
    }

    /// Also record every read the cache itself issues. Off by default so a
    /// long-lived store does not grow a trace.
    pub fn with_trace() -> PageCache {
        let mut cache = PageCache::new();
        cache.tracing = true;
        cache
    }

    /// Turn trace recording on or off. Turning it off also drops the buffer.
    pub fn set_tracing(&mut self, enabled: bool) {
        self.tracing = enabled;
        if !enabled {
            self.trace.clear();
        }
    }

    /// Empty every slot and reset the last-use clock. Diagnostic/test helper;
    /// the client has no equivalent call.
    pub fn clear(&mut self) {
        for slot in &mut self.slots {
            slot.reset();
            slot.stamp = 0;
        }
        self.clock = 0;
    }

    /// `PageCacheClose` (`0x1424f9eb0`): mark the table closed and invalidate
    /// every populated slot. The read-only container mode 2 does not call it.
    pub fn close(&mut self) {
        self.closed = true;
        self.invalidate(None);
    }

    /// `PageCacheReopen` (`0x1424f9f90`): clear the closed marker.
    pub fn reopen(&mut self) {
        self.closed = false;
    }

    /// `PageCacheInvalidateAll` (`0x1424fab50`): invalidate every populated
    /// slot without touching the closed marker.
    pub fn invalidate_all(&mut self) {
        self.invalidate(None);
    }

    /// `PageCacheInvalidateRange` (`0x1424fabd0`): invalidate the slots whose
    /// page overlaps `[offset, offset + len)` in `archive`.
    pub fn invalidate_range(&mut self, archive: u32, offset: u64, len: u32) {
        self.invalidate(Some((archive, offset, len)));
    }

    fn invalidate(&mut self, range: Option<(u32, u64, u32)>) {
        for slot in &mut self.slots {
            if !slot.populated {
                continue;
            }
            if let Some((archive, offset, len)) = range {
                let page_start = slot.page;
                let page_end = page_start + u64::from(PAGE_SIZE);
                let start = offset;
                let end = offset + u64::from(len);
                if slot.archive != archive || !(start < page_end && page_start < end) {
                    continue;
                }
            }
            if slot.usable() {
                slot.archive = 0;
                slot.page = 0;
                slot.stored = 0;
                slot.populated = false;
            } else {
                slot.invalid = true;
            }
        }
    }

    /// Resident `(archive, page, stored bytes, stamp)` entries, lowest slot
    /// first. Diagnostics and tests; no read path uses it.
    pub fn resident(&self) -> Vec<(u32, u64, u32, u64)> {
        self.slots
            .iter()
            .filter(|slot| slot.populated)
            .map(|slot| (slot.archive, slot.page, slot.stored, slot.stamp))
            .collect()
    }

    /// The reads this cache issued since the last `clear_trace`.
    pub fn trace(&self) -> &[ReadOp] {
        &self.trace
    }

    pub fn clear_trace(&mut self) {
        self.trace.clear();
    }

    fn record(&mut self, page: bool, archive: u32, offset: u64, len: u32) {
        if self.tracing {
            self.trace.push(ReadOp {
                page,
                archive,
                offset,
                len,
            });
        }
    }

    /// Read `len` bytes at `offset` (archive-relative) for `archive`.
    ///
    /// `read_at` is one archive read at an absolute archive offset; a short
    /// return is the archive's end and is not an error. The caller must direct
    /// read the whole range on [`Outcome::Bypass`].
    pub fn read<F>(
        &mut self,
        archive: u32,
        offset: u64,
        len: u32,
        out: &mut [u8],
        mut read_at: F,
    ) -> io::Result<Outcome>
    where
        F: FnMut(u32, u64, &mut [u8]) -> io::Result<usize>,
    {
        assert!(out.len() >= len as usize, "output buffer smaller than len");
        if len == 0 {
            return Ok(Outcome::Served(0));
        }
        if self.closed {
            return Ok(Outcome::Bypass);
        }
        let mask = u64::from(PAGE_SIZE - 1);
        if offset & mask == 0 && u64::from(len) & mask == 0 {
            // `ArchiveRead` sends an aligned range straight to the handle.
            return Ok(Outcome::Bypass);
        }

        let plan = Plan::new(offset, len);
        let mut head_served = 0u32;
        let mut middle_served = 0u32;
        let mut tail_served = 0u32;

        if let Some(head) = plan.head {
            match self.serve_fragment(archive, head, out, &mut read_at)? {
                FragmentOutcome::Served(bytes) => head_served = bytes,
                FragmentOutcome::Bypass => return Ok(Outcome::Bypass),
            }
            if head_served < head.len {
                return Ok(Outcome::Served(head_served));
            }
        }

        if let Some(middle) = plan.middle {
            self.record(false, archive, middle.page, middle.len);
            let end = middle.dest as usize + middle.len as usize;
            middle_served =
                read_at(archive, middle.page, &mut out[middle.dest as usize..end])? as u32;
            if middle_served < middle.len {
                return Ok(Outcome::Served(head_served + middle_served));
            }
        }

        if let Some(tail) = plan.tail {
            match self.serve_fragment(archive, tail, out, &mut read_at)? {
                FragmentOutcome::Served(bytes) => tail_served = bytes,
                FragmentOutcome::Bypass => return Ok(Outcome::Bypass),
            }
        }

        Ok(Outcome::Served(head_served + middle_served + tail_served))
    }

    /// Serve one head or tail fragment, from a hit or by loading its page.
    fn serve_fragment<F>(
        &mut self,
        archive: u32,
        fragment: Fragment,
        out: &mut [u8],
        read_at: &mut F,
    ) -> io::Result<FragmentOutcome>
    where
        F: FnMut(u32, u64, &mut [u8]) -> io::Result<usize>,
    {
        let min_len = fragment.in_page + fragment.len;
        if let Some(index) = self.find(archive, fragment.page, min_len) {
            self.touch(index);
            copy_fragment(&self.slots[index], fragment, out);
            return Ok(FragmentOutcome::Served(fragment.len));
        }

        let Some(index) = self.claim(archive, fragment.page) else {
            // Every slot is busy/failed. The install's `cache+0x10` is 1
            // (non-blocking), so the client asks for a direct read instead of
            // waiting; the single-threaded reader never reaches this state.
            return Ok(FragmentOutcome::Bypass);
        };
        self.touch(index);
        self.load(index, archive, fragment.page, read_at)?;
        Ok(FragmentOutcome::Served(copy_fragment(
            &self.slots[index],
            fragment,
            out,
        )))
    }

    /// Find a usable slot whose key matches and whose stored length covers
    /// `min_len`. Returns `None` when the key matches a busy/failed slot or
    /// when the stored length is too short.
    fn find(&self, archive: u32, page: u64, min_len: u32) -> Option<usize> {
        self.slots.iter().position(|slot| {
            !slot.invalid
                && slot.archive == archive
                && slot.page == page
                && slot.usable()
                && slot.stored >= min_len
        })
    }

    /// Pick the usable slot with the smallest stamp (ties to the lowest
    /// index), ignoring busy/failed ones. This is the eviction rule of
    /// `PageSlotFind` (`0x1424fa440`): the largest `clock - stamp` wins.
    fn claim(&mut self, archive: u32, page: u64) -> Option<usize> {
        let mut best: Option<(usize, u64)> = None;
        for (index, slot) in self.slots.iter().enumerate() {
            if !slot.usable() {
                continue;
            }
            let distance = self.clock.wrapping_sub(slot.stamp);
            if best.is_none_or(|(_, best_distance)| distance > best_distance) {
                best = Some((index, distance));
            }
        }
        let (index, _) = best?;
        let slot = &mut self.slots[index];
        slot.archive = archive;
        slot.page = page;
        slot.stored = 0;
        slot.populated = true;
        slot.invalid = false;
        Some(index)
    }

    /// `page_table+0x20++`, `slot+0x20 = clock`.
    fn touch(&mut self, index: usize) {
        self.clock = self.clock.wrapping_add(1);
        self.slots[index].stamp = self.clock;
    }

    /// Read a whole page into the slot. A short read stores the bytes actually
    /// read; a failure resets the slot to the empty usable state, as the
    /// completion at `0x1424f9bf0` does.
    fn load<F>(&mut self, index: usize, archive: u32, page: u64, read_at: &mut F) -> io::Result<()>
    where
        F: FnMut(u32, u64, &mut [u8]) -> io::Result<usize>,
    {
        self.record(true, archive, page, PAGE_SIZE);
        let slot = &mut self.slots[index];
        slot.archive = archive;
        slot.page = page;
        slot.stored = 0;
        slot.populated = true;
        slot.invalid = false;
        slot.failed = true;
        match read_at(archive, page, &mut slot.data) {
            Ok(bytes) => {
                slot.stored = bytes as u32;
                slot.failed = false;
                Ok(())
            }
            Err(error) => {
                slot.reset();
                Err(error)
            }
        }
    }
}

/// The answer of one head/tail fragment service.
enum FragmentOutcome {
    Served(u32),
    Bypass,
}

fn copy_fragment(slot: &Slot, fragment: Fragment, out: &mut [u8]) -> u32 {
    let start = fragment.dest as usize;
    let end = start + fragment.len as usize;
    let page_offset = fragment.in_page as usize;
    let available = (slot.stored as usize).saturating_sub(page_offset);
    let copied = available.min(fragment.len as usize);
    out[start..start + copied].copy_from_slice(&slot.data[page_offset..page_offset + copied]);
    if copied < fragment.len as usize {
        out[start + copied..end].fill(0);
    }
    copied as u32
}

/// One head or tail fragment of the read plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fragment {
    pub page: u64,
    pub in_page: u32,
    pub len: u32,
    /// Destination address/offset stored by the client at `+0x30`/`+0x78`.
    pub dest: u64,
}

/// One whole-page run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Middle {
    pub page: u64,
    pub len: u32,
    pub dest: u64,
}

/// The client's head/middle/tail split (`PageReadPlan`, `0x1424faa20`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub head: Option<Fragment>,
    pub middle: Option<Middle>,
    pub tail: Option<Fragment>,
}

/// Size of the client's read-plan struct. The head subplan sits at `+0x30`,
/// the whole-page run at `+0x58..+0x77`, and the tail subplan at `+0x78`.
pub const PLAN_SIZE: usize = 0xb0;

fn get_u32(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().expect("4 bytes"))
}

fn get_u64(raw: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(raw[at..at + 8].try_into().expect("8 bytes"))
}

fn put_u32(raw: &mut [u8], at: usize, value: u32) {
    raw[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(raw: &mut [u8], at: usize, value: u64) {
    raw[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

/// Write the client's `0xb0`-byte read plan, field for field.
///
/// This is the body of `PageReadPlan` (`0x1424faa20`) with the refcounted
/// handle publication left to the caller: `handle` is the qword stored at
/// `+0x10` (the client skips the store when it already equals the field), and
/// `arg7`/`arg8` are stored at `+0x20`/`+0x28`. A field not written for a
/// branch is left untouched, exactly as the client leaves it; the production
/// caller passes a zeroed buffer.
///
/// Layout, verified against the function bytes:
///
/// ```text
/// +0x00 u64 id/state (written 0)      +0x08 u64 (untouched)
/// +0x10 u64 handle                    +0x18 u64 (untouched)
/// +0x20 u64 arg7                      +0x28 u64 arg8
/// +0x30 u64 head dest                 +0x38 u64 head page
/// +0x40 u32 head len                  +0x44 u32 (untouched: served)
/// +0x48 u64 (untouched: slot)         +0x50 u32 head in-page
/// +0x54 u8  (untouched)               +0x58 u8  middle flag (written 0)
/// +0x60 u64 middle page               +0x68 u64 middle dest
/// +0x70 u32 middle len                +0x74 u32 (untouched)
/// +0x78 u64 tail dest                 +0x80 u64 tail page
/// +0x88 u32 tail len                  +0x8c u32 (untouched: served)
/// +0x90 u64 (untouched: slot)         +0x98 u32 tail in-page (written 0)
/// +0x9c u8  (untouched)               +0xa0..+0xaf (untouched)
/// ```
#[allow(clippy::too_many_arguments)]
pub fn plan_read_into(
    page_size: u32,
    plan: &mut [u8],
    handle: u64,
    dest: u64,
    offset: u64,
    len: u64,
    arg7: u64,
    arg8: u64,
) {
    assert_eq!(plan.len(), PLAN_SIZE, "plan buffer must be 0xb0 bytes");
    put_u64(plan, 0x00, 0);
    put_u64(plan, 0x10, handle);
    put_u64(plan, 0x20, arg7);
    put_u64(plan, 0x28, arg8);

    let page_size = u64::from(page_size);
    let mask = page_size - 1;
    let mut page = offset & !mask;
    let in_page = (offset - page) as u32;
    let mut remaining = len as u32;
    let mut dest = dest;

    if in_page != 0 {
        let take = remaining.min((page_size - u64::from(in_page)) as u32);
        put_u64(plan, 0x30, dest);
        put_u64(plan, 0x38, page);
        put_u32(plan, 0x40, take);
        put_u32(plan, 0x50, in_page);
        dest += u64::from(take);
        remaining -= take;
        page += page_size;
    }

    if page_size as u32 <= remaining {
        let take = remaining & !(page_size as u32 - 1);
        plan[0x58] = 0;
        put_u64(plan, 0x60, page);
        put_u64(plan, 0x68, dest);
        put_u32(plan, 0x70, take);
        dest += u64::from(take);
        remaining -= take;
        page += u64::from(take);
    }

    if remaining != 0 {
        put_u64(plan, 0x78, dest);
        put_u64(plan, 0x80, page);
        put_u32(plan, 0x88, remaining);
        put_u32(plan, 0x98, 0);
    }
}

impl Plan {
    /// Split `len` bytes at `offset`. The arithmetic is the client's:
    /// `page = offset & ~(page_size - 1)`, the head runs to that page's end,
    /// the middle is `len & ~(page_size - 1)` after the head, and the tail is
    /// whatever is left, starting at the next page boundary.
    pub fn new(offset: u64, len: u32) -> Plan {
        let mut raw = [0u8; PLAN_SIZE];
        plan_read_into(PAGE_SIZE, &mut raw, 0, 0, offset, u64::from(len), 0, 0);

        let in_page = get_u32(&raw, 0x50);
        let head = (in_page != 0).then(|| Fragment {
            page: get_u64(&raw, 0x38),
            in_page,
            len: get_u32(&raw, 0x40),
            dest: get_u64(&raw, 0x30),
        });
        let middle_len = get_u32(&raw, 0x70);
        let middle = (middle_len != 0).then(|| Middle {
            page: get_u64(&raw, 0x60),
            len: middle_len,
            dest: get_u64(&raw, 0x68),
        });
        let tail_len = get_u32(&raw, 0x88);
        let tail = (tail_len != 0).then(|| Fragment {
            page: get_u64(&raw, 0x80),
            in_page: get_u32(&raw, 0x98),
            len: tail_len,
            dest: get_u64(&raw, 0x78),
        });

        Plan { head, middle, tail }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// A synthetic archive set that records every read, can return fewer bytes
    /// than asked (end of data) and can fail one call.
    struct Source {
        archives: BTreeMap<u32, Vec<u8>>,
        ops: Vec<(u32, u64, u32)>,
        fail_at: Option<(u32, u64)>,
        short_at: Option<(u32, u64, usize)>,
    }

    impl Source {
        fn new() -> Source {
            Source {
                archives: BTreeMap::new(),
                ops: Vec::new(),
                fail_at: None,
                short_at: None,
            }
        }

        fn add(&mut self, archive: u32, bytes: Vec<u8>) {
            self.archives.insert(archive, bytes);
        }

        fn read(&mut self, archive: u32, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
            self.ops.push((archive, offset, buf.len() as u32));
            if self.fail_at == Some((archive, offset)) {
                return Err(io::Error::other("synthetic read failure"));
            }
            let data = self.archives.get(&archive).expect("synthetic archive");
            let mut available = data.len().saturating_sub(offset as usize).min(buf.len());
            if let Some((short_archive, short_offset, short_len)) = self.short_at {
                if short_archive == archive && short_offset == offset {
                    available = available.min(short_len);
                }
            }
            if available != 0 {
                buf[..available]
                    .copy_from_slice(&data[offset as usize..offset as usize + available]);
            }
            Ok(available)
        }
    }

    fn page_pattern(page: u64, len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| ((page as usize + i) % 251) as u8)
            .collect()
    }

    /// A small unaligned read uses one whole-page read and returns the exact
    /// bytes; a second read of the same page is a hit with no I/O.
    #[test]
    fn an_unaligned_read_loads_one_page_then_hits() {
        let mut source = Source::new();
        let archive = page_pattern(0, 3 * PAGE_SIZE as usize);
        source.add(4, archive.clone());
        let mut cache = PageCache::with_trace();
        let offset = 0x1234u64;
        let len = 0x400u32;

        let mut out = vec![0u8; len as usize];
        let outcome = cache
            .read(4, offset, len, &mut out, |a, o, b| source.read(a, o, b))
            .expect("read");
        assert_eq!(outcome, Outcome::Served(len));
        assert_eq!(
            out[..],
            archive[offset as usize..offset as usize + len as usize]
        );
        assert_eq!(
            cache.trace(),
            &[ReadOp {
                page: true,
                archive: 4,
                offset: 0,
                len: PAGE_SIZE
            }]
        );
        assert_eq!(source.ops, vec![(4, 0, PAGE_SIZE)]);
        assert_eq!(cache.resident(), vec![(4, 0, PAGE_SIZE, 1)]);

        let mut out2 = vec![0u8; len as usize];
        let outcome = cache
            .read(4, offset, len, &mut out2, |a, o, b| source.read(a, o, b))
            .expect("second read");
        assert_eq!(outcome, Outcome::Served(len));
        assert_eq!(out2, out);
        assert_eq!(source.ops.len(), 1, "a hit must not touch the archive");
        assert_eq!(cache.resident(), vec![(4, 0, PAGE_SIZE, 2)]);
    }

    /// The alignment rule: offset and length both page multiples bypass the
    /// cache in one direct read.
    #[test]
    fn an_aligned_read_bypasses_the_cache() {
        let mut source = Source::new();
        let archive = page_pattern(0, 3 * PAGE_SIZE as usize);
        source.add(1, archive.clone());
        let mut cache = PageCache::with_trace();
        let offset = PAGE_SIZE as u64;
        let len = 2 * PAGE_SIZE;

        assert_eq!(
            cache
                .read(1, offset, len, &mut vec![0u8; len as usize], |a, o, b| {
                    source.read(a, o, b)
                })
                .expect("read"),
            Outcome::Bypass
        );
        assert!(cache.trace().is_empty());
        assert_eq!(cache.resident(), Vec::new());

        // An aligned offset with a short, unaligned length is not a bypass.
        assert_eq!(
            cache
                .read(1, offset, 0x100, &mut vec![0u8; 0x100], |a, o, b| source
                    .read(a, o, b))
                .expect("read"),
            Outcome::Served(0x100)
        );
        assert_eq!(cache.resident(), vec![(1, offset, PAGE_SIZE, 1)]);
    }

    /// An unaligned range spanning pages: head page, one direct whole-page
    /// run, tail page — each in the client's order.
    #[test]
    fn a_range_spanning_pages_uses_head_middle_and_tail() {
        let mut source = Source::new();
        let archive = page_pattern(0, 4 * PAGE_SIZE as usize);
        source.add(2, archive.clone());
        let mut cache = PageCache::with_trace();
        let offset = 0x100u64;
        let len = 2 * PAGE_SIZE + 0x200;
        let mut out = vec![0u8; len as usize];

        let outcome = cache
            .read(2, offset, len, &mut out, |a, o, b| source.read(a, o, b))
            .expect("read");
        assert_eq!(outcome, Outcome::Served(len));
        assert_eq!(
            out[..],
            archive[offset as usize..offset as usize + len as usize]
        );
        assert_eq!(
            cache.trace(),
            &[
                ReadOp {
                    page: true,
                    archive: 2,
                    offset: 0,
                    len: PAGE_SIZE
                },
                ReadOp {
                    page: false,
                    archive: 2,
                    offset: PAGE_SIZE as u64,
                    len: PAGE_SIZE
                },
                ReadOp {
                    page: true,
                    archive: 2,
                    offset: 2 * PAGE_SIZE as u64,
                    len: PAGE_SIZE
                },
            ]
        );
        // The source sees the cache's page loads plus the direct middle.
        assert_eq!(
            source.ops,
            vec![
                (2, 0, PAGE_SIZE),
                (2, PAGE_SIZE as u64, PAGE_SIZE),
                (2, 2 * PAGE_SIZE as u64, PAGE_SIZE),
            ]
        );
    }

    /// Seventeen distinct pages evict slot 0; reading page 0 again reloads it.
    #[test]
    fn eviction_is_least_recently_used() {
        let mut source = Source::new();
        let pages = SLOT_COUNT + 1;
        source.add(3, page_pattern(0, pages * PAGE_SIZE as usize));
        let mut cache = PageCache::with_trace();

        for index in 0..pages {
            let offset = index as u64 * PAGE_SIZE as u64 + 0x10;
            let mut out = vec![0u8; 0x10];
            assert_eq!(
                cache
                    .read(3, offset, 0x10, &mut out, |a, o, b| source.read(a, o, b))
                    .expect("read"),
                Outcome::Served(0x10)
            );
        }
        let resident = cache.resident();
        assert_eq!(resident.len(), SLOT_COUNT);
        assert!(
            resident.iter().all(|(_, page, _, _)| *page != 0),
            "page 0 must be the evicted slot"
        );
        assert!(resident
            .iter()
            .any(|(_, page, _, _)| *page == 16 * PAGE_SIZE as u64));

        source.ops.clear();
        let mut out = vec![0u8; 0x10];
        cache
            .read(3, 0x10, 0x10, &mut out, |a, o, b| source.read(a, o, b))
            .expect("reload");
        assert_eq!(source.ops, vec![(3, 0, PAGE_SIZE)], "page 0 reloads");
        // Page 1 was the next-oldest and is evicted by the reload.
        assert!(!cache
            .resident()
            .iter()
            .any(|(_, page, _, _)| *page == PAGE_SIZE as u64));
    }

    /// A hit requires the whole fragment to be inside the bytes the archive
    /// actually returned. A short last page therefore re-reads on every
    /// request that runs past it, and zero-fills the missing part.
    #[test]
    fn a_short_page_is_not_a_hit_and_zero_fills() {
        let mut source = Source::new();
        source.add(5, page_pattern(0, 0x1800));
        let mut cache = PageCache::with_trace();

        let offset = 0x1000u64;
        let len = 0x1000u32; // in_page 0x1000 + len > 0x1800
        let mut out = vec![0xAAu8; len as usize];
        let outcome = cache
            .read(5, offset, len, &mut out, |a, o, b| source.read(a, o, b))
            .expect("read");
        let served = 0x1800 - 0x1000;
        assert_eq!(outcome, Outcome::Served(served));
        assert_eq!(&out[..served as usize], &page_pattern(0, 0x1800)[0x1000..]);
        assert!(out[served as usize..].iter().all(|b| *b == 0));

        // The slot holds 0x1800 bytes; the same request is still a miss and
        // reads the page again.
        source.ops.clear();
        let mut out2 = vec![0u8; len as usize];
        let outcome = cache
            .read(5, offset, len, &mut out2, |a, o, b| source.read(a, o, b))
            .expect("second read");
        assert_eq!(outcome, Outcome::Served(served));
        assert_eq!(source.ops, vec![(5, 0, PAGE_SIZE)]);
        assert_eq!(cache.resident()[0].2, 0x1800);
    }

    /// A zero-length read reports 0 without touching the archive or the cache.
    #[test]
    fn a_zero_length_read_does_no_io() {
        let mut source = Source::new();
        source.add(6, page_pattern(0, PAGE_SIZE as usize));
        let mut cache = PageCache::with_trace();
        assert_eq!(
            cache
                .read(6, 0x1234, 0, &mut [], |a, o, b| source.read(a, o, b))
                .expect("read"),
            Outcome::Served(0)
        );
        assert!(source.ops.is_empty());
        assert!(cache.resident().is_empty());
    }

    /// A read failure is status 8; the slot returns to the empty usable state.
    #[test]
    fn an_io_error_resets_the_slot() {
        let mut source = Source::new();
        source.add(7, page_pattern(0, PAGE_SIZE as usize));
        source.fail_at = Some((7, 0));
        let mut cache = PageCache::with_trace();
        let mut out = vec![0u8; 0x40];

        let error = cache
            .read(7, 0x10, 0x40, &mut out, |a, o, b| source.read(a, o, b))
            .expect_err("synthetic failure");
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(cache.resident().is_empty(), "a failed load clears the slot");

        // The call does not poison the slot: the next read retries.
        source.fail_at = None;
        let outcome = cache
            .read(7, 0x10, 0x40, &mut out, |a, o, b| source.read(a, o, b))
            .expect("retry");
        assert_eq!(outcome, Outcome::Served(0x40));
    }

    /// A closed table bypasses the caller; reopening lets it cache again.
    #[test]
    fn a_closed_table_bypasses() {
        let mut source = Source::new();
        source.add(8, page_pattern(0, PAGE_SIZE as usize));
        let mut cache = PageCache::new();
        cache.close();
        assert_eq!(
            cache
                .read(8, 0x10, 0x10, &mut [0u8; 0x10], |a, o, b| source
                    .read(a, o, b))
                .expect("read"),
            Outcome::Bypass
        );
        cache.reopen();
        assert_eq!(
            cache
                .read(8, 0x10, 0x10, &mut [0u8; 0x10], |a, o, b| source
                    .read(a, o, b))
                .expect("read"),
            Outcome::Served(0x10)
        );
    }

    /// The plan arithmetic, pinned field by field.
    #[test]
    fn plan_splits_like_the_client() {
        let plan = Plan::new(0x123, 0x10);
        assert_eq!(
            plan.head,
            Some(Fragment {
                page: 0,
                in_page: 0x123,
                len: 0x10,
                dest: 0
            })
        );
        assert_eq!(plan.middle, None);
        assert_eq!(plan.tail, None);

        let plan = Plan::new(PAGE_SIZE as u64, 0x10);
        assert_eq!(plan.head, None);
        assert_eq!(plan.middle, None);
        assert_eq!(
            plan.tail,
            Some(Fragment {
                page: PAGE_SIZE as u64,
                in_page: 0,
                len: 0x10,
                dest: 0
            })
        );

        let plan = Plan::new(0x10, 2 * PAGE_SIZE + 0x20);
        assert_eq!(
            plan.head,
            Some(Fragment {
                page: 0,
                in_page: 0x10,
                len: PAGE_SIZE - 0x10,
                dest: 0
            })
        );
        assert_eq!(
            plan.middle,
            Some(Middle {
                page: PAGE_SIZE as u64,
                len: PAGE_SIZE,
                dest: u64::from(PAGE_SIZE) - 0x10
            })
        );
        assert_eq!(
            plan.tail,
            Some(Fragment {
                page: 2 * PAGE_SIZE as u64,
                in_page: 0,
                len: 0x30,
                dest: 2 * u64::from(PAGE_SIZE) - 0x10,
            })
        );
        assert_eq!(
            plan.head.unwrap().dest as u32
                + plan.head.unwrap().len
                + plan.middle.unwrap().len
                + plan.tail.unwrap().len,
            2 * PAGE_SIZE + 0x20
        );
    }
}
