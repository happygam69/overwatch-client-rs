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
//!
//! The `cmf_key_154504` and `cmf_iv_154504` entries read the 512-byte key
//! table from the same `state` block (the client reads it as a global at
//! `0x1436100c0`) and call `ow_tank::manifest::key_154504` /
//! `iv_154504`. The IV entry takes the client's string object, extracts the
//! name through its inline/heap layout, hashes it with the same `sha1` crate
//! the manifest decryption path uses, and passes the digest to the shipped
//! function.
//!
//! The `bundle_lookup` entry reads the client's tagged handle object (64
//! bytes: `+0` payload pointer, `+8` tag dword) and the payload that follows
//! it from one pointee buffer, then calls `ow_tank::bundle::entry_lookup`.
//! That function returns the client's `payload + offset` as a guest address,
//! so the fixture passes the payload's guest base in the client's dead first
//! argument slot (RCX) and the probe returns the same guest value.
//!
//! The `bundle_directory_lookup` entry takes the directory object, the output
//! slot and the target GUID in the client's RCX/RDX/R8 slots and the
//! directory's structure pool in the dead R9 slot, all as guest slices of the
//! unit's `state` block (the client reads the type-entry pointer through its
//! fixed global, which the state covers). It calls
//! `ow_tank::bundle::directory_lookup` and returns the out slot's guest
//! address, which is the client's `param_2` return value.
//!
//! The texture leaves are plain integer entries: `payload_guid` calls
//! `ow_tank::guid::payload_of` (`0x142e37520`), `texture_request_count` calls
//! `ow_tank::texture::request_count` (`0x142e37600`) and
//! `texture_count_bits_shifted` calls `ow_tank::texture::count_bits_shifted`
//! (`0x142e374d0`). Each casts the u64 wire slots to the client's u32/int
//! parameter widths. The `crc32_seeded` entry is the material name hash
//! (`0x141b52ca0`): a plain arena buffer, its length, and the running CRC
//! initial value.
//!
//! The descriptor entries follow the client's own field reads:
//! `texture_format_family_bpp` calls `format_family_and_bpp`
//! (`0x142e372b0`) with two 4-byte out slots, `texture_descriptor` parses the
//! 0x20-byte header and writes `descriptor()`/`object_flags()` (`0x142e2d900`
//! for the builder), and `texture_descriptor_size_walk` reads the u64 at
//! `+0`, the flags at `+8` and the format at `+0x60` of the client's descriptor
//! object before calling `descriptor_size_walk` (`0x142e2dbb0`).

use std::io::{self, BufRead, Write};

/// The probe's copy of the unit's preloaded state snapshot.
///
/// A guest address is translated by subtracting `base`, so a pointer in the
/// blob can be followed exactly as the client follows it. The state region is
/// also the state-differential channel: `main` returns the whole buffer after
/// every call when one is loaded.
struct State {
    base: u64,
    /// The unit's declared snapshot, kept so each call starts from it exactly
    /// as the oracle re-seeds its state region before every call.
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

    fn reset(&mut self) {
        self.buf.copy_from_slice(&self.snapshot);
    }

    fn off(&self, guest: u64) -> Option<usize> {
        let off = guest.checked_sub(self.base)? as usize;
        (off <= self.buf.len()).then_some(off)
    }
}

/// Copy a guest slice into the state and rebase its relative pointers, exactly
/// as the oracle rebases the pointee it writes.
fn guest_write(state: &mut State, guest: u64, mem: &[u8], relocs: &[usize]) -> Result<(), String> {
    let off = state.off(guest).ok_or("guest-outside-state")?;
    let end = off.checked_add(mem.len()).ok_or("guest-slice-overflow")?;
    if end > state.buf.len() {
        return Err("guest-slice-too-long".into());
    }
    state.buf[off..end].copy_from_slice(mem);
    for &r in relocs {
        let slot = off.checked_add(r).ok_or("reloc-overflow")?;
        let bytes: [u8; 8] = state
            .buf
            .get(slot..slot + 8)
            .ok_or("reloc-out-of-range")?
            .try_into()
            .unwrap();
        let abs = guest.wrapping_add(u64::from_le_bytes(bytes));
        state.buf[slot..slot + 8].copy_from_slice(&abs.to_le_bytes());
    }
    Ok(())
}

const CHUNK_TABLE_SLOTS: usize = 0x24;
const CHUNK_TABLE_STRIDE: usize = 0x20;
const CHUNK_TABLE_LEN: usize = CHUNK_TABLE_SLOTS * CHUNK_TABLE_STRIDE;

/// The client's key table is 512 bytes at `0x1436100c0`.
const CMF_TABLE_LEN: usize = 512;
/// `Header::parse` reads the build at `+0` and the data count at `+0x20`.
const CMF_HEADER_LEN: usize = 48;
/// The client string uses the 16-byte inline buffer and the size/capacity
/// dwords at `+0x10` / `+0x14`.
const CMF_STRING_INLINE: usize = 16;
const CMF_STRING_OBJECT: usize = 0x20;

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

/// `PayloadGuid` (`0x142e37520`): `u64 (u64 parent, uint index)`.
fn payload_guid_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 2 {
        return Err(format!("expected 2 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let parent = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg0 must be u64".into()),
    };
    let index = match it.next() {
        Some(Arg::U64(v)) => v as u32,
        _ => return Err("arg1 must be u64".into()),
    };
    let guid = ow_tank::guid::payload_of(parent, index);
    Ok((format!("{:x}", guid), Vec::new()))
}

/// `FUN_142e37600`: `int (int payload_count, int mip_count, int base_mips, int
/// streamed)`.
fn texture_request_count_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 4 {
        return Err(format!("expected 4 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let mut take = || -> Result<u32, String> {
        match it.next() {
            Some(Arg::U64(v)) => Ok(v as u32),
            _ => Err("args must be u64".into()),
        }
    };
    let payload = take()?;
    let mip = take()?;
    let base = take()?;
    let streamed = take()?;
    let ret = ow_tank::texture::request_count(payload, mip, base, streamed);
    Ok((format!("{:x}", ret as u64), Vec::new()))
}

/// `FUN_142e374d0`: `uint (uint value, char bias)`; popcounts
/// `value >> ((bias + 2) & 31)`.
fn texture_count_bits_shifted_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 2 {
        return Err(format!("expected 2 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let value = match it.next() {
        Some(Arg::U64(v)) => v as u32,
        _ => return Err("arg0 must be u64".into()),
    };
    let bias = match it.next() {
        Some(Arg::U64(v)) => v as u32,
        _ => return Err("arg1 must be u64".into()),
    };
    let ret = ow_tank::texture::count_bits_shifted(value, bias);
    Ok((format!("{:x}", ret as u64), Vec::new()))
}

/// `FUN_142e372b0`: `void (uint format, uint *family, uint *bpp)`.
///
/// The client's format switch writes both out dwords on a match and leaves
/// them untouched otherwise. The unit's fixture zeroes both, so the shipped
/// function's `(0, 0)` default and the client's untouched zero agree. The
/// fixture keeps the format in 0..=0xff; the probe refuses wider values
/// because the client compares the full 32-bit argument and the Rust switch
/// takes the stored byte.
fn texture_format_family_bpp_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 3 {
        return Err(format!("expected 3 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let format = match it.next() {
        Some(Arg::U64(v)) => v as u32,
        _ => return Err("arg0 must be u64".into()),
    };
    let family = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 4 => b,
        _ => return Err("arg1 must be the 4-byte family out".into()),
    };
    let bpp = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() == 4 => b,
        _ => return Err("arg2 must be the 4-byte bpp out".into()),
    };
    if format > 0xff {
        return Err("format-must-fit-u8".into());
    }
    let (family_v, bpp_v) = ow_tank::texture::format_family_and_bpp(format as u8);
    let mut family_after = family;
    family_after.copy_from_slice(&(family_v as u32).to_le_bytes());
    let mut bpp_after = bpp;
    bpp_after.copy_from_slice(&bpp_v.to_le_bytes());
    Ok(("-".to_string(), vec![family_after, bpp_after]))
}

/// `FUN_142e2d900`: `object * (object *out, ushort *header)`.
///
/// The unit compares the descriptor qword and the object's flags dword. The
/// fixture pins byte `+0x07` to 0 because the builder writes only
/// `flags | 0x100000`; the loader (`0x141dacf00`) adds `0x4000` later when that
/// byte is 1, so the builder unit must not claim that bit.
fn texture_descriptor_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 2 {
        return Err(format!("expected 2 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let out = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 12 => b,
        _ => return Err("arg0 must be the out object".into()),
    };
    let header = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x20 => b,
        _ => return Err("arg1 must be the 0x20-byte header".into()),
    };
    let parsed = ow_tank::texture::TextureHeader::parse(&header)
        .map_err(|e| format!("header-parse: {e}"))?;
    let mut out_after = out;
    out_after[0..8].copy_from_slice(&parsed.descriptor().to_le_bytes());
    out_after[8..12].copy_from_slice(&parsed.object_flags().to_le_bytes());
    Ok(("-".to_string(), vec![out_after, header]))
}

/// `FUN_142e2dbb0`: `uint (object *descriptor, int bias)`.
///
/// The object is the client's 0x68-byte descriptor record: the u64 at `+0`, the
/// flags dword at `+8` and the stored format byte at `+0x60`. The probe reads
/// those three fields exactly as the client's wrapper does and calls
/// `ow_tank::texture::descriptor_size_walk`.
fn texture_descriptor_size_walk_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 2 {
        return Err(format!("expected 2 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let object = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() && b.len() >= 0x68 => b,
        _ => return Err("arg0 must be the 0x68-byte descriptor object".into()),
    };
    let bias = match it.next() {
        Some(Arg::U64(v)) => v as u32,
        _ => return Err("arg1 must be u64".into()),
    };
    let descriptor = u64::from_le_bytes(object[0..8].try_into().unwrap());
    let flags = u32::from_le_bytes(object[8..12].try_into().unwrap());
    let format = object[0x60];
    let ret = ow_tank::texture::descriptor_size_walk(descriptor, flags, format, bias);
    Ok((format!("{:x}", ret as u64), vec![object]))
}

/// The 512-byte table from the unit's `state` block.
fn cmf_table(state: &State) -> Result<&[u8; CMF_TABLE_LEN], String> {
    if !state.loaded() {
        return Err("missing-state-table".into());
    }
    if state.buf.len() < CMF_TABLE_LEN {
        return Err("state-must-hold-512-table-bytes".into());
    }
    state.buf[..CMF_TABLE_LEN]
        .try_into()
        .map_err(|_| "state-table-conversion".into())
}

/// Extract the name bytes from the client's string object. The object stores
/// the characters inline when the capacity is 16 or less; otherwise `+0` holds
/// the data pointer, which the fixture writes as a relative offset (0x20) into
/// the same object buffer. The harness rebases that slot for the oracle; the
/// probe reads the same relative offset from the slot the fixture declared.
fn cmf_name_bytes<'a>(obj: &'a [u8], relocs: &[usize]) -> Result<&'a [u8], String> {
    if obj.len() < CMF_STRING_OBJECT {
        return Err("string-object-too-short".into());
    }
    let size = u32::from_le_bytes(obj[0x10..0x14].try_into().unwrap()) as usize;
    let capacity = u32::from_le_bytes(obj[0x14..0x18].try_into().unwrap()) as usize;
    let start = if capacity > CMF_STRING_INLINE {
        let slot = *relocs.first().ok_or("string-needs-data-reloc")?;
        let end = slot.checked_add(8).ok_or("string-reloc-overflow")?;
        let raw = obj.get(slot..end).ok_or("string-reloc-out-of-range")?;
        u64::from_le_bytes(raw.try_into().unwrap()) as usize
    } else {
        0
    };
    obj.get(start..start + size)
        .ok_or_else(|| "string-out-of-range".into())
}

/// `CmfKey154504` (`0x142e48a20`): `(Header*, u8 out[32], int len)`.
fn cmf_key_154504_entry(args: Vec<Arg>, state: &State) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 3 {
        return Err(format!("expected 3 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let header = match it.next() {
        Some(Arg::Ptr(b, _, None)) => b,
        _ => return Err("arg0 must be a header buffer".into()),
    };
    let out = match it.next() {
        Some(Arg::Ptr(b, _, None)) => b,
        _ => return Err("arg1 must be an output buffer".into()),
    };
    let len = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg2 must be u64".into()),
    };
    if len != 32 {
        return Err("key-length-must-be-32".into());
    }
    if out.len() != 32 {
        return Err("out-must-be-32-bytes".into());
    }
    let table = cmf_table(state)?;
    let key = ow_tank::manifest::key_154504(table);
    Ok(("-".to_string(), vec![header, key.to_vec()]))
}

/// `CmfIv154504` (`0x142e48700`): `(string*, Header*, u8 out[16], int len)`.
fn cmf_iv_154504_entry(args: Vec<Arg>, state: &State) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 4 {
        return Err(format!("expected 4 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let (name_obj, relocs) = match it.next() {
        Some(Arg::Ptr(b, r, None)) => (b, r),
        _ => return Err("arg0 must be a string object".into()),
    };
    let header = match it.next() {
        Some(Arg::Ptr(b, _, None)) => b,
        _ => return Err("arg1 must be a header buffer".into()),
    };
    let out = match it.next() {
        Some(Arg::Ptr(b, _, None)) => b,
        _ => return Err("arg2 must be an output buffer".into()),
    };
    let len = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg3 must be u64".into()),
    };
    if len != 16 {
        return Err("iv-length-must-be-16".into());
    }
    if out.len() != 16 {
        return Err("out-must-be-16-bytes".into());
    }
    if header.len() < CMF_HEADER_LEN {
        return Err("header-must-be-48-bytes".into());
    }
    let name = cmf_name_bytes(&name_obj, &relocs)?;
    let digest: [u8; 20] = {
        use sha1::{Digest, Sha1};
        let mut hasher = Sha1::new();
        hasher.update(name);
        hasher.finalize().into()
    };
    let parsed = ow_tank::manifest::Header {
        build_version: u32::from_le_bytes(header[0..4].try_into().unwrap()),
        data_patch_count: 0,
        entry_patch_count: 0,
        data_count: i32::from_le_bytes(header[0x20..0x24].try_into().unwrap()),
        entry_count: 0,
        magic: 0,
    };
    let table = cmf_table(state)?;
    let iv = ow_tank::manifest::iv_154504(&parsed, &digest, table);
    Ok(("-".to_string(), vec![name_obj, header, iv.to_vec()]))
}

/// The handle object is 64 bytes: `+0` payload pointer, `+8` tag dword.
const BUNDLE_OBJECT_LEN: usize = 0x40;

/// `BundleEntryLookup` (`0x141fd5820`): `(directory*, handle, u64 GUID)`.
///
/// The client's first argument (RCX) is dead, so the fixture puts the payload's
/// guest address there. The handle (RDX) masks to a 64-byte object whose `+0`
/// qword is the payload pointer and whose `+8` dword carries the tag; the GUID
/// is in R8. The probe receives that object followed by the payload as one
/// pointee buffer (R9, also dead) and calls the shipped
/// `ow_tank::bundle::entry_lookup`. The return is the client's guest address
/// (`payload + offset`) or `0`; no host pointer crosses the wire.
fn bundle_lookup_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 4 {
        return Err(format!("expected 4 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let guest_base = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg0 must be u64".into()),
    };
    let handle = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg1 must be u64".into()),
    };
    let guid = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg2 must be u64".into()),
    };
    let region = match it.next() {
        Some(Arg::Ptr(b, _, None)) if b.len() >= BUNDLE_OBJECT_LEN => b,
        _ => return Err("arg3 must be an object-and-payload region".into()),
    };
    let object_tag = u32::from_le_bytes(region[8..12].try_into().unwrap());
    // The region is the handle object followed by the payload. A real bundle
    // payload is at least the 5-byte header, so an object-only region is the
    // client's null payload pointer.
    let payload = if region.len() > BUNDLE_OBJECT_LEN {
        Some(&region[BUNDLE_OBJECT_LEN..])
    } else {
        None
    };
    let ret = ow_tank::bundle::entry_lookup(handle, object_tag, payload, guid, guest_base);
    Ok((format!("{:x}", ret), vec![region]))
}

/// `BundleDirectoryLookup` (`0x141fd5160`): `(directory*, u64* out, u64 GUID)`.
///
/// RCX is the directory object (the client always passes its fixed
/// `DAT_1441b3b88`), RDX the output slot and R8 the target GUID. R9 is dead:
/// the fixture uses it for the structure pool the directory's absolute
/// pointers name. Every buffer is a guest slice of the unit's state, so the
/// probe follows the blob's pointers exactly as the client does. The client
/// returns `param_2`, the output pointer, and writes the handle (or `-1`) to
/// `*param_2`; the probe returns the same guest address and the out slot's
/// bytes carry the handle.
///
/// `is_loaded` models `FUN_141fcbf40`: the fixture puts every group's bundle
/// GUID at 0, the client's unconditional "loaded" answer. Any other GUID
/// would need the live content-database set, which this unit cannot model.
fn bundle_directory_lookup_entry(
    args: Vec<Arg>,
    state: &mut State,
) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 4 {
        return Err(format!("expected 4 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let (dir_mem, dir_relocs, dir_guest) = match it.next() {
        Some(Arg::Ptr(b, r, Some(g))) => (b, r, g),
        _ => return Err("arg0 must be a guest directory buffer".into()),
    };
    let (out_mem, out_guest) = match it.next() {
        Some(Arg::Ptr(b, r, Some(g))) if r.is_empty() && b.len() == 8 => (b, g),
        _ => return Err("arg1 must be the 8-byte out slot".into()),
    };
    let guid = match it.next() {
        Some(Arg::U64(v)) => v,
        _ => return Err("arg2 must be u64".into()),
    };
    let (pool_mem, pool_relocs, pool_guest) = match it.next() {
        Some(Arg::Ptr(b, r, Some(g))) => (b, r, g),
        _ => return Err("arg3 must be a guest structure pool".into()),
    };
    if !state.loaded() {
        return Err("state-not-loaded".into());
    }
    state.reset();
    guest_write(state, dir_guest, &dir_mem, &dir_relocs)?;
    guest_write(state, pool_guest, &pool_mem, &pool_relocs)?;
    guest_write(state, out_guest, &out_mem, &[])?;
    let handle = {
        let directory = ow_tank::bundle::Directory::new(&state.buf, state.base, dir_guest);
        ow_tank::bundle::directory_lookup(&directory, guid, |bundle| bundle == 0)
    };
    let off = state.off(out_guest).ok_or("out-outside-state")?;
    state.buf[off..off + 8].copy_from_slice(&handle.to_le_bytes());
    let mut out_after = out_mem;
    out_after.copy_from_slice(&handle.to_le_bytes());
    Ok((
        format!("{:x}", out_guest),
        vec![dir_mem, out_after, pool_mem],
    ))
}

/// `FUN_141b52ca0`: `uint (byte *data, uint len, uint initial)`.
///
/// The client's table CRC32, called by its lazy string wrapper with initial 0.
/// The pointee is the fixture buffer; `len` bounds the bytes the client reads.
fn crc32_seeded_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    if args.len() != 3 {
        return Err(format!("expected 3 args, got {}", args.len()));
    }
    let mut it = args.into_iter();
    let data = match it.next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
        _ => return Err("arg0 must be a plain arena buffer".into()),
    };
    let len = match it.next() {
        // The client's parameter is `uint param_2`, so the wire u64 is read at
        // the client's width. The unit's fixture only supplies 0..64 bytes, so
        // lengths above 64 (and the u32 truncation above 2^32) are untested.
        Some(Arg::U64(v)) => v as u32 as usize,
        _ => return Err("arg1 must be u64".into()),
    };
    let initial = match it.next() {
        Some(Arg::U64(v)) => v as u32,
        _ => return Err("arg2 must be u64".into()),
    };
    let bytes = data.get(..len).ok_or("len-past-pointee")?;
    let ret = ow_tank::material::crc32_seeded(bytes, initial);
    Ok((format!("{:x}", ret as u64), vec![data]))
}

fn handle_call(
    entry: &str,
    args: Vec<Arg>,
    state: &mut State,
) -> Result<(String, Vec<Vec<u8>>), String> {
    match entry {
        // crates/ow-tank/src/mapguid.rs
        "chunk_guid" => chunk_guid_entry(args),
        // crates/ow-tank/src/manifest.rs
        "cmf_key_154504" => cmf_key_154504_entry(args, state),
        "cmf_iv_154504" => cmf_iv_154504_entry(args, state),
        // crates/ow-tank/src/bundle.rs
        "bundle_lookup" => bundle_lookup_entry(args),
        "bundle_directory_lookup" => bundle_directory_lookup_entry(args, state),
        // crates/ow-tank/src/guid.rs
        "payload_guid" => payload_guid_entry(args),
        // crates/ow-tank/src/material.rs
        "crc32_seeded" => crc32_seeded_entry(args),
        // crates/ow-tank/src/texture.rs
        "texture_request_count" => texture_request_count_entry(args),
        "texture_count_bits_shifted" => texture_count_bits_shifted_entry(args),
        "texture_format_family_bpp" => texture_format_family_bpp_entry(args),
        "texture_descriptor" => texture_descriptor_entry(args),
        "texture_descriptor_size_walk" => texture_descriptor_size_walk_entry(args),
        other => Err(format!("unknown-entry-{}", other)),
    }
}

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    // The harness sends the unit's `state` snapshot before the first call.
    // `chunk_guid` reads its table through the pointee argument, so the bytes
    // are only echoed back on the state channel; the bundle-directory entry
    // follows the blob's guest pointers through the same buffer.
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
