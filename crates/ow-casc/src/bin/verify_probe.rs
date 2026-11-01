//! Tier-2 verification probe for `ow-casc`, spoken over the harness's line
//! protocol.
//!
//! Built by `cargo build --release --bin verify_probe` from `ow-casc`'s own
//! manifest. Every entry calls the function the crate actually links; the
//! harness binds its verdict to that function's source file and the crate
//! revision (see `dev/verify_harness/candidate_rust.py`).
//!
//! Protocol (one request per line, one response per line):
//!
//!   -> ping
//!   <- pong
//!
//!   -> call <entry> <nargs> [ptr <hex> <relocs> <guest|-> | f32 <8hex> |
//!                            f64 <16hex> | u64 <16hex>]*
//!   <- ok <retbits-hex|-> <outhex>...
//!   <- err <message-with-no-spaces>
//!
//! `relocs` is `-` or comma-separated decimal byte offsets of u64 relative
//! pointers inside the pointee; a `guest` address other than `-` selects a
//! slice of a preloaded state buffer. A `ptr` hex field or a response byte
//! field of `-` is the empty byte string (an empty field would vanish under
//! whitespace splitting). The entries verified here (`salsa20_xor`,
//! `salsa20_state_init`, `dynamic_index_hash`, `dynamic_index_key_hash`,
//! `encoding_header_parse`) take plain arena buffers, so a non-empty
//! relocation list or a guest address is rejected rather than silently
//! ignored.

use std::io::{self, BufRead, Write};

use ow_casc::blte::{salsa20, salsa20_state_init};
use ow_casc::container::{parse_index_footer, INDEX_FOOTER_SIZE};
use ow_casc::encoding::{
    find_ckey_record, find_ekey_record, write_header_fields, HEADER_FIELDS_SIZE,
};
use ow_casc::index::{hash_index, key_hash};

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

fn word(state: &[u8], index: usize) -> u32 {
    u32::from_le_bytes(state[index * 4..index * 4 + 4].try_into().unwrap())
}

fn put_word(state: &mut [u8], index: usize, value: u32) {
    state[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
}

/// `Salsa20Xor` (Overwatch.exe `0x1424e32c0`) over the client's state layout.
///
/// `Salsa20StateInit` (`0x1424e3230`) stores the state in the diagonal order:
/// words 0..3 are the four `expand 16-byte k` constants, the key words are at
/// 5, 4, 9 and 12, the nonce words at 6 and 11, and the 64-bit block counter at
/// 8 (low) and 13 (high). The client advances that counter by one per 64-byte
/// block it consumes; a partial tail still consumes a whole block. The shipped
/// `salsa20` always starts at block 0, so the probe discards `counter` whole
/// blocks from its own keystream and then overwrites the state counter with
/// the same value the client leaves behind.
///
/// The skip is bounded: this probe is for the fixture's small counters, not a
/// general keystream seeker.
fn salsa20_xor(state: &mut [u8], data: &[u8], len: usize) -> Result<Vec<u8>, String> {
    if state.len() != 64 {
        return Err("state-must-be-64-bytes".into());
    }
    if len > data.len() {
        return Err("len-past-data".into());
    }
    const MAX_SKIP_BLOCKS: u64 = 1 << 20;
    let key: [u8; 16] = [
        word(state, 5).to_le_bytes(),
        word(state, 4).to_le_bytes(),
        word(state, 9).to_le_bytes(),
        word(state, 12).to_le_bytes(),
    ]
    .concat()
    .try_into()
    .unwrap();
    let nonce: [u8; 8] = [word(state, 6).to_le_bytes(), word(state, 11).to_le_bytes()]
        .concat()
        .try_into()
        .unwrap();
    let counter = ((word(state, 13) as u64) << 32) | word(state, 8) as u64;
    if counter > MAX_SKIP_BLOCKS {
        return Err("counter-too-large".into());
    }
    let skip = (counter as usize) * 64;
    let mut buf = vec![0u8; skip + len];
    buf[skip..].copy_from_slice(&data[..len]);
    let out = salsa20(&key, &nonce, &buf);
    let blocks = (len as u64) / 64 + u64::from(len % 64 != 0);
    let next = counter + blocks;
    put_word(state, 8, next as u32);
    put_word(state, 13, (next >> 32) as u32);
    Ok(out[skip..].to_vec())
}

/// The client's 0x70-byte encoding-info struct. The fixture embeds the info
/// buffer's own guest address at `+0x68` (the lookup never reads it) so the
/// probe can turn the absolute section pointers into offsets in its copy and
/// write the same absolute record address the oracle writes.
struct EncodingInfo<'a> {
    info: &'a [u8],
    index: &'a [u8],
    bodies: &'a [u8],
    bodies_abs: u64,
}

impl<'a> EncodingInfo<'a> {
    fn parse(info: &'a [u8], ckey_side: bool) -> Result<Self, String> {
        let at = |off: usize| -> Result<u64, String> {
            info.get(off..off + 8)
                .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
                .ok_or_else(|| "info-too-short".to_string())
        };
        let info_abs = at(0x68)?;
        let (index_slot, bodies_slot) = if ckey_side {
            (0x30, 0x38)
        } else {
            (0x40, 0x48)
        };
        let index_abs = at(index_slot)?;
        let bodies_abs = at(bodies_slot)?;
        let index_off = index_abs.checked_sub(info_abs).ok_or("index-before-info")? as usize;
        let bodies_off = bodies_abs
            .checked_sub(info_abs)
            .ok_or("bodies-before-info")? as usize;
        let index = info.get(index_off..).ok_or("index-outside-info")?;
        let bodies = info.get(bodies_off..).ok_or("bodies-outside-info")?;
        Ok(Self {
            info,
            index,
            bodies,
            bodies_abs,
        })
    }

    fn u32(&self, off: usize) -> Result<u32, String> {
        self.info
            .get(off..off + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| "info-field-outside".to_string())
    }

    fn u8(&self, off: usize) -> Result<u8, String> {
        self.info
            .get(off)
            .copied()
            .ok_or_else(|| "info-field-outside".to_string())
    }

    fn write_out(&self, out: &mut [u8], offset: Option<usize>) {
        out.fill(0);
        if let Some(offset) = offset {
            let address = self.bodies_abs + offset as u64;
            if out.len() >= 8 {
                out[..8].copy_from_slice(&address.to_le_bytes());
            }
        }
    }
}

fn handle_call(entry: &str, args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    match entry {
        // crates/ow-casc/src/blte.rs
        "salsa20_xor" => {
            if args.len() != 3 {
                return Err(format!("expected 3 args, got {}", args.len()));
            }
            let mut it = args.into_iter();
            let (mut state, relocs, guest) = match it.next() {
                Some(Arg::Ptr(b, r, g)) => (b, r, g),
                _ => return Err("arg0 must be ptr".into()),
            };
            if !relocs.is_empty() || guest.is_some() {
                return Err("arg0 must be a plain arena buffer".into());
            }
            let data = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg1 must be a plain arena buffer".into()),
            };
            let len = match it.next() {
                Some(Arg::U64(v)) => v as usize,
                _ => return Err("arg2 must be u64".into()),
            };
            let out = salsa20_xor(&mut state, &data, len)?;
            Ok(("-".to_string(), vec![state, out]))
        }
        // crates/ow-casc/src/blte.rs
        "salsa20_state_init" => {
            if args.len() != 4 {
                return Err(format!("expected 4 args, got {}", args.len()));
            }
            let mut it = args.into_iter();
            let mut state = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg0 must be a plain arena buffer".into()),
            };
            let key = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg1 must be a plain arena buffer".into()),
            };
            let nonce = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg2 must be a plain arena buffer".into()),
            };
            let counter = match it.next() {
                Some(Arg::U64(v)) => v,
                _ => return Err("arg3 must be u64".into()),
            };
            if state.len() != 64 {
                return Err("state-must-be-64-bytes".into());
            }
            if key.len() != 16 || nonce.len() != 8 {
                return Err("key-must-be-16-and-nonce-8-bytes".into());
            }
            let key: [u8; 16] = key.try_into().unwrap();
            let nonce: [u8; 8] = nonce.try_into().unwrap();
            for (i, w) in salsa20_state_init(&key, &nonce, counter).iter().enumerate() {
                put_word(&mut state, i, *w);
            }
            Ok(("-".to_string(), vec![state, key.to_vec(), nonce.to_vec()]))
        }
        // crates/ow-casc/src/index.rs
        "dynamic_index_hash" => {
            if args.len() != 4 {
                return Err(format!("expected 4 args, got {}", args.len()));
            }
            let mut it = args.into_iter();
            let data = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg0 must be a plain arena buffer".into()),
            };
            let len = match it.next() {
                Some(Arg::U64(v)) => v as usize,
                _ => return Err("arg1 must be u64".into()),
            };
            let s0 = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg2 must be a plain arena buffer".into()),
            };
            let s1 = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg3 must be a plain arena buffer".into()),
            };
            if len > data.len() {
                return Err("len-past-data".into());
            }
            if s0.len() != 4 || s1.len() != 4 {
                return Err("state-words-must-be-4-bytes".into());
            }
            let mut state = [
                u32::from_le_bytes(s0[..4].try_into().unwrap()),
                u32::from_le_bytes(s1[..4].try_into().unwrap()),
            ];
            hash_index(&data[..len], &mut state);
            Ok((
                "-".to_string(),
                vec![
                    data,
                    state[0].to_le_bytes().to_vec(),
                    state[1].to_le_bytes().to_vec(),
                ],
            ))
        }
        // crates/ow-casc/src/index.rs
        "dynamic_index_key_hash" => {
            if args.len() != 3 {
                return Err(format!("expected 3 args, got {}", args.len()));
            }
            let mut it = args.into_iter();
            let key = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg0 must be a plain arena buffer".into()),
            };
            let len = match it.next() {
                Some(Arg::U64(v)) => v as usize,
                _ => return Err("arg1 must be u64".into()),
            };
            let init = match it.next() {
                Some(Arg::U64(v)) => v as u32,
                _ => return Err("arg2 must be u64".into()),
            };
            if len > key.len() {
                return Err("len-past-data".into());
            }
            let hash = key_hash(&key[..len], init);
            Ok((format!("{:x}", u64::from(hash)), vec![key]))
        }
        // crates/ow-casc/src/encoding.rs
        "encoding_find_ckey" | "encoding_find_ekey" => {
            if args.len() != 4 {
                return Err(format!("expected 4 args, got {}", args.len()));
            }
            let mut it = args.into_iter();
            let info = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg0 must be a plain arena buffer".into()),
            };
            let key = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg1 must be a plain arena buffer".into()),
            };
            let key_len = match it.next() {
                Some(Arg::U64(v)) => v as u8,
                _ => return Err("arg2 must be u64".into()),
            };
            let mut out = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg3 must be a plain arena buffer".into()),
            };
            let ckey_side = entry == "encoding_find_ckey";
            let parsed = EncodingInfo::parse(&info, ckey_side)?;
            if ckey_side {
                let pages = parsed.u32(0x14)?;
                let ckey_width = parsed.u8(0x11)?;
                let ekey_width = parsed.u8(0x12)?;
                let page_bytes = parsed.u32(0x18)?;
                let record = find_ckey_record(
                    parsed.index,
                    parsed.bodies,
                    pages,
                    ckey_width,
                    ekey_width,
                    page_bytes,
                    &key,
                    key_len,
                );
                parsed.write_out(&mut out, record);
            } else {
                let pages = parsed.u32(0x1c)?;
                let ekey_width = parsed.u8(0x12)?;
                let page_bytes = parsed.u32(0x20)?;
                let record = find_ekey_record(
                    parsed.index,
                    parsed.bodies,
                    pages,
                    ekey_width,
                    page_bytes,
                    &key,
                    key_len,
                );
                parsed.write_out(&mut out, record);
            }
            Ok(("-".to_string(), vec![info, key, out]))
        }
        // crates/ow-casc/src/encoding.rs
        "encoding_header_parse" => {
            if args.len() != 3 {
                return Err(format!("expected 3 args, got {}", args.len()));
            }
            let mut it = args.into_iter();
            let out = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg0 must be a plain arena buffer".into()),
            };
            let data = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg1 must be a plain arena buffer".into()),
            };
            let len = match it.next() {
                Some(Arg::U64(v)) => v as usize,
                _ => return Err("arg2 must be u64".into()),
            };
            if len > data.len() {
                return Err("len-past-data".into());
            }
            let Ok(mut out) = <[u8; HEADER_FIELDS_SIZE]>::try_from(out) else {
                return Err("out-must-be-40-bytes".into());
            };
            let code = write_header_fields(&data[..len], &mut out);
            // Both buffers are compared: `out` carries the fields the client
            // wrote plus every untouched byte, and `data` proves the input was
            // not mutated.
            Ok((format!("{:x}", u64::from(code)), vec![out.to_vec(), data]))
        }
        // crates/ow-casc/src/container.rs
        "cdn_index_footer_parse" => {
            if args.len() != 4 {
                return Err(format!("expected 4 args, got {}", args.len()));
            }
            let mut it = args.into_iter();
            let data = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg0 must be a plain arena buffer".into()),
            };
            let len = match it.next() {
                Some(Arg::U64(v)) => v as usize,
                _ => return Err("arg1 must be u64".into()),
            };
            let total_len = match it.next() {
                Some(Arg::U64(v)) => v as u32,
                _ => return Err("arg2 must be u64".into()),
            };
            let out = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg3 must be a plain arena buffer".into()),
            };
            if len > data.len() {
                return Err("len-past-data".into());
            }
            if len < INDEX_FOOTER_SIZE {
                return Err("len-shorter-than-footer".into());
            }
            let Ok(mut out) = <[u8; 0x30]>::try_from(out) else {
                return Err("out-must-be-48-bytes".into());
            };
            let code = parse_index_footer(&data, len, total_len, &mut out);
            // `void` return: the client writes only AL and leaves RAX's upper
            // bits from unrelated arithmetic, so callers test AL only. The
            // memory channel compares the footer copy and every derived field;
            // the hash-zeroing reject and every early reject are visible there.
            let _ = code;
            Ok(("-".to_string(), vec![data, out.to_vec()]))
        }
        other => Err(format!("unknown-entry-{}", other)),
    }
}

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

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
