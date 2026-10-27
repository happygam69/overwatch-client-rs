//! Tier-2 verification probe for `ow-tank`, spoken over the harness's line
//! protocol.
//!
//! Built by `cargo build --release --bin verify_probe` from `ow-tank`'s own
//! manifest. Every entry calls the function the crate actually links; the
//! harness binds its verdict to that function's source file and the crate
//! revision (see `dev/verify_harness/candidate_rust.py`).
//!
//! Protocol (one request per line, one response per line):
//!
//!   -> ping
//!   <- pong
//!
//!   -> state <hex base> <hex bytes>
//!   <- ok
//!
//!   -> call <entry> <nargs> [ptr <hex> <relocs> <guest|-> | f32 <8hex> |
//!                            f64 <16hex> | u64 <16hex>]*
//!   <- ok <retbits-hex|-> <outhex> [<statehex>]
//!   <- err <message-with-no-spaces>
//!
//! `relocs` is `-` or comma-separated decimal byte offsets of u64 relative
//! pointers inside the pointee; a `guest` address other than `-` selects a
//! slice of the preloaded state buffer. A `ptr` hex field, a response byte
//! field, or a state byte field of `-` is the empty byte string.
//!
//! The `chunk_guid` entry reads the kind qword at `+8` of the table slot the
//! client's `MapChunkGuid` (`0x141b32e20`) indexes and passes it to
//! `ow_tank::mapguid::chunk_guid`. The table is supplied as a plain pointee
//! buffer of the client's own `0x24 * 0x20` layout; the harness models the
//! same bytes at the table's image address in its `state` block, so both
//! sides read one table.

use std::io::{self, BufRead, Write};

const CHUNK_TABLE_SLOTS: usize = 0x24;
const CHUNK_TABLE_STRIDE: usize = 0x20;
const CHUNK_TABLE_LEN: usize = CHUNK_TABLE_SLOTS * CHUNK_TABLE_STRIDE;

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

#[allow(dead_code)] // f32/f64 slots are part of the protocol surface.
enum Arg {
    Ptr(Vec<u8>, Vec<usize>, Option<u64>),
    F32(u32),
    F64(u64),
    U64(u64),
}

/// The client's `MapChunkGuid` (`0x141b32e20`) is `u64 (u32 map, i32 slot)`.
/// It reads the kind qword at `+8` of the stride-`0x20` table entry for
/// `slot`, shifts that 64-bit kind left by 32 and ORs in the zero-extended
/// map. The shipped formula takes the kind directly, so this entry performs
/// exactly the table read the client performs and then calls the shipped
/// function.
fn chunk_guid_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 3 {
        return Err(format!("expected 3 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let map = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg0 must be u64".into()),
    };
    let slot = match it.next() {
        Some(Arg::U64(v)) => v as usize,
        _ => return Err("arg1 must be u64".into()),
    };
    let table = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
        _ => return Err("arg2 must be a plain arena buffer".into()),
    };
    if slot >= CHUNK_TABLE_SLOTS {
        return Err("slot-out-of-table".into());
    }
    if table.len() < CHUNK_TABLE_LEN {
        return Err("table-must-be-1152-bytes".into());
    }
    let at = slot * CHUNK_TABLE_STRIDE + 8;
    let kind = u64::from_le_bytes(table[at..at + 8].try_into().unwrap());
    let guid = ow_tank::mapguid::chunk_guid(map, kind);
    Ok((format!("{:x}", guid), vec![table]))
}

fn handle_call(entry: &str, args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    match entry {
        // crates/ow-tank/src/mapguid.rs
        "chunk_guid" => chunk_guid_entry(args),
        other => Err(format!("unknown-entry-{}", other)),
    }
}

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    // The harness sends the unit's `state` snapshot before the first call.
    // `chunk_guid` reads its table through the pointee argument, so the bytes
    // are only echoed back on the state channel.
    let mut state: Option<Vec<u8>> = None;

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
            let _ = base;
            state = Some(bytes);
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
        match handle_call(entry, args) {
            Ok((ret, outs)) => {
                let mut resp = format!("ok {}", ret);
                for o in outs {
                    resp.push(' ');
                    resp.push_str(&hex_encode(&o));
                }
                if let Some(s) = &state {
                    resp.push(' ');
                    resp.push_str(&hex_encode(s));
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
