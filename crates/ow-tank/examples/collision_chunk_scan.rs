//! What the practice range's `MAP_CHUNK_COLLISION` payload actually holds.
//!
//! The container is read (`ow_tank::chunkkind`): a 0x38 header naming a sector
//! array and a tail array. The sector array's four runs are tiny - a few dozen
//! vertices - while the payload is 21.8 MB, so the body between the header and
//! the sector array is not described by it. This prints what that body looks
//! like: the first bytes, how many 12-byte triples read as map-scale
//! coordinates, and how the last sector's runs read.
//!
//! Read-only over the install; skipped when no install is present.
//!
//! ```text
//! cargo run --release -p ow-tank --example collision_chunk_scan
//! ```

use ow_tank::chunkkind::Collision;
use ow_tank::keytable::Sources;
use ow_tank::{GameData, ManifestSelector};

const PRACTICE_COLLISION: u64 = 0x0DD0_0011_0000_0688;

/// Where a vertical line through `(x, z)` meets a triangle.
fn column_hit(p: &[[f32; 3]], at: [f32; 2]) -> Option<f32> {
    let (x0, z0) = (p[0][0], p[0][2]);
    let (x1, z1) = (p[1][0], p[1][2]);
    let (x2, z2) = (p[2][0], p[2][2]);
    let det = (z1 - z2) * (x0 - x2) + (x2 - x1) * (z0 - z2);
    if det.abs() < 1e-12 {
        return None;
    }
    let a = ((z1 - z2) * (at[0] - x2) + (x2 - x1) * (at[1] - z2)) / det;
    let b = ((z2 - z0) * (at[0] - x2) + (x0 - x2) * (at[1] - z2)) / det;
    let c = 1.0 - a - b;
    if a < 0.0 || b < 0.0 || c < 0.0 {
        return None;
    }
    Some(a * p[0][1] + b * p[1][1] + c * p[2][1])
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(root) = ow_casc::install::detect_root() else {
        println!("no install present");
        return Ok(());
    };
    let sources = Sources::discover(std::slice::from_ref(&root));
    let data = GameData::open(&root, &ManifestSelector::default(), &sources)?;
    let bytes = data.open_asset(PRACTICE_COLLISION)?;
    println!("payload {} bytes", bytes.len());
    print!("first 64:");
    for b in &bytes[..64] {
        print!(" {b:02x}");
    }
    println!();

    let collision = Collision::parse(&bytes)?;
    let h = collision.header;
    println!(
        "header: count_a {} count_b {} rva_10 {:#x} rva_18 {:#x} rva_20 {:#x} rva_28 {:#x} unknown_30 {:#x}",
        h.count_a, h.count_b, h.rva_10, h.rva_18, h.rva_20, h.rva_28, h.unknown_30
    );
    let body_end = h.rva_20 as usize;
    println!("body: 0..{body_end} ({body_end} bytes before the sector array)");

    // Does the body read as a triangle soup? Triples of f32 in the map's bounds
    // are coordinates; u32s below 1e6 are indices.
    let mut floats = 0usize;
    let mut in_bounds = 0usize;
    let mut small_u32 = 0usize;
    let mut total_u32 = 0usize;
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for triple in bytes[..body_end].chunks_exact(12) {
        let f =
            |o: usize| f32::from_le_bytes([triple[o], triple[o + 1], triple[o + 2], triple[o + 3]]);
        let v = [f(0), f(4), f(8)];
        if v.iter().all(|x| x.is_finite() && x.abs() < 1e5) {
            floats += 1;
            for lane in 0..3 {
                low[lane] = low[lane].min(v[lane]);
                high[lane] = high[lane].max(v[lane]);
            }
            if (-200.0..400.0).contains(&v[0])
                && (-100.0..200.0).contains(&v[1])
                && (-300.0..300.0).contains(&v[2])
            {
                in_bounds += 1;
            }
        }
    }
    for word in bytes[..body_end].chunks_exact(4) {
        total_u32 += 1;
        let v = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        if v < 1_000_000 {
            small_u32 += 1;
        }
    }
    let triples = body_end / 12;
    println!(
        "body as 12-byte triples: {triples} triples, {floats} fully finite/small, {in_bounds} in the map's bounds"
    );
    if floats > 0 {
        println!("  triple range low {low:?} high {high:?}");
    }
    println!(
        "body as u32: {total_u32} words, {small_u32} below 1e6 ({:.1}%)",
        small_u32 as f32 * 100.0 / total_u32.max(1) as f32
    );

    // The sector array's own element runs, first and last sector.
    let sectors: Vec<_> = collision.sectors().collect();
    for sector in [&sectors[0], &sectors[sectors.len() - 1]] {
        println!(
            "sector {}: pointers {:x?} counts {:?} runs {:x?}",
            sector.index,
            sector.pointers,
            sector.counts(),
            sector.runs()
        );
        let vertices = sector.vertices(&bytes);
        if let (Some(first), Some(last)) = (vertices.first(), vertices.last()) {
            println!(
                "  vertices {}: first {first:?} last {last:?}",
                vertices.len()
            );
        }
    }
    // Is the body a Vec4-corner triangle soup? Every 16-byte quad's fourth lane
    // should be zero, and every third corner should close a triangle.
    let mut w_zero = 0usize;
    let mut quads = 0usize;
    let mut tri_ok = 0usize;
    let mut tris = 0usize;
    for (i, quad) in bytes[..body_end].chunks_exact(16).enumerate() {
        let f = |o: usize| f32::from_le_bytes([quad[o], quad[o + 1], quad[o + 2], quad[o + 3]]);
        quads += 1;
        if f(12) == 0.0 {
            w_zero += 1;
        }
        let _ = i;
    }
    for tri in bytes[..body_end].chunks_exact(48) {
        let f = |base: usize, o: usize| {
            f32::from_le_bytes([
                tri[base + o],
                tri[base + o + 1],
                tri[base + o + 2],
                tri[base + o + 3],
            ])
        };
        tris += 1;
        if f(12, 0) == 0.0 && f(28, 0) == 0.0 && f(44, 0) == 0.0 {
            tri_ok += 1;
        }
    }
    println!(
        "16-byte quads: {quads} ({:.0}% with a zero fourth lane); 48-byte groups: {tris} ({:.0}% with all three fourth lanes zero)",
        w_zero as f32 * 100.0 / quads.max(1) as f32,
        tri_ok as f32 * 100.0 / tris.max(1) as f32
    );
    // The first six Vec4 corners at 0x3c, as the soup would read them.
    println!("corners from 0x3c:");
    for corner in 0..6 {
        let base = 0x3c + corner * 16;
        let f = |o: usize| {
            f32::from_le_bytes([
                bytes[base + o],
                bytes[base + o + 1],
                bytes[base + o + 2],
                bytes[base + o + 3],
            ])
        };
        println!(
            "  corner {corner}: ({:+.4}, {:+.4}, {:+.4}, {:+.4})",
            f(0),
            f(4),
            f(8),
            f(12)
        );
    }
    // The tail's three pointers and three counts split the body exactly:
    // 0x40 with 203,999 elements, 0x31CE30 with 236,957, 0xA581D0 with 343,474,
    // and the last two are 32-byte strides to the sector array.
    let f32_at = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let i32_at = |at: usize| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    println!("body split check:");
    for (start, count) in [
        (0x40usize, 203_999usize),
        (0x31CE30, 236_957),
        (0xA581D0, 343_474),
    ] {
        let next = if start == 0x40 {
            0x31CE30
        } else if start == 0x31CE30 {
            0xA581D0
        } else {
            0x14D3810
        };
        println!(
            "  {start:#x}: {count} element(s), stride {:.3}",
            (next - start) as f64 / count as f64
        );
    }
    for (name, start) in [("0x31ce30", 0x31CE30usize), ("0xa581d0", 0xA581D0)] {
        println!("{name} as eight floats:");
        for i in 0..4 {
            let at = start + i * 0x20;
            let mut row = String::new();
            for k in 0..8 {
                let v = f32_at(at + k * 4);
                row.push_str(&format!(" {v:+.3}"));
            }
            println!("  {i}:{row}");
        }
        println!("{name} as eight u32:");
        for i in 0..2 {
            let at = start + i * 0x20;
            let mut row = String::new();
            for k in 0..8 {
                let o = at + k * 4;
                let v = u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
                row.push_str(&format!(" {v:>10}"));
            }
            println!("  {i}:{row}");
        }
    }
    println!("first vertex records at 0x40:");
    for i in 0..4 {
        let at = 0x40 + i * 16;
        println!(
            "  ({:+.3}, {:+.3}, {:+.3}, {:+.3})",
            f32_at(at),
            f32_at(at + 4),
            f32_at(at + 8),
            f32_at(at + 12)
        );
    }
    println!("first 0x20 records at 0x31CE30 as three i32 indices + 8 bytes:");
    for i in 0..4 {
        let at = 0x31CE30 + i * 0x20;
        println!(
            "  [{}, {}, {}] ids {:04x?} mask {:04x}",
            i32_at(at),
            i32_at(at + 4),
            i32_at(at + 8),
            u16::from_le_bytes([bytes[at + 0x18], bytes[at + 0x19]]),
            u16::from_le_bytes([bytes[at + 0x1c], bytes[at + 0x1d]])
        );
    }
    println!("first 0x20 records at 0xA581D0 as a node box + child/mask/axis/leaf:");
    for i in 0..4 {
        let at = 0xA581D0 + i * 0x20;
        println!(
            "  min ({:+.3}, {:+.3}, {:+.3}, {:+.3}) max ({:+.3}, {:+.3}, {:+.3}) child {} mask {:#06x} axis {} leaf {}",
            f32_at(at),
            f32_at(at + 4),
            f32_at(at + 8),
            f32_at(at + 12),
            f32_at(at + 16),
            f32_at(at + 20),
            f32_at(at + 24),
            i32_at(at + 0x18),
            u16::from_le_bytes([bytes[at + 0x1c], bytes[at + 0x1d]]),
            bytes[at + 0x1e],
            bytes[at + 0x1f]
        );
    }

    // The tail array: 0x38 bytes, one element. Its words may name the body.
    let tails: Vec<_> = collision.tails().collect();
    println!("tail elements: {}", tails.len());
    if let Some(tail) = tails.first() {
        println!("tail raw: {:02x?}", tail.raw);
        for (i, word) in tail.raw.chunks_exact(4).enumerate() {
            let v = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
            println!("  [{i:02}] u32 {v:#010x} ({v}) f {}", f32::from_bits(v));
        }
    }

    // Compare the body's readings against the rendered surfaces at a column:
    // model 0x0334b's roof sits at 10.096/10.373, 0x078c's floors at 30.411 and
    // 31.411, 0x03401's at 30.778/40.123/40.291 (the render column of
    // `ow-client --walk-column 52.54 10.98 --render-column`).
    for (name, base, stride) in [("vec4 @0x40", 0x40usize, 16usize), ("vec3 @0x40", 0x40, 12)] {
        let mut heights = Vec::new();
        let count = (body_end - base) / stride;
        let corners: Vec<[f32; 3]> = (0..count)
            .map(|i| {
                let at = base + i * stride;
                let f = |o: usize| {
                    f32::from_le_bytes([
                        bytes[at + o],
                        bytes[at + o + 1],
                        bytes[at + o + 2],
                        bytes[at + o + 3],
                    ])
                };
                [f(0), f(4), f(8)]
            })
            .collect();
        for tri in corners.chunks_exact(3) {
            if let Some(y) = column_hit(tri, [54.27, 12.7]) {
                heights.push(y);
            }
        }
        heights.sort_by(f32::total_cmp);
        heights.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
        println!(
            "{name}: {} corner(s), {} column surface(s): {:.3?}",
            corners.len(),
            heights.len(),
            &heights[..heights.len().min(12)]
        );
    }

    // The body's own start, read as a mesh-shaped header: an i32 count, then
    // pointers, would be visible as small integers.
    println!("body words 0..24 (u32):");
    for (i, word) in bytes[..96].chunks_exact(4).enumerate() {
        let v = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        let f = f32::from_bits(v);
        print!("  [{i:02}] {v:#010x} f {f:+.4}");
        if i % 2 == 1 {
            println!();
        }
    }
    Ok(())
}
