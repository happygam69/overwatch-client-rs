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
//! `salsa20_state_init`, `md5_transform`, `dynamic_index_hash`,
//! `dynamic_index_key_hash`, `encoding_header_parse`,
//! `encoding_page_index_search`) take plain arena buffers, so a non-empty
//! relocation list or a guest address is rejected rather than silently
//! ignored.

use std::io::{self, BufRead, Write};

use ow_casc::blte::{md5_transform, salsa20, salsa20_state_init};
use ow_casc::container::{
    decode_page_record, parse_index_footer, select_page, IndexFooter, INDEX_FOOTER_SIZE, PAGE_MISS,
};
use ow_casc::encoding::{
    find_ckey_record, find_ekey_record, page_index_search, write_header_fields, HEADER_FIELDS_SIZE,
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

/// The CDN index object `CdnIndexPageSelect` (`0x1424da730`) takes: the
/// footer fields at the client's own offsets, the 64-bit pointers into the
/// same arena buffer, and the two whole-file fields `CdnIndexPageResident`
/// writes. The fixture embeds the object's own guest address at `+0xa8` (the
/// selector never reads that slot) so the probe can turn the absolute pointers
/// into offsets in its copy, exactly as `EncodingInfo` does with `+0x68`.
struct CdnIndexObject<'a> {
    bytes: &'a [u8],
}

impl<'a> CdnIndexObject<'a> {
    fn u8_at(&self, off: usize) -> Result<u8, String> {
        self.bytes
            .get(off)
            .copied()
            .ok_or_else(|| format!("object-shorter-than-{off:#x}"))
    }

    fn u32_at(&self, off: usize) -> Result<u32, String> {
        self.bytes
            .get(off..off + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| format!("object-shorter-than-{off:#x}"))
    }

    fn u64_at(&self, off: usize) -> Result<u64, String> {
        self.bytes
            .get(off..off + 8)
            .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| format!("object-shorter-than-{off:#x}"))
    }

    /// The object's own arena address, kept in the unused `+0xa8` slot.
    fn own_address(&self) -> Result<u64, String> {
        self.u64_at(0xa8)
    }

    /// An arena offset for an absolute pointer the fixture stored.
    fn rel(&self, absolute: u64) -> Result<usize, String> {
        absolute
            .checked_sub(self.own_address()?)
            .map(|v| v as usize)
            .ok_or_else(|| "pointer-before-object".to_string())
    }

    /// The footer fields the selector reads, at the client's object offsets.
    fn footer(&self) -> Result<IndexFooter, String> {
        Ok(IndexFooter {
            // The selector reads no version byte; the object has none.
            version: 0,
            page_size: self.u32_at(0x6c)?,
            offset_bytes: self.u8_at(0x5c)?,
            size_bytes: self.u8_at(0x5d)?,
            key_bytes: self.u8_at(0x5e)?,
            checksum_size: self.u8_at(0x5f)?,
            element_count: self.u32_at(0x60)?,
            entries_per_page: self.u32_at(0x74)?,
            page_count: self.u32_at(0x78)?,
            last_key_bytes: self.u32_at(0x7c)?,
            checksum_bytes: self.u32_at(0x80)?,
        })
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
        // crates/ow-casc/src/blte.rs
        "md5_transform" => {
            if args.len() != 2 {
                return Err(format!("expected 2 args, got {}", args.len()));
            }
            let mut it = args.into_iter();
            let block = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg0 must be a plain arena buffer".into()),
            };
            let state = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg1 must be a plain arena buffer".into()),
            };
            let block: [u8; 64] = block
                .as_slice()
                .try_into()
                .map_err(|_| "block-must-be-64-bytes".to_string())?;
            let mut words = <[u8; 16]>::try_from(state.as_slice())
                .map_err(|_| "state-must-be-16-bytes".to_string())?;
            let mut state_words = [0u32; 4];
            for (index, word) in state_words.iter_mut().enumerate() {
                *word = u32::from_le_bytes(words[index * 4..index * 4 + 4].try_into().unwrap());
            }
            md5_transform(&mut state_words, &block);
            for (index, word) in state_words.iter().enumerate() {
                words[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
            }
            // The block is returned unchanged as well, so a transform that
            // wrote through its input pointer is a compared divergence.
            Ok(("-".to_string(), vec![block.to_vec(), words.to_vec()]))
        }
        // crates/ow-casc/src/encoding.rs
        "encoding_page_index_search" => {
            if args.len() != 5 {
                return Err(format!("expected 5 args, got {}", args.len()));
            }
            let mut it = args.into_iter();
            let index = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg0 must be a plain arena buffer".into()),
            };
            let pages = match it.next() {
                Some(Arg::U64(v)) => v,
                _ => return Err("arg1 must be u64".into()),
            };
            let width = match it.next() {
                Some(Arg::U64(v)) => v,
                _ => return Err("arg2 must be u64".into()),
            };
            let key = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg3 must be a plain arena buffer".into()),
            };
            let key_len = match it.next() {
                Some(Arg::U64(v)) => v,
                _ => return Err("arg4 must be u64".into()),
            };
            let pages = u32::try_from(pages).map_err(|_| "pages-past-u32".to_string())?;
            let width = u8::try_from(width).map_err(|_| "width-past-u8".to_string())?;
            let key_len = u8::try_from(key_len).map_err(|_| "key-length-past-u8".to_string())?;
            // The client reads the entry without a bounds check; the fixture
            // never asks for an index it does not supply, so a short one is a
            // fixture error rather than a safety edge.
            let needed = (pages as usize) * (width as usize + 16);
            if index.len() < needed {
                return Err("index-shorter-than-pages-times-stride".into());
            }
            if usize::from(key_len) > key.len() {
                return Err("key-length-past-key".into());
            }
            let page = page_index_search(&index, pages, width, &key, key_len);
            Ok((format!("{:x}", u64::from(page)), vec![index, key]))
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
            // The client contracts only AL: success is `XOR AL,AL`, every
            // reject is `MOV AL,0x8`. The harness's `u8` return kind masks
            // RAX to the low byte, so the status is compared exactly while
            // the memory channel compares the footer copy and every derived
            // field. RAX's upper bits are ignored, as the client's callers do.
            Ok((format!("{:x}", u64::from(code)), vec![data, out.to_vec()]))
        }
        // crates/ow-casc/src/container.rs
        "cdn_index_page_select" => {
            if args.len() != 6 {
                return Err(format!("expected 6 args, got {}", args.len()));
            }
            let mut it = args.into_iter();
            let mut index = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg0 must be a plain arena buffer".into()),
            };
            let key = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg1 must be a plain arena buffer".into()),
            };
            let key_len = match it.next() {
                Some(Arg::U64(v)) => {
                    u8::try_from(v).map_err(|_| "key-length-past-u8".to_string())?
                }
                _ => return Err("arg2 must be u64".into()),
            };
            let mut out_offset = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg3 must be a plain arena buffer".into()),
            };
            // Arguments 5 and 6 travel on the stack in the client's ABI; the
            // oracle writes them at the caller's [rsp+0x28] and [rsp+0x30].
            let mut out_size = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg4 must be a plain arena buffer".into()),
            };
            let mut out_key16 = match it.next() {
                Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
                _ => return Err("arg5 must be a plain arena buffer".into()),
            };
            // The client reads the query and the object without a bounds
            // check; the fixture never asks for a region it does not supply,
            // so a short one is a fixture error, not a safety edge.
            if usize::from(key_len) > key.len() {
                return Err("key-length-past-key".into());
            }
            if out_offset.len() < 8 || out_size.len() < 4 || out_key16.len() < 16 {
                return Err("out-buffer-too-short".into());
            }
            let (footer, own, last_keys_off, bodies_abs, whole_file) = {
                let object = CdnIndexObject { bytes: &index };
                (
                    object.footer()?,
                    object.own_address()?,
                    object.rel(object.u64_at(0x88)?)?,
                    object.u64_at(0x50)?,
                    object.u8_at(0x38)?,
                )
            };
            if whole_file != 0 {
                // Declared gap: the streamed/HTTP `CdnIndexPageResident`
                // branch is read but not implemented, and this unit's object
                // is always whole-file.
                return Err("streamed-index-branch-not-covered".into());
            }
            let last_keys_span = (footer.page_count as usize) * usize::from(footer.key_bytes);
            let page = {
                let last_keys = index
                    .get(last_keys_off..last_keys_off + last_keys_span)
                    .ok_or("last-keys-outside-index")?;
                select_page(
                    last_keys,
                    footer.page_count,
                    footer.key_bytes,
                    &key,
                    key_len,
                )
            };
            // The client reports AL only: 0 on a hit, 6 on a miss, otherwise
            // the resident status. The harness masks both sides to the byte.
            let mut status = 6u64;
            if page != PAGE_MISS {
                // `CdnIndexPageResident` (`0x1424da590`), whole-file branch
                // (`index+0x38 == 0`): the current page becomes `page` and the
                // page pointer `pageBodies + (u32)(page * pageSize)`. Those two
                // object fields are the client's side effects of the call, so
                // the probe writes them back and the unit compares the whole
                // object; a candidate that skipped them diverges.
                let page_ptr =
                    bodies_abs.wrapping_add(u64::from(page.wrapping_mul(footer.page_size)));
                index[0x98..0xa0].copy_from_slice(&page_ptr.to_le_bytes());
                index[0xa0..0xa4].copy_from_slice(&page.to_le_bytes());
                let page_base = page_ptr
                    .checked_sub(own)
                    .map(|v| v as usize)
                    .ok_or("page-before-object")?;
                let body = index.get(page_base..).ok_or("page-outside-index")?;
                if let Some(record) = decode_page_record(&footer, page, body, &key, key_len) {
                    out_offset[..8].copy_from_slice(&record.offset.to_le_bytes());
                    out_size[..4].copy_from_slice(&record.size.to_le_bytes());
                    out_key16[..16].copy_from_slice(&record.key16);
                    status = 0;
                }
            }
            Ok((
                format!("{:x}", status),
                vec![index, key, out_offset, out_size, out_key16],
            ))
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
