//! The read client renderer, native backend.
//!
//! The production path is **D3D11**. Its device and swap-chain creation
//! (stage D1), its swap-chain resources and views (stage D2), its per-frame
//! order (stage D3) and its command replay dispatch plus the read clear /
//! draw / viewport handlers (stages D4 and D5) are read from `Overwatch.exe`
//! and implemented here; the D3D12 path is the client's beta path and is not
//! implemented in this crate.
//!
//! The module is Windows-only because it links the real D3D11 and DXGI entry
//! points. It carries no `winit` dependency: the caller passes a window handle
//! and a size, so the same code serves the client and the headless smoke
//! example.

#![cfg(windows)]

pub mod batch;
pub mod d3d11;
pub mod frame;
pub mod replay;
pub mod request;
pub mod state;

pub use batch::{
    readback_b, upload_a, Batch, Batch387, BatchAItem, BatchBItem, BatchError, BatchItem352,
    ViewReference60, BATCH_ID, BATCH_ITEM_ID, RING_DETACH_ID, RING_TOGGLE_ID, STATE_NOT_READY,
    STATE_PENDING, STATE_READY, VIEW_REFERENCE_ID,
};
pub use d3d11::{
    Device, Error, SurfaceDescription, SwapChainViewDescription, WindowSurface, D1_CLEAR_COLOUR,
    STAGE, STAGES, VIEW_STAGE,
};
pub use frame::{FrameLogEntry, Step};
pub use replay::{
    D3d11Sink, Error as ReplayError, Handler, Opcode, Record, RecordHeader, RenderTargetSet,
    ResourceTable, Sink, Summary, OPCODE_COUNT, OPCODE_TABLE, RECORD_HEADER_SIZE,
    RECORD_OPCODE_OFFSET, RECORD_SIZE_OFFSET,
};
pub use request::{
    view_format, DeviceRequest, RenderTargetViewDesc, ShaderResourceViewDesc, SwapChainRequest,
    SwapChainViewRequest, SwapResource, UnorderedAccessViewDesc, ViewStep, VIEW_FORMAT_TABLE,
};
pub use state::{
    op_0c_alias_unbind, test_view_alias, CacheSink, DepthStencilEntry, QueueEntry,
    RenderTargetEntry, ReplayState, SrvCache, Stage, TargetSink, UavCache, UavKind, ViewDesc,
    ViewTable, GRAPHICS_ORDER, MEMORY_ORDER, SRV_SLOTS, UAV_SLOTS,
};
