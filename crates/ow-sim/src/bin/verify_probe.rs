//! Tier-2 verification probe for `ow-sim`.
//!
//! The harness builds this binary with `cargo build --release --bin
//! verify_probe` and speaks its line protocol:
//!
//! ```text
//! -> ping
//! <- pong
//!
//! -> state <hex base> <hex bytes>
//! <- ok
//!
//! -> call <entry> <nargs> [ptr <hex> <relocs> <guest|-> | f32 <8hex> |
//!                          f64 <16hex> | u64 <16hex>]*
//! <- ok <retbits-hex|-> <outhex> [<statehex>]
//! <- err <message-with-no-spaces>
//! ```
//!
//! `mover_ramp_vector` proves `MoverRampVector` (`0x140ca75b0`): the probe
//! reads the timer, the two scale slots and the four-lane direction from the
//! body pointee, and the flags byte and scalar from the caller's
//! `body + 0x10d0` block, then calls `ow_sim::movement::ramp_vector`.

use std::io::{self, BufRead, Write};

/// The probe's copy of a unit's preloaded state snapshot. The ramp unit has
/// no state block, but the protocol's `state` line is still accepted so the
/// binary stays a general probe.
#[allow(dead_code)]
struct State {
    base: u64,
    snapshot: Vec<u8>,
    buf: Vec<u8>,
}

impl State {
    fn new() -> State {
        State {
            base: 0,
            snapshot: Vec::new(),
            buf: Vec::new(),
        }
    }

    fn loaded(&self) -> bool {
        !self.snapshot.is_empty()
    }
}

#[allow(dead_code)] // f32/f64/u64 slots are part of the protocol surface.
enum Arg {
    Ptr(Vec<u8>, Vec<usize>, Option<u64>),
    F32(u32),
    F64(u64),
    U64(u64),
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s == "-" {
        return Some(Vec::new());
    }
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

fn hex_encode(b: &[u8]) -> String {
    if b.is_empty() {
        return "-".to_string();
    }
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{:02x}", x));
    }
    s
}

fn f32_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

/// `MoverRampVector` (`0x140ca75b0`): `void (body*, out[4], flags_block*)`.
///
/// The client reads `body + 0x10ec`, `body + 0x5a8`, `body + 0x5b4`,
/// `body + 0xc0..0xd0`, and `flags_block + 0` / `+0x2c`; it writes four f32
/// lanes to `out`. The probe performs the same field reads and calls the
/// shipped arithmetic.
fn mover_ramp_vector_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 3 {
        return Err(format!("expected 3 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let body = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x10f0 => b,
        _ => return Err("arg0 must be the mover body".into()),
    };
    let out = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 16 => b,
        _ => return Err("arg1 must be the 16-byte out".into()),
    };
    let flags = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x30 => b,
        _ => return Err("arg2 must be the 0x30-byte flags block".into()),
    };
    let timer = f32_at(&body, ow_sim::movement::TIMER);
    let scale_clear = f32_at(&body, ow_sim::movement::SCALE_CLEAR);
    let scale_set = f32_at(&body, ow_sim::movement::SCALE_SET);
    let direction = [
        f32_at(&body, ow_sim::movement::DIRECTION),
        f32_at(&body, ow_sim::movement::DIRECTION + 4),
        f32_at(&body, ow_sim::movement::DIRECTION + 8),
        f32_at(&body, ow_sim::movement::DIRECTION + 12),
    ];
    let flag_bit_set = flags[0] & ow_sim::movement::SCALE_BIT != 0;
    let offset = f32_at(&flags, ow_sim::movement::RAMP_OFFSET);
    let lanes = ow_sim::movement::ramp_vector(
        timer,
        flag_bit_set,
        scale_clear,
        scale_set,
        offset,
        direction,
    );
    let mut out_after = out;
    for (lane, value) in lanes.iter().enumerate() {
        out_after[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    Ok(("-".to_string(), vec![body, out_after, flags]))
}

/// `BuildMoverPhysicsCommand` (`0x140cb74a0`): `void (body*, out[8])`.
///
/// The client calls the movement gate (`0x140ca7d80`) first. The unit pins
/// `body + 0x698` to zero, so the gate's record loop cannot run and the gate
/// is exactly `body[0x1a78] != 0 && body[0x658] != 0`; the probe refuses a
/// positive record count instead of guessing the indirect path.
fn build_mover_command_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 2 {
        return Err(format!("expected 2 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let body = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x1a80 => b,
        _ => return Err("arg0 must be the mover body".into()),
    };
    let out = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 8 => b,
        _ => return Err("arg1 must be the 8-byte out".into()),
    };
    let record_count = i32::from_le_bytes(body[0x698..0x69c].try_into().unwrap());
    if record_count > 0 {
        return Err("gate-record-loop-not-modelled".into());
    }
    let gate = body[0x1a78] != 0 && body[0x658] != 0;
    let state = ow_sim::movement::MoverCommandState {
        gate,
        state_1a79: body[0x1a79],
        flag_652: body[0x652] != 0,
        byte_1a7a: body[0x1a7a],
        byte_1a7b: body[0x1a7b] != 0,
        byte_1a75: body[0x1a75] != 0,
        state_1a60: i32::from_le_bytes(body[0x1a60..0x1a64].try_into().unwrap()),
        byte_1a7f: body[0x1a7f] != 0,
        action_23c: u16::from_le_bytes(body[0x23c..0x23e].try_into().unwrap()),
    };
    let command = ow_sim::movement::build_physics_command(&state);
    let mut out_after = out;
    out_after[..6].copy_from_slice(&command);
    Ok(("-".to_string(), vec![body, out_after]))
}

fn scalar_body(args: Vec<Arg>) -> Result<Vec<u8>, String> {
    let mut it = args.into_iter();
    match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x1100 => Ok(b),
        _ => Err("arg0 must be the mover body".into()),
    }
}

/// `MoverRampScalar` (`0x140ca75f0`): `f32 (body*)`.
fn mover_ramp_scalar_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    let body = scalar_body(args)?;
    let value = ow_sim::movement::ramp_scale(
        f32_at(&body, ow_sim::movement::TIMER),
        body[ow_sim::movement::FLAGS_BLOCK] & ow_sim::movement::SCALE_BIT != 0,
        f32_at(&body, ow_sim::movement::SCALE_CLEAR),
        f32_at(&body, ow_sim::movement::SCALE_SET),
    );
    Ok((format!("{:x}", value.to_bits()), vec![body]))
}

/// `MoverRampScalarOffset` (`0x140ca7570`): `f32 (body*)`, with the scalar at
/// `body + 0x10fc` added after the scale multiply.
fn mover_ramp_scalar_offset_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    let body = scalar_body(args)?;
    let value = ow_sim::movement::ramp_offset(
        f32_at(&body, ow_sim::movement::TIMER),
        body[ow_sim::movement::FLAGS_BLOCK] & ow_sim::movement::SCALE_BIT != 0,
        f32_at(&body, ow_sim::movement::SCALE_CLEAR),
        f32_at(&body, ow_sim::movement::SCALE_SET),
        f32_at(&body, 0x10fc),
    );
    Ok((format!("{:x}", value.to_bits()), vec![body]))
}

/// `MoverWishSpring` (`0x140cc0dc0`): `void (body*, input*, float dt)`.
///
/// The fixture pins the matching state branch and the state-id sentinel, so
/// `FUN_140ca5d00` is the identity; the probe supplies a zero world velocity
/// for the non-matching branch, which the unit does not cover.
fn mover_wish_spring_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 3 {
        return Err(format!("expected 3 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let body = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x1a80 => b,
        _ => return Err("arg0 must be the mover body".into()),
    };
    let input = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0xa0 => b,
        _ => return Err("arg1 must be the 0xa0-byte input object".into()),
    };
    let dt = match it.next() {
        Some(Arg::F32(bits)) => f32::from_bits(bits),
        _ => return Err("arg2 must be f32".into()),
    };
    let mut body_after = body;
    ow_sim::movement::wish_spring(&mut body_after, &input, dt, [0.0; 4]);
    Ok(("-".to_string(), vec![body_after, input]))
}

/// `MoverWishForward` (`0x140cb5510`): `void (vec4*, matrix16*)`.
///
/// The oracle starts inside the leaf at `0x140cb5534` with the vector in RBX
/// and the matrix on the stack; the unit's `startRegs` seeds both. The probe
/// receives the same matrix as a normal pointee and calls the shipped
/// `forward_transform`.
fn mover_wish_forward_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 2 {
        return Err(format!("expected 2 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let v = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 16 => b,
        _ => return Err("arg0 must be the 16-byte vector".into()),
    };
    let matrix = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 64 => b,
        _ => return Err("arg1 must be the 16-float matrix".into()),
    };
    let mut vector = [0.0f32; 4];
    for (lane, value) in vector.iter_mut().enumerate() {
        *value = f32::from_le_bytes(v[lane * 4..lane * 4 + 4].try_into().unwrap());
    }
    let mut m = [0.0f32; 16];
    for (i, value) in m.iter_mut().enumerate() {
        *value = f32::from_le_bytes(matrix[i * 4..i * 4 + 4].try_into().unwrap());
    }
    ow_sim::movement::forward_transform(&mut vector, &m);
    let mut v_after = v;
    for (lane, value) in vector.iter().enumerate() {
        v_after[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    Ok(("-".to_string(), vec![v_after, matrix]))
}

/// `MoverWishInverse` (`0x140cb5350`): `void (vec4*, matrix16*)`.
///
/// The oracle starts inside the block at `0x140cb537b` with the vector in RBX
/// and the matrix at `[RSP+0x30]`; the unit's `startRegs` and state block seed
/// both. The probe receives the same matrix as a normal pointee.
fn mover_wish_inverse_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 2 {
        return Err(format!("expected 2 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let v = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 16 => b,
        _ => return Err("arg0 must be the 16-byte vector".into()),
    };
    let matrix = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 64 => b,
        _ => return Err("arg1 must be the 16-float matrix".into()),
    };
    let mut vector = [0.0f32; 4];
    for (lane, value) in vector.iter_mut().enumerate() {
        *value = f32::from_le_bytes(v[lane * 4..lane * 4 + 4].try_into().unwrap());
    }
    let mut m = [0.0f32; 16];
    for (i, value) in m.iter_mut().enumerate() {
        *value = f32::from_le_bytes(matrix[i * 4..i * 4 + 4].try_into().unwrap());
    }
    ow_sim::movement::inverse_transform(&mut vector, &m);
    let mut v_after = v;
    for (lane, value) in vector.iter().enumerate() {
        v_after[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    Ok(("-".to_string(), vec![v_after, matrix]))
}

/// `FUN_140ca7700`: `void (body*, vec4* out, vec4* input)`.
///
/// The oracle resolves the matrix through the synthetic state record; the
/// probe receives the same matrix as its fourth argument and passes it to the
/// shipped `wish_forward_copy`. The two body variants exercise the transform
/// path (flag/handle set, state id 1) and the sentinel no-op path (flag 0,
/// state id `0xffffffff`).
fn mover_wish_copy_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 4 {
        return Err(format!("expected 4 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let body = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x1a80 => b,
        _ => return Err("arg0 must be the mover body".into()),
    };
    let out = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 16 => b,
        _ => return Err("arg1 must be the 16-byte output".into()),
    };
    let input = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 16 => b,
        _ => return Err("arg2 must be the 16-byte input".into()),
    };
    let matrix = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 64 => b,
        _ => return Err("arg3 must be the 16-float matrix".into()),
    };
    let id = u32::from_le_bytes(body[0x1520..0x1524].try_into().unwrap());
    let sentinel = id == 0xffff_ffff || (id & 0x1fff_ffff) == 0x1fff_ffff;
    let mut out_v = [0.0f32; 4];
    for (lane, value) in out_v.iter_mut().enumerate() {
        *value = f32::from_le_bytes(out[lane * 4..lane * 4 + 4].try_into().unwrap());
    }
    let mut in_v = [0.0f32; 4];
    for (lane, value) in in_v.iter_mut().enumerate() {
        *value = f32::from_le_bytes(input[lane * 4..lane * 4 + 4].try_into().unwrap());
    }
    let mut m = [0.0f32; 16];
    for (i, value) in m.iter_mut().enumerate() {
        *value = f32::from_le_bytes(matrix[i * 4..i * 4 + 4].try_into().unwrap());
    }
    let resolved = if sentinel { None } else { Some(&m) };
    ow_sim::movement::wish_forward_copy(&mut out_v, &in_v, resolved);
    let mut out_after = out;
    for (lane, value) in out_v.iter().enumerate() {
        out_after[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    Ok(("-".to_string(), vec![body, out_after, input, matrix]))
}

/// `FUN_140cb83b0` / `FUN_140c71e10`: the state-record transform lookup.
///
/// The first five arguments match the client call (`out`, state-id pointer,
/// state id, service field, handle); the rest give the candidate the state
/// block and the runtime decode constants. The oracle ignores the extras.
fn mover_state_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 11 {
        return Err(format!("expected 11 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let out = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 64 => b,
        _ => return Err("arg0 must be the 64-byte output matrix".into()),
    };
    let id_ptr = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 4 => b,
        _ => return Err("arg1 must be the 4-byte state id".into()),
    };
    let id = match it.next() {
        Some(Arg::U64(v)) => v as u32,
        _ => return Err("arg2 must be the state id".into()),
    };
    let _service = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg3 must be the service field".into()),
    };
    let handle = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg4 must be the state handle".into()),
    };
    let state = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x80 => b,
        _ => return Err("arg5 must be the state block".into()),
    };
    let base = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg6 must be the state base".into()),
    };
    let record = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg7 must be the record address".into()),
    };
    let q = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg8 must be Q".into()),
    };
    let decode_byte = match it.next() {
        Some(Arg::U64(v)) => v as u8,
        _ => return Err("arg9 must be the decode byte".into()),
    };
    let sentinel = match it.next() {
        Some(Arg::U64(v)) => v as u32,
        _ => return Err("arg10 must be the sentinel id".into()),
    };

    let mem = ow_sim::state::MemoryView::new(base, &state);
    let mut matrix = [0.0f32; 16];
    let found = ow_sim::state::state_transform(
        &mut matrix,
        record,
        id,
        handle,
        &mem,
        ow_sim::state::StateLookup {
            q,
            decode_byte,
            sentinel_id: sentinel,
        },
    );
    let mut out_after = out;
    if found {
        for (lane, value) in matrix.iter().enumerate() {
            out_after[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(("-".to_string(), vec![out_after, id_ptr, state]))
}

/// `FUN_140ca5c50`: `void (body*, vec4*)`, the state transform in place.
///
/// The client resolves the state handle and service field itself; the probe
/// receives the same state block and composes the already-verified
/// `state_transform` with `forward_transform`. Extra args give the candidate
/// the block and the runtime decode constants; the oracle ignores them.
fn mover_state_vector_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 8 {
        return Err(format!("expected 8 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let body = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x1a80 => b,
        _ => return Err("arg0 must be the mover body".into()),
    };
    let out = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 16 => b,
        _ => return Err("arg1 must be the 16-byte vector".into()),
    };
    let state = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x80 => b,
        _ => return Err("arg2 must be the state block".into()),
    };
    let base = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg3 must be the state base".into()),
    };
    let record = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg4 must be the record address".into()),
    };
    let q = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg5 must be Q".into()),
    };
    let decode_byte = match it.next() {
        Some(Arg::U64(v)) => v as u8,
        _ => return Err("arg6 must be the decode byte".into()),
    };
    let sentinel = match it.next() {
        Some(Arg::U64(v)) => v as u32,
        _ => return Err("arg7 must be the sentinel id".into()),
    };

    let id = u32::from_le_bytes(body[0x1520..0x1524].try_into().unwrap());
    let handle = u64::from_le_bytes(body[0x19b0..0x19b8].try_into().unwrap());
    let mem = ow_sim::state::MemoryView::new(base, &state);
    let mut matrix = [0.0f32; 16];
    let found = ow_sim::state::state_transform(
        &mut matrix,
        record,
        id,
        handle,
        &mem,
        ow_sim::state::StateLookup {
            q,
            decode_byte,
            sentinel_id: sentinel,
        },
    );
    let mut out_after = out;
    if found {
        let mut v = [0.0f32; 4];
        for (lane, value) in v.iter_mut().enumerate() {
            *value = f32::from_le_bytes(out_after[lane * 4..lane * 4 + 4].try_into().unwrap());
        }
        ow_sim::movement::forward_transform(&mut v, &matrix);
        for (lane, value) in v.iter().enumerate() {
            out_after[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(("-".to_string(), vec![body, out_after, state]))
}

/// `FUN_1405a87d0` (`0x1405a87d0`): default-construct the movement-settings
/// object. The client writes self-referential pointers from the object's own
/// address, so the probe receives that address (`base`) explicitly.
fn mover_default_settings_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 3 {
        return Err(format!("expected 3 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let out = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x600 => b,
        _ => return Err("arg0 must be the settings object".into()),
    };
    let base = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg1 must be the object address".into()),
    };
    let global = match it.next() {
        Some(Arg::U64(v)) => v as u32,
        _ => return Err("arg2 must be the sentinel global".into()),
    };
    let mut after = out;
    ow_sim::settings::default_settings(&mut after, base, global);
    Ok(("-".to_string(), vec![after]))
}

/// `FUN_140c862e0` (`0x140c862e0`): apply a movement asset to the settings
/// object. The asset's three pointer fields are passed as separate pointees;
/// the probe never dereferences an address.
fn mover_apply_settings_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 12 {
        return Err(format!("expected 12 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let out = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x600 => b,
        _ => return Err("arg0 must be the settings object".into()),
    };
    let asset = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x140 => b,
        _ => return Err("arg1 must be the asset".into()),
    };
    // The client's third argument. The remaining arguments are candidate-only.
    let record = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg2 must be the state record".into()),
    };
    let base = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg3 must be the object address".into()),
    };
    let scales = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 8 => b,
        _ => return Err("arg4 must be the scale pair".into()),
    };
    let gates = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 3 => b,
        _ => return Err("arg5 must be the gate bytes".into()),
    };
    let ids = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
        _ => return Err("arg6 must be the id array".into()),
    };
    let state_block = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
        _ => return Err("arg7 must be the state block".into()),
    };
    let state_base = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg8 must be the state base".into()),
    };
    let q = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg9 must be Q".into()),
    };
    let decode_byte = match it.next() {
        Some(Arg::U64(v)) => v as u8,
        _ => return Err("arg10 must be the decode byte".into()),
    };
    let sentinel = match it.next() {
        Some(Arg::U64(v)) => v as u32,
        _ => return Err("arg11 must be the sentinel id".into()),
    };

    let pointees = ow_sim::settings::AssetPointees {
        scales: &scales,
        gates: &gates,
        ids: &ids,
    };
    let state = if state_block.is_empty() {
        None
    } else {
        Some((record, state_base, state_block.as_slice()))
    };
    let mut after = out;
    let ret = ow_sim::settings::apply_settings(
        &mut after,
        base,
        &asset,
        &pointees,
        state,
        ow_sim::state::StateLookup {
            q,
            decode_byte,
            sentinel_id: sentinel,
        },
    )
    .ok_or("settings-vector-growth-open")?;
    // One pointee per pointer argument, in argument order: out, asset,
    // scales, gates, ids, state. The apply only writes `out`.
    Ok((
        format!("{ret:x}"),
        vec![after, asset, scales, gates, ids, state_block],
    ))
}

/// `FUN_140cbcbe0` (`0x140cbcbe0`): the 37-kind state-modifier evaluator.
fn mover_apply_modifiers_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 8 {
        return Err(format!("expected 8 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let body = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x1c00 => b,
        _ => return Err("arg0 must be the mover body".into()),
    };
    let base = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg1 must be the body address".into()),
    };
    let records = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() % 0x38 == 0 => b,
        _ => return Err("arg2 must be the modifier records".into()),
    };
    let gate = match it.next() {
        Some(Arg::U64(v)) => v != 0,
        _ => return Err("arg3 must be the gate".into()),
    };
    let feature_bit = match it.next() {
        Some(Arg::U64(v)) => v != 0,
        _ => return Err("arg4 must be the feature bit".into()),
    };
    let g1 = match it.next() {
        Some(Arg::F64(v)) => f64::from_bits(v),
        _ => return Err("arg5 must be the first clamp bound".into()),
    };
    let g2 = match it.next() {
        Some(Arg::F64(v)) => f64::from_bits(v),
        _ => return Err("arg6 must be the second clamp bound".into()),
    };
    let vtable = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
        _ => return Err("arg7 must be the fake vtable".into()),
    };
    let mut after = body;
    ow_sim::modifiers::apply_state_modifiers(&mut after, base, &records, gate, feature_bit, g1, g2);
    Ok(("-".to_string(), vec![after, records, vtable]))
}

/// `FUN_140ca7d80` (`0x140ca7d80`): the mover's movement gate.
fn mover_gate_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 3 {
        return Err(format!("expected 3 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let body = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x1c00 => b,
        _ => return Err("arg0 must be the mover body".into()),
    };
    let base = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg1 must be the body address".into()),
    };
    let results = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
        _ => return Err("arg2 must be the per-record results".into()),
    };
    let gate = ow_sim::gate::movement_gate(&body, &results, base);
    Ok((format!("{:x}", u64::from(gate)), vec![body, results]))
}

/// `FUN_142cd4ff0` (`0x142cd4ff0`): resolve a character handle and return
/// its position as four f32 lanes in XMM0.
fn mover_character_position_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 3 {
        return Err(format!("expected 3 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let handle = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg0 must be the character handle".into()),
    };
    let state = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && !b.is_empty() => b,
        _ => return Err("arg1 must be the state block".into()),
    };
    let base = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg2 must be the state base".into()),
    };
    let mem = ow_sim::state::MemoryView::new(base, &state);
    let out = ow_sim::world::character_position(handle, &mem, base);
    let mut bits = 0u128;
    for (lane, value) in out.iter().enumerate() {
        bits |= u128::from(value.to_bits()) << (lane * 32);
    }
    Ok((format!("{:032x}", bits), vec![state]))
}

fn position_bits(out: [f32; 4]) -> String {
    let mut bits = 0u128;
    for (lane, value) in out.iter().enumerate() {
        bits |= u128::from(value.to_bits()) << (lane * 32);
    }
    format!("{:032x}", bits)
}

fn matrix_at(bytes: &[u8]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for (lane, value) in out.iter_mut().enumerate() {
        *value = f32_at(bytes, lane * 4);
    }
    out
}

fn bounds_bytes(bounds: [f32; 8]) -> Vec<u8> {
    bounds
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn quad_bytes(quad: [f32; 4]) -> Vec<u8> {
    quad.iter().flat_map(|value| value.to_le_bytes()).collect()
}

/// The full 0x20-byte output pointee: the leaf writes only its first quad.
fn out_pointee(original: &[u8], quad: [f32; 4]) -> Vec<u8> {
    let mut bytes = original.to_vec();
    bytes[..0x10].copy_from_slice(&quad_bytes(quad));
    bytes
}

/// `0x142cd5540`: the collision sweep's bounds stage.
///
/// One pointer carries the query block (0x80 bytes) followed by the local
/// support-point array its `+0x30` pointer refers to; the oracle's arena puts
/// both in the same pointee so the baked pointer is the real address.
fn collision_sweep_bounds_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 4 {
        return Err(format!("expected 4 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let block = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x80 => b,
        _ => return Err("arg0 must be the query block plus its points".into()),
    };
    let expansion_out = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 32 => b,
        _ => return Err("arg1 must be the 32-byte expansion output".into()),
    };
    let start_out = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 32 => b,
        _ => return Err("arg2 must be the 32-byte start output".into()),
    };
    let end_out = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 32 => b,
        _ => return Err("arg3 must be the 32-byte end output".into()),
    };
    let (start, end, expansion) = ow_sim::collide::sweep_bounds(&block[..0x80], &block[0x80..]);
    Ok((
        "-".to_string(),
        vec![
            block,
            out_pointee(&expansion_out, expansion),
            out_pointee(&start_out, start),
            out_pointee(&end_out, end),
        ],
    ))
}

fn shape_bounds_entry(
    args: Vec<Arg>,
    record_len: usize,
    kind: &str,
) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 3 {
        return Err(format!("expected 3 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let record = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= record_len => b,
        _ => return Err("arg0 must be the shape record".into()),
    };
    let _out = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 32 => b,
        _ => return Err("arg1 must be the 32-byte output".into()),
    };
    let matrix = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 64 => b,
        _ => return Err("arg2 must be the 4x4 matrix".into()),
    };
    let m = matrix_at(&matrix);
    let bounds = match kind {
        "sphere" => ow_sim::shapes::sphere_bounds(&record, &m),
        "capsule" => ow_sim::shapes::capsule_bounds(&record, &m),
        "box" => ow_sim::shapes::box_bounds(&record, &m),
        _ => return Err("unknown shape kind".into()),
    };
    Ok(("-".to_string(), vec![record, bounds_bytes(bounds), matrix]))
}

fn shape_record_entry(
    args: Vec<Arg>,
    record_len: usize,
    kind: &str,
) -> Result<(String, Vec<Vec<u8>>), String> {
    shape_bounds_entry(args, record_len, kind)
}

/// `0x142cb9030`: the shape-record dispatcher.
fn shape_dispatch_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 4 {
        return Err(format!("expected 4 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let record = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x28 => b,
        _ => return Err("arg0 must be the shape record".into()),
    };
    let world = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x338 => b,
        _ => return Err("arg1 must be the world block".into()),
    };
    let matrix = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 64 => b,
        _ => return Err("arg2 must be the 4x4 matrix".into()),
    };
    let base = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg3 must be the world base".into()),
    };
    let mut record_after = record;
    let applied =
        ow_sim::shapes::shape_bounds(&mut record_after, &world, base, &matrix_at(&matrix));
    if !applied {
        // The fixture never selects the open type 3, but keep the response
        // shape stable if a future case does.
    }
    Ok(("-".to_string(), vec![record_after, world, matrix]))
}

/// `0x142ccd410`: the type-0 sphere bounds.
fn shape_sphere_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    shape_record_entry(args, 0x20, "sphere")
}

/// `0x142ccd380`: the type-1 capsule bounds.
fn shape_capsule_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    shape_record_entry(args, 0x24, "capsule")
}

/// `0x142ccf460`: the type-4 box bounds.
fn shape_box_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    shape_record_entry(args, 0x30, "box")
}

/// `0x142cd3720`: the type-2 point-cloud bounds.
fn shape_points_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 4 {
        return Err(format!("expected 4 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let record = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x34 => b,
        _ => return Err("arg0 must be the shape record".into()),
    };
    let _out = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 32 => b,
        _ => return Err("arg1 must be the 32-byte output".into()),
    };
    let matrix = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 64 => b,
        _ => return Err("arg2 must be the 4x4 matrix".into()),
    };
    let points = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
        _ => return Err("arg3 must be the point array".into()),
    };
    let bounds = ow_sim::shapes::point_cloud_bounds(&record, &points, &matrix_at(&matrix));
    Ok((
        "-".to_string(),
        vec![record, bounds_bytes(bounds), matrix, points],
    ))
}

/// `0x142ccdf40` type 0 (sphere), no-support-point path: the swept-sphere
/// narrowphase. The oracle's record is `{centre, radius}`, the transform is
/// `{position, world->local quaternion}` and the block is the query block.
fn collision_sphere_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 4 {
        return Err(format!("expected 4 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let record = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x20 => b,
        _ => return Err("arg0 must be the type-0 record".into()),
    };
    let out = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x44 => b,
        _ => return Err("arg1 must be the 0x44-byte output".into()),
    };
    let block = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x78 => b,
        _ => return Err("arg2 must be the query block".into()),
    };
    let xform = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x20 => b,
        _ => return Err("arg3 must be the shape transform".into()),
    };
    let mut out_after = out;
    if let Some(quads) = ow_sim::collide::sphere_sweep(&record, &block, &xform) {
        out_after[0x00..0x10].copy_from_slice(&quad_bytes(quads[0]));
        out_after[0x10..0x20].copy_from_slice(&quad_bytes(quads[1]));
        out_after[0x20..0x30].copy_from_slice(&quad_bytes(quads[2]));
        out_after[0x30..0x34].copy_from_slice(&(-1i32).to_le_bytes());
        out_after[0x34..0x38].copy_from_slice(&0i32.to_le_bytes());
        out_after[0x38..0x3a].copy_from_slice(&0xffffu16.to_le_bytes());
        out_after[0x3c..0x44].copy_from_slice(&0u64.to_le_bytes());
    }
    Ok(("-".to_string(), vec![record, out_after, block, xform]))
}

/// `0x142cbb350`: the query's layer filter. The compared channel is the
/// whole return register: the leaf writes only `AL` (the bool), and `EAX`
/// still holds the last 16-bit value it loaded - `a[2] & b[0]` on the two
/// early exits, `b[2]` once the second mask test has run. The probe
/// reproduces that so the byte above the bool is compared too.
fn collision_filter_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 2 {
        return Err(format!("expected 2 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let a = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 8 => b,
        _ => return Err("arg0 must be the query filter words".into()),
    };
    let b = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 8 => b,
        _ => return Err("arg1 must be the shape filter words".into()),
    };
    let passed = ow_sim::collide::shape_filter(&a, &b);
    let first = u16::from_le_bytes([a[2], a[3]]) & u16::from_le_bytes([b[0], b[1]]);
    let second = u16::from_le_bytes([b[2], b[3]]);
    let residue = if a[7] != 0 || first == 0 {
        u32::from(first)
    } else {
        u32::from(second)
    };
    let ret = (residue & 0xffff_ff00) | u32::from(passed);
    Ok((format!("{:x}", ret), vec![a, b]))
}

/// `0x142cd56e0`: the collider's "is this triangle unusable" test. Three vec4
/// corners, no pointee written; the compared channel is the whole return
/// register (a `MOVMSKPS` mask on the non-finite exit, else 1 or 0).
fn triangle_degenerate_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 3 {
        return Err(format!("expected 3 args, got {}", args.len()));
    }
    let mut quads = [[0.0f32; 4]; 3];
    let mut pointees = Vec::new();
    for (index, arg) in args.into_iter().enumerate() {
        let bytes = match arg {
            Arg::Ptr(b, r, g) if r.is_empty() && g.is_none() && b.len() == 16 => b,
            _ => return Err(format!("arg{index} must be a 16-byte vec4")),
        };
        for lane in 0..4 {
            let raw = u32::from_le_bytes([
                bytes[lane * 4],
                bytes[lane * 4 + 1],
                bytes[lane * 4 + 2],
                bytes[lane * 4 + 3],
            ]);
            quads[index][lane] = f32::from_bits(raw);
        }
        pointees.push(bytes);
    }
    let ret = ow_sim::collide::triangle_degenerate(quads[0], quads[1], quads[2]);
    Ok((format!("{:x}", ret), pointees))
}

/// `0x142ccd7b0` type 1 (capsule), the analytic path plus its two sphere-leaf
/// recursions. Same record/output shape as the sphere, with a 0x30-byte record.
fn collision_capsule_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 4 {
        return Err(format!("expected 4 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let record = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x30 => b,
        _ => return Err("arg0 must be the type-1 record".into()),
    };
    let out = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x44 => b,
        _ => return Err("arg1 must be the 0x44-byte output".into()),
    };
    let block = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x78 => b,
        _ => return Err("arg2 must be the query block".into()),
    };
    let xform = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x20 => b,
        _ => return Err("arg3 must be the shape transform".into()),
    };
    let mut out_after = out;
    if let Some(quads) = ow_sim::collide::capsule_sweep(&record, &block, &xform) {
        out_after[0x00..0x10].copy_from_slice(&quad_bytes(quads[0]));
        out_after[0x10..0x20].copy_from_slice(&quad_bytes(quads[1]));
        out_after[0x20..0x30].copy_from_slice(&quad_bytes(quads[2]));
        out_after[0x30..0x34].copy_from_slice(&(-1i32).to_le_bytes());
        out_after[0x34..0x38].copy_from_slice(&0i32.to_le_bytes());
        out_after[0x38..0x3a].copy_from_slice(&0xffffu16.to_le_bytes());
        out_after[0x3c..0x44].copy_from_slice(&0u64.to_le_bytes());
    }
    Ok(("-".to_string(), vec![record, out_after, block, xform]))
}

/// `0x142caed30` type 3 (mesh), the no-support-point walk. The element
/// pointee carries the 0x50-byte pool element followed by its whole world;
/// the element's three pointers are relative to that base, so the mesh the
/// walk reads starts right after the element.
fn mesh_sweep_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 4 {
        return Err(format!("expected 4 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let element = match it.next() {
        Some(Arg::Ptr(b, _, g)) if g.is_none() && b.len() > 0x50 => b,
        _ => return Err("arg0 must be the type-3 element plus its world".into()),
    };
    let out = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x44 => b,
        _ => return Err("arg1 must be the 0x44-byte output".into()),
    };
    let block = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x78 => b,
        _ => return Err("arg2 must be the query block".into()),
    };
    let xform = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x20 => b,
        _ => return Err("arg3 must be the shape transform".into()),
    };
    let world = ow_sim::collide::MeshWorld {
        base: 0x50,
        bytes: &element[0x50..],
    };
    let sweep = ow_sim::collide::mesh_sweep(&element[..0x50], &world, &block, &xform);
    let mut out_after = out;
    if let Some(hit) = sweep.hit {
        out_after[0x00..0x10].copy_from_slice(&quad_bytes(hit.point));
        out_after[0x10..0x20].copy_from_slice(&quad_bytes(hit.normal));
        out_after[0x20..0x30].copy_from_slice(&quad_bytes(hit.fraction));
        out_after[0x30..0x34].copy_from_slice(&hit.triangle.to_le_bytes());
        out_after[0x34..0x36].copy_from_slice(&hit.ids[0].to_le_bytes());
        out_after[0x36..0x38].copy_from_slice(&hit.ids[1].to_le_bytes());
        out_after[0x38..0x3a].copy_from_slice(&0xffffu16.to_le_bytes());
    }
    out_after[0x3c..0x40].copy_from_slice(&sweep.internals.to_le_bytes());
    out_after[0x40..0x44].copy_from_slice(&sweep.leaves.to_le_bytes());
    Ok(("-".to_string(), vec![element, out_after, block, xform]))
}

fn vec4_at(bytes: &[u8], offset: usize) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    for (lane, value) in out.iter_mut().enumerate() {
        *value = f32_at(bytes, offset + lane * 4);
    }
    out
}

/// `MoveCharacterController` (`0x142cd4bc0`), no-shape path.
fn mover_move_character_entry(
    args: Vec<Arg>,
    _state: &mut State,
) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 3 {
        return Err(format!("expected 3 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let entry = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x28 => b,
        _ => return Err("arg0 must be the entry".into()),
    };
    let record = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x194 => b,
        _ => return Err("arg1 must be the record".into()),
    };
    let block = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 32 => b,
        _ => return Err("arg2 must be the 32-byte move block".into()),
    };
    let mut entry_after = entry;
    let mut record_after = record;
    ow_sim::world::move_character_controller(
        &mut entry_after,
        &mut record_after,
        vec4_at(&block, 0),
        vec4_at(&block, 16),
    );
    Ok(("-".to_string(), vec![entry_after, record_after, block]))
}

/// `MoveCharacterController` (`0x142cd4bc0`) with the shape-query loop.
fn mover_move_character_world_entry(
    args: Vec<Arg>,
    _state: &mut State,
) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 5 {
        return Err(format!("expected 5 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let entry_addr = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg0 must be the entry address".into()),
    };
    let record_addr = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg1 must be the record address".into()),
    };
    let block = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 32 => b,
        _ => return Err("arg2 must be the 32-byte move block".into()),
    };
    let mut world = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x2000 => b,
        _ => return Err("arg3 must be the world block".into()),
    };
    let base = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg4 must be the world base".into()),
    };
    let entry_off = entry_addr.checked_sub(base).ok_or("entry outside world")? as usize;
    let record_off = record_addr
        .checked_sub(base)
        .ok_or("record outside world")? as usize;
    if entry_off + 0x28 > world.len() || record_off + 0x338 > world.len() {
        return Err("entry/record outside world".into());
    }
    let (left, right) = world.split_at_mut(record_off);
    let entry = &mut left[entry_off..entry_off + 0x28];
    let applied = ow_sim::world::move_character_controller_world(
        entry,
        right,
        record_addr,
        vec4_at(&block, 0),
        vec4_at(&block, 16),
    );
    if !applied {
        // The fixture's contact id is -1; keep the response shape stable if
        // a future case enters the open event.
    }
    Ok(("-".to_string(), vec![block, world]))
}

/// `ApplyCharacterControllerMove` (`0x142cd5370`), no-shape path.
fn mover_apply_character_move_entry(
    args: Vec<Arg>,
    state: &mut State,
) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 3 {
        return Err(format!("expected 3 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let handle = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg0 must be the character handle".into()),
    };
    let velocity = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 16 => b,
        _ => return Err("arg1 must be the velocity".into()),
    };
    let base = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg2 must be the state base".into()),
    };
    if base != state.base {
        return Err("state base mismatch".into());
    }
    let mut after = state.snapshot.clone();
    ow_sim::world::apply_character_controller_move(
        handle,
        vec4_at(&velocity, 0),
        &mut after,
        base,
        base,
    );
    state.buf = after;
    Ok(("-".to_string(), vec![velocity]))
}

/// `SetCharacterControllerVelocity` (`0x142cbd1b0`), character path.
fn mover_set_velocity_entry(
    args: Vec<Arg>,
    state: &mut State,
) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 3 {
        return Err(format!("expected 3 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let slot = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg0 must be the world slot".into()),
    };
    let velocity = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 16 => b,
        _ => return Err("arg1 must be the velocity".into()),
    };
    let base = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg2 must be the state base".into()),
    };
    if base != state.base {
        return Err("state base mismatch".into());
    }
    let mut after = state.snapshot.clone();
    ow_sim::world::set_character_controller_velocity(
        slot,
        vec4_at(&velocity, 0),
        &mut after,
        base,
        base,
    );
    state.buf = after;
    Ok(("-".to_string(), vec![velocity]))
}

/// `FUN_142cb4770` (`0x142cb4770`): the rigid-body position leaf.
fn mover_rigid_position_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 3 {
        return Err(format!("expected 3 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let handle = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg0 must be the rigid handle".into()),
    };
    let state = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && !b.is_empty() => b,
        _ => return Err("arg1 must be the state block".into()),
    };
    let base = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg2 must be the state base".into()),
    };
    let mem = ow_sim::state::MemoryView::new(base, &state);
    Ok((
        position_bits(ow_sim::world::rigid_position(handle, &mem, base)),
        vec![state],
    ))
}

/// `FUN_142cf5630` (`0x142cf5630`): the variant-tag position dispatch.
fn mover_variant_position_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 3 {
        return Err(format!("expected 3 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let variant = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg0 must be the variant object".into()),
    };
    let state = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && !b.is_empty() => b,
        _ => return Err("arg1 must be the state block".into()),
    };
    let base = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg2 must be the state base".into()),
    };
    let mem = ow_sim::state::MemoryView::new(base, &state);
    Ok((
        position_bits(ow_sim::world::variant_position(variant, &mem, base)),
        vec![state],
    ))
}

/// `FUN_142cbcec0` (`0x142cbcec0`): the world-slot position dispatch.
fn mover_world_position_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 3 {
        return Err(format!("expected 3 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let slot = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg0 must be the world slot".into()),
    };
    let state = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && !b.is_empty() => b,
        _ => return Err("arg1 must be the state block".into()),
    };
    let base = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg2 must be the state base".into()),
    };
    let mem = ow_sim::state::MemoryView::new(base, &state);
    let out = ow_sim::world::world_position(slot, &mem, base);
    Ok((position_bits(out), vec![state]))
}

fn handle_call(
    entry: &str,
    args: Vec<Arg>,
    state: &mut State,
) -> Result<(String, Vec<Vec<u8>>), String> {
    match entry {
        // crates/ow-sim/src/movement.rs
        "mover_ramp_vector" => mover_ramp_vector_entry(args),
        "mover_wish_forward" => mover_wish_forward_entry(args),
        "mover_wish_inverse" => mover_wish_inverse_entry(args),
        "mover_wish_copy" => mover_wish_copy_entry(args),
        "mover_default_settings" => mover_default_settings_entry(args),
        "mover_apply_settings" => mover_apply_settings_entry(args),
        "mover_apply_modifiers" => mover_apply_modifiers_entry(args),
        "mover_gate" => mover_gate_entry(args),
        "mover_character_position" => mover_character_position_entry(args),
        "mover_world_position" => mover_world_position_entry(args),
        "mover_rigid_position" => mover_rigid_position_entry(args),
        "mover_move_character" => mover_move_character_entry(args, state),
        "mover_move_character_world" => mover_move_character_world_entry(args, state),
        "mover_apply_character_move" => mover_apply_character_move_entry(args, state),
        "mover_set_velocity" => mover_set_velocity_entry(args, state),
        "shape_sphere" => shape_sphere_entry(args),
        "shape_capsule" => shape_capsule_entry(args),
        "shape_box" => shape_box_entry(args),
        "shape_points" => shape_points_entry(args),
        "shape_dispatch" => shape_dispatch_entry(args),
        "collision_sweep_bounds" => collision_sweep_bounds_entry(args),
        "collision_sphere" => collision_sphere_entry(args),
        "collision_filter" => collision_filter_entry(args),
        "triangle_degenerate" => triangle_degenerate_entry(args),
        "collision_capsule" => collision_capsule_entry(args),

        "build_bvh" => build_bvh_entry(args),
        "mesh_sweep" => mesh_sweep_entry(args),
        "mover_variant_position" => mover_variant_position_entry(args),
        "mover_state_transform" => mover_state_entry(args),
        "mover_state_vector" => mover_state_vector_entry(args),
        "mover_wish_spring" => mover_wish_spring_entry(args),
        "mover_ramp_scalar" => mover_ramp_scalar_entry(args),
        "mover_ramp_scalar_offset" => mover_ramp_scalar_offset_entry(args),
        "build_mover_command" => build_mover_command_entry(args),
        other => Err(format!("unknown-entry-{}", other)),
    }
}

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut state = State::new();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks[0] == "ping" {
            let _ = writeln!(out, "pong");
            let _ = out.flush();
            continue;
        }
        if toks[0] == "state" {
            if toks.len() != 3 {
                let _ = writeln!(out, "err bad-state");
                let _ = out.flush();
                continue;
            }
            let base = match u64::from_str_radix(toks[1], 16) {
                Ok(b) => b,
                Err(_) => {
                    let _ = writeln!(out, "err bad-state-base");
                    let _ = out.flush();
                    continue;
                }
            };
            let bytes = match hex_decode(toks[2]) {
                Some(b) if !b.is_empty() => b,
                _ => {
                    let _ = writeln!(out, "err bad-state-bytes");
                    let _ = out.flush();
                    continue;
                }
            };
            state.base = base;
            state.buf = bytes.clone();
            state.snapshot = bytes;
            let _ = writeln!(out, "ok");
            let _ = out.flush();
            continue;
        }
        if toks[0] != "call" || toks.len() < 3 {
            let _ = writeln!(out, "err bad-request");
            let _ = out.flush();
            continue;
        }
        let entry = toks[1];
        let nargs: usize = toks[2].parse().unwrap_or(0);
        let mut args = Vec::with_capacity(nargs);
        let mut i = 3;
        let mut bad = None;
        for _ in 0..nargs {
            if i >= toks.len() {
                bad = Some("truncated-args");
                break;
            }
            let kind = toks[i];
            i += 1;
            match kind {
                "ptr" => {
                    if i + 2 >= toks.len() {
                        bad = Some("truncated-ptr");
                        break;
                    }
                    let bytes = match hex_decode(toks[i]) {
                        Some(b) => b,
                        None => {
                            bad = Some("bad-ptr-hex");
                            break;
                        }
                    };
                    let relocs = if toks[i + 1] == "-" {
                        Vec::new()
                    } else {
                        toks[i + 1]
                            .split(',')
                            .filter_map(|o| o.parse::<usize>().ok())
                            .collect()
                    };
                    let guest = if toks[i + 2] == "-" {
                        None
                    } else {
                        match u64::from_str_radix(toks[i + 2], 16) {
                            Ok(g) => Some(g),
                            Err(_) => {
                                bad = Some("bad-guest-hex");
                                break;
                            }
                        }
                    };
                    args.push(Arg::Ptr(bytes, relocs, guest));
                    i += 3;
                }
                "f32" => {
                    if i >= toks.len() {
                        bad = Some("truncated-f32");
                        break;
                    }
                    match u32::from_str_radix(toks[i], 16) {
                        Ok(b) => args.push(Arg::F32(b)),
                        Err(_) => {
                            bad = Some("bad-f32-hex");
                            break;
                        }
                    }
                    i += 1;
                }
                "f64" => {
                    if i >= toks.len() {
                        bad = Some("truncated-f64");
                        break;
                    }
                    match u64::from_str_radix(toks[i], 16) {
                        Ok(b) => args.push(Arg::F64(b)),
                        Err(_) => {
                            bad = Some("bad-f64-hex");
                            break;
                        }
                    }
                    i += 1;
                }
                "u64" => {
                    if i >= toks.len() {
                        bad = Some("truncated-u64");
                        break;
                    }
                    match u64::from_str_radix(toks[i], 16) {
                        Ok(b) => args.push(Arg::U64(b)),
                        Err(_) => {
                            bad = Some("bad-u64-hex");
                            break;
                        }
                    }
                    i += 1;
                }
                _ => {
                    bad = Some("bad-kind");
                    break;
                }
            }
        }
        if let Some(e) = bad {
            let _ = writeln!(out, "err {}", e);
            let _ = out.flush();
            continue;
        }
        match handle_call(entry, args, &mut state) {
            Ok((ret, outs)) => {
                let mut resp = format!("ok {}", ret);
                for o in outs {
                    resp.push(' ');
                    resp.push_str(&hex_encode(&o));
                }
                if state.loaded() {
                    resp.push(' ');
                    resp.push_str(&hex_encode(&state.buf));
                }
                let _ = writeln!(out, "{}", resp);
            }
            Err(e) => {
                let msg: String = e.chars().filter(|c| !c.is_whitespace()).collect();
                let _ = writeln!(out, "err {}", msg);
            }
        }
        let _ = out.flush();
    }
}

/// `0x142ccb1b0` the BVH builder: `(state, faces, count, bins, prefix)`.
///
/// The candidate only needs the face array and the count; the two scratch
/// addresses and the state's node pointer are the caller's business. The
/// compared channels are the state (whose `+0x38` used count the builder
/// bumps) and the node array the state points at.
fn build_bvh_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 6 {
        return Err(format!("expected 6 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let mut state = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 0x40 => b,
        _ => return Err("arg0 must be the 0x40-byte state".into()),
    };
    let faces_blob = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
        _ => return Err("arg1 must be the face array".into()),
    };
    let count = match it.next() {
        Some(Arg::U64(value)) => value as usize,
        _ => return Err("arg2 must be the face count".into()),
    };
    if faces_blob.len() < count * 0x60 {
        return Err("the face array is shorter than count".into());
    }
    // arg3/arg4 are the caller's bin and prefix scratch (raw addresses); arg5
    // is the node array the builder fills.
    match it.nth(2) {
        Some(Arg::Ptr(_, r, g)) if r.is_empty() && g.is_none() => {}
        _ => return Err("arg5 must be the node array".into()),
    }
    let mut faces = Vec::with_capacity(count);
    for slot in 0..count {
        let at = slot * 0x60;
        let face = &faces_blob[at..at + 0x60];
        let quad = |offset: usize| -> [f32; 4] {
            let mut out = [0.0f32; 4];
            for lane in 0..4 {
                out[lane] = f32::from_bits(u32::from_le_bytes([
                    face[offset + lane * 4],
                    face[offset + lane * 4 + 1],
                    face[offset + lane * 4 + 2],
                    face[offset + lane * 4 + 3],
                ]));
            }
            out
        };
        let index = |offset: usize| -> i32 {
            i32::from_le_bytes([
                face[offset],
                face[offset + 1],
                face[offset + 2],
                face[offset + 3],
            ])
        };
        faces.push(ow_sim::collider::Face {
            min: quad(0x00),
            max: quad(0x10),
            centroid: quad(0x20),
            marker: index(0x30),
            area: f32::from_bits(index(0x34) as u32),
            index: [index(0x40), index(0x44), index(0x48)],
            mode: u16::from_le_bytes([face[0x50], face[0x51]]),
            a: u16::from_le_bytes([face[0x52], face[0x53]]),
            b: u16::from_le_bytes([face[0x54], face[0x55]]),
            source: slot,
            bucket: index(0x30),
            id_bits: u16::from_le_bytes([face[0x54], face[0x55]]),
        });
    }
    let (built, ordering) = ow_sim::collider::build(&faces).map_err(|err| format!("{err:?}"))?;
    if built.len() > 2 * count - 1 {
        return Err("the builder emitted more nodes than a binary tree allows".into());
    }
    state[0x38..0x3c].copy_from_slice(&(built.len() as i32).to_le_bytes());
    // The builder writes each face's bin into the record at `+0x30`, and the
    // splits permute the array, so the returned face blob is the final one.
    let mut faces_blob = faces_blob;
    if ordering.len() == count {
        let original = faces_blob.clone();
        for (slot, face) in ordering.iter().enumerate() {
            let from = face.source * 0x60;
            faces_blob[slot * 0x60..slot * 0x60 + 0x60]
                .copy_from_slice(&original[from..from + 0x60]);
            let at = slot * 0x60 + 0x30;
            faces_blob[at..at + 4].copy_from_slice(&face.bucket.to_le_bytes());
        }
    }
    let mut out = vec![0xCDu8; (2 * count - 1) * 0x50];
    // The fixture pins the face array at `ARENA_BASE + 0x200`; the client keeps
    // an absolute pointer at each leaf node's `+0x28`.
    const FACE_ARENA_ADDRESS: u64 = 0x2_0000_0200;
    ow_sim::collider::write_build_nodes(&built, FACE_ARENA_ADDRESS, &mut out);
    Ok(("0".to_string(), vec![state, faces_blob, out]))
}
