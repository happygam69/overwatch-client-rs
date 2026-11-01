//! The read per-frame mover velocity law.
//!
//! Both live update paths end by handing one 128-bit vector to
//! `SetCharacterControllerVelocity` (`0x142cbd1b0`). The vector is
//! `state_transform(wish) + ramp_vector(body)`, and the client builds it in
//! this order:
//!
//! ```text
//! MoverUpdateFrame (0x140cbb260)
//!   0x140cbbdc2  movups xmm0, [rsi + 0x1140]   ; the wish vector
//!   0x140cbbdd6  call   FUN_140ca5c50          ; state transform, in place
//!   0x140cbbde6  call   FUN_140ca75b0          ; ramp vector -> [rsp + 0x50]
//!   0x140cbbdf5  addps  xmm0, [rsp + 0x50]     ; state + ramp
//!   0x140cbbe0b  call   SetCharacterControllerVelocity([rsi + 0x1ad8], &v)
//!
//! MoverUpdateCommand (0x140cb5760)
//!   0x140cb5897  call   FUN_140ca75b0          ; ramp vector -> [rbp - 0x10]
//!   0x140cb589c  movaps xmm0, [rbp - 0x10]
//!   0x140cb58a4  addps  xmm0, [r14 + 0x20]     ; ramp + state
//!   0x140cb58b4  call   SetCharacterControllerVelocity([r15 + 0x1ad8], &v)
//! ```
//!
//! Both call sites pass `body + 0x10d0` as the ramp leaf's third argument
//! (`0x140cbb890` sets `R15` to it, `0x140cb5889` passes it directly), so the
//! ramp reads the flags byte at `body + 0x10d0` and the offset at
//! `body + 0x10fc`.
//!
//! The two callees are already verified leaves: `fn/MoverStateVector`
//! (`0x140ca5c50`, which resolves the state record and applies the forward
//! transform `0x140cb5510`) and `fn/MoverRampVector` (`0x140ca75b0`). This
//! module is their composition in the client's order. The composition has no
//! client-side unit of its own (both operands are separate units), and the
//! producer of the wish vector at `+0x1140` — the fan writer
//! `FUN_140cb86c0` — is still open, so nothing here invents an input.
//!
//! The published vector's *meaning* is not settled either. The client's
//! `SetCharacterControllerVelocity` (`0x142cbd1b0`) writes the vector to the
//! controller entry's `+0x10`, the same field its matching getter
//! (`0x142cbcec0` -> `0x142cf5630`) reads back; for the rigid variant that
//! record is a `dmBody`, where `+0x10` is the solve writeback's aux vector and
//! the position is `+0xf0` (`docs/formats/domino.md`). Whether the value is a
//! position-like transform or a velocity is open, so this module names it the
//! way the client's caller does and makes no claim beyond the arithmetic.

use crate::movement::{
    forward_transform, ramp_vector, sse_add, DIRECTION, FLAGS_BLOCK, RAMP_OFFSET, SCALE_BIT,
    SCALE_CLEAR, SCALE_SET, TIMER,
};

/// The wish vector the spring writes, `body + 0x1140`.
pub const WISH: usize = 0x1140;

#[inline]
fn f32_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[inline]
fn lanes_at(bytes: &[u8], offset: usize) -> [f32; 4] {
    [
        f32_at(bytes, offset),
        f32_at(bytes, offset + 4),
        f32_at(bytes, offset + 8),
        f32_at(bytes, offset + 12),
    ]
}

/// The wish vector as the frame law reads it: four `f32` lanes at
/// `body + 0x1140`.
pub fn wish(body: &[u8]) -> [f32; 4] {
    lanes_at(body, WISH)
}

/// The ramp leaf's inputs, read from the body and its `body + 0x10d0` block.
///
/// `body + 0xc0` is the four-lane direction, `+0x10ec` the timer,
/// `+0x5a8`/`+0x5b4` the two scale slots and `+0x10fc` the offset
/// (`flags + 0x2c`). The flags bit that selects the second scale slot is bit
/// 1 of the block's first byte.
pub fn ramp_from_body(body: &[u8], flags: &[u8]) -> [f32; 4] {
    debug_assert!(body.len() >= FLAGS_BLOCK + RAMP_OFFSET + 4);
    debug_assert!(flags.len() >= RAMP_OFFSET + 4);
    ramp_vector(
        f32_at(body, TIMER),
        flags[0] & SCALE_BIT != 0,
        f32_at(body, SCALE_CLEAR),
        f32_at(body, SCALE_SET),
        f32_at(flags, RAMP_OFFSET),
        lanes_at(body, DIRECTION),
    )
}

/// `FUN_140ca5c50`: transform the wish vector through the resolved state
/// matrix, in place.
///
/// The client copies the vector, resolves the state object and calls
/// `FUN_140cb5510` (the verified forward transform). When the lookup declines
/// — the sentinel state id, or a handle that does not match — the client
/// leaves the vector untouched, which is the `None` arm here.
pub fn state_vector(wish: [f32; 4], matrix: Option<&[f32; 16]>) -> [f32; 4] {
    let mut v = wish;
    if let Some(m) = matrix {
        forward_transform(&mut v, m);
    }
    v
}

/// The `MoverUpdateFrame` addition: `ADDPS xmm0=state, src=ramp`.
///
/// One `f32` add per lane with the SSE operand rule, so a NaN result keeps the
/// client's payload and sign.
pub fn frame_velocity(state: [f32; 4], ramp: [f32; 4]) -> [f32; 4] {
    [
        sse_add(state[0], ramp[0]),
        sse_add(state[1], ramp[1]),
        sse_add(state[2], ramp[2]),
        sse_add(state[3], ramp[3]),
    ]
}

/// The `MoverUpdateCommand` addition: `ADDPS xmm0=ramp, src=state`.
///
/// The same arithmetic as [`frame_velocity`] with the operands swapped, which
/// differs only in which NaN payload survives.
pub fn command_velocity(ramp: [f32; 4], state: [f32; 4]) -> [f32; 4] {
    [
        sse_add(ramp[0], state[0]),
        sse_add(ramp[1], state[1]),
        sse_add(ramp[2], state[2]),
        sse_add(ramp[3], state[3]),
    ]
}

/// The mover command block's length: the actor record's `+0x00..+0x14`.
pub const COMMAND_LEN: usize = 0x14;
/// The actor's current command record, `0x14068d1a6`.
pub const ACTOR_COMMAND: usize = 0x1210;
/// The actor's next command record; its `+0x04`/`+0x06` are the yaw and pitch
/// the packer keeps (`0x14068d1d5`, `0x14068d1c5`).
pub const ACTOR_NEXT_COMMAND: usize = 0x1224;
/// The actor's sequence counter, `0x14068d1ae`.
pub const ACTOR_SEQUENCE: usize = 0xe14;

/// `0x14068cf10`'s packer: the command block handed to `MoverUpdateFrame`.
///
/// ```text
/// 0x14068d1a6  movups xmm0, [r15 + 0x1210]   ; the current record, 16 bytes
/// 0x14068d19b  mov    eax,  [r15 + 0x1220]   ; the current record's flag dword
/// 0x14068d1cd  movups [rbp - 0x70], xmm0     ; command + 0x00..0x0f
/// 0x14068d1c2  mov    [rbp - 0x60], eax      ; command + 0x10..0x13
/// 0x14068d1d5  movzx  eax, word [r15 + 0x1228]   ; the NEXT record's yaw
/// 0x14068d1dd  mov    word [rbp - 0x6c], ax      ; command + 0x04
/// 0x14068d1c5  movzx  eax, word [r15 + 0x122a]   ; the NEXT record's pitch
/// 0x14068d1d1  mov    word [rbp - 0x6a], ax      ; command + 0x06
/// ```
///
/// So the block is the current record verbatim, with the **next** record's
/// yaw and pitch overlaid: the newest aim is used, the rest of the command
/// words stay those of the record being consumed. `+0x1228`/`+0x122a` are
/// `next + 0x04`/`+0x06`, the same yaw/pitch slots the current record has.
pub fn pack_command(current: &[u8], next: &[u8]) -> [u8; COMMAND_LEN] {
    let mut command: [u8; COMMAND_LEN] = current[..COMMAND_LEN].try_into().unwrap();
    command[0x04..0x06].copy_from_slice(&next[0x04..0x06]);
    command[0x06..0x08].copy_from_slice(&next[0x06..0x08]);
    command
}

/// `0x14068d1ae`/`0x14068d1bf`: the sequence passed to `MoverUpdateFrame` is
/// the actor's counter **incremented**, as a 32-bit wrap.
pub fn command_sequence(actor: &[u8]) -> u32 {
    let counter = u32::from_le_bytes(
        actor[ACTOR_SEQUENCE..ACTOR_SEQUENCE + 4]
            .try_into()
            .unwrap(),
    );
    counter.wrapping_add(1)
}

/// The whole `MoverUpdateFrame` tail from a body slice and its flags block:
/// `frame_velocity(state_vector(wish), ramp_from_body)`.
///
/// `matrix` is the resolved state transform (`None` when the lookup declines),
/// so the caller owns the still-open lookup rather than this stage guessing
/// one.
pub fn update_frame(body: &[u8], flags: &[u8], matrix: Option<&[f32; 16]>) -> [f32; 4] {
    let state = state_vector(wish(body), matrix);
    frame_velocity(state, ramp_from_body(body, flags))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A body slice with the fields the law reads, at their client offsets.
    fn body_with(
        wish: [f32; 4],
        direction: [f32; 4],
        timer: f32,
        scale_clear: f32,
        scale_set: f32,
    ) -> Vec<u8> {
        let mut body = vec![0u8; WISH + 0x10];
        for (lane, value) in wish.iter().enumerate() {
            body[WISH + lane * 4..WISH + lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (lane, value) in direction.iter().enumerate() {
            body[DIRECTION + lane * 4..DIRECTION + lane * 4 + 4]
                .copy_from_slice(&value.to_le_bytes());
        }
        body[TIMER..TIMER + 4].copy_from_slice(&timer.to_le_bytes());
        body[SCALE_CLEAR..SCALE_CLEAR + 4].copy_from_slice(&scale_clear.to_le_bytes());
        body[SCALE_SET..SCALE_SET + 4].copy_from_slice(&scale_set.to_le_bytes());
        body
    }

    fn flags_block(bit: bool, offset: f32) -> Vec<u8> {
        let mut flags = vec![0u8; 0x30];
        if bit {
            flags[0] = SCALE_BIT;
        }
        flags[RAMP_OFFSET..RAMP_OFFSET + 4].copy_from_slice(&offset.to_le_bytes());
        flags
    }

    fn identity() -> [f32; 16] {
        let mut m = [0.0f32; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        m
    }

    #[test]
    fn ramp_reads_the_clear_scale_slot() {
        let body = body_with([0.0; 4], [1.0, 2.0, 3.0, 4.0], 0.5, 2.0, 9.0);
        let flags = flags_block(false, 0.0);
        // minss(0.5, 1.0) * 2.0 + 0.0 = 1.0
        assert_eq!(ramp_from_body(&body, &flags), [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn ramp_reads_the_set_scale_slot_and_offset() {
        let body = body_with([0.0; 4], [1.0, 0.0, 0.0, 0.0], 1.0, 2.0, 9.0);
        let flags = flags_block(true, 0.25);
        // minss(1.0, 1.0) * 9.0 + 0.25 = 9.25
        assert_eq!(ramp_from_body(&body, &flags)[0], 9.25);
    }

    #[test]
    fn timer_is_clamped_before_the_scale() {
        let body = body_with([0.0; 4], [1.0, 0.0, 0.0, 0.0], 4.0, 3.0, 0.0);
        let flags = flags_block(false, 0.0);
        // minss(4.0, 1.0) * 3.0 = 3.0, not 12.0
        assert_eq!(ramp_from_body(&body, &flags)[0], 3.0);
    }

    #[test]
    fn state_vector_without_a_matrix_is_the_wish() {
        let wish = [1.5, -2.0, 0.25, 4.0];
        assert_eq!(state_vector(wish, None), wish);
    }

    #[test]
    fn state_vector_applies_the_forward_transform() {
        let mut m = identity();
        m[12] = 10.0;
        m[13] = 20.0;
        m[14] = 30.0;
        let wish = [1.0, 2.0, 3.0, 4.0];
        // The transform overwrites the w lane with the matrix's `c3.w`.
        assert_eq!(state_vector(wish, Some(&m)), [11.0, 22.0, 33.0, 1.0]);
    }

    #[test]
    fn frame_velocity_adds_state_then_ramp() {
        let state = [1.0, 2.0, 3.0, 4.0];
        let ramp = [0.5, 0.25, -1.0, 0.0];
        assert_eq!(frame_velocity(state, ramp), [1.5, 2.25, 2.0, 4.0]);
    }

    #[test]
    fn frame_and_command_orders_differ_only_in_the_nan_payload() {
        let quiet_state = f32::from_bits(0x7fc0_0001);
        let quiet_ramp = f32::from_bits(0x7fc0_0002);
        let frame = frame_velocity([quiet_state; 4], [quiet_ramp; 4]);
        let command = command_velocity([quiet_ramp; 4], [quiet_state; 4]);
        assert_eq!(frame[0].to_bits(), 0x7fc0_0001);
        assert_eq!(command[0].to_bits(), 0x7fc0_0002);
    }

    #[test]
    fn the_packer_keeps_the_current_word_but_takes_the_next_aim() {
        let mut current = [0u8; COMMAND_LEN];
        current[0x04..0x06].copy_from_slice(&100i16.to_le_bytes()); // old yaw
        current[0x06..0x08].copy_from_slice(&(-200i16).to_le_bytes()); // old pitch
        current[0x0c..0x10].copy_from_slice(&0x0102_0304u32.to_le_bytes()); // buttons
        current[0x10..0x14].copy_from_slice(&7u32.to_le_bytes()); // flag dword
        let mut next = [0u8; COMMAND_LEN];
        next[0x04..0x06].copy_from_slice(&(-5i16).to_le_bytes()); // new yaw
        next[0x06..0x08].copy_from_slice(&9i16.to_le_bytes()); // new pitch
        next[0x0c..0x10].copy_from_slice(&0xdead_beefu32.to_le_bytes()); // ignored

        let command = pack_command(&current, &next);
        assert_eq!(&command[0x04..0x06], &(-5i16).to_le_bytes());
        assert_eq!(&command[0x06..0x08], &9i16.to_le_bytes());
        // Everything else is the current record, including its buttons.
        assert_eq!(&command[0x0c..0x10], &0x0102_0304u32.to_le_bytes());
        assert_eq!(&command[0x10..0x14], &7u32.to_le_bytes());
    }

    #[test]
    fn the_sequence_is_the_actor_counter_plus_one() {
        let mut actor = vec![0u8; ACTOR_SEQUENCE + 4];
        actor[ACTOR_SEQUENCE..ACTOR_SEQUENCE + 4].copy_from_slice(&41u32.to_le_bytes());
        assert_eq!(command_sequence(&actor), 42);
    }

    #[test]
    fn the_sequence_wraps_like_the_increment() {
        let mut actor = vec![0u8; ACTOR_SEQUENCE + 4];
        actor[ACTOR_SEQUENCE..ACTOR_SEQUENCE + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(command_sequence(&actor), 0);
    }

    #[test]
    fn update_frame_is_the_composition() {
        let body = body_with([1.0, 2.0, 3.0, 4.0], [2.0, 0.0, 0.0, 0.0], 0.5, 1.0, 0.0);
        let flags = flags_block(false, 0.0);
        let mut m = identity();
        m[12] = 1.0;
        // state = (2, 2, 3, 1); ramp = 0.5 * (2, 0, 0, 0)
        assert_eq!(update_frame(&body, &flags, Some(&m)), [3.0, 2.0, 3.0, 1.0]);
        // With no resolved state the wish passes through untouched.
        assert_eq!(update_frame(&body, &flags, None), [2.0, 2.0, 3.0, 4.0]);
    }
}
