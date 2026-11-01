//! The client's per-frame order on the D3D11 path (stage D3).
//!
//! The order is read from the engine frame functions and the `pm_dx11` /
//! `pm_dxgi` leaves they reach. Each step names the read call site. A step
//! whose D3D11 body is not read is [`Body::Open`]: the driver logs it and
//! does **not** fake the operation. Only [`Step::Present`] has a complete
//! read body today, because the client's frame is a recorded command stream
//! (D4) and its pass list is open.
//!
//! The steps:
//!
//! 1. **FrameBegin** — frame-begin slot 129 (`0x1433585d0`) →
//!    `FUN_141ef0600`: finish the pending frame (per-view submit and
//!    swap-chain update), release the deferred frame-slot targets, recycle a
//!    descriptor block.
//! 2. **GpuWait** — wait/pacing slot 130 (`0x1433585d8`) →
//!    `FUN_141ef9c80`: GPU-pacing/marker step and deferred-target release.
//! 3. **ScenePrepare** — `FUN_141ef8140`: per-frame buffer update, resource
//!    transitions, record `0x30` for the frame command lists.
//! 4. **UploadFlush** — `FUN_142650340` → `FUN_142650430`
//!    (`CmdQueue::FinishInternal`): flush, signal the fence, then a blocking
//!    `WaitForSingleObject(INFINITE)`.
//! 5. **CommandReplay** — the flush dispatches through the queue vtable
//!    `+0x50` to `pm_dx11::CmdQueueImpl::Submit` (`0x142677dc0`), whose
//!    immediate path calls `FUN_14266c690` under the device lock.
//! 6. **FenceWait** — the pm fence object's completed-value/set-event wait
//!    (`FUN_14265adf0` in the per-view submit path).
//! 7. **PerViewSubmit** — `FUN_141f0caa0`: emit the `0x30` records, flush
//!    (`FUN_142650800`), wait on the frame's fence, release the slot.
//! 8. **FrameEnd** — frame-end slot 131 (`0x1433585e0`) →
//!    `FUN_141ef0800`: final composite/blit records, the two `0x387` batch
//!    records, per-view submit, then present.
//! 9. **Present** — `FUN_14265e8f0` → `pm_dxgi::Instance::Present`
//!    (`0x142630790`) → `IDXGISwapChain::Present(sync interval, flags)`.
//!
//! **What is not read.** The caller that invokes the shared frame slots is
//! not identified, so the adjacency of steps 1 and 2 comes from their vtable
//! slots (129 then 130), not from a read call site. The colour clear and
//! viewport apply are named in the replay (D5: opcodes `0x07`, `0x04`, `0x29`
//! and `0x2a`), but the render-target **bind** is not: the replay's only
//! `OMSetRenderTargets` call is the unbind in opcode `0x0d`; the bind lives in
//! the pm state-cache apply (`FUN_142674980`), whose replay trigger is not
//! read. The D3D11 fence/query wait body is not read. See
//! `docs/formats/renderer.md`.

use crate::d3d11::{Error, WindowSurface};

/// Which stage of the client's renderer this module implements.
pub const STAGE: &str = "D3";

/// One step of the read frame order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    FrameBegin,
    GpuWait,
    ScenePrepare,
    UploadFlush,
    CommandReplay,
    FenceWait,
    PerViewSubmit,
    FrameEnd,
    Present,
}

/// Whether the step's D3D11 body is read (and therefore executed) or open
/// (logged only, with the reason).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Body {
    /// The D3D11 leaf is read and this driver executes it.
    Executed,
    /// The call site is read, the D3D11 body is not. The driver never fakes
    /// the operation; the string says what is missing.
    Open(&'static str),
}

impl Step {
    /// The read order, top to bottom. The unit test pins it.
    ///
    /// Steps 1–2 are the two shared start-of-frame slots in vtable order;
    /// the outer caller is not read. Steps 3–9 are the read call sequence:
    /// scene prepare ends in the upload finish, the finish dispatches to the
    /// queue submit (replay), the per-view submit flushes and waits, and the
    /// frame end presents through `pm_dxgi`.
    pub const ORDER: [Step; 9] = [
        Step::FrameBegin,
        Step::GpuWait,
        Step::ScenePrepare,
        Step::UploadFlush,
        Step::CommandReplay,
        Step::FenceWait,
        Step::PerViewSubmit,
        Step::FrameEnd,
        Step::Present,
    ];

    /// The step name used in the runtime log.
    pub const fn label(self) -> &'static str {
        match self {
            Step::FrameBegin => "FrameBegin",
            Step::GpuWait => "GpuWait",
            Step::ScenePrepare => "ScenePrepare",
            Step::UploadFlush => "UploadFlush",
            Step::CommandReplay => "CommandReplay",
            Step::FenceWait => "FenceWait",
            Step::PerViewSubmit => "PerViewSubmit",
            Step::FrameEnd => "FrameEnd",
            Step::Present => "Present",
        }
    }

    /// The client entry this step names, with its current-image address.
    pub const fn client_entry(self) -> &'static str {
        match self {
            Step::FrameBegin => "0x141ef0600 (frame-begin slot 129, 0x1433585d0)",
            Step::GpuWait => "0x141ef9c80 (wait/pacing slot 130, 0x1433585d8)",
            Step::ScenePrepare => "0x141ef8140 (scene prepare / buffer update)",
            Step::UploadFlush => "0x142650340 -> 0x142650430 (queue finish, flush + wait)",
            Step::CommandReplay => "0x142677dc0 -> 0x14266c690 (pm_dx11 submit -> replay)",
            Step::FenceWait => "0x14265adf0 (pm fence wait)",
            Step::PerViewSubmit => "0x141f0caa0 (per-view submit)",
            Step::FrameEnd => "0x141ef0800 (frame-end slot 131, 0x1433585e0)",
            Step::Present => "0x14265e8f0 -> 0x142630790 (pm_dxgi present)",
        }
    }

    /// Whether the driver executes this step.
    pub const fn body(self) -> Body {
        match self {
            Step::Present => Body::Executed,
            Step::FrameBegin => Body::Open(
                "the pending submit, deferred-target release and descriptor recycle are read; \
                 the D3D11 leaves are the read submit path and D2 resize",
            ),
            Step::GpuWait => {
                Body::Open("the wait is a pm fence object; its D3D11 implementation is not read")
            }
            Step::ScenePrepare => Body::Open(
                "the buffer contents, pass list and record-id -> opcode translation are open",
            ),
            Step::UploadFlush => Body::Open(
                "the flush body is read, but there is no read record stream in the crate to \
                 flush; D4's executor is implemented and synthetically tested",
            ),
            Step::CommandReplay => Body::Open(
                "the 64-opcode dispatch and the read handlers are implemented in `replay`, but \
                 no read record stream exists in the crate; the step is not executed",
            ),
            Step::FenceWait => Body::Open("the pm fence object's D3D11 implementation is not read"),
            Step::PerViewSubmit => Body::Open(
                "record emission is open; the pm queue and the submit/present leaves are read",
            ),
            Step::FrameEnd => Body::Open(
                "the final-composite and blit pass bodies are not read; Present is the read leaf",
            ),
        }
    }
}

/// One executed step, for a test or a caller that wants the order back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameLogEntry {
    pub step: Step,
    pub body: Body,
}

/// The D3 `CommandReplay` wiring point: run one contiguous D4 record stream
/// through the replay executor and its sink.
///
/// The step itself stays [`Body::Open`] because no read record stream exists in
/// the crate yet. This function is the place a read stream will enter once one
/// does; the synthetic tests call the executor directly.
pub fn run_replay<S: crate::replay::Sink>(
    stream: &[u8],
    sink: &mut S,
) -> Result<crate::replay::Summary, crate::replay::Error> {
    crate::replay::execute(stream, sink)
}

/// Run the pinned step order, logging one `renderer: D3:` line per step and
/// handing each step to `visit`.
///
/// This is the testable core: the unit test passes a visitor that records the
/// order and proves the driver visits exactly [`Step::ORDER`].
pub fn run_steps<F>(mut visit: F) -> Result<Vec<FrameLogEntry>, Error>
where
    F: FnMut(Step, Body) -> Result<(), Error>,
{
    let mut log = Vec::with_capacity(Step::ORDER.len());
    for step in Step::ORDER {
        let body = step.body();
        match body {
            Body::Executed => println!("renderer: D3: {}", step.label()),
            Body::Open(reason) => {
                println!("renderer: D3: {} (open: {reason})", step.label());
            }
        }
        visit(step, body)?;
        log.push(FrameLogEntry { step, body });
    }
    Ok(log)
}

/// Run one frame of the D3D11 path through the step list.
///
/// The D2 views are untouched; the swap chain is the one D1/D2 created and
/// the RTV is the one D2 created. `placeholder_clear` keeps the D1 visible
/// placeholder (a declared non-client colour) so the window is not garbage;
/// it is logged as a D1 placeholder, never as a D3 step.
pub fn run_frame(
    surface: &mut WindowSurface,
    frame: u32,
    placeholder_clear: bool,
) -> Result<Vec<FrameLogEntry>, Error> {
    println!("renderer: D3: frame {frame}");
    run_steps(|step, _body| {
        if step == Step::Present {
            if placeholder_clear {
                println!(
                    "renderer: D1 placeholder: ClearRenderTargetView \
                     (no client clear value is read)"
                );
                surface.clear_placeholder()?;
            }
            surface.present()?;
        }
        Ok(())
    })
}

/// Run `frames` frames starting at `first_frame`.
pub fn run_frames(
    surface: &mut WindowSurface,
    first_frame: u32,
    frames: u32,
    placeholder_clear: bool,
) -> Result<Vec<FrameLogEntry>, Error> {
    let mut last = Vec::new();
    for frame in first_frame..first_frame.saturating_add(frames) {
        last = run_frame(surface, frame, placeholder_clear)?;
    }
    Ok(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_read_step_order_is_pinned() {
        assert_eq!(
            Step::ORDER.map(Step::label),
            [
                "FrameBegin",
                "GpuWait",
                "ScenePrepare",
                "UploadFlush",
                "CommandReplay",
                "FenceWait",
                "PerViewSubmit",
                "FrameEnd",
                "Present",
            ]
        );
        assert_eq!(STAGE, "D3");
    }

    #[test]
    fn every_step_names_a_read_client_entry() {
        for step in Step::ORDER {
            let entry = step.client_entry();
            assert!(
                entry.starts_with("0x14"),
                "{} must name a current-image address, got {entry}",
                step.label()
            );
        }
        // Spot-check the addresses the docs record.
        assert!(Step::FrameBegin.client_entry().contains("0x141ef0600"));
        assert!(Step::GpuWait.client_entry().contains("0x141ef9c80"));
        assert!(Step::FrameEnd.client_entry().contains("0x141ef0800"));
        assert!(Step::PerViewSubmit.client_entry().contains("0x141f0caa0"));
        assert!(Step::Present.client_entry().contains("0x142630790"));
    }

    #[test]
    fn only_present_is_executed_and_every_open_step_says_why() {
        assert_eq!(Step::Present.body(), Body::Executed);
        let open = Step::ORDER
            .iter()
            .filter(|step| matches!(step.body(), Body::Open(_)))
            .count();
        assert_eq!(open, Step::ORDER.len() - 1);
        for step in Step::ORDER {
            if let Body::Open(reason) = step.body() {
                assert!(
                    !reason.is_empty(),
                    "{} must say what its D3D11 body is missing",
                    step.label()
                );
            }
        }
    }

    #[test]
    fn the_driver_visits_the_pinned_order_once() {
        let mut seen = Vec::new();
        let log = run_steps(|step, _body| {
            seen.push(step);
            Ok(())
        })
        .expect("the pure driver has no GPU call");
        assert_eq!(seen, Step::ORDER.to_vec());
        assert_eq!(log.len(), Step::ORDER.len());
        assert_eq!(log.last().unwrap().step, Step::Present);
    }

    #[test]
    fn the_replay_wiring_point_is_the_executor() {
        struct EmptySink;
        impl crate::replay::Sink for EmptySink {
            fn set_render_target_set(
                &mut self,
                _set: crate::replay::RenderTargetSet,
            ) -> Result<(), crate::replay::Error> {
                unreachable!("an empty stream calls no operation")
            }

            fn resolve_render_target_set(&mut self) -> Result<(), crate::replay::Error> {
                unreachable!("an empty stream calls no operation")
            }

            fn clear_render_target_view(
                &mut self,
                _view: u64,
                _colour: [f32; 4],
            ) -> Result<(), crate::replay::Error> {
                unreachable!("an empty stream calls no operation")
            }

            fn clear_depth_stencil_view(
                &mut self,
                _view: u64,
                _flags: u32,
                _depth: f32,
                _stencil: u8,
            ) -> Result<(), crate::replay::Error> {
                unreachable!("an empty stream calls no operation")
            }

            fn draw(
                &mut self,
                _vertex_count: u32,
                _start_vertex: u32,
            ) -> Result<(), crate::replay::Error> {
                unreachable!("an empty stream calls no operation")
            }

            fn draw_instanced(
                &mut self,
                _vertex_count: u32,
                _instance_count: u32,
                _start_vertex: u32,
                _start_instance: u32,
            ) -> Result<(), crate::replay::Error> {
                unreachable!("an empty stream calls no operation")
            }

            fn draw_indexed(
                &mut self,
                _index_count: u32,
                _start_index: u32,
                _base_vertex: i32,
            ) -> Result<(), crate::replay::Error> {
                unreachable!("an empty stream calls no operation")
            }

            fn draw_indexed_instanced(
                &mut self,
                _index_count: u32,
                _instance_count: u32,
                _start_index: u32,
                _base_vertex: i32,
                _start_instance: u32,
            ) -> Result<(), crate::replay::Error> {
                unreachable!("an empty stream calls no operation")
            }

            fn set_viewport(
                &mut self,
                _viewport: crate::replay::Viewport,
            ) -> Result<(), crate::replay::Error> {
                unreachable!("an empty stream calls no operation")
            }

            fn set_scissor_rect(
                &mut self,
                _rect: crate::replay::ScissorRect,
            ) -> Result<(), crate::replay::Error> {
                unreachable!("an empty stream calls no operation")
            }

            fn dispatch(&mut self, _x: u32, _y: u32, _z: u32) -> Result<(), crate::replay::Error> {
                unreachable!("an empty stream calls no operation")
            }

            fn missing_opcode(&mut self, _opcode: u32) -> Result<(), crate::replay::Error> {
                unreachable!("an empty stream calls no operation")
            }

            fn open(
                &mut self,
                _opcode: crate::replay::Opcode,
                _reason: &'static str,
            ) -> Result<(), crate::replay::Error> {
                unreachable!("an empty stream calls no operation")
            }
        }

        let summary = run_replay(&[], &mut EmptySink).expect("an empty stream is valid");
        assert_eq!(summary, crate::replay::Summary::default());
        assert!(matches!(Step::CommandReplay.body(), Body::Open(_)));
    }

    #[test]
    fn a_failing_step_stops_the_frame() {
        let mut seen = Vec::new();
        let result = run_steps(|step, _body| {
            seen.push(step);
            if step == Step::CommandReplay {
                return Err(Error::MissingObject("test stop"));
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(
            seen,
            vec![
                Step::FrameBegin,
                Step::GpuWait,
                Step::ScenePrepare,
                Step::UploadFlush,
                Step::CommandReplay,
            ]
        );
    }
}
