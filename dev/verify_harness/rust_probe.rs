// Tier-2 candidate probe: a real Rust implementation of the verified
// pilot functions, speaking the harness's line protocol on stdin/stdout.
//
// Built with plain `rustc` (no workspace, no crates) so it stays a probe
// and not a project dependency. candidate_rust.py owns the build and the
// protocol; this file owns only behaviour.
//
// Protocol (one request per line, one response per line):
//   -> call <entry> <nargs> [ptr <hex> <relocs>|f32 <8hex>|u64 <16hex>]*
//   <- ok <retbits-hex|-> <outhex>...
//   <- err <message-with-no-spaces>
// relocs is '-' or comma-separated decimal byte offsets of u64 relative
// pointers inside the pointee (same contract as ArgSpec.relocs).
//
// Entries:
//   anim_time_wrap        - the verified semantics (SSE minss/maxss clamp)
//   anim_time_wrap_broken - the original Ghidra bug (IEEE sequential ifs);
//                           the selftest requires the harness to catch it

use std::io::{self, BufRead, Write};

// Clean-room rewrites live in rust_rewrites/ so each function is a
// self-contained file the harness can point at. include! pastes the
// source in at compile time; there is no module path to configure.
include!("rust_rewrites/OrthonormalizeCameraBasis.rs");

fn minss(a: f32, b: f32) -> f32 {
    // SSE MINSS: unordered or a >= b -> second source. C/Rust `<` is
    // false on NaN, so `if a < b { a } else { b }` matches exactly.
    if a < b {
        a
    } else {
        b }
}

fn maxss(a: f32, b: f32) -> f32 {
    if a > b {
        a
    } else {
        b
    }
}

fn cvttss2si(x: f32) -> i32 {
    // x86 cvttss2si: truncate toward zero; NaN/inf/out-of-range ->
    // 0x80000000 (integer indeterminate). Rust's `as i32` saturates,
    // which differs, so the invalid cases are modelled explicitly.
    if x.is_nan() || x >= 2147483648.0 || x < -2147483648.0 {
        i32::MIN
    } else {
        x as i32
    }
}

/// range: float[4] = [min, max/period, slot2, flag-as-bits]; t: sample time.
/// Flag bit 0 selects modular wrap; otherwise clamp via minss/maxss.
fn anim_time_wrap(range: *const f32, t: f32) -> f32 {
    unsafe {
        let min = *range;
        let max = *range.add(1);
        let flag = (*range.add(3)).to_bits();
        if flag & 1 != 0 {
            let n = cvttss2si(t / max);
            t - (n as f32) * max
        } else {
            maxss(minss(t, max), min)
        }
    }
}

/// Projection descriptor field loads, byte-offset exact (same offsets the
/// decompile uses). Returns NaN if the load is misaligned - the harness
/// always hands a suitably sized buffer.
#[inline]
unsafe fn desc_f32(desc: *const u8, off: usize) -> f32 {
    f32::from_le_bytes([
        *desc.add(off),
        *desc.add(off + 1),
        *desc.add(off + 2),
        *desc.add(off + 3),
    ])
}

/// BuildPerspectiveMatrix (0x142075a70): leaf. Reads a projection
/// descriptor, writes a 4x4 matrix (16 floats) to `out`. `reverse` != 0
/// selects the reverse-Z style near/far mapping (negated terms). The
/// float op order matches the decompile exactly so bit patterns agree.
/// Returns `out` (ignored by units that declare `returns: void`).
unsafe fn build_perspective_matrix(desc: *const u8, out: *mut f32, reverse: u64) -> *mut f32 {
    let f05c = desc_f32(desc, 0x5c);
    let f060 = desc_f32(desc, 0x60);
    let f064 = desc_f32(desc, 100); // *(param_1 + 100) == +0x64
    let f068 = desc_f32(desc, 0x68);
    let f06c = desc_f32(desc, 0x6c);
    let f070 = desc_f32(desc, 0x70);
    let f074 = desc_f32(desc, 0x74);
    let f078 = desc_f32(desc, 0x78);
    let f07c = desc_f32(desc, 0x7c);
    let f080 = desc_f32(desc, 0x80);
    let f084 = desc_f32(desc, 0x84);
    let f088 = desc_f32(desc, 0x88);
    let f08c = desc_f32(desc, 0x8c);
    let f090 = desc_f32(desc, 0x90);
    let f094 = desc_f32(desc, 0x94);
    let f098 = desc_f32(desc, 0x98);
    let f04c = desc_f32(desc, 0x4c);
    let f050 = desc_f32(desc, 0x50);
    let f054 = desc_f32(desc, 0x54);
    let f058 = desc_f32(desc, 0x58);

    let a = f05c;
    let b = f060 - a;
    let c = f094 - f08c;
    let d = f090 - f098;
    let inv_b = 1.0f32 / b;
    let e = f054;

    *out.add(1) = 0.0;
    *out.add(2) = 0.0;
    *out.add(3) = 0.0;
    *out.add(4) = 0.0;
    *out = ((a + a) / c) * e;
    let f = f058;
    *out.add(6) = 0.0;
    *out.add(7) = 0.0;
    *out.add(5) = ((a + a) / d) * f;

    let mut m20 = ((f054 - 1.0) - f04c) - (f08c + f094) / c;
    *out.add(8) = m20;
    let mut m21 = (f050 - (f058 - 1.0)) - (f090 + f098) / d;
    *out.add(9) = m21;

    let m22 = if reverse == 0 {
        f060 / b
    } else {
        -(inv_b * a)
    };
    *out.add(10) = m22;
    *out.add(11) = 1.0;
    *out.add(12) = 0.0;
    *out.add(13) = 0.0;

    let mut f060b = f060;
    if reverse == 0 {
        f060b = -f060;
    }
    *out.add(15) = 0.0;
    *out.add(14) = f060b * a * inv_b;

    m20 = f064 / ((f084 - f07c) * f074) + m20;
    *out.add(8) = m20;
    m21 = f068 / ((f088 - f080) * f078) + m21;
    *out.add(9) = m21;

    *out.add(8) = (f06c + f06c) / c + m20;
    *out.add(9) = (f070 + f070) / d + m21;

    out
}

/// InitProjectionDescriptor (0x142075230): leaf. Seeds the descriptor's
/// default scales to 1.0f and clears the near/far/skew slots. Returns the
/// same pointer (units declare `returns: void` so the pointer value is
/// not compared across arena vs host).
unsafe fn init_projection_descriptor(desc: *mut u8) -> *mut u8 {
    // Matches the decompile's dword/qword stores at the same offsets.
    for off in [0x40u32, 0x4c, 0x5c, 0x64, 0x6c] {
        // +0x40 is a dword 0; +0x4c/+0x5c/+0x64/+0x6c are qword 0.
        let n = if off == 0x40 { 4 } else { 8 };
        for i in 0..n {
            *desc.add((off + i) as usize) = 0;
        }
    }
    for off in [0x54usize, 0x58, 0x74, 0x78] {
        let bits = 1.0f32.to_le_bytes();
        for i in 0..4 {
            *desc.add(off + i) = bits[i];
        }
    }
    desc
}

/// The decompile as Ghidra first emitted it: IEEE-ordered sequential ifs
/// for the clamp (propagates NaN instead of falling to the bound). Kept
/// as a known-bad entry so the selftest can require its detection.
fn anim_time_wrap_broken(range: *const f32, t: f32) -> f32 {
    unsafe {
        let min = *range;
        let max = *range.add(1);
        let flag = (*range.add(3)).to_bits();
        if flag & 1 != 0 {
            let n = cvttss2si(t / max);
            t - (n as f32) * max
        } else {
            let mut t2 = t;
            if max <= t2 {
                t2 = max;
            }
            if t2 <= min {
                t2 = min;
            }
            t2
        }
    }
}

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

enum Arg {
    Ptr(Vec<u8>, Vec<usize>),
    F32(u32),
    U64(u64),
}

fn handle_call(entry: &str, args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    match entry {
        "anim_time_wrap" | "anim_time_wrap_broken" => {
            if args.len() != 2 {
                return Err(format!("expected 2 args, got {}", args.len()));
            }
            let (mut buf, relocs) = match &args[0] {
                Arg::Ptr(b, r) => (b.clone(), r.clone()),
                _ => return Err("arg0 must be ptr".into()),
            };
            let t = match &args[1] {
                Arg::F32(bits) => f32::from_bits(*bits),
                _ => return Err("arg1 must be f32".into()),
            };
            // Rebase relative pointers into this buffer, same contract as
            // the C candidate's wrapper and the oracle's arena write.
            let base = buf.as_mut_ptr() as u64;
            for off in relocs {
                if off + 8 > buf.len() {
                    return Err(format!("reloc {} out of range", off));
                }
                let rel = u64::from_le_bytes(buf[off..off + 8].try_into().unwrap());
                buf[off..off + 8].copy_from_slice(&(base + rel).to_le_bytes());
            }
            let f: fn(*const f32, f32) -> f32 = if entry == "anim_time_wrap" {
                anim_time_wrap
            } else {
                anim_time_wrap_broken
            };
            let r = f(buf.as_ptr() as *const f32, t);
            Ok((format!("{:08x}", r.to_bits()), vec![buf]))
        }
        "build_perspective_matrix" => {
            if args.len() != 3 {
                return Err(format!("expected 3 args, got {}", args.len()));
            }
            let desc = match &args[0] {
                Arg::Ptr(b, r) if r.is_empty() => b.clone(),
                Arg::Ptr(_, r) if !r.is_empty() => return Err("desc has relocs".into()),
                _ => return Err("arg0 must be ptr".into()),
            };
            let mut out = match &args[1] {
                Arg::Ptr(b, r) if r.is_empty() => b.clone(),
                Arg::Ptr(_, r) if !r.is_empty() => return Err("out has relocs".into()),
                _ => return Err("arg1 must be ptr".into()),
            };
            if out.len() < 64 {
                out.resize(64, 0);
            }
            let reverse = match &args[2] {
                Arg::U64(v) => *v,
                _ => return Err("arg2 must be u64".into()),
            };
            unsafe {
                build_perspective_matrix(desc.as_ptr(), out.as_mut_ptr() as *mut f32, reverse);
            }
            // void return: ret bits unused; out is the memory channel.
            Ok(("-".to_string(), vec![desc, out]))
        }
        "init_projection_descriptor" => {
            if args.len() != 1 {
                return Err(format!("expected 1 arg, got {}", args.len()));
            }
            let mut desc = match &args[0] {
                Arg::Ptr(b, r) if r.is_empty() => b.clone(),
                Arg::Ptr(_, r) if !r.is_empty() => return Err("desc has relocs".into()),
                _ => return Err("arg0 must be ptr".into()),
            };
            unsafe {
                init_projection_descriptor(desc.as_mut_ptr());
            }
            Ok(("-".to_string(), vec![desc]))
        }
        "orthonormalize_camera_basis" => {
            if args.len() != 1 {
                return Err(format!("expected 1 arg, got {}", args.len()));
            }
            let mut desc = match &args[0] {
                Arg::Ptr(b, r) if r.is_empty() => b.clone(),
                Arg::Ptr(_, r) if !r.is_empty() => return Err("desc has relocs".into()),
                _ => return Err("arg0 must be ptr".into()),
            };
            orthonormalize_camera_basis(&mut desc);
            Ok(("-".to_string(), vec![desc]))
        }
        other => Err(format!("unknown entry {}", other)),
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
                    if i + 1 >= toks.len() {
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
                    args.push(Arg::Ptr(bytes, relocs));
                    i += 2;
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
