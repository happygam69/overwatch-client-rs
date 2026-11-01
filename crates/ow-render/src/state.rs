//! The replay's per-stage SRV/UAV caches and the `0x0c` alias-unbind model.
//!
//! The D3D11 replay keeps one cache per shader stage. The graphics SRV blocks
//! are laid out in memory **VS, PS, HS, DS, GS, CS** (slot tables `0x29a0`,
//! `0x49d0`, `0x6a00`, `0x8a30`, `0xaa60`, `0xca90`), while the opcodes
//! `0x16..0x1b` populate them in the API order VS, HS, DS, GS, PS, CS. The
//! `0x0c` alias loop uses the memory order VS, PS, HS, DS, GS.
//!
//! **SRV cache (`0x2030` bytes in the client).**
//!
//! | offset | meaning |
//! |---|---|
//! | `+0x1800` | queue pointer, `0x18`-byte entries |
//! | `+0x1808` | last-applied list, same entry shape |
//! | `+0x1810` / `+0x1814` | queue / last-applied counts |
//! | `+0x1818` | 128 shadow slots, `ViewDesc` (0x10 bytes) |
//! | `+0x2018` | 64-bit live bitmap (`BTR`/`BTS`, index `& 63`) |
//! | `+0x2020` / `+0x2024` | start / count, a bounding range |
//! | `+0x2028` | device pointer |
//!
//! **UAV cache (`0x1028` bytes).** Same shape with 64 slots at `+0xc18`,
//! live mask `+0x1018`, start/count `+0x1020`/`+0x1024`. The OM and CS blocks
//! are at `0xd2a8`/`0xeef0`.
//!
//! The live bitmap is 64 bits even though an SRV block has 128 shadow slots;
//! the client's `BTR RAX, RDI` uses the index modulo 64. This module reproduces
//! that exactly and rejects an index outside the shadow array rather than
//! reading past it.
//!
//! The queue is consumed by a binder that was not found in the image. This
//! module models the queue, the flushes that empty a live range, and the
//! `0x0c` alias-unbind that moves aliasing entries to the queue. It does not
//! pretend to bind the queued entries.

use std::fmt;

/// The six shader stages, with the client's stage ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    Vs = 0,
    Hs = 1,
    Ds = 2,
    Gs = 3,
    Ps = 6,
    Cs = 7,
}

impl Stage {
    /// The client's stage byte.
    pub const fn id(self) -> u8 {
        self as u8
    }

    /// The source name used in tests and logs.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Vs => "VS",
            Self::Hs => "HS",
            Self::Ds => "DS",
            Self::Gs => "GS",
            Self::Ps => "PS",
            Self::Cs => "CS",
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The memory order of the six SRV blocks. `0x0c` and the flushes use this
/// order; it is **not** the opcode order `0x16..0x1b`.
pub const MEMORY_ORDER: [Stage; 6] = [
    Stage::Vs,
    Stage::Ps,
    Stage::Hs,
    Stage::Ds,
    Stage::Gs,
    Stage::Cs,
];

/// The five graphics stages used by the `0x0c` alias loop.
pub const GRAPHICS_ORDER: [Stage; 5] = [Stage::Vs, Stage::Ps, Stage::Hs, Stage::Ds, Stage::Gs];

/// The two UAV caches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UavKind {
    /// Output-merger UAVs (`0xd2a8`).
    Om,
    /// Compute-shader UAVs (`0xeef0`).
    Cs,
}

/// One view descriptor. The client stores 16 bytes: view pointer, key, range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewDesc {
    /// `ID3D11View*`; never dereferenced by this model.
    pub view: usize,
    /// Compared for equality by `TestViewAlias`; the producer is open.
    pub key: u32,
    /// `mipFirst:4 | mipLast:4 << 4 | arrayFirst:11 << 8 | arrayLast:11 << 19`.
    pub range: u32,
}

impl ViewDesc {
    /// The first mip in the range.
    pub const fn mip_first(self) -> u32 {
        self.range & 0xf
    }

    /// The last mip in the range.
    pub const fn mip_last(self) -> u32 {
        (self.range >> 4) & 0xf
    }

    /// The first array slice.
    pub const fn array_first(self) -> u32 {
        (self.range >> 8) & 0x7ff
    }

    /// The last array slice.
    pub const fn array_last(self) -> u32 {
        (self.range >> 19) & 0x7ff
    }

    /// Build a descriptor from the four range fields. Values are masked to
    /// their client widths.
    pub const fn from_ranges(
        mip_first: u32,
        mip_last: u32,
        array_first: u32,
        array_last: u32,
    ) -> Self {
        Self {
            view: 0,
            key: 0,
            range: (mip_first & 0xf)
                | ((mip_last & 0xf) << 4)
                | ((array_first & 0x7ff) << 8)
                | ((array_last & 0x7ff) << 19),
        }
    }
}

/// The D3D side of the alias test: `ID3D11View::GetResource`.
pub trait ViewTable {
    /// Return the underlying `ID3D11Resource*` for a view, or 0.
    fn get_resource(&self, view: usize) -> usize;
}

/// `TestViewAlias` (`0x14265fb90`): equal key, overlapping mip and array
/// ranges, and the same underlying resource.
pub fn test_view_alias(a: &ViewDesc, b: &ViewDesc, table: &dyn ViewTable) -> bool {
    if a.view == 0 || b.view == 0 || a.key != b.key {
        return false;
    }
    if a.mip_first() > b.mip_last() || b.mip_first() > a.mip_last() {
        return false;
    }
    if a.array_first() > b.array_last() || b.array_first() > a.array_last() {
        return false;
    }
    table.get_resource(a.view) == table.get_resource(b.view)
}

/// One queued descriptor: the 16-byte view descriptor plus its slot index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueEntry {
    pub desc: ViewDesc,
    pub index: u32,
}

/// A per-stage SRV cache. The client has 128 shadow slots but a 64-bit live
/// bitmap; the model keeps both widths.
pub struct SrvCache {
    /// `+0x1800` / `+0x1810`: pending descriptors.
    pub queue: Vec<QueueEntry>,
    /// `+0x1808` / `+0x1814`: last-applied descriptors.
    pub applied: Vec<QueueEntry>,
    /// `+0x1818`: 128 shadow slots.
    pub slots: [ViewDesc; SRV_SLOTS],
    /// `+0x2018`: live bitmap. Bit `index & 63`.
    pub live: u64,
    /// `+0x2020`: lowest live slot.
    pub start: u32,
    /// `+0x2024`: highest live slot minus start, plus one.
    pub count: u32,
    /// `+0x2028`: the device pointer.
    pub device: usize,
}

/// The number of SRV shadow slots.
pub const SRV_SLOTS: usize = 128;

/// The number of UAV shadow slots.
pub const UAV_SLOTS: usize = 64;

impl SrvCache {
    /// An empty cache.
    pub fn new(device: usize) -> Self {
        Self {
            queue: Vec::new(),
            applied: Vec::new(),
            slots: [ViewDesc::default(); SRV_SLOTS],
            live: 0,
            start: 0,
            count: 0,
            device,
        }
    }

    /// Append a descriptor unless its index is already queued or applied.
    /// The client compares the index, not the whole entry.
    pub fn push_if_absent(&mut self, desc: ViewDesc, index: u32) {
        if self.queue.iter().any(|entry| entry.index == index) {
            return;
        }
        if self.applied.iter().any(|entry| entry.index == index) {
            return;
        }
        self.queue.push(QueueEntry { desc, index });
    }

    /// Recompute the bounding range from the 64-bit live bitmap.
    pub fn recompute(&mut self) {
        if self.live == 0 {
            self.start = 0;
            self.count = 0;
            return;
        }
        let low = self.live.trailing_zeros();
        let high = 63 - self.live.leading_zeros();
        self.start = low;
        self.count = high - low + 1;
    }

    /// One of the six identical SRV flushes
    /// (`0x142678bf0`/`0x142678a50`/`0x142678570`/`0x142678710`/
    /// `0x142678d90`/`0x1426788b0`).
    ///
    /// Queue every live slot, clear the live range, then issue one setter call
    /// with a full null array. A second call with `count == 0` issues nothing.
    pub fn flush<S: CacheSink>(&mut self, sink: &mut S, stage: Stage) {
        if self.count == 0 {
            return;
        }
        let start = self.start;
        let count = self.count;
        for offset in 0..count {
            let index = start + offset;
            let Some(slot) = self.slots.get(index as usize).copied() else {
                break;
            };
            if slot.view != 0 {
                self.push_if_absent(slot, index);
                self.slots[index as usize] = ViewDesc::default();
                self.live &= !(1u64 << (index & 63));
            }
        }
        let nulls = vec![0usize; count as usize];
        sink.set_shader_resources(stage, start, count, &nulls);
        self.start = 0;
        self.count = 0;
    }

    /// The per-stage part of `0x0c` (`0x14266d7a2..0x14266de71`).
    ///
    /// For each live slot that aliases `target`, append it to the queue when
    /// the replay mask bit is set, clear the slot and live bit, and issue one
    /// setter call for that single slot. The setter call happens even when the
    /// mask bit is clear.
    pub fn unbind_alias<S: CacheSink>(
        &mut self,
        target: ViewDesc,
        mask: u64,
        table: &dyn ViewTable,
        sink: &mut S,
        stage: Stage,
    ) {
        if self.count == 0 {
            return;
        }
        for offset in 0..self.count {
            let index = self.start + offset;
            let Some(slot) = self.slots.get(index as usize).copied() else {
                break;
            };
            if !test_view_alias(&slot, &target, table) {
                continue;
            }
            if (mask >> (index & 63)) & 1 != 0 {
                self.push_if_absent(slot, index);
            }
            self.slots[index as usize] = ViewDesc::default();
            self.live &= !(1u64 << (index & 63));
            sink.set_shader_resources(stage, index, 1, &[0]);
        }
        self.recompute();
    }
}

/// A per-stage UAV cache. Same shape as [`SrvCache`] with 64 slots.
pub struct UavCache {
    /// `+0xc00` / `+0xc10`: pending descriptors.
    pub queue: Vec<QueueEntry>,
    /// `+0xc08` / `+0xc14`: last-applied descriptors.
    pub applied: Vec<QueueEntry>,
    /// `+0xc18`: 64 shadow slots.
    pub slots: [ViewDesc; UAV_SLOTS],
    /// `+0x1018`: live bitmap.
    pub live: u64,
    /// `+0x1020`: lowest live slot.
    pub start: u32,
    /// `+0x1024`: bounding count.
    pub count: u32,
    /// `+0x2028`: the device pointer.
    pub device: usize,
}

impl UavCache {
    /// An empty cache.
    pub fn new(device: usize) -> Self {
        Self {
            queue: Vec::new(),
            applied: Vec::new(),
            slots: [ViewDesc::default(); UAV_SLOTS],
            live: 0,
            start: 0,
            count: 0,
            device,
        }
    }

    /// Append a descriptor unless its index is already queued or applied.
    pub fn push_if_absent(&mut self, desc: ViewDesc, index: u32) {
        if self.queue.iter().any(|entry| entry.index == index) {
            return;
        }
        if self.applied.iter().any(|entry| entry.index == index) {
            return;
        }
        self.queue.push(QueueEntry { desc, index });
    }

    /// Recompute the bounding range from the live bitmap.
    pub fn recompute(&mut self) {
        if self.live == 0 {
            self.start = 0;
            self.count = 0;
            return;
        }
        let low = self.live.trailing_zeros();
        let high = 63 - self.live.leading_zeros();
        self.start = low;
        self.count = high - low + 1;
    }

    /// The CS UAV full unbind (`0x142678f30`), also the shape `0x0c` inlines
    /// at `0x14266d524..0x14266d668`.
    pub fn flush<S: CacheSink>(&mut self, sink: &mut S, kind: UavKind) {
        if self.count == 0 {
            return;
        }
        let start = self.start;
        let count = self.count;
        for offset in 0..count {
            let index = start + offset;
            let Some(slot) = self.slots.get(index as usize).copied() else {
                break;
            };
            if slot.view != 0 {
                self.push_if_absent(slot, index);
                self.slots[index as usize] = ViewDesc::default();
                self.live &= !(1u64 << (index & 63));
            }
        }
        let nulls = vec![0usize; count as usize];
        sink.set_unordered_access_views(kind, start, count, &nulls);
        self.start = 0;
        self.count = 0;
    }
}

/// The `0x0d` resolve fields of one entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResolveInfo {
    /// `+0x11`: `0` release-only, `1` no-op, `2` resolve and release,
    /// `3` resolve-only.
    pub action: u8,
    /// `+0x18`: handle of the destination resource record.
    pub destination: u64,
    /// `+0x34` on the source view: the source subresource.
    pub source_subresource: u32,
}

/// One resolved `0x0c` render-target entry.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderTargetEntry {
    /// The view and its alias-test fields.
    pub desc: ViewDesc,
    /// `+0x10`: `0` release, `1` no-op, `2` clear.
    pub action: u8,
    /// `+0x20`: the clear colour.
    pub colour: [f32; 4],
    /// `+0x11`/`+0x18`/`+0x34`: the `0x0d` resolve fields.
    pub resolve: Option<ResolveInfo>,
}

/// One resolved `0x0c` depth-stencil entry.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DepthStencilEntry {
    /// The view and its alias-test fields.
    pub desc: ViewDesc,
    /// `+0x10`: `2` clears depth.
    pub depth_action: u8,
    /// `+0x20`: the depth clear value.
    pub depth: f32,
    /// `+0x28`: `2` clears stencil.
    pub stencil_action: u8,
    /// `+0x38`: the stencil clear value.
    pub stencil: u8,
    /// `+0x29`: the second release action.
    pub release_action: u8,
    /// `+0x11`/`+0x18`/`+0x34`: the `0x0d` resolve fields.
    pub resolve: Option<ResolveInfo>,
}

impl DepthStencilEntry {
    /// The `ClearDepthStencilView` flags:
    /// `(depth_action == 2 ? 1 : 0) | (stencil_action == 2 ? 2 : 0)`.
    pub const fn flags(&self) -> u32 {
        let depth = if self.depth_action == 2 { 1 } else { 0 };
        let stencil = if self.stencil_action == 2 { 2 } else { 0 };
        depth | stencil
    }
}

/// The sink for the cache model's exact setter calls. The real D3D11 sink
/// resolves the handles through its resource table; the recording sink in the
/// tests is the oracle.
pub trait CacheSink {
    /// `*SetShaderResources(stage, start, count, views)`.
    fn set_shader_resources(&mut self, stage: Stage, start: u32, count: u32, views: &[usize]);

    /// `*SetUnorderedAccessViews(kind, start, count, views)`.
    fn set_unordered_access_views(
        &mut self,
        kind: UavKind,
        start: u32,
        count: u32,
        views: &[usize],
    );

    /// `OMSetRenderTargets(views, dsv)`.
    fn set_render_targets(&mut self, views: &[usize], dsv: Option<usize>);
}

/// A sink that can also execute the `0x0c` clears and pm releases.
pub trait TargetSink: CacheSink {
    /// `ClearRenderTargetView(view, colour)`.
    fn clear_render_target_view(&mut self, view: usize, colour: [f32; 4]);

    /// The pm release slot `+0x3b0` for an RTV action `0`.
    fn release_render_target_view(&mut self, view: usize);

    /// `ClearDepthStencilView(view, flags, depth, stencil)`.
    fn clear_depth_stencil_view(&mut self, view: usize, flags: u32, depth: f32, stencil: u8);

    /// `ResolveSubresource(destination, source view, format)`.
    fn resolve_subresource(
        &mut self,
        destination: u64,
        source_view: usize,
        source_subresource: u32,
    );
}

/// The complete `0x0c` operation on resolved descriptors: per-RTV clear or
/// release, the DSV clear, the alias-unbind, then the final
/// `OMSetRenderTargets`.
#[allow(clippy::too_many_arguments)]
pub fn apply_render_targets<S: TargetSink>(
    graphics: &mut [SrvCache; 5],
    cs_srv: &mut SrvCache,
    cs_uav: &mut UavCache,
    masks: [u64; 5],
    targets: &[RenderTargetEntry],
    depth: Option<DepthStencilEntry>,
    table: &dyn ViewTable,
    sink: &mut S,
) {
    for target in targets {
        match target.action {
            0 => sink.release_render_target_view(target.desc.view),
            2 => sink.clear_render_target_view(target.desc.view, target.colour),
            _ => {}
        }
    }
    if let Some(depth) = depth {
        let flags = depth.flags();
        if flags != 0 {
            sink.clear_depth_stencil_view(depth.desc.view, flags, depth.depth, depth.stencil);
        }
    }

    let views: Vec<ViewDesc> = targets.iter().map(|target| target.desc).collect();
    op_0c_alias_unbind(
        graphics,
        cs_srv,
        cs_uav,
        masks,
        &views,
        depth.map(|depth| depth.desc),
        table,
        sink,
    );
}

/// The `0x0d` pending-target resolve, release and final unbind.
///
/// The client loops the saved RTV entries and then the saved DSV. The resolve
/// action at `+0x11` drives the work: `0` releases only, `1` does nothing,
/// `2` resolves and releases, `3` resolves only. The final
/// `OMSetRenderTargets(0, NULL, NULL)` is unconditional.
pub fn resolve_pending_targets<S: TargetSink>(
    targets: &[RenderTargetEntry],
    depth: Option<DepthStencilEntry>,
    sink: &mut S,
) {
    for target in targets {
        if let Some(resolve) = target.resolve {
            if resolve.action == 2 || resolve.action == 3 {
                sink.resolve_subresource(
                    resolve.destination,
                    target.desc.view,
                    resolve.source_subresource,
                );
            }
            if resolve.action == 0 || resolve.action == 2 {
                sink.release_render_target_view(target.desc.view);
            }
        }
    }
    if let Some(depth) = depth {
        if let Some(resolve) = depth.resolve {
            if resolve.action == 2 || resolve.action == 3 {
                sink.resolve_subresource(
                    resolve.destination,
                    depth.desc.view,
                    resolve.source_subresource,
                );
            }
            if (resolve.action == 0 || resolve.action == 2)
                && (depth.release_action == 0 || depth.release_action == 2)
            {
                sink.release_render_target_view(depth.desc.view);
            }
        }
    }
    sink.set_render_targets(&[], None);
}

/// The `0x0c` alias-unbind and final bind, operation for operation.
///
/// `graphics` is in [`GRAPHICS_ORDER`] (VS, PS, HS, DS, GS), `masks` follows
/// the same order and is the `0xf378`/`0xf388`/`0xf3a0`/`0xf3b0`/`0xf3c0`
/// replay mask. `targets` are the new RTV descriptors, `dsv` the new depth
/// descriptor. The CS UAV and CS SRV ranges are flushed unconditionally before
/// the per-target loop.
pub fn op_0c_alias_unbind<S: CacheSink>(
    graphics: &mut [SrvCache; 5],
    cs_srv: &mut SrvCache,
    cs_uav: &mut UavCache,
    masks: [u64; 5],
    targets: &[ViewDesc],
    dsv: Option<ViewDesc>,
    table: &dyn ViewTable,
    sink: &mut S,
) {
    cs_uav.flush(sink, UavKind::Cs);
    cs_srv.flush(sink, Stage::Cs);

    for target in targets.iter().copied().chain(dsv) {
        for (index, stage) in GRAPHICS_ORDER.iter().copied().enumerate() {
            graphics[index].unbind_alias(target, masks[index], table, sink, stage);
        }
    }

    let views: Vec<usize> = targets.iter().map(|target| target.view).collect();
    sink.set_render_targets(&views, dsv.map(|target| target.view));
}

/// The replay's cache state: five graphics SRV caches, the CS SRV cache, the
/// CS UAV cache and the five replay masks.
pub struct ReplayState {
    pub graphics: [SrvCache; 5],
    pub cs_srv: SrvCache,
    pub cs_uav: UavCache,
    pub masks: [u64; 5],
}

impl ReplayState {
    /// An empty state.
    pub fn new() -> Self {
        Self {
            graphics: std::array::from_fn(|_| SrvCache::new(0)),
            cs_srv: SrvCache::new(0),
            cs_uav: UavCache::new(0),
            masks: [0; 5],
        }
    }

    /// Apply one resolved `0x0c` target set through this state.
    pub fn apply_targets<S: TargetSink>(
        &mut self,
        targets: &[RenderTargetEntry],
        depth: Option<DepthStencilEntry>,
        table: &dyn ViewTable,
        sink: &mut S,
    ) {
        apply_render_targets(
            &mut self.graphics,
            &mut self.cs_srv,
            &mut self.cs_uav,
            self.masks,
            targets,
            depth,
            table,
            sink,
        );
    }
}

impl Default for ReplayState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct TestViews {
        resources: HashMap<usize, usize>,
    }

    impl TestViews {
        fn new(entries: &[(usize, usize)]) -> Self {
            Self {
                resources: entries.iter().copied().collect(),
            }
        }
    }

    impl ViewTable for TestViews {
        fn get_resource(&self, view: usize) -> usize {
            self.resources.get(&view).copied().unwrap_or(0)
        }
    }

    #[derive(Debug, PartialEq)]
    enum Call {
        Srv {
            stage: Stage,
            start: u32,
            count: u32,
            views: Vec<usize>,
        },
        Uav {
            kind: UavKind,
            start: u32,
            count: u32,
            views: Vec<usize>,
        },
        RenderTargets {
            views: Vec<usize>,
            dsv: Option<usize>,
        },
        ClearRtv {
            view: usize,
            colour: [f32; 4],
        },
        ReleaseRtv {
            view: usize,
        },
        ClearDsv {
            view: usize,
            flags: u32,
            depth: f32,
            stencil: u8,
        },
        Resolve {
            destination: u64,
            source_view: usize,
            source_subresource: u32,
        },
    }

    #[derive(Default)]
    struct RecordingSink {
        calls: Vec<Call>,
    }

    impl CacheSink for RecordingSink {
        fn set_shader_resources(&mut self, stage: Stage, start: u32, count: u32, views: &[usize]) {
            self.calls.push(Call::Srv {
                stage,
                start,
                count,
                views: views.to_vec(),
            });
        }

        fn set_unordered_access_views(
            &mut self,
            kind: UavKind,
            start: u32,
            count: u32,
            views: &[usize],
        ) {
            self.calls.push(Call::Uav {
                kind,
                start,
                count,
                views: views.to_vec(),
            });
        }

        fn set_render_targets(&mut self, views: &[usize], dsv: Option<usize>) {
            self.calls.push(Call::RenderTargets {
                views: views.to_vec(),
                dsv,
            });
        }
    }

    impl TargetSink for RecordingSink {
        fn clear_render_target_view(&mut self, view: usize, colour: [f32; 4]) {
            self.calls.push(Call::ClearRtv { view, colour });
        }

        fn release_render_target_view(&mut self, view: usize) {
            self.calls.push(Call::ReleaseRtv { view });
        }

        fn clear_depth_stencil_view(&mut self, view: usize, flags: u32, depth: f32, stencil: u8) {
            self.calls.push(Call::ClearDsv {
                view,
                flags,
                depth,
                stencil,
            });
        }

        fn resolve_subresource(
            &mut self,
            destination: u64,
            source_view: usize,
            source_subresource: u32,
        ) {
            self.calls.push(Call::Resolve {
                destination,
                source_view,
                source_subresource,
            });
        }
    }

    fn view(view: usize, key: u32) -> ViewDesc {
        ViewDesc {
            view,
            key,
            range: ViewDesc::from_ranges(0, 0, 0, 0).range,
        }
    }

    #[test]
    fn alias_truth_table_matches_the_client() {
        let table = TestViews::new(&[(0x11, 0x900), (0x22, 0x900), (0x33, 0x901), (0x44, 0x902)]);

        let a = view(0x11, 7);
        assert!(test_view_alias(&a, &view(0x22, 7), &table));
        assert!(!test_view_alias(&a, &view(0x22, 8), &table));
        assert!(!test_view_alias(&a, &view(0x33, 7), &table));

        let mip0 = ViewDesc {
            view: 0x11,
            key: 7,
            range: ViewDesc::from_ranges(0, 0, 0, 0).range,
        };
        let mip1 = ViewDesc {
            view: 0x11,
            key: 7,
            range: ViewDesc::from_ranges(1, 1, 0, 0).range,
        };
        assert!(!test_view_alias(&mip0, &mip1, &table));

        let array01 = ViewDesc {
            view: 0x11,
            key: 7,
            range: ViewDesc::from_ranges(0, 0, 0, 1).range,
        };
        let array23 = ViewDesc {
            view: 0x11,
            key: 7,
            range: ViewDesc::from_ranges(0, 0, 2, 3).range,
        };
        let array03 = ViewDesc {
            view: 0x11,
            key: 7,
            range: ViewDesc::from_ranges(0, 0, 0, 3).range,
        };
        let array27 = ViewDesc {
            view: 0x11,
            key: 7,
            range: ViewDesc::from_ranges(0, 0, 2, 7).range,
        };
        assert!(!test_view_alias(&array01, &array23, &table));
        assert!(test_view_alias(&array03, &array27, &table));
        assert!(!test_view_alias(&ViewDesc::default(), &a, &table));
        assert!(!test_view_alias(&view(0x11, 7), &view(0x44, 7), &table));
    }

    #[test]
    fn range_bitfield_round_trips_at_the_boundaries() {
        let zero = ViewDesc::from_ranges(0, 0, 0, 0);
        assert_eq!(
            (
                zero.mip_first(),
                zero.mip_last(),
                zero.array_first(),
                zero.array_last()
            ),
            (0, 0, 0, 0)
        );

        let max = ViewDesc::from_ranges(0xf, 0xf, 0x7ff, 0x7ff);
        assert_eq!(max.range, 0x3fff_ffff);
        assert_eq!(
            (
                max.mip_first(),
                max.mip_last(),
                max.array_first(),
                max.array_last()
            ),
            (0xf, 0xf, 0x7ff, 0x7ff)
        );
    }

    #[test]
    fn srv_flush_is_idempotent_and_preserves_the_queue() {
        let mut cache = SrvCache::new(0x100);
        cache.start = 3;
        cache.count = 2;
        cache.slots[3] = view(0x11, 1);
        cache.slots[4] = view(0x22, 2);
        cache.live = (1 << 3) | (1 << 4);
        cache.queue.push(QueueEntry {
            desc: view(0x11, 1),
            index: 3,
        });

        let mut sink = RecordingSink::default();
        cache.flush(&mut sink, Stage::Vs);
        assert_eq!(
            sink.calls,
            vec![Call::Srv {
                stage: Stage::Vs,
                start: 3,
                count: 2,
                views: vec![0, 0],
            }]
        );
        assert_eq!(cache.start, 0);
        assert_eq!(cache.count, 0);
        assert_eq!(cache.live, 0);
        assert_eq!(cache.slots[3], ViewDesc::default());
        assert_eq!(cache.slots[4], ViewDesc::default());
        assert_eq!(cache.queue.len(), 2);
        assert_eq!(cache.queue[0].index, 3);
        assert_eq!(cache.queue[1].index, 4);

        cache.flush(&mut sink, Stage::Vs);
        assert_eq!(sink.calls.len(), 1);
    }

    #[test]
    fn uav_flush_uses_the_range_setter() {
        let mut cache = UavCache::new(0x100);
        cache.start = 2;
        cache.count = 2;
        cache.slots[3] = view(0x11, 1);
        cache.live = 1 << 3;

        let mut sink = RecordingSink::default();
        cache.flush(&mut sink, UavKind::Cs);
        assert_eq!(
            sink.calls,
            vec![Call::Uav {
                kind: UavKind::Cs,
                start: 2,
                count: 2,
                views: vec![0, 0],
            }]
        );
        assert_eq!(cache.start, 0);
        assert_eq!(cache.count, 0);
        assert_eq!(cache.live, 0);
        assert_eq!(cache.queue.len(), 1);
        assert_eq!(cache.queue[0].index, 3);
    }

    #[test]
    fn op_0c_unbinds_in_the_read_order() {
        let table = TestViews::new(&[(0x11, 0x900), (0x33, 0x900), (0x44, 0x901), (0x22, 0x901)]);
        let mut graphics = std::array::from_fn(|_| SrvCache::new(0x100));
        graphics[0].start = 5;
        graphics[0].count = 1;
        graphics[0].slots[5] = view(0x11, 7);
        graphics[0].live = 1 << 5;
        graphics[1].start = 9;
        graphics[1].count = 1;
        graphics[1].slots[9] = view(0x44, 8);
        graphics[1].live = 1 << 9;

        let mut cs_srv = SrvCache::new(0x100);
        let mut cs_uav = UavCache::new(0x100);
        let mut sink = RecordingSink::default();

        op_0c_alias_unbind(
            &mut graphics,
            &mut cs_srv,
            &mut cs_uav,
            [1 << 5, 1 << 9, 0, 0, 0],
            &[view(0x33, 7), view(0x22, 8)],
            None,
            &table,
            &mut sink,
        );

        assert_eq!(
            sink.calls,
            vec![
                Call::Srv {
                    stage: Stage::Vs,
                    start: 5,
                    count: 1,
                    views: vec![0],
                },
                Call::Srv {
                    stage: Stage::Ps,
                    start: 9,
                    count: 1,
                    views: vec![0],
                },
                Call::RenderTargets {
                    views: vec![0x33, 0x22],
                    dsv: None,
                },
            ]
        );
        assert_eq!(graphics[0].queue.len(), 1);
        assert_eq!(graphics[1].queue.len(), 1);
    }

    #[test]
    fn op_0c_mask_clear_still_calls_the_setter_without_queueing() {
        let table = TestViews::new(&[(0x11, 0x900), (0x33, 0x900)]);
        let mut graphics = std::array::from_fn(|_| SrvCache::new(0x100));
        graphics[0].start = 5;
        graphics[0].count = 1;
        graphics[0].slots[5] = view(0x11, 7);
        graphics[0].live = 1 << 5;

        let mut cs_srv = SrvCache::new(0x100);
        let mut cs_uav = UavCache::new(0x100);
        let mut sink = RecordingSink::default();

        op_0c_alias_unbind(
            &mut graphics,
            &mut cs_srv,
            &mut cs_uav,
            [0, 0, 0, 0, 0],
            &[view(0x33, 7)],
            None,
            &table,
            &mut sink,
        );

        assert_eq!(
            sink.calls[0],
            Call::Srv {
                stage: Stage::Vs,
                start: 5,
                count: 1,
                views: vec![0],
            }
        );
        assert!(graphics[0].queue.is_empty());
        assert_eq!(graphics[0].count, 0);
    }

    #[test]
    fn op_0c_dsv_only_still_binds_the_depth_view() {
        let table = TestViews::new(&[(0x44, 0x901), (0x55, 0x901)]);
        let mut graphics = std::array::from_fn(|_| SrvCache::new(0x100));
        graphics[1].start = 2;
        graphics[1].count = 1;
        graphics[1].slots[2] = view(0x44, 8);
        graphics[1].live = 1 << 2;

        let mut cs_srv = SrvCache::new(0x100);
        let mut cs_uav = UavCache::new(0x100);
        let mut sink = RecordingSink::default();

        op_0c_alias_unbind(
            &mut graphics,
            &mut cs_srv,
            &mut cs_uav,
            [0, 1 << 2, 0, 0, 0],
            &[],
            Some(view(0x55, 8)),
            &table,
            &mut sink,
        );

        assert_eq!(
            sink.calls,
            vec![
                Call::Srv {
                    stage: Stage::Ps,
                    start: 2,
                    count: 1,
                    views: vec![0],
                },
                Call::RenderTargets {
                    views: vec![],
                    dsv: Some(0x55),
                },
            ]
        );
    }

    #[test]
    fn depth_flags_are_the_read_expression() {
        let mut entry = DepthStencilEntry {
            desc: ViewDesc::default(),
            depth_action: 2,
            depth: 1.0,
            stencil_action: 2,
            stencil: 7,
            ..Default::default()
        };
        assert_eq!(entry.flags(), 3);
        entry.stencil_action = 1;
        assert_eq!(entry.flags(), 1);
        entry.depth_action = 1;
        entry.stencil_action = 2;
        assert_eq!(entry.flags(), 2);
        entry.stencil_action = 1;
        assert_eq!(entry.flags(), 0);
    }

    #[test]
    fn apply_render_targets_clears_releases_and_binds_in_order() {
        let table = TestViews::new(&[(0x11, 0x900), (0x33, 0x900), (0x44, 0x901), (0x55, 0x901)]);
        let mut graphics = std::array::from_fn(|_| SrvCache::new(0x100));
        graphics[0].start = 1;
        graphics[0].count = 1;
        graphics[0].slots[1] = view(0x11, 7);
        graphics[0].live = 1 << 1;
        graphics[1].start = 4;
        graphics[1].count = 1;
        graphics[1].slots[4] = view(0x44, 9);
        graphics[1].live = 1 << 4;

        let mut cs_srv = SrvCache::new(0x100);
        let mut cs_uav = UavCache::new(0x100);
        let mut sink = RecordingSink::default();
        let targets = [
            RenderTargetEntry {
                desc: view(0x33, 7),
                action: 0,
                colour: [0.0; 4],
                ..Default::default()
            },
            RenderTargetEntry {
                desc: view(0x22, 8),
                action: 2,
                colour: [0.25, 0.5, 0.75, 1.0],
                ..Default::default()
            },
        ];
        let depth = DepthStencilEntry {
            desc: view(0x55, 9),
            depth_action: 2,
            depth: 1.0,
            stencil_action: 2,
            stencil: 7,
            ..Default::default()
        };

        apply_render_targets(
            &mut graphics,
            &mut cs_srv,
            &mut cs_uav,
            [1 << 1, 1 << 4, 0, 0, 0],
            &targets,
            Some(depth),
            &table,
            &mut sink,
        );

        assert_eq!(
            sink.calls,
            vec![
                Call::ReleaseRtv { view: 0x33 },
                Call::ClearRtv {
                    view: 0x22,
                    colour: [0.25, 0.5, 0.75, 1.0],
                },
                Call::ClearDsv {
                    view: 0x55,
                    flags: 3,
                    depth: 1.0,
                    stencil: 7,
                },
                Call::Srv {
                    stage: Stage::Vs,
                    start: 1,
                    count: 1,
                    views: vec![0],
                },
                Call::Srv {
                    stage: Stage::Ps,
                    start: 4,
                    count: 1,
                    views: vec![0],
                },
                Call::RenderTargets {
                    views: vec![0x33, 0x22],
                    dsv: Some(0x55),
                },
            ]
        );
    }

    #[test]
    fn resolve_pending_targets_follows_the_action_matrix() {
        let targets = [
            RenderTargetEntry {
                desc: view(0x11, 1),
                resolve: Some(ResolveInfo {
                    action: 2,
                    destination: 0x100,
                    source_subresource: 1,
                }),
                ..Default::default()
            },
            RenderTargetEntry {
                desc: view(0x22, 2),
                resolve: Some(ResolveInfo {
                    action: 0,
                    destination: 0,
                    source_subresource: 0,
                }),
                ..Default::default()
            },
            RenderTargetEntry {
                desc: view(0x33, 3),
                resolve: Some(ResolveInfo {
                    action: 1,
                    destination: 0,
                    source_subresource: 0,
                }),
                ..Default::default()
            },
            RenderTargetEntry {
                desc: view(0x44, 4),
                resolve: Some(ResolveInfo {
                    action: 3,
                    destination: 0x103,
                    source_subresource: 3,
                }),
                ..Default::default()
            },
        ];
        let depth = DepthStencilEntry {
            desc: view(0x55, 5),
            release_action: 2,
            resolve: Some(ResolveInfo {
                action: 2,
                destination: 0x104,
                source_subresource: 4,
            }),
            ..Default::default()
        };
        let mut sink = RecordingSink::default();
        resolve_pending_targets(&targets, Some(depth), &mut sink);
        assert_eq!(
            sink.calls,
            vec![
                Call::Resolve {
                    destination: 0x100,
                    source_view: 0x11,
                    source_subresource: 1,
                },
                Call::ReleaseRtv { view: 0x11 },
                Call::ReleaseRtv { view: 0x22 },
                Call::Resolve {
                    destination: 0x103,
                    source_view: 0x44,
                    source_subresource: 3,
                },
                Call::Resolve {
                    destination: 0x104,
                    source_view: 0x55,
                    source_subresource: 4,
                },
                Call::ReleaseRtv { view: 0x55 },
                Call::RenderTargets {
                    views: vec![],
                    dsv: None,
                },
            ]
        );
    }
}
