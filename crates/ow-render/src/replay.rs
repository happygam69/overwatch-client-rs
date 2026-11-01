//! The client's D3D11 command replay: record header, 64-opcode dispatch and
//! the read handlers (stages D4 and D5).
//!
//! The dispatch is `FUN_14266c690` (`0x14266c690`). A record is a 0x18-byte
//! header followed by its payload:
//!
//! | offset | width | meaning |
//! |---|---|---|
//! | `+0x00` | 8 | segment link, read only when the read offset crosses the segment capacity |
//! | `+0x08` | 4 | segment capacity |
//! | `+0x10` | 4 | 32-bit opcode, little-endian |
//! | `+0x14` | 4 | record byte size, little-endian, **includes the header** |
//! | `+0x18` | .. | payload |
//!
//! Opcodes `0x00..=0x3f` jump through the 64-entry table at
//! [`OPCODE_TABLE`] (`0x142671154`). An opcode above `0x3f` calls the
//! missing-opcode log at [`MISSING_OPCODE_LOG`] (`0x1434983d0`,
//! `D3D11: missing op code: %u`) and the replay continues with the next
//! record. Opcode `0x00` is the end/flush record: it runs the nine stage-cache
//! flush helpers and returns from the replay. The executor models it as
//! end-of-stream because it defers no bindings, so there is nothing to flush.
//!
//! The payload conventions are the client's:
//!
//! - all integers are little-endian;
//! - an 8-byte value is a client pointer/handle; the executor treats it as an
//!   opaque key and never dereferences it, and the D3D11 sink resolves it
//!   through [`ResourceTable`];
//! - a 16-byte block is four `f32` values (colour, blend factor) or four `u32`
//!   values (UAV clear), depending on the opcode;
//! - a viewport is six `f32` values: x, y, width, height, min depth, max
//!   depth;
//! - a scissor is left/top/width/height and the client passes
//!   `left, top, left + width, top + height`.
//!
//! **What is implemented.** The header parse, the full 64-opcode space, the
//! missing-opcode path and the executor are implemented. The handlers whose
//! `ID3D11DeviceContext` call and arguments are fully read and self-contained
//! are executed by [`D3d11Sink`]: `0x00` (end), `0x07`
//! `ClearRenderTargetView`, `0x08` `ClearDepthStencilView`, `0x0c`
//! `RenderTargetSet`, `0x1d` (no-op), `0x25` `Draw`/`DrawInstanced`, `0x26`
//! `DrawIndexed`/`DrawIndexedInstanced`, `0x29` `RSSetViewports`, `0x2a`
//! `RSSetScissorRects` and `0x30` `Dispatch`.
//!
//! **Batch `0x0c..=0x1b`: read.** `0x0c`/`0x0d` are the render-target
//! bind/unbind pair; the other fourteen carry no self-contained context call:
//!
//! - `0x0c` is the render-target set: a count at `+0x18`, an RTV array pointer
//!   at `+0x20` (0x30-byte entries: pm view `+0x00`, native RTV through
//!   `[view+0x10]`, action `+0x10`, colour `+0x20`), a DSV block at `+0x28`
//!   (native DSV through `[view+0x20]`, depth action `+0x10`, depth `+0x20`,
//!   stencil action `+0x28`, stencil `+0x38`) and a state id at `+0x30`. Each
//!   entry clears through `+0x190` or releases through pm `+0x3b0`; the DSV
//!   clears through `+0x1a8`; then `OMSetRenderTargets(count, array, dsv)` at
//!   `0x14266e54d`.
//! - `0x0d` applies the pending state built by `0x0c`: `ResolveSubresource`
//!   (`+0x1c8`) per MSAA entry, then `OMSetRenderTargets(0, NULL, NULL)`
//!   (`+0x108`) at `0x14266e6ba`. It has no record payload.
//! - `0x0e` resets the five graphics constant-buffer counters and six
//!   resource-cache counters, flushes the UAV cache through `0x1426790e0`,
//!   and copies a 0x58-byte state block from `[record+0x18]+0x28`.
//! - `0x0f` applies a state block at `[record+0x18]` through the input-layout,
//!   topology, shader, depth-stencil, blend and rasterizer setters.
//! - `0x10..=0x15` are the six constant-buffer binds (VS, HS, DS, GS, PS, CS):
//!   start slot `+0x1c`, count `+0x20`, a 0x20-byte descriptor array at
//!   `+0x28`; `0x142676130` runs `SyncBufferForUsage` per element and defers
//!   the bind to the end-record flush `0x142660620`.
//! - `0x16..=0x1b` are the six shader-resource binds in the same stage order:
//!   start slot `+0x1c`, count `+0x20`, a 0x20-byte descriptor array at
//!   `+0x28`; `0x1426781b0` runs `SyncBufferForUsage` per element and the
//!   bind is deferred to the per-stage flush (`0x142678bf0` and siblings).
//!
//! In every one of the sixteen the record holds a pointer into a state block
//! or a descriptor array, and the resource objects behind it are not in the
//! record. The executor logs the opcode and never fakes the call.
//!
//! **What is open.** `0x0c` is implemented: the executor carries the resolved
//! RTV/DSV handles to [`D3d11Sink::set_render_target_set`], which uses the
//! [`crate::state`] model for the clears, alias-unbind and final bind. `0x0d`
//! is read and its resolve/release path is documented, but its pending-state
//! wiring is the next step. The per-stage caches are modelled but not yet
//! populated by `0x16..=0x1b` and the UAV path, and the queued-SRV binder is
//! not read. The other 54 opcodes are [`Handler::Open`] with the read gap
//! named. Many carry an out-of-band pointer (a state block, a
//! descriptor array, a query, a string) or need the pm resource table
//! (`SyncBufferForUsage`), which this crate does not have. The executor logs
//! them and never fakes the operation. `FUN_142674980` is a per-draw
//! UAV/CB/SRV state flush reached from `0x25`/`0x26`/`0x27`/`0x28`; it is not
//! the render-target bind and never calls `OMSetRenderTargets`.
//!
//! **No capture oracle.** A capture of the client is impossible because of the
//! anti-cheat, so the proof is the read expression plus the synthetic record
//! streams in this module's tests. The recording sink is the oracle; no GPU
//! and no client capture is involved. The D3D11 sink's COM calls are not
//! exercised by a test because there is no read record stream and no client
//! capture.

use std::collections::HashMap;
use std::fmt;

use windows::core::Interface;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11DepthStencilView, ID3D11DeviceContext, ID3D11RenderTargetView, ID3D11Resource,
    ID3D11ShaderResourceView, ID3D11UnorderedAccessView, D3D11_VIEWPORT,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;

use crate::state::{
    resolve_pending_targets, CacheSink, DepthStencilEntry, RenderTargetEntry, ReplayState, Stage,
    TargetSink, UavKind, ViewTable,
};

/// The replay stage.
pub const STAGE: &str = "D4";
/// The clear / bind / viewport handler stage.
pub const HANDLER_STAGE: &str = "D5";
/// The number of opcodes in the client's table.
pub const OPCODE_COUNT: usize = 64;
/// The current-image address of the 64-entry dispatch table.
pub const OPCODE_TABLE: u64 = 0x0001_4267_1154;
/// The current-image address of the `D3D11: missing op code: %u` string.
pub const MISSING_OPCODE_LOG: u64 = 0x0001_4349_83d0;
/// The record opcode offset.
pub const RECORD_OPCODE_OFFSET: usize = 0x10;
/// The record size offset.
pub const RECORD_SIZE_OFFSET: usize = 0x14;
/// The record header size; the payload starts here.
pub const RECORD_HEADER_SIZE: usize = 0x18;

/// The client's 64-opcode space. The discriminant is the opcode byte.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Opcode {
    End = 0x00,
    Op01 = 0x01,
    Op02 = 0x02,
    Op03 = 0x03,
    Op04 = 0x04,
    Op05 = 0x05,
    Op06 = 0x06,
    ClearRenderTargetView = 0x07,
    ClearDepthStencilView = 0x08,
    Op09 = 0x09,
    Op0a = 0x0a,
    Op0b = 0x0b,
    Op0c = 0x0c,
    Op0d = 0x0d,
    Op0e = 0x0e,
    Op0f = 0x0f,
    Op10 = 0x10,
    Op11 = 0x11,
    Op12 = 0x12,
    Op13 = 0x13,
    Op14 = 0x14,
    Op15 = 0x15,
    Op16 = 0x16,
    Op17 = 0x17,
    Op18 = 0x18,
    Op19 = 0x19,
    Op1a = 0x1a,
    Op1b = 0x1b,
    Op1c = 0x1c,
    NoOp = 0x1d,
    Op1e = 0x1e,
    Op1f = 0x1f,
    Op20 = 0x20,
    Op21 = 0x21,
    Op22 = 0x22,
    Op23 = 0x23,
    Op24 = 0x24,
    Draw = 0x25,
    DrawIndexed = 0x26,
    Op27 = 0x27,
    Op28 = 0x28,
    SetViewports = 0x29,
    SetScissorRects = 0x2a,
    Op2b = 0x2b,
    Op2c = 0x2c,
    Op2d = 0x2d,
    Op2e = 0x2e,
    Op2f = 0x2f,
    Dispatch = 0x30,
    Op31 = 0x31,
    Op32 = 0x32,
    Op33 = 0x33,
    Op34 = 0x34,
    Op35 = 0x35,
    Op36 = 0x36,
    Op37 = 0x37,
    Op38 = 0x38,
    Op39 = 0x39,
    Op3a = 0x3a,
    Op3b = 0x3b,
    Op3c = 0x3c,
    Op3d = 0x3d,
    Op3e = 0x3e,
    Op3f = 0x3f,
}

impl Opcode {
    /// Every opcode in table order.
    pub const ALL: [Opcode; OPCODE_COUNT] = [
        Opcode::End,
        Opcode::Op01,
        Opcode::Op02,
        Opcode::Op03,
        Opcode::Op04,
        Opcode::Op05,
        Opcode::Op06,
        Opcode::ClearRenderTargetView,
        Opcode::ClearDepthStencilView,
        Opcode::Op09,
        Opcode::Op0a,
        Opcode::Op0b,
        Opcode::Op0c,
        Opcode::Op0d,
        Opcode::Op0e,
        Opcode::Op0f,
        Opcode::Op10,
        Opcode::Op11,
        Opcode::Op12,
        Opcode::Op13,
        Opcode::Op14,
        Opcode::Op15,
        Opcode::Op16,
        Opcode::Op17,
        Opcode::Op18,
        Opcode::Op19,
        Opcode::Op1a,
        Opcode::Op1b,
        Opcode::Op1c,
        Opcode::NoOp,
        Opcode::Op1e,
        Opcode::Op1f,
        Opcode::Op20,
        Opcode::Op21,
        Opcode::Op22,
        Opcode::Op23,
        Opcode::Op24,
        Opcode::Draw,
        Opcode::DrawIndexed,
        Opcode::Op27,
        Opcode::Op28,
        Opcode::SetViewports,
        Opcode::SetScissorRects,
        Opcode::Op2b,
        Opcode::Op2c,
        Opcode::Op2d,
        Opcode::Op2e,
        Opcode::Op2f,
        Opcode::Dispatch,
        Opcode::Op31,
        Opcode::Op32,
        Opcode::Op33,
        Opcode::Op34,
        Opcode::Op35,
        Opcode::Op36,
        Opcode::Op37,
        Opcode::Op38,
        Opcode::Op39,
        Opcode::Op3a,
        Opcode::Op3b,
        Opcode::Op3c,
        Opcode::Op3d,
        Opcode::Op3e,
        Opcode::Op3f,
    ];

    /// The opcode byte.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// The opcode for a byte in `0..=0x3f`, or `None` for the missing-opcode
    /// path.
    pub const fn from_u8(value: u8) -> Option<Self> {
        if value < OPCODE_COUNT as u8 {
            Some(Self::ALL[value as usize])
        } else {
            None
        }
    }

    /// The handler-table target for this opcode, read from `0x142671154`.
    pub const fn target(self) -> u64 {
        HANDLER_TARGETS[self as usize]
    }

    /// The read status of this opcode.
    pub const fn handler(self) -> Handler {
        HANDLERS[self as usize]
    }

    /// The short name used in logs and docs.
    pub const fn name(self) -> &'static str {
        OPCODE_NAMES[self as usize]
    }
}

/// Whether a handler's context call and arguments are fully read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handler {
    /// The sink method named here is the read client call.
    Implemented(&'static str),
    /// The handler is read only up to the named gap; the executor logs it and
    /// does not call an operation.
    Open(&'static str),
}

/// The 64 table targets, in opcode order. Read from the dwords at
/// `0x142671154` (each is `0x140000000 + RVA`).
pub const HANDLER_TARGETS: [u64; OPCODE_COUNT] = [
    0x1426_7102d,
    0x1426_6c805,
    0x1426_6ca72,
    0x1426_6cc1a,
    0x1426_6ce03,
    0x1426_6d0f4,
    0x1426_6d16b,
    0x1426_6d1ab,
    0x1426_6d1ed,
    0x1426_6d22d,
    0x1426_6d298,
    0x1426_6d303,
    0x1426_6d36b,
    0x1426_6e55c,
    0x1426_6e6d3,
    0x1426_6e7a7,
    0x1426_6e9da,
    0x1426_6ea13,
    0x1426_6ea4c,
    0x1426_6ea85,
    0x1426_6eabe,
    0x1426_6eaf7,
    0x1426_6eb30,
    0x1426_6ec66,
    0x1426_6eda6,
    0x1426_6eee6,
    0x1426_6f026,
    0x1426_6f166,
    0x1426_6f2a6,
    0x1426_6c736,
    0x1426_6f323,
    0x1426_6f466,
    0x1426_6f5a6,
    0x1426_6f659,
    0x1426_6f71a,
    0x1426_6f7dd,
    0x1426_6f919,
    0x1426_701c9,
    0x1426_70267,
    0x1426_70320,
    0x1426_70409,
    0x1426_704b7,
    0x1426_70538,
    0x1426_705a8,
    0x1426_705dc,
    0x1426_70639,
    0x1426_7067b,
    0x1426_706e5,
    0x1426_7072a,
    0x1426_70789,
    0x1426_70803,
    0x1426_709ae,
    0x1426_70a25,
    0x1426_70a53,
    0x1426_70a9d,
    0x1426_70af9,
    0x1426_70b49,
    0x1426_70c3a,
    0x1426_70c9c,
    0x1426_70d02,
    0x1426_70da3,
    0x1426_70e14,
    0x1426_70ed8,
    0x1426_70f3b,
];

const OPCODE_NAMES: [&str; OPCODE_COUNT] = [
    "End",
    "Op01",
    "Op02",
    "Op03",
    "Op04",
    "Op05",
    "Op06",
    "ClearRenderTargetView",
    "ClearDepthStencilView",
    "Op09",
    "Op0a",
    "Op0b",
    "Op0c",
    "Op0d",
    "Op0e",
    "Op0f",
    "Op10",
    "Op11",
    "Op12",
    "Op13",
    "Op14",
    "Op15",
    "Op16",
    "Op17",
    "Op18",
    "Op19",
    "Op1a",
    "Op1b",
    "Op1c",
    "NoOp",
    "Op1e",
    "Op1f",
    "Op20",
    "Op21",
    "Op22",
    "Op23",
    "Op24",
    "Draw",
    "DrawIndexed",
    "Op27",
    "Op28",
    "SetViewports",
    "SetScissorRects",
    "Op2b",
    "Op2c",
    "Op2d",
    "Op2e",
    "Op2f",
    "Dispatch",
    "Op31",
    "Op32",
    "Op33",
    "Op34",
    "Op35",
    "Op36",
    "Op37",
    "Op38",
    "Op39",
    "Op3a",
    "Op3b",
    "Op3c",
    "Op3d",
    "Op3e",
    "Op3f",
];

const HANDLERS: [Handler; OPCODE_COUNT] = [
    Handler::Implemented("end"),
    Handler::Open(
        "pm resource handle at +0x18 and a second qword at +0x20; SyncBufferForUsage then \
         context +0x18/+0x170/+0x70/+0x78/+0x10 (update/map/copy/unmap); the resource table \
         and the map/copy path are not implemented",
    ),
    Handler::Open(
        "format-size helpers 0x14265b2c0/0x141f122f0 and a resource descriptor at +0x18; the \
         payload maps a texture update whose resource table is not implemented",
    ),
    Handler::Open(
        "UpdateSubresource at context +0x180; the source data and box are out-of-band pointers, \
         so the record bytes alone cannot form the call",
    ),
    Handler::Open(
        "Map/Unmap record: resource handle at +0x18, fields at +0x20/+0x30/+0x40, helpers \
         0x14265b2c0/0x141f122f0/0x142669b10, then context +0x70 (Map) and +0x78 (Unmap); the \
         resource table and map/unmap path are not implemented. (The earlier ClearDepthStencilView \
         mapping was wrong: the depth clear is 0x08, not 0x04.)",
    ),
    Handler::Open(
        "two resource handles at +0x18/+0x20 resolved through SyncBufferForUsage; CopyResource \
         at context +0x178; the resource table is not implemented",
    ),
    Handler::Open(
        "pm resource pointers at +0x18/+0x20 dereferenced at +0x8; CopyResource at context \
         +0x178; the resource table is not implemented",
    ),
    Handler::Implemented("clear_render_target_view"),
    Handler::Implemented("clear_depth_stencil_view"),
    Handler::Open(
        "ClearUnorderedAccessViewFloat (+0x1a0): UAV view at +0x18, four f32 at +0x20; the \
         preceding SyncBufferForUsage uses the out-of-band resource descriptor at +0x10 and \
         usage word at +0x38",
    ),
    Handler::Open(
        "ClearUnorderedAccessViewUint (+0x198): UAV view at +0x18, four u32 at +0x20; the \
         preceding SyncBufferForUsage uses the out-of-band resource descriptor at +0x10 and \
         usage word at +0x38",
    ),
    Handler::Open(
        "ResolveSubresource (+0x1c8): destination descriptor at +0x18 and source descriptor at \
         +0x20 (each dereferenced at +0x8/+0x34), format from the table 0x1434965d0; the \
         resource descriptors are out-of-band pointers",
    ),
    Handler::Implemented("set_render_target_set"),
    Handler::Implemented("resolve_render_target_set"),
    Handler::Open(
        "graphics-state reset: zeroes five constant-buffer counters and six resource-cache \
         counters, flushes the UAV cache through 0x1426790e0 \
         (OMSetRenderTargetsAndUnorderedAccessViews +0x110), then copies the 0x58-byte state \
         block at [record+0x18]+0x28; the state block pointer is out-of-band",
    ),
    Handler::Open(
        "state-block apply at [record+0x18]: IASetInputLayout (+0x88), IASetPrimitiveTopology \
         (+0xc0), VSSetShader (+0x58), PSSetShader (+0x48), DSSetShader (+0x200), HSSetShader \
         (+0x1e0), GSSetShader (+0xb8), OMSetDepthStencilState (+0x120), OMSetBlendState \
         (+0x118), RSSetState (+0x158), pm blend-state slot (pm+0xa0 vtable +0x48) and a \
         0x58-byte block at state-block +0x60; the state block is out-of-band",
    ),
    Handler::Open(
        "VSSetConstantBuffers cache: start slot +0x1c, count +0x20, 0x20-byte descriptor array \
         +0x28; 0x142676130 runs SyncBufferForUsage per element and defers the bind to the \
         end-record flush 0x142660620 (stage 0: VSSetConstantBuffers +0x38 on the zero path, \
         pm extension +0x3b8 on the bind path); the array and resource objects are out-of-band",
    ),
    Handler::Open(
        "HSSetConstantBuffers cache: start slot +0x1c, count +0x20, 0x20-byte descriptor array \
         +0x28; 0x142676130 runs SyncBufferForUsage per element and defers the bind to the \
         end-record flush 0x142660620 (stage 1: HSSetConstantBuffers +0x1f0 on the zero path, \
         pm extension +0x3c0 on the bind path); the array and resource objects are out-of-band",
    ),
    Handler::Open(
        "DSSetConstantBuffers cache: start slot +0x1c, count +0x20, 0x20-byte descriptor array \
         +0x28; 0x142676130 runs SyncBufferForUsage per element and defers the bind to the \
         end-record flush 0x142660620 (stage 2: DSSetConstantBuffers +0x210 on the zero path, \
         pm extension +0x3c8 on the bind path); the array and resource objects are out-of-band",
    ),
    Handler::Open(
        "GSSetConstantBuffers cache: start slot +0x1c, count +0x20, 0x20-byte descriptor array \
         +0x28; 0x142676130 runs SyncBufferForUsage per element and defers the bind to the \
         end-record flush 0x142660620 (stage 3: GSSetConstantBuffers +0xb0 on the zero path, \
         pm extension +0x3d0 on the bind path); the array and resource objects are out-of-band",
    ),
    Handler::Open(
        "PSSetConstantBuffers cache: start slot +0x1c, count +0x20, 0x20-byte descriptor array \
         +0x28; 0x142676130 runs SyncBufferForUsage per element and defers the bind to the \
         end-record flush 0x142660620 (stage 6: PSSetConstantBuffers +0x80 on the zero path, \
         pm extension +0x3d8 on the bind path); the array and resource objects are out-of-band",
    ),
    Handler::Open(
        "CSSetConstantBuffers cache: start slot +0x1c, count +0x20, 0x20-byte descriptor array \
         +0x28; 0x142676130 runs SyncBufferForUsage per element and defers the bind to the \
         end-record flush 0x142660620 (stage 7: CSSetConstantBuffers +0x238 on the zero path, \
         pm extension +0x3e0 on the bind path); the array and resource objects are out-of-band",
    ),
    Handler::Open(
        "VSSetShaderResources cache: start slot +0x1c, count +0x20, 0x20-byte descriptor array \
         +0x28; 0x1426781b0 runs SyncBufferForUsage per element and the bind is deferred to \
         0x142678bf0 -> VSSetShaderResources (+0xc8); the array and resource objects are \
         out-of-band",
    ),
    Handler::Open(
        "HSSetShaderResources cache: start slot +0x1c, count +0x20, 0x20-byte descriptor array \
         +0x28; 0x1426781b0 runs SyncBufferForUsage per element and the bind is deferred to \
         0x142678570 -> HSSetShaderResources (+0x1d8); the array and resource objects are \
         out-of-band",
    ),
    Handler::Open(
        "DSSetShaderResources cache: start slot +0x1c, count +0x20, 0x20-byte descriptor array \
         +0x28; 0x1426781b0 runs SyncBufferForUsage per element and the bind is deferred to \
         0x142678710 -> DSSetShaderResources (+0x1f8); the array and resource objects are \
         out-of-band",
    ),
    Handler::Open(
        "GSSetShaderResources cache: start slot +0x1c, count +0x20, 0x20-byte descriptor array \
         +0x28; 0x1426781b0 runs SyncBufferForUsage per element and the bind is deferred to \
         0x142678d90 -> GSSetShaderResources (+0xf8); the array and resource objects are \
         out-of-band",
    ),
    Handler::Open(
        "PSSetShaderResources cache: start slot +0x1c, count +0x20, 0x20-byte descriptor array \
         +0x28; 0x1426781b0 runs SyncBufferForUsage per element and the bind is deferred to \
         0x142678a50 -> PSSetShaderResources (+0x40); the array and resource objects are \
         out-of-band",
    ),
    Handler::Open(
        "CSSetShaderResources cache: start slot +0x1c, count +0x20, 0x20-byte descriptor array \
         +0x28; 0x1426781b0 runs SyncBufferForUsage per element and the bind is deferred to \
         0x1426788b0 -> CSSetShaderResources (+0x218); the array and resource objects are \
         out-of-band",
    ),
    Handler::Open(
        "a table-dispatched function selected by a payload byte; the table target is not read",
    ),
    Handler::Implemented("no_op"),
    Handler::Open(
        "shader-resource bind for a stage: start slot at +0x1c, count at +0x20, 0x20-byte \
         element array pointer at +0x28; SyncBufferForUsage per element; deferred cache flush",
    ),
    Handler::Open(
        "shader-resource bind for a stage: start slot at +0x1c, count at +0x20, 0x20-byte \
         element array pointer at +0x28; SyncBufferForUsage per element; deferred cache flush",
    ),
    Handler::Open(
        "constant upload through 0x1426798c0 and a function pointer; the upload cadence and \
         target are not implemented",
    ),
    Handler::Open("resource lookup then an object vtable slot 0 call; the object type is not read"),
    Handler::Open(
        "resource lookup then 0x1426608a0 and a function pointer; the cache update is not \
         implemented",
    ),
    Handler::Open(
        "resource lookup then 0x1426608a0 and a function pointer; the cache update is not \
         implemented",
    ),
    Handler::Open(
        "stage-cache apply; calls SyncBufferForUsage and the stage setters; the deferred cache \
         state is not implemented",
    ),
    Handler::Implemented("draw"),
    Handler::Implemented("draw_indexed"),
    Handler::Open(
        "two SyncBufferForUsage lookups and a pm cache +0x50 call with a descriptor at +0x20; \
         the cache method is not read",
    ),
    Handler::Open(
        "two SyncBufferForUsage lookups and a pm cache +0x58 call with a descriptor at +0x20; \
         the cache method is not read",
    ),
    Handler::Implemented("set_viewport"),
    Handler::Implemented("set_scissor_rect"),
    Handler::Open(
        "OMSetDepthStencilState at context +0x120; the state comes from the cached state block \
         and the stencil ref is at +0x18",
    ),
    Handler::Open(
        "OMSetBlendState at context +0x118; the state comes from the cached state block, the \
         blend factor is at +0x10 and the sample mask at +0x20",
    ),
    Handler::Open(
        "pm object at queue +0x38, +0x48 call with the 16-byte payload at +0x10 shuffled \
         to float args and a byte arg (call site 0x142670667); the same setter shape as \
         the device+0xa0 blend call, ruled out as a render-target bind trigger; the \
         cache method is not read",
    ),
    Handler::Open(
        "flushes the PS shader-resource cache (0x142678f30); the deferred cache state is not \
         implemented",
    ),
    Handler::Open(
        "CSSetShader at context +0x228 (call site 0x1426706fb): the shader handle is at \
         [state+0x8] with NULL class instances and count 0, where the state object pointer \
         is at +0x18; the state object is out-of-band",
    ),
    Handler::Implemented("dispatch"),
    Handler::Open(
        "DispatchIndirect at context +0x150; the buffer handle at +0x18 is resolved through \
         SyncBufferForUsage and the offset is at +0x20; the resource table is not implemented",
    ),
    Handler::Open(
        "creates an SRV through the device and calls GenerateMips at context +0x1b0; the SRV \
         creation is not implemented",
    ),
    Handler::Open(
        "ID3DUserDefinedAnnotation::BeginEvent with a string pointer at +0x10; annotation \
         events are not part of the context sink",
    ),
    Handler::Open(
        "ID3DUserDefinedAnnotation::EndEvent; annotation events are not part of the context \
         sink",
    ),
    Handler::Open(
        "ID3DUserDefinedAnnotation::SetMarker with a string pointer at +0x10; annotation \
         events are not part of the context sink",
    ),
    Handler::Open(
        "Begin(query) at context +0xd8 with a query taken from an array pointer at +0x18; the \
         query array is out-of-band",
    ),
    Handler::Open(
        "End(query) at context +0xe0 with a query taken from an array pointer at +0x18; the \
         query array is out-of-band",
    ),
    Handler::Open(
        "appends a query result to a list (0x142679ff0/0x142388210/memcpy); the list state is \
         not implemented",
    ),
    Handler::Open(
        "Begin+End on two queries from an array at +0x10; the query array is out-of-band",
    ),
    Handler::Open("End on two queries from an array at +0x10; the query array is out-of-band"),
    Handler::Open("grows a query list (0x1424c45a0); the list state is not implemented"),
    Handler::Open(
        "pm query/event method +0x30 with payload at +0x18/+0x20; the pm object is not read",
    ),
    Handler::Open(
        "pm query/event method +0x30 with payload at +0x10/+0x20; the pm object is not read",
    ),
    Handler::Open("pm method +0x38 with payload at +0x18; the pm object is not read"),
    Handler::Open("pm method +0x38 in a loop; the pm object is not read"),
];

/// One record header, parsed from the first [`RECORD_HEADER_SIZE`] bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordHeader {
    pub opcode: u32,
    pub size: u32,
}

impl RecordHeader {
    /// Parse the little-endian opcode and size. The slice must contain the
    /// 0x18-byte header.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < RECORD_HEADER_SIZE {
            return Err(Error::TruncatedHeader {
                offset: 0,
                available: bytes.len(),
            });
        }
        Ok(Self {
            opcode: u32::from_le_bytes(bytes[0x10..0x14].try_into().expect("4 bytes")),
            size: u32::from_le_bytes(bytes[0x14..0x18].try_into().expect("4 bytes")),
        })
    }
}

/// One record: its header and the exact bytes the client advances over.
#[derive(Clone, Copy, Debug)]
pub struct Record<'a> {
    pub header: RecordHeader,
    pub bytes: &'a [u8],
}

impl<'a> Record<'a> {
    /// Parse a record from a stream that starts at the record. The size is
    /// validated against `bytes.len()` and must include the header.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        let header = RecordHeader::parse(bytes)?;
        let size = header.size as usize;
        if size < RECORD_HEADER_SIZE {
            return Err(Error::InvalidRecordSize {
                offset: 0,
                size: header.size,
            });
        }
        if size > bytes.len() {
            return Err(Error::TruncatedRecord {
                offset: 0,
                size: header.size,
                available: bytes.len(),
            });
        }
        Ok(Self {
            header,
            bytes: &bytes[..size],
        })
    }

    /// The payload after the 0x18-byte header.
    pub fn payload(&self) -> &'a [u8] {
        &self.bytes[RECORD_HEADER_SIZE..]
    }

    /// The opcode, or `None` for the missing-opcode path.
    pub fn opcode(&self) -> Option<Opcode> {
        Opcode::from_u8(self.header.opcode as u8)
            .filter(|_| self.header.opcode < OPCODE_COUNT as u32)
    }

    fn field(&self, offset: usize, width: usize) -> Result<&'a [u8], Error> {
        let end = offset.checked_add(width).ok_or(Error::FieldOutOfRange {
            field_offset: offset,
            width,
            record_size: self.bytes.len(),
        })?;
        self.bytes.get(offset..end).ok_or(Error::FieldOutOfRange {
            field_offset: offset,
            width,
            record_size: self.bytes.len(),
        })
    }

    /// A little-endian `u8` at an absolute record offset.
    pub fn u8_at(&self, offset: usize) -> Result<u8, Error> {
        Ok(self.field(offset, 1)?[0])
    }

    /// A little-endian `u32` at an absolute record offset.
    pub fn u32_at(&self, offset: usize) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(
            self.field(offset, 4)?.try_into().expect("4 bytes"),
        ))
    }

    /// A little-endian `i32` at an absolute record offset.
    pub fn i32_at(&self, offset: usize) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(
            self.field(offset, 4)?.try_into().expect("4 bytes"),
        ))
    }

    /// A little-endian `f32` at an absolute record offset.
    pub fn f32_at(&self, offset: usize) -> Result<f32, Error> {
        Ok(f32::from_bits(self.u32_at(offset)?))
    }

    /// A little-endian `u64` at an absolute record offset.
    pub fn u64_at(&self, offset: usize) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(
            self.field(offset, 8)?.try_into().expect("8 bytes"),
        ))
    }

    /// Four little-endian `f32` values at an absolute record offset.
    pub fn f32x4_at(&self, offset: usize) -> Result<[f32; 4], Error> {
        let bytes = self.field(offset, 16)?;
        let mut values = [0.0; 4];
        for (index, value) in values.iter_mut().enumerate() {
            *value =
                f32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().expect("4 bytes"));
        }
        Ok(values)
    }
}

/// A viewport, in the client's `D3D11_VIEWPORT` field order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub min_depth: f32,
    pub max_depth: f32,
}

/// A scissor rectangle. The client passes these four values to
/// `RSSetScissorRects`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScissorRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// The resolved `0x0c` record fields. `render_targets` and `depth_stencil`
/// are handles into [`ResourceTable`]; `state_id` is opaque and saved for
/// `0x27`/`0x28`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderTargetSet {
    pub count: u32,
    pub render_targets: u64,
    pub depth_stencil: u64,
    pub state_id: u64,
}

/// What the executor did with a stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// Records seen, including the missing-opcode records.
    pub records: usize,
    /// Records whose handler called a sink operation (including `end`/no-op).
    pub implemented: usize,
    /// Records whose handler was logged as open.
    pub open: usize,
    /// Records above `0x3f` that took the missing-opcode path.
    pub missing: usize,
    /// Bytes consumed, including the end record.
    pub bytes_consumed: usize,
}

/// The sink the executor calls. Every method mirrors one read client call.
/// The executor never calls an operation method for an [`Handler::Open`]
/// opcode; it calls [`Sink::open`] instead.
pub trait Sink {
    /// Opcode `0x00`: the client's end/flush record.
    fn end(&mut self) -> Result<(), Error> {
        Ok(())
    }

    /// Opcode `0x1d`: the table entry is the replay loop head, so the record
    /// does nothing.
    fn no_op(&mut self, _opcode: Opcode) -> Result<(), Error> {
        Ok(())
    }

    /// `ID3D11DeviceContext::ClearRenderTargetView(view, colour)`.
    fn clear_render_target_view(&mut self, view: u64, colour: [f32; 4]) -> Result<(), Error>;

    /// `ID3D11DeviceContext::ClearDepthStencilView(view, flags, depth,
    /// stencil)`.
    fn clear_depth_stencil_view(
        &mut self,
        view: u64,
        flags: u32,
        depth: f32,
        stencil: u8,
    ) -> Result<(), Error>;

    /// Opcode `0x0c`: apply the resolved render-target set, its clears and
    /// the per-stage alias-unbind.
    fn set_render_target_set(&mut self, set: RenderTargetSet) -> Result<(), Error>;

    /// Opcode `0x0d`: resolve and release the pending targets, then unbind.
    fn resolve_render_target_set(&mut self) -> Result<(), Error>;

    /// `ID3D11DeviceContext::Draw(vertex_count, start_vertex)`.
    fn draw(&mut self, vertex_count: u32, start_vertex: u32) -> Result<(), Error>;

    /// `ID3D11DeviceContext::DrawInstanced(vertex_count, instance_count,
    /// start_vertex, start_instance)`.
    fn draw_instanced(
        &mut self,
        vertex_count: u32,
        instance_count: u32,
        start_vertex: u32,
        start_instance: u32,
    ) -> Result<(), Error>;

    /// `ID3D11DeviceContext::DrawIndexed(index_count, start_index,
    /// base_vertex)`.
    fn draw_indexed(
        &mut self,
        index_count: u32,
        start_index: u32,
        base_vertex: i32,
    ) -> Result<(), Error>;

    /// `ID3D11DeviceContext::DrawIndexedInstanced(index_count,
    /// instance_count, start_index, base_vertex, start_instance)`.
    fn draw_indexed_instanced(
        &mut self,
        index_count: u32,
        instance_count: u32,
        start_index: u32,
        base_vertex: i32,
        start_instance: u32,
    ) -> Result<(), Error>;

    /// `ID3D11DeviceContext::RSSetViewports(1, &viewport)`.
    fn set_viewport(&mut self, viewport: Viewport) -> Result<(), Error>;

    /// `ID3D11DeviceContext::RSSetScissorRects(1, &rect)`.
    fn set_scissor_rect(&mut self, rect: ScissorRect) -> Result<(), Error>;

    /// `ID3D11DeviceContext::Dispatch(x, y, z)`.
    fn dispatch(&mut self, x: u32, y: u32, z: u32) -> Result<(), Error>;

    /// An opcode above `0x3f`: the client logs `D3D11: missing op code: %u`
    /// and continues.
    fn missing_opcode(&mut self, opcode: u32) -> Result<(), Error>;

    /// A read handler whose call is not implemented. The sink must not fake
    /// the operation.
    fn open(&mut self, opcode: Opcode, reason: &'static str) -> Result<(), Error>;
}

/// Execute a contiguous record stream. The client's segmented ring is not
/// modelled: a record that runs past the end is an error, not a read into the
/// next segment. The client has no such bounds check; this is a safety
/// addition for the synthetic executor.
pub fn execute<S: Sink>(stream: &[u8], sink: &mut S) -> Result<Summary, Error> {
    let mut summary = Summary::default();
    let mut offset = 0usize;
    while offset < stream.len() {
        let remaining = &stream[offset..];
        if remaining.len() < RECORD_HEADER_SIZE {
            return Err(Error::TruncatedHeader {
                offset,
                available: remaining.len(),
            });
        }
        let header = RecordHeader::parse(remaining)?;
        let size = header.size as usize;
        if size < RECORD_HEADER_SIZE {
            return Err(Error::InvalidRecordSize {
                offset,
                size: header.size,
            });
        }
        if size > remaining.len() {
            return Err(Error::TruncatedRecord {
                offset,
                size: header.size,
                available: remaining.len(),
            });
        }
        let record = Record {
            header,
            bytes: &remaining[..size],
        };
        summary.records += 1;
        offset += size;
        summary.bytes_consumed = offset;

        let Some(opcode) = record.opcode() else {
            sink.missing_opcode(header.opcode)?;
            summary.missing += 1;
            continue;
        };

        match opcode.handler() {
            Handler::Open(reason) => {
                sink.open(opcode, reason)?;
                summary.open += 1;
            }
            Handler::Implemented(_) => {
                dispatch(opcode, &record, sink)?;
                summary.implemented += 1;
                if opcode == Opcode::End {
                    return Ok(summary);
                }
            }
        }
    }
    Ok(summary)
}

fn dispatch<S: Sink>(opcode: Opcode, record: &Record<'_>, sink: &mut S) -> Result<(), Error> {
    match opcode {
        Opcode::End => sink.end(),
        Opcode::NoOp => sink.no_op(opcode),
        Opcode::ClearRenderTargetView => {
            sink.clear_render_target_view(record.u64_at(0x18)?, record.f32x4_at(0x20)?)
        }
        Opcode::ClearDepthStencilView => sink.clear_depth_stencil_view(
            record.u64_at(0x18)?,
            record.u32_at(0x20)?,
            record.f32_at(0x24)?,
            record.u8_at(0x28)?,
        ),
        Opcode::Op0c => sink.set_render_target_set(RenderTargetSet {
            count: record.u32_at(0x18)?,
            render_targets: record.u64_at(0x20)?,
            depth_stencil: record.u64_at(0x28)?,
            state_id: record.u64_at(0x30)?,
        }),
        Opcode::Op0d => sink.resolve_render_target_set(),
        Opcode::Draw => {
            let vertex_count = record.u32_at(0x18)?;
            let instance_count = record.u32_at(0x1c)?;
            let start_vertex = record.u32_at(0x20)?;
            let start_instance = record.u32_at(0x24)?;
            if instance_count == 1 && start_instance == 0 {
                sink.draw(vertex_count, start_vertex)
            } else {
                sink.draw_instanced(vertex_count, instance_count, start_vertex, start_instance)
            }
        }
        Opcode::DrawIndexed => {
            let index_count = record.u32_at(0x18)?;
            let instance_count = record.u32_at(0x1c)?;
            let start_index = record.u32_at(0x20)?;
            let base_vertex = record.i32_at(0x24)?;
            let start_instance = record.u32_at(0x28)?;
            if instance_count == 1 && start_instance == 0 {
                sink.draw_indexed(index_count, start_index, base_vertex)
            } else {
                sink.draw_indexed_instanced(
                    index_count,
                    instance_count,
                    start_index,
                    base_vertex,
                    start_instance,
                )
            }
        }
        Opcode::SetViewports => sink.set_viewport(Viewport {
            x: record.f32_at(0x18)?,
            y: record.f32_at(0x1c)?,
            width: record.f32_at(0x20)?,
            height: record.f32_at(0x24)?,
            min_depth: record.f32_at(0x28)?,
            max_depth: record.f32_at(0x2c)?,
        }),
        Opcode::SetScissorRects => {
            let left = record.i32_at(0x18)?;
            let top = record.i32_at(0x1c)?;
            let width = record.u32_at(0x20)?;
            let height = record.u32_at(0x24)?;
            sink.set_scissor_rect(ScissorRect {
                left,
                top,
                right: left.wrapping_add(width as i32),
                bottom: top.wrapping_add(height as i32),
            })
        }
        Opcode::Dispatch => sink.dispatch(
            record.u32_at(0x18)?,
            record.u32_at(0x1c)?,
            record.u32_at(0x20)?,
        ),
        _ => unreachable!("only implemented opcodes reach dispatch"),
    }
}

/// A handle-to-view table for the D3D11 sink. The client's records carry pm
/// handles; this table is the bridge the crate does not have yet. A handle
/// that is not registered is an error, never a guessed pointer.
/// A `0x0d` destination resource and its subresource/format.
pub struct ResolveTargetResource {
    pub resource: ID3D11Resource,
    pub subresource: u32,
    pub format: u32,
}

#[derive(Default)]
pub struct ResourceTable {
    render_targets: HashMap<u64, ID3D11RenderTargetView>,
    depth_stencils: HashMap<u64, ID3D11DepthStencilView>,
    render_target_arrays: HashMap<u64, Vec<RenderTargetEntry>>,
    depth_stencil_entries: HashMap<u64, DepthStencilEntry>,
    resolve_targets: HashMap<u64, ResolveTargetResource>,
}

impl ResourceTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_render_target(&mut self, handle: u64, view: ID3D11RenderTargetView) {
        self.render_targets.insert(handle, view);
    }

    pub fn insert_depth_stencil(&mut self, handle: u64, view: ID3D11DepthStencilView) {
        self.depth_stencils.insert(handle, view);
    }

    pub fn render_target(&self, handle: u64) -> Option<&ID3D11RenderTargetView> {
        self.render_targets.get(&handle)
    }

    pub fn depth_stencil(&self, handle: u64) -> Option<&ID3D11DepthStencilView> {
        self.depth_stencils.get(&handle)
    }

    /// Register the out-of-band RTV array for a `0x0c` record handle.
    pub fn insert_render_target_array(&mut self, handle: u64, entries: Vec<RenderTargetEntry>) {
        self.render_target_arrays.insert(handle, entries);
    }

    /// Register the out-of-band DSV block for a `0x0c` record handle.
    pub fn insert_depth_stencil_entry(&mut self, handle: u64, entry: DepthStencilEntry) {
        self.depth_stencil_entries.insert(handle, entry);
    }

    /// Resolve a `0x0c` RTV array handle.
    pub fn render_target_array(&self, handle: u64) -> Option<&[RenderTargetEntry]> {
        self.render_target_arrays.get(&handle).map(Vec::as_slice)
    }

    /// Resolve a `0x0c` DSV block handle.
    pub fn depth_stencil_entry(&self, handle: u64) -> Option<&DepthStencilEntry> {
        self.depth_stencil_entries.get(&handle)
    }

    /// Register a `0x0d` destination resource.
    pub fn insert_resolve_target(&mut self, handle: u64, target: ResolveTargetResource) {
        self.resolve_targets.insert(handle, target);
    }

    /// Resolve a `0x0d` destination resource handle.
    pub fn resolve_target(&self, handle: u64) -> Option<&ResolveTargetResource> {
        self.resolve_targets.get(&handle)
    }
}

/// The D3D11 sink: the read `ID3D11DeviceContext` calls with the record
/// arguments. Views are resolved through [`ResourceTable`]; an unregistered
/// handle is [`Error::UnresolvedHandle`], never a fake call.
pub struct D3d11Sink<'a> {
    context: &'a ID3D11DeviceContext,
    resources: &'a ResourceTable,
    state: ReplayState,
    pending: Option<PendingTargets>,
}

struct PendingTargets {
    targets: Vec<RenderTargetEntry>,
    depth: Option<DepthStencilEntry>,
}

impl<'a> D3d11Sink<'a> {
    pub fn new(context: &'a ID3D11DeviceContext, resources: &'a ResourceTable) -> Self {
        Self {
            context,
            resources,
            state: ReplayState::new(),
            pending: None,
        }
    }
}

/// The target/cache sink used by one `0x0c` record. It borrows the context and
/// the resource table, never the replay state, so the state can be borrowed
/// mutably at the same time.
struct D3d11TargetSink<'a> {
    context: &'a ID3D11DeviceContext,
    resources: &'a ResourceTable,
}

impl CacheSink for D3d11TargetSink<'_> {
    fn set_shader_resources(&mut self, stage: Stage, start: u32, count: u32, views: &[usize]) {
        debug_assert!(
            views.iter().all(|view| *view == 0),
            "0x0c only issues null SRV arrays"
        );
        let nulls = vec![None::<ID3D11ShaderResourceView>; count as usize];
        unsafe {
            match stage {
                Stage::Vs => self.context.VSSetShaderResources(start, Some(&nulls)),
                Stage::Hs => self.context.HSSetShaderResources(start, Some(&nulls)),
                Stage::Ds => self.context.DSSetShaderResources(start, Some(&nulls)),
                Stage::Gs => self.context.GSSetShaderResources(start, Some(&nulls)),
                Stage::Ps => self.context.PSSetShaderResources(start, Some(&nulls)),
                Stage::Cs => self.context.CSSetShaderResources(start, Some(&nulls)),
            }
        }
    }

    fn set_unordered_access_views(
        &mut self,
        kind: UavKind,
        start: u32,
        count: u32,
        views: &[usize],
    ) {
        debug_assert!(
            views.iter().all(|view| *view == 0),
            "0x0c only issues null UAV arrays"
        );
        let nulls = vec![None::<ID3D11UnorderedAccessView>; count as usize];
        unsafe {
            match kind {
                UavKind::Cs => {
                    self.context
                        .CSSetUnorderedAccessViews(start, count, Some(nulls.as_ptr()), None)
                }
                UavKind::Om => {
                    // The OM UAV flush (0x1426790e0) needs NumViews=0xffffffff
                    // with a null RTV array; the typed wrapper cannot express
                    // that count. It is not reached by 0x0c.
                    panic!("OM UAV flush is not wired yet");
                }
            }
        }
    }

    fn set_render_targets(&mut self, views: &[usize], dsv: Option<usize>) {
        let targets: Vec<Option<ID3D11RenderTargetView>> = views
            .iter()
            .map(|view| self.resources.render_target(*view as u64).cloned())
            .collect();
        let depth = dsv.and_then(|view| self.resources.depth_stencil(view as u64).cloned());
        unsafe {
            self.context
                .OMSetRenderTargets(Some(&targets), depth.as_ref());
        }
    }
}

impl TargetSink for D3d11TargetSink<'_> {
    fn clear_render_target_view(&mut self, view: usize, colour: [f32; 4]) {
        if let Some(view) = self.resources.render_target(view as u64) {
            unsafe {
                self.context.ClearRenderTargetView(view, &colour);
            }
        }
    }

    fn release_render_target_view(&mut self, _view: usize) {
        // pm refcount bookkeeping, not a D3D11 call.
    }

    fn clear_depth_stencil_view(&mut self, view: usize, flags: u32, depth: f32, stencil: u8) {
        if let Some(view) = self.resources.depth_stencil(view as u64) {
            unsafe {
                self.context
                    .ClearDepthStencilView(view, flags, depth, stencil);
            }
        }
    }

    fn resolve_subresource(
        &mut self,
        destination: u64,
        source_view: usize,
        source_subresource: u32,
    ) {
        let Some(destination) = self.resources.resolve_target(destination) else {
            return;
        };
        let source = if let Some(view) = self.resources.render_target(source_view as u64) {
            unsafe { view.GetResource() }.ok()
        } else {
            self.resources
                .depth_stencil(source_view as u64)
                .and_then(|view| unsafe { view.GetResource() }.ok())
        };
        let Some(source) = source else {
            return;
        };
        unsafe {
            self.context.ResolveSubresource(
                &destination.resource,
                destination.subresource,
                &source,
                source_subresource,
                DXGI_FORMAT(destination.format as i32),
            );
        }
    }
}

/// `TestViewAlias`'s `GetResource` side, over the handle table.
struct ResourceViewTable<'a> {
    resources: &'a ResourceTable,
}

impl ViewTable for ResourceViewTable<'_> {
    fn get_resource(&self, handle: usize) -> usize {
        let handle = handle as u64;
        if let Some(view) = self.resources.render_target(handle) {
            return unsafe { view.GetResource() }
                .map(|resource| resource.as_raw() as usize)
                .unwrap_or(0);
        }
        if let Some(view) = self.resources.depth_stencil(handle) {
            return unsafe { view.GetResource() }
                .map(|resource| resource.as_raw() as usize)
                .unwrap_or(0);
        }
        0
    }
}

impl Sink for D3d11Sink<'_> {
    fn set_render_target_set(&mut self, set: RenderTargetSet) -> Result<(), Error> {
        let resources = self.resources;
        let targets =
            resources
                .render_target_array(set.render_targets)
                .ok_or(Error::UnresolvedHandle {
                    opcode: Opcode::Op0c,
                    handle: set.render_targets,
                })?;
        let count = set.count as usize;
        if count > targets.len() {
            return Err(Error::TargetCountOutOfRange {
                count: set.count,
                available: targets.len(),
            });
        }
        let depth = if set.depth_stencil == 0 {
            None
        } else {
            Some(*resources.depth_stencil_entry(set.depth_stencil).ok_or(
                Error::UnresolvedHandle {
                    opcode: Opcode::Op0c,
                    handle: set.depth_stencil,
                },
            )?)
        };
        self.pending = Some(PendingTargets {
            targets: targets[..count].to_vec(),
            depth,
        });
        let table = ResourceViewTable { resources };
        let mut target_sink = D3d11TargetSink {
            context: self.context,
            resources,
        };
        self.state
            .apply_targets(&targets[..count], depth, &table, &mut target_sink);
        Ok(())
    }

    fn resolve_render_target_set(&mut self) -> Result<(), Error> {
        let Some(pending) = self.pending.take() else {
            return Err(Error::NoPendingTargets);
        };
        let mut target_sink = D3d11TargetSink {
            context: self.context,
            resources: self.resources,
        };
        resolve_pending_targets(&pending.targets, pending.depth, &mut target_sink);
        Ok(())
    }

    fn clear_render_target_view(&mut self, view: u64, colour: [f32; 4]) -> Result<(), Error> {
        let view = self
            .resources
            .render_target(view)
            .ok_or(Error::UnresolvedHandle {
                opcode: Opcode::ClearRenderTargetView,
                handle: view,
            })?;
        unsafe {
            self.context.ClearRenderTargetView(view, &colour);
        }
        Ok(())
    }

    fn clear_depth_stencil_view(
        &mut self,
        view: u64,
        flags: u32,
        depth: f32,
        stencil: u8,
    ) -> Result<(), Error> {
        let view = self
            .resources
            .depth_stencil(view)
            .ok_or(Error::UnresolvedHandle {
                opcode: Opcode::ClearDepthStencilView,
                handle: view,
            })?;
        unsafe {
            self.context
                .ClearDepthStencilView(view, flags, depth, stencil);
        }
        Ok(())
    }

    fn draw(&mut self, vertex_count: u32, start_vertex: u32) -> Result<(), Error> {
        unsafe {
            self.context.Draw(vertex_count, start_vertex);
        }
        Ok(())
    }

    fn draw_instanced(
        &mut self,
        vertex_count: u32,
        instance_count: u32,
        start_vertex: u32,
        start_instance: u32,
    ) -> Result<(), Error> {
        unsafe {
            self.context
                .DrawInstanced(vertex_count, instance_count, start_vertex, start_instance);
        }
        Ok(())
    }

    fn draw_indexed(
        &mut self,
        index_count: u32,
        start_index: u32,
        base_vertex: i32,
    ) -> Result<(), Error> {
        unsafe {
            self.context
                .DrawIndexed(index_count, start_index, base_vertex);
        }
        Ok(())
    }

    fn draw_indexed_instanced(
        &mut self,
        index_count: u32,
        instance_count: u32,
        start_index: u32,
        base_vertex: i32,
        start_instance: u32,
    ) -> Result<(), Error> {
        unsafe {
            self.context.DrawIndexedInstanced(
                index_count,
                instance_count,
                start_index,
                base_vertex,
                start_instance,
            );
        }
        Ok(())
    }

    fn set_viewport(&mut self, viewport: Viewport) -> Result<(), Error> {
        let viewport = D3D11_VIEWPORT {
            TopLeftX: viewport.x,
            TopLeftY: viewport.y,
            Width: viewport.width,
            Height: viewport.height,
            MinDepth: viewport.min_depth,
            MaxDepth: viewport.max_depth,
        };
        unsafe {
            self.context.RSSetViewports(Some(&[viewport]));
        }
        Ok(())
    }

    fn set_scissor_rect(&mut self, rect: ScissorRect) -> Result<(), Error> {
        let rect = RECT {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        };
        unsafe {
            self.context.RSSetScissorRects(Some(&[rect]));
        }
        Ok(())
    }

    fn dispatch(&mut self, x: u32, y: u32, z: u32) -> Result<(), Error> {
        unsafe {
            self.context.Dispatch(x, y, z);
        }
        Ok(())
    }

    fn missing_opcode(&mut self, opcode: u32) -> Result<(), Error> {
        println!("renderer: D4: D3D11: missing op code: {opcode}");
        Ok(())
    }

    fn open(&mut self, opcode: Opcode, reason: &'static str) -> Result<(), Error> {
        println!(
            "renderer: D4: opcode 0x{:02x} ({}) open: {reason}",
            opcode.as_u8(),
            opcode.name()
        );
        Ok(())
    }
}

/// A replay error. The first four variants are the synthetic stream's safety
/// checks; the client itself trusts the record size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    TruncatedHeader {
        offset: usize,
        available: usize,
    },
    InvalidRecordSize {
        offset: usize,
        size: u32,
    },
    TruncatedRecord {
        offset: usize,
        size: u32,
        available: usize,
    },
    FieldOutOfRange {
        field_offset: usize,
        width: usize,
        record_size: usize,
    },
    UnresolvedHandle {
        opcode: Opcode,
        handle: u64,
    },
    TargetCountOutOfRange {
        count: u32,
        available: usize,
    },
    NoPendingTargets,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedHeader { offset, available } => write!(
                f,
                "record header at stream offset 0x{offset:x} needs 0x{RECORD_HEADER_SIZE:x} \
                 bytes, only 0x{available:x} available"
            ),
            Self::InvalidRecordSize { offset, size } => write!(
                f,
                "record at stream offset 0x{offset:x} has size 0x{size:x}, smaller than the \
                 0x{RECORD_HEADER_SIZE:x}-byte header"
            ),
            Self::TruncatedRecord {
                offset,
                size,
                available,
            } => write!(
                f,
                "record at stream offset 0x{offset:x} declares 0x{size:x} bytes, only \
                 0x{available:x} available"
            ),
            Self::FieldOutOfRange {
                field_offset,
                width,
                record_size,
            } => write!(
                f,
                "field at record offset 0x{field_offset:x} (+0x{width:x}) is outside the \
                 0x{record_size:x}-byte record"
            ),
            Self::UnresolvedHandle { opcode, handle } => write!(
                f,
                "opcode 0x{:02x} ({}) handle 0x{handle:016x} is not in the resource table",
                opcode.as_u8(),
                opcode.name()
            ),
            Self::TargetCountOutOfRange { count, available } => write!(
                f,
                "0x0c target count {count} exceeds the {available}-entry array"
            ),
            Self::NoPendingTargets => f.write_str("0x0d has no pending 0x0c target set"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    enum Call {
        End,
        NoOp(u8),
        RenderTargetSet {
            count: u32,
            render_targets: u64,
            depth_stencil: u64,
            state_id: u64,
        },
        ResolvePending,
        ClearRender {
            view: u64,
            colour: [f32; 4],
        },
        ClearDepthStencil {
            view: u64,
            flags: u32,
            depth: f32,
            stencil: u8,
        },
        Draw {
            vertex_count: u32,
            start_vertex: u32,
        },
        DrawInstanced {
            vertex_count: u32,
            instance_count: u32,
            start_vertex: u32,
            start_instance: u32,
        },
        DrawIndexed {
            index_count: u32,
            start_index: u32,
            base_vertex: i32,
        },
        DrawIndexedInstanced {
            index_count: u32,
            instance_count: u32,
            start_index: u32,
            base_vertex: i32,
            start_instance: u32,
        },
        Viewport(Viewport),
        Scissor(ScissorRect),
        Dispatch {
            x: u32,
            y: u32,
            z: u32,
        },
        Missing(u32),
        Open(u8),
    }

    #[derive(Default)]
    struct RecordingSink {
        calls: Vec<Call>,
    }

    impl Sink for RecordingSink {
        fn end(&mut self) -> Result<(), Error> {
            self.calls.push(Call::End);
            Ok(())
        }

        fn no_op(&mut self, opcode: Opcode) -> Result<(), Error> {
            self.calls.push(Call::NoOp(opcode.as_u8()));
            Ok(())
        }

        fn set_render_target_set(&mut self, set: RenderTargetSet) -> Result<(), Error> {
            self.calls.push(Call::RenderTargetSet {
                count: set.count,
                render_targets: set.render_targets,
                depth_stencil: set.depth_stencil,
                state_id: set.state_id,
            });
            Ok(())
        }

        fn resolve_render_target_set(&mut self) -> Result<(), Error> {
            self.calls.push(Call::ResolvePending);
            Ok(())
        }

        fn clear_render_target_view(&mut self, view: u64, colour: [f32; 4]) -> Result<(), Error> {
            self.calls.push(Call::ClearRender { view, colour });
            Ok(())
        }

        fn clear_depth_stencil_view(
            &mut self,
            view: u64,
            flags: u32,
            depth: f32,
            stencil: u8,
        ) -> Result<(), Error> {
            self.calls.push(Call::ClearDepthStencil {
                view,
                flags,
                depth,
                stencil,
            });
            Ok(())
        }

        fn draw(&mut self, vertex_count: u32, start_vertex: u32) -> Result<(), Error> {
            self.calls.push(Call::Draw {
                vertex_count,
                start_vertex,
            });
            Ok(())
        }

        fn draw_instanced(
            &mut self,
            vertex_count: u32,
            instance_count: u32,
            start_vertex: u32,
            start_instance: u32,
        ) -> Result<(), Error> {
            self.calls.push(Call::DrawInstanced {
                vertex_count,
                instance_count,
                start_vertex,
                start_instance,
            });
            Ok(())
        }

        fn draw_indexed(
            &mut self,
            index_count: u32,
            start_index: u32,
            base_vertex: i32,
        ) -> Result<(), Error> {
            self.calls.push(Call::DrawIndexed {
                index_count,
                start_index,
                base_vertex,
            });
            Ok(())
        }

        fn draw_indexed_instanced(
            &mut self,
            index_count: u32,
            instance_count: u32,
            start_index: u32,
            base_vertex: i32,
            start_instance: u32,
        ) -> Result<(), Error> {
            self.calls.push(Call::DrawIndexedInstanced {
                index_count,
                instance_count,
                start_index,
                base_vertex,
                start_instance,
            });
            Ok(())
        }

        fn set_viewport(&mut self, viewport: Viewport) -> Result<(), Error> {
            self.calls.push(Call::Viewport(viewport));
            Ok(())
        }

        fn set_scissor_rect(&mut self, rect: ScissorRect) -> Result<(), Error> {
            self.calls.push(Call::Scissor(rect));
            Ok(())
        }

        fn dispatch(&mut self, x: u32, y: u32, z: u32) -> Result<(), Error> {
            self.calls.push(Call::Dispatch { x, y, z });
            Ok(())
        }

        fn missing_opcode(&mut self, opcode: u32) -> Result<(), Error> {
            self.calls.push(Call::Missing(opcode));
            Ok(())
        }

        fn open(&mut self, opcode: Opcode, _reason: &'static str) -> Result<(), Error> {
            self.calls.push(Call::Open(opcode.as_u8()));
            Ok(())
        }
    }

    fn record(opcode: u8, size: usize, fields: &[(usize, &[u8])]) -> Vec<u8> {
        let mut bytes = vec![0u8; size];
        bytes[0x10..0x14].copy_from_slice(&(opcode as u32).to_le_bytes());
        bytes[0x14..0x18].copy_from_slice(&(size as u32).to_le_bytes());
        for (offset, value) in fields {
            bytes[*offset..*offset + value.len()].copy_from_slice(value);
        }
        bytes
    }

    fn f32_bytes(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn the_opcode_table_matches_the_client() {
        assert_eq!(OPCODE_TABLE, 0x0001_4267_1154);
        assert_eq!(MISSING_OPCODE_LOG, 0x0001_4349_83d0);
        assert_eq!(RECORD_OPCODE_OFFSET, 0x10);
        assert_eq!(RECORD_SIZE_OFFSET, 0x14);
        assert_eq!(RECORD_HEADER_SIZE, 0x18);
        assert_eq!(HANDLER_TARGETS[0], 0x1426_7102d);
        assert_eq!(HANDLER_TARGETS[4], 0x1426_6ce03);
        assert_eq!(HANDLER_TARGETS[7], 0x1426_6d1ab);
        assert_eq!(HANDLER_TARGETS[8], 0x1426_6d1ed);
        assert_eq!(Opcode::ClearDepthStencilView.target(), 0x1426_6d1ed);
        assert_eq!(HANDLER_TARGETS[0x1d], 0x1426_6c736);
        assert_eq!(HANDLER_TARGETS[0x25], 0x1426_701c9);
        assert_eq!(HANDLER_TARGETS[0x26], 0x1426_70267);
        assert_eq!(HANDLER_TARGETS[0x29], 0x1426_704b7);
        assert_eq!(HANDLER_TARGETS[0x2a], 0x1426_70538);
        assert_eq!(HANDLER_TARGETS[0x30], 0x1426_7072a);
        assert_eq!(HANDLER_TARGETS[0x3f], 0x1426_70f3b);
        // The target list is strictly increasing except for the opcode-0
        // epilogue and the opcode-0x1d loop-head entry, which are read from
        // their own table slots.
        let mut sorted = HANDLER_TARGETS;
        sorted.sort_unstable();
        assert_eq!(sorted.len(), OPCODE_COUNT);
        assert_eq!(sorted[0], 0x1426_6c736);
        assert_eq!(*sorted.last().unwrap(), 0x1426_7102d);
    }

    #[test]
    fn the_64_opcode_enum_is_complete_and_indexed_by_byte() {
        assert_eq!(Opcode::ALL.len(), OPCODE_COUNT);
        for (index, opcode) in Opcode::ALL.iter().enumerate() {
            assert_eq!(opcode.as_u8() as usize, index);
            assert_eq!(Opcode::from_u8(index as u8), Some(*opcode));
            assert!(!opcode.name().is_empty());
            assert!(opcode.target() >= 0x1426_6c736);
        }
        assert_eq!(Opcode::from_u8(0x3f), Some(Opcode::Op3f));
        assert_eq!(Opcode::from_u8(0x40), None);
        assert_eq!(Opcode::from_u8(0xff), None);
    }

    #[test]
    fn the_record_header_is_little_endian_and_includes_the_header() {
        let bytes = record(0x26, 0x30, &[]);
        let header = RecordHeader::parse(&bytes).unwrap();
        assert_eq!(header.opcode, 0x26);
        assert_eq!(header.size, 0x30);
        let parsed = Record::parse(&bytes).unwrap();
        assert_eq!(parsed.payload().len(), 0x18);
        assert_eq!(parsed.opcode(), Some(Opcode::DrawIndexed));
    }

    #[test]
    fn clear_render_target_view_arguments_are_exact() {
        let colour = [0.5f32, 0.25, 0.125, 1.0];
        let bytes = record(
            0x07,
            0x30,
            &[
                (0x18, &0xdead_beef_0123_4567u64.to_le_bytes()),
                (0x20, &f32_bytes(&colour)),
            ],
        );
        let mut sink = RecordingSink::default();
        execute(&bytes, &mut sink).unwrap();
        assert_eq!(
            sink.calls,
            vec![Call::ClearRender {
                view: 0xdead_beef_0123_4567,
                colour,
            }]
        );
    }

    #[test]
    fn clear_depth_stencil_view_arguments_are_exact() {
        // 0x08 is ClearDepthStencilView (+0x1a8), not a table dispatch.
        // The client reads the clear flags from +0x20, the depth float from
        // +0x24 and the stencil byte from +0x28 (low byte of the dword).
        let bytes = record(
            0x08,
            0x30,
            &[
                (0x18, &0xfeed_face_1234_5678u64.to_le_bytes()),
                (0x20, &3u32.to_le_bytes()),
                (0x24, &0.25f32.to_le_bytes()),
                (0x28, &0x1234_5678u32.to_le_bytes()),
            ],
        );
        let mut sink = RecordingSink::default();
        execute(&bytes, &mut sink).unwrap();
        assert_eq!(
            sink.calls,
            vec![Call::ClearDepthStencil {
                view: 0xfeed_face_1234_5678,
                flags: 3,
                depth: 0.25,
                stencil: 0x78,
            }]
        );
    }

    #[test]
    fn render_target_set_fields_are_carried_to_the_sink() {
        let bytes = record(
            0x0c,
            0x38,
            &[
                (0x18, &2u32.to_le_bytes()),
                (0x20, &0x1111u64.to_le_bytes()),
                (0x28, &0x2222u64.to_le_bytes()),
                (0x30, &0x3333u64.to_le_bytes()),
            ],
        );
        let mut sink = RecordingSink::default();
        execute(&bytes, &mut sink).unwrap();
        assert_eq!(
            sink.calls,
            vec![Call::RenderTargetSet {
                count: 2,
                render_targets: 0x1111,
                depth_stencil: 0x2222,
                state_id: 0x3333,
            }]
        );
    }

    #[test]
    fn opcode_0d_calls_the_pending_resolve() {
        let bytes = record(0x0d, 0x18, &[]);
        let mut sink = RecordingSink::default();
        execute(&bytes, &mut sink).unwrap();
        assert_eq!(sink.calls, vec![Call::ResolvePending]);
    }

    #[test]
    fn draw_selects_the_client_shortcut_and_the_instanced_call() {
        let draw = record(
            0x25,
            0x28,
            &[
                (0x18, &36u32.to_le_bytes()),
                (0x1c, &1u32.to_le_bytes()),
                (0x20, &12u32.to_le_bytes()),
                (0x24, &0u32.to_le_bytes()),
            ],
        );
        let instanced = record(
            0x25,
            0x28,
            &[
                (0x18, &36u32.to_le_bytes()),
                (0x1c, &4u32.to_le_bytes()),
                (0x20, &12u32.to_le_bytes()),
                (0x24, &2u32.to_le_bytes()),
            ],
        );
        let mut sink = RecordingSink::default();
        execute(&draw, &mut sink).unwrap();
        execute(&instanced, &mut sink).unwrap();
        assert_eq!(
            sink.calls,
            vec![
                Call::Draw {
                    vertex_count: 36,
                    start_vertex: 12,
                },
                Call::DrawInstanced {
                    vertex_count: 36,
                    instance_count: 4,
                    start_vertex: 12,
                    start_instance: 2,
                },
            ]
        );
    }

    #[test]
    fn draw_indexed_selects_the_client_shortcut_and_the_instanced_call() {
        let indexed = record(
            0x26,
            0x30,
            &[
                (0x18, &300u32.to_le_bytes()),
                (0x1c, &1u32.to_le_bytes()),
                (0x20, &6u32.to_le_bytes()),
                (0x24, &(-2i32).to_le_bytes()),
                (0x28, &0u32.to_le_bytes()),
            ],
        );
        let instanced = record(
            0x26,
            0x30,
            &[
                (0x18, &300u32.to_le_bytes()),
                (0x1c, &3u32.to_le_bytes()),
                (0x20, &6u32.to_le_bytes()),
                (0x24, &(-2i32).to_le_bytes()),
                (0x28, &1u32.to_le_bytes()),
            ],
        );
        let mut sink = RecordingSink::default();
        execute(&indexed, &mut sink).unwrap();
        execute(&instanced, &mut sink).unwrap();
        assert_eq!(
            sink.calls,
            vec![
                Call::DrawIndexed {
                    index_count: 300,
                    start_index: 6,
                    base_vertex: -2,
                },
                Call::DrawIndexedInstanced {
                    index_count: 300,
                    instance_count: 3,
                    start_index: 6,
                    base_vertex: -2,
                    start_instance: 1,
                },
            ]
        );
    }

    #[test]
    fn viewport_and_scissor_arguments_are_exact() {
        let viewport = Viewport {
            x: 1.0,
            y: 2.0,
            width: 1280.0,
            height: 720.0,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let viewport_record = record(
            0x29,
            0x30,
            &[
                (0x18, &f32_bytes(&[1.0, 2.0, 1280.0, 720.0])),
                (0x28, &f32_bytes(&[0.0, 1.0])),
            ],
        );
        let scissor_record = record(
            0x2a,
            0x28,
            &[
                (0x18, &10i32.to_le_bytes()),
                (0x1c, &20i32.to_le_bytes()),
                (0x20, &100u32.to_le_bytes()),
                (0x24, &50u32.to_le_bytes()),
            ],
        );
        let mut sink = RecordingSink::default();
        execute(&viewport_record, &mut sink).unwrap();
        execute(&scissor_record, &mut sink).unwrap();
        assert_eq!(
            sink.calls,
            vec![
                Call::Viewport(viewport),
                Call::Scissor(ScissorRect {
                    left: 10,
                    top: 20,
                    right: 110,
                    bottom: 70,
                }),
            ]
        );
    }

    #[test]
    fn dispatch_arguments_are_exact() {
        let bytes = record(
            0x30,
            0x28,
            &[
                (0x18, &8u32.to_le_bytes()),
                (0x1c, &4u32.to_le_bytes()),
                (0x20, &2u32.to_le_bytes()),
            ],
        );
        let mut sink = RecordingSink::default();
        execute(&bytes, &mut sink).unwrap();
        assert_eq!(sink.calls, vec![Call::Dispatch { x: 8, y: 4, z: 2 }]);
    }

    #[test]
    fn the_missing_opcode_path_logs_and_continues() {
        let missing = record(0x40, 0x18, &[]);
        let clear = record(
            0x07,
            0x30,
            &[
                (0x18, &1u64.to_le_bytes()),
                (0x20, &f32_bytes(&[0.0, 0.0, 0.0, 1.0])),
            ],
        );
        let mut stream = missing;
        stream.extend_from_slice(&clear);
        let mut sink = RecordingSink::default();
        let summary = execute(&stream, &mut sink).unwrap();
        assert_eq!(summary.records, 2);
        assert_eq!(summary.missing, 1);
        assert_eq!(summary.implemented, 1);
        assert_eq!(
            sink.calls,
            vec![
                Call::Missing(0x40),
                Call::ClearRender {
                    view: 1,
                    colour: [0.0, 0.0, 0.0, 1.0],
                },
            ]
        );
    }

    #[test]
    fn a_truncated_header_is_an_error() {
        let bytes = vec![0u8; 0x10];
        let mut sink = RecordingSink::default();
        assert_eq!(
            execute(&bytes, &mut sink),
            Err(Error::TruncatedHeader {
                offset: 0,
                available: 0x10,
            })
        );
        assert!(sink.calls.is_empty());
    }

    #[test]
    fn a_truncated_record_is_an_error() {
        let mut bytes = record(0x30, 0x28, &[]);
        bytes.truncate(0x20);
        let mut sink = RecordingSink::default();
        assert_eq!(
            execute(&bytes, &mut sink),
            Err(Error::TruncatedRecord {
                offset: 0,
                size: 0x28,
                available: 0x20,
            })
        );
        assert!(sink.calls.is_empty());
    }

    #[test]
    fn a_record_smaller_than_the_header_is_an_error() {
        let mut bytes = vec![0u8; 0x18];
        bytes[0x10..0x14].copy_from_slice(&0x30u32.to_le_bytes());
        bytes[0x14..0x18].copy_from_slice(&0x10u32.to_le_bytes());
        let mut sink = RecordingSink::default();
        assert_eq!(
            execute(&bytes, &mut sink),
            Err(Error::InvalidRecordSize {
                offset: 0,
                size: 0x10,
            })
        );
        assert!(sink.calls.is_empty());
    }

    #[test]
    fn an_open_opcode_reports_open_and_never_calls_an_operation() {
        let bytes = record(0x03, 0x20, &[]);
        let mut sink = RecordingSink::default();
        let summary = execute(&bytes, &mut sink).unwrap();
        assert_eq!(summary.open, 1);
        assert_eq!(summary.implemented, 0);
        assert_eq!(sink.calls, vec![Call::Open(0x03)]);
        match Opcode::Op03.handler() {
            Handler::Open(reason) => assert!(reason.contains("UpdateSubresource")),
            Handler::Implemented(method) => panic!("0x03 must be open, got {method}"),
        }
    }

    #[test]
    fn every_open_opcode_reports_open_and_calls_no_operation() {
        for opcode in Opcode::ALL {
            if !matches!(opcode.handler(), Handler::Open(_)) {
                continue;
            }
            let bytes = record(opcode.as_u8(), 0x18, &[]);
            let mut sink = RecordingSink::default();
            let summary = execute(&bytes, &mut sink).unwrap();
            assert_eq!(summary.open, 1, "opcode 0x{:02x}", opcode.as_u8());
            assert_eq!(summary.implemented, 0, "opcode 0x{:02x}", opcode.as_u8());
            assert_eq!(
                sink.calls,
                vec![Call::Open(opcode.as_u8())],
                "opcode 0x{:02x} must only report open",
                opcode.as_u8()
            );
        }
    }

    #[test]
    fn the_state_cache_and_bind_batch_is_open_with_a_named_gap() {
        // 0x0e..=0x1b: every handler carries a state block or descriptor
        // array pointer, so the record bytes alone cannot form the call.
        assert_eq!(
            Opcode::Op0c.handler(),
            Handler::Implemented("set_render_target_set")
        );
        assert_eq!(
            Opcode::Op0d.handler(),
            Handler::Implemented("resolve_render_target_set")
        );
        let expected: [(u8, &str); 14] = [
            (0x0e, "state block pointer is out-of-band"),
            (0x0f, "state block is out-of-band"),
            (0x10, "VSSetConstantBuffers cache"),
            (0x11, "HSSetConstantBuffers cache"),
            (0x12, "DSSetConstantBuffers cache"),
            (0x13, "GSSetConstantBuffers cache"),
            (0x14, "PSSetConstantBuffers cache"),
            (0x15, "CSSetConstantBuffers cache"),
            (0x16, "VSSetShaderResources cache"),
            (0x17, "HSSetShaderResources cache"),
            (0x18, "DSSetShaderResources cache"),
            (0x19, "GSSetShaderResources cache"),
            (0x1a, "PSSetShaderResources cache"),
            (0x1b, "CSSetShaderResources cache"),
        ];
        for (byte, needle) in expected {
            let opcode = Opcode::from_u8(byte).expect("opcode in 0..=0x3f");
            match opcode.handler() {
                Handler::Open(reason) => {
                    assert!(
                        reason.contains(needle),
                        "opcode 0x{byte:02x} must name '{needle}', got: {reason}"
                    );
                    assert!(
                        reason.contains("out-of-band"),
                        "opcode 0x{byte:02x} must name its out-of-band gap"
                    );
                }
                Handler::Implemented(method) => {
                    panic!("opcode 0x{byte:02x} must stay open, got {method}")
                }
            }
            let bytes = record(byte, 0x18, &[]);
            let mut sink = RecordingSink::default();
            let summary = execute(&bytes, &mut sink).unwrap();
            assert_eq!(summary.open, 1, "opcode 0x{byte:02x}");
            assert_eq!(summary.implemented, 0, "opcode 0x{byte:02x}");
            assert_eq!(
                sink.calls,
                vec![Call::Open(byte)],
                "opcode 0x{byte:02x} must only report open"
            );
        }
    }

    #[test]
    fn an_open_bind_record_never_dereferences_its_descriptor_pointer() {
        // A synthetic bind record carries a deliberately invalid descriptor
        // array pointer. The executor must report the opcode open and must
        // not read through the pointer.
        let bytes = record(
            0x10,
            0x30,
            &[
                (0x18, &0x0000_0001_0000_0000u64.to_le_bytes()),
                (0x20, &4u32.to_le_bytes()),
                (0x28, &0xdead_beef_dead_beefu64.to_le_bytes()),
            ],
        );
        let mut sink = RecordingSink::default();
        let summary = execute(&bytes, &mut sink).unwrap();
        assert_eq!(summary.open, 1);
        assert_eq!(summary.implemented, 0);
        assert_eq!(sink.calls, vec![Call::Open(0x10)]);
    }

    #[test]
    fn every_open_opcode_says_why_and_every_implemented_opcode_names_a_sink_method() {
        for opcode in Opcode::ALL {
            match opcode.handler() {
                Handler::Open(reason) => {
                    assert!(
                        !reason.is_empty(),
                        "opcode 0x{:02x} must name its read gap",
                        opcode.as_u8()
                    );
                }
                Handler::Implemented(method) => {
                    assert!(
                        !method.is_empty(),
                        "opcode 0x{:02x} must name its sink method",
                        opcode.as_u8()
                    );
                }
            }
        }
        let implemented = Opcode::ALL
            .iter()
            .filter(|opcode| matches!(opcode.handler(), Handler::Implemented(_)))
            .count();
        assert_eq!(implemented, 11);
    }

    #[test]
    fn a_mixed_stream_preserves_the_client_order() {
        let mut stream = Vec::new();
        stream.extend_from_slice(&record(
            0x07,
            0x30,
            &[
                (0x18, &2u64.to_le_bytes()),
                (0x20, &f32_bytes(&[0.1, 0.2, 0.3, 1.0])),
            ],
        ));
        stream.extend_from_slice(&record(
            0x29,
            0x30,
            &[
                (0x18, &f32_bytes(&[0.0, 0.0, 1920.0, 1080.0])),
                (0x28, &f32_bytes(&[0.0, 1.0])),
            ],
        ));
        stream.extend_from_slice(&record(
            0x26,
            0x30,
            &[
                (0x18, &12u32.to_le_bytes()),
                (0x1c, &1u32.to_le_bytes()),
                (0x20, &0u32.to_le_bytes()),
                (0x24, &0i32.to_le_bytes()),
                (0x28, &0u32.to_le_bytes()),
            ],
        ));
        stream.extend_from_slice(&record(
            0x30,
            0x28,
            &[
                (0x18, &1u32.to_le_bytes()),
                (0x1c, &2u32.to_le_bytes()),
                (0x20, &3u32.to_le_bytes()),
            ],
        ));
        stream.extend_from_slice(&record(0x00, 0x18, &[]));
        let mut sink = RecordingSink::default();
        let summary = execute(&stream, &mut sink).unwrap();
        assert_eq!(summary.records, 5);
        assert_eq!(summary.implemented, 5);
        assert_eq!(summary.open, 0);
        assert_eq!(summary.missing, 0);
        assert_eq!(summary.bytes_consumed, stream.len());
        assert_eq!(
            sink.calls,
            vec![
                Call::ClearRender {
                    view: 2,
                    colour: [0.1, 0.2, 0.3, 1.0],
                },
                Call::Viewport(Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: 1920.0,
                    height: 1080.0,
                    min_depth: 0.0,
                    max_depth: 1.0,
                }),
                Call::DrawIndexed {
                    index_count: 12,
                    start_index: 0,
                    base_vertex: 0,
                },
                Call::Dispatch { x: 1, y: 2, z: 3 },
                Call::End,
            ]
        );
    }

    #[test]
    fn the_end_opcode_stops_the_stream() {
        let mut stream = record(0x00, 0x18, &[]);
        stream.extend_from_slice(&record(0x30, 0x28, &[]));
        let mut sink = RecordingSink::default();
        let summary = execute(&stream, &mut sink).unwrap();
        assert_eq!(summary.records, 1);
        assert_eq!(summary.bytes_consumed, 0x18);
        assert_eq!(sink.calls, vec![Call::End]);
    }

    #[test]
    fn the_no_op_opcode_is_the_loop_head() {
        let bytes = record(0x1d, 0x18, &[]);
        let mut sink = RecordingSink::default();
        let summary = execute(&bytes, &mut sink).unwrap();
        assert_eq!(summary.implemented, 1);
        assert_eq!(sink.calls, vec![Call::NoOp(0x1d)]);
        assert_eq!(Opcode::NoOp.target(), 0x1426_6c736);
    }

    #[test]
    fn the_stage_constants_are_declared() {
        assert_eq!(STAGE, "D4");
        assert_eq!(HANDLER_STAGE, "D5");
        assert_eq!(OPCODE_COUNT, 64);
    }
}
