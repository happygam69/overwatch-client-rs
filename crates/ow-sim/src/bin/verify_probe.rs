//! Tier-2 verification probe: **real crate code**, spoken over the harness's
//! line protocol.
//!
//! Built by `cargo build --release --bin verify_probe`, not by `rustc` against
//! a copy. Every entry below calls the function the crate actually links; the
//! harness binds its verdict to that function's source file and the crate
//! revision (see `dev/verify_harness/candidate_rust.py`).
//!
//! Protocol (one request per line, one response per line):
//!
//!   -> ping
//!   <- pong
//!
//!   -> state <base-hex> <bytes-hex>
//!   <- ok
//!
//!   -> call <entry> <nargs> [ptr <hex> <relocs> <guest|-> | f32 <8hex> |
//!                            f64 <16hex> | u64 <16hex>]*
//!   <- ok <retbits-hex|-> <outhex>... [<statehex>]
//!   <- err <message-with-no-spaces>
//!
//! `relocs` is `-` or comma-separated decimal byte offsets of u64 relative
//! pointers inside the pointee. A `guest` address other than `-` means the
//! pointee is a slice of the loaded state buffer; the probe translates it, so
//! a stateful function sees the same relative layout the oracle does. When a
//! state buffer is loaded the whole buffer is returned after the pointee
//! outputs, which is the state-differential channel.
//!
//! The probe deliberately keeps a small, explicit set of entries. Adding one
//! is the same act as claiming the crate function is the thing the harness is
//! verifying, so each entry names the crate path in a comment.

use std::io::{self, BufRead, Write};

use ow_sim::camera::orthonormalize_camera_basis;
use ow_sim::movement::{move_toward, Controller, Input, MoverSpec};

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

fn hex_encode(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{:02x}", x));
    }
    s
}

fn f32_at(b: &[u8], off: usize) -> f32 {
    let mut x = [0u8; 4];
    x.copy_from_slice(&b[off..off + 4]);
    f32::from_le_bytes(x)
}

fn put_f32(b: &mut [u8], off: usize, v: f32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    let mut x = [0u8; 4];
    x.copy_from_slice(&b[off..off + 4]);
    u32::from_le_bytes(x)
}

/// Wire layout for a `Controller` snapshot, used by the stateful
/// `controller_advance` entry. This is a harness contract, not a client
/// layout: the client's mover struct is a separate, much larger object and a
/// client unit must translate at its fixture boundary.
const CTL_LEN: usize = 72;
const CTL_POS: usize = 0;
const CTL_YAW: usize = 12;
const CTL_VERTICAL: usize = 16;
const CTL_GROUNDED: usize = 20;
const CTL_NORMAL: usize = 24;
const CTL_SLOPE: usize = 36;
const CTL_VEL: usize = 40;
const CTL_DT: usize = 48;
const CTL_FORWARD: usize = 52;
const CTL_SIDE: usize = 56;
const CTL_CROUCH: usize = 60;
const CTL_JUMP: usize = 64;
const CTL_SPEED: usize = 68;

#[allow(dead_code)] // f32/u64 slots are part of the protocol surface.
enum Arg {
    Ptr(Vec<u8>, Vec<usize>, Option<u64>),
    F32(u32),
    U64(u64),
}

fn controller_from_bytes(b: &[u8]) -> Controller {
    let mut c = Controller::new(
        MoverSpec::from_base_speed(f32_at(b, CTL_SPEED)),
        [
            f32_at(b, CTL_POS),
            f32_at(b, CTL_POS + 4),
            f32_at(b, CTL_POS + 8),
        ],
        f32_at(b, CTL_YAW),
    );
    c.vertical = f32_at(b, CTL_VERTICAL);
    c.grounded = u32_at(b, CTL_GROUNDED) != 0;
    c.ground_normal = [
        f32_at(b, CTL_NORMAL),
        f32_at(b, CTL_NORMAL + 4),
        f32_at(b, CTL_NORMAL + 8),
    ];
    c.slope_vertical = f32_at(b, CTL_SLOPE);
    c.vel = [f32_at(b, CTL_VEL), f32_at(b, CTL_VEL + 4)];
    c
}

fn controller_to_bytes(c: &Controller, b: &mut [u8]) {
    put_f32(b, CTL_POS, c.pos[0]);
    put_f32(b, CTL_POS + 4, c.pos[1]);
    put_f32(b, CTL_POS + 8, c.pos[2]);
    put_f32(b, CTL_YAW, c.yaw);
    put_f32(b, CTL_VERTICAL, c.vertical);
    b[CTL_GROUNDED..CTL_GROUNDED + 4].copy_from_slice(&(c.grounded as u32).to_le_bytes());
    put_f32(b, CTL_NORMAL, c.ground_normal[0]);
    put_f32(b, CTL_NORMAL + 4, c.ground_normal[1]);
    put_f32(b, CTL_NORMAL + 8, c.ground_normal[2]);
    put_f32(b, CTL_SLOPE, c.slope_vertical);
    put_f32(b, CTL_VEL, c.vel[0]);
    put_f32(b, CTL_VEL + 4, c.vel[1]);
}

/// The probe's state buffer and base guest address. Empty until `state`.
struct State {
    base: u64,
    buf: Vec<u8>,
}

impl State {
    fn new() -> State {
        State {
            base: 0,
            buf: Vec::new(),
        }
    }

    fn loaded(&self) -> bool {
        !self.buf.is_empty()
    }

    /// Host pointer for a guest address inside the loaded state.
    fn host(&mut self, guest: u64, len: usize) -> Result<*mut u8, String> {
        if !self.loaded() {
            return Err("state-not-loaded".into());
        }
        if guest < self.base {
            return Err("guest-below-state".into());
        }
        let off = (guest - self.base) as usize;
        if off >= self.buf.len() {
            return Err("guest-above-state".into());
        }
        if len > self.buf.len() - off {
            return Err("guest-slice-too-long".into());
        }
        Ok(unsafe { self.buf.as_mut_ptr().add(off) })
    }
}

fn handle_call(
    entry: &str,
    args: Vec<Arg>,
    state: &mut State,
) -> Result<(String, Vec<Vec<u8>>), String> {
    match entry {
        // crates/ow-sim/src/camera.rs
        "orthonormalize_camera_basis" => {
            if args.len() != 1 {
                return Err(format!("expected 1 arg, got {}", args.len()));
            }
            let (mut buf, guest) = match &args[0] {
                Arg::Ptr(b, r, g) => {
                    if g.is_none() {
                        if !r.is_empty() {
                            return Err("unexpected-relocs".into());
                        }
                        (b.clone(), None)
                    } else if r.is_empty() {
                        (b.clone(), *g)
                    } else {
                        // Rebase relative pointers into the state slice, then
                        // copy the slice back over the snapshot.
                        let guest = g.unwrap();
                        let p = state.host(guest, b.len())?;
                        let mut tmp = b.clone();
                        for off in r {
                            let rel = u64::from_le_bytes(b[*off..*off + 8].try_into().unwrap());
                            let abs = p as u64 + rel;
                            tmp[*off..*off + 8].copy_from_slice(&abs.to_le_bytes());
                        }
                        (tmp, Some(guest))
                    }
                }
                _ => return Err("arg0 must be ptr".into()),
            };
            orthonormalize_camera_basis(&mut buf);
            if let Some(guest) = guest {
                let p = state.host(guest, buf.len())?;
                unsafe {
                    std::ptr::copy_nonoverlapping(buf.as_ptr(), p, buf.len());
                }
            }
            Ok(("-".to_string(), vec![buf]))
        }
        // crates/ow-sim/src/movement.rs
        "move_toward" => {
            if args.len() != 1 {
                return Err(format!("expected 1 arg, got {}", args.len()));
            }
            let mut buf = match &args[0] {
                Arg::Ptr(b, _, _) if b.len() >= 20 => b.clone(),
                _ => return Err("arg0 must be ptr(20)".into()),
            };
            let v = [f32_at(&buf, 0), f32_at(&buf, 4)];
            let target = [f32_at(&buf, 8), f32_at(&buf, 12)];
            let limit = f32_at(&buf, 16);
            let out = move_toward(v, target, limit);
            put_f32(&mut buf, 0, out[0]);
            put_f32(&mut buf, 4, out[1]);
            Ok(("-".to_string(), vec![buf]))
        }
        // crates/ow-sim/src/movement.rs - stateful pilot entry.
        "controller_advance" => {
            if args.len() != 1 {
                return Err(format!("expected 1 arg, got {}", args.len()));
            }
            let mut buf = match &args[0] {
                Arg::Ptr(b, r, g) => {
                    if !r.is_empty() || g.is_none() {
                        return Err("controller_advance needs a guest ptr".into());
                    }
                    if b.len() < CTL_LEN {
                        return Err("controller-snapshot-too-small".into());
                    }
                    b.clone()
                }
                _ => return Err("arg0 must be ptr".into()),
            };
            let mut c = controller_from_bytes(&buf);
            let dt = f32_at(&buf, CTL_DT);
            let input = Input {
                forward: f32_at(&buf, CTL_FORWARD),
                side: f32_at(&buf, CTL_SIDE),
                crouch: u32_at(&buf, CTL_CROUCH) != 0,
                jump: u32_at(&buf, CTL_JUMP) != 0,
            };
            c.advance(dt, input);
            // The caller may not care about the whole struct; the contract is
            // the same wire layout so a fixture can compare field by field.
            controller_to_bytes(&c, &mut buf);
            // Write the result back into the state buffer at the guest slot
            // the fixture named, so the state channel sees it.
            if let Arg::Ptr(_, _, Some(guest)) = &args[0] {
                let p = state.host(*guest, CTL_LEN)?;
                unsafe {
                    std::ptr::copy_nonoverlapping(buf.as_ptr(), p, CTL_LEN);
                }
            }
            Ok(("-".to_string(), vec![buf]))
        }
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
        if toks[0] == "state" && toks.len() == 3 {
            let base = match u64::from_str_radix(toks[1], 16) {
                Ok(v) => v,
                Err(_) => {
                    let _ = writeln!(out, "err bad-state-base");
                    let _ = out.flush();
                    continue;
                }
            };
            let bytes = match hex_decode(toks[2]) {
                Some(b) => b,
                None => {
                    let _ = writeln!(out, "err bad-state-hex");
                    let _ = out.flush();
                    continue;
                }
            };
            state.base = base;
            state.buf = bytes;
            // The response is deliberately terse: the harness already knows
            // the bytes it sent.
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
