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

#[allow(dead_code)] // f32/u64 slots are part of the protocol surface.
enum Arg {
    Ptr(Vec<u8>, Vec<usize>, Option<u64>),
    F32(u32),
    U64(u64),
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
