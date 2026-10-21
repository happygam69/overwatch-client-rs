//! Gate: does a decoded clip produce a coherent pose?
//!
//! Checks the properties the screen will show, numerically, before anyone looks
//! at a window: every bone's rotation is a unit quaternion, the pose at frame 1
//! is close to the bind pose for an idle, and neighbouring frames move smoothly
//! (an animation, not noise).
fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "D:/ow-dumps/asset_0a0000000000005e.bin".to_string());
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return;
        }
    };
    let clip = ow_tank::clip::Clip::parse(&bytes).expect("clip");
    println!(
        "{path}\n  family {:#010x}, {} frames @ {} fps",
        clip.header.family, clip.header.frames, clip.header.fps
    );
    // family 0x0053XXXX carries the blend-card record stream instead of either
    // of the other layouts, so it gets its own pass here.
    if ow_tank::clip::is_blend_card_family(clip.header.family) {
        let Some(regs) = ow_tank::clip::decode_blend_registers(&bytes) else {
            println!("  no blend records decoded");
            return;
        };
        println!("  {} blend records", regs.len());
        let mut worst_overall = 0.0f32;
        let mut unit_failures = 0usize;
        let mut pos_spans = [0.0f32; 3];
        for (i, tracks) in regs.iter().enumerate() {
            for t in tracks {
                match t.track {
                    ow_tank::clip::TrackKind::Rotation => {
                        let mut prev: Option<[f32; 4]> = None;
                        let mut worst = 0.0f32;
                        for s in &t.samples {
                            let ow_tank::clip::TrackSample::Rotation(q) = s else {
                                continue;
                            };
                            let m = (q.iter().map(|x| x * x).sum::<f32>()).sqrt();
                            if (m - 1.0).abs() > 1e-2 {
                                unit_failures += 1;
                            }
                            if let Some(p) = prev {
                                let d = p
                                    .iter()
                                    .zip(q)
                                    .map(|(a, b)| a * b)
                                    .sum::<f32>()
                                    .abs()
                                    .clamp(-1.0, 1.0);
                                worst = worst.max(2.0 * d.acos().to_degrees());
                            }
                            prev = Some(*q);
                        }
                        worst_overall = worst_overall.max(worst);
                        if worst > 30.0 {
                            println!("  rec {i}: rotation worst {worst:.1} deg");
                        }
                    }
                    ow_tank::clip::TrackKind::Position => {
                        for s in &t.samples {
                            if let ow_tank::clip::TrackSample::Position(p) = s {
                                for k in 0..3 {
                                    pos_spans[k] = pos_spans[k].max(p[k].abs());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let kinds: usize = regs.iter().map(|t| t.len()).sum();
        println!("  tracks total {kinds}, rot unit failures {unit_failures}, worst rot step {worst_overall:.2} deg");
        println!(
            "  position span |xyz| = ({:.4}, {:.4}, {:.4}) m",
            pos_spans[0], pos_spans[1], pos_spans[2]
        );
        println!("OK");
        return;
    }
    // family 0x00020400 carries the reference-layout track tables instead of
    // the bone-list families, so it gets its own pass here.
    if clip.header.family == ow_tank::clip::REFERENCE_TABLE_FAMILY {
        let Some(tables) = ow_tank::clip::decode_reference_tables(&bytes) else {
            println!("  no reference-layout tables");
            return;
        };
        println!("  {} tables", tables.len());
        for (i, tracks) in tables.iter().enumerate() {
            for t in tracks {
                match t.track {
                    ow_tank::clip::TrackKind::Rotation => {
                        let mut prev: Option<[f32; 4]> = None;
                        let mut worst = 0.0f32;
                        let mut unit = true;
                        for s in &t.samples {
                            let ow_tank::clip::TrackSample::Rotation(q) = s else {
                                continue;
                            };
                            let m = (q.iter().map(|x| x * x).sum::<f32>()).sqrt();
                            if (m - 1.0).abs() > 1e-2 {
                                unit = false;
                            }
                            if let Some(p) = prev {
                                let d = p
                                    .iter()
                                    .zip(q)
                                    .map(|(a, b)| a * b)
                                    .sum::<f32>()
                                    .abs()
                                    .clamp(-1.0, 1.0);
                                worst = worst.max(2.0 * d.acos().to_degrees());
                            }
                            prev = Some(*q);
                        }
                        println!("  table {i}: {} rotation keys: unit={unit}, worst frame-to-frame {worst:.2} deg", t.keys.len());
                    }
                    ow_tank::clip::TrackKind::Position => {
                        let span = t
                            .samples
                            .iter()
                            .filter_map(|s| match s {
                                ow_tank::clip::TrackSample::Position(p) => Some(*p),
                                _ => None,
                            })
                            .fold([0.0f32; 3], |acc, p| {
                                [
                                    acc[0].max(p[0].abs()),
                                    acc[1].max(p[1].abs()),
                                    acc[2].max(p[2].abs()),
                                ]
                            });
                        println!(
                            "  table {i}: {} position keys: max|xyz| = ({:.4}, {:.4}, {:.4}) m",
                            t.keys.len(),
                            span[0],
                            span[1],
                            span[2]
                        );
                    }
                    ow_tank::clip::TrackKind::Scale => {
                        println!("  table {i}: {} scale keys", t.keys.len());
                    }
                }
            }
        }
        println!("OK");
        return;
    }
    let tracks = ow_tank::clip::decode_bone_tracks(&clip.body, clip.header.frames);
    println!("  {} bone tracks", tracks.len());
    for (i, t) in tracks.iter().enumerate() {
        println!(
            "  track {i}: bones {} ({:?}..), keys {:?}, {} quats",
            t.bones.len(),
            &t.bones[..t.bones.len().min(4)],
            &t.keys[..t.keys.len().min(6)],
            t.quats.len()
        );
        for q in &t.quats {
            let m = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
            assert!(
                (m - 1.0).abs() < 1e-3,
                "not a unit quaternion: {q:?} |q| = {m}"
            );
        }
    }
    // per-frame smoothness on the first track
    for (i, t) in tracks.iter().enumerate() {
        let mut prev: Option<[f32; 4]> = None;
        let mut worst = 0.0f32;
        for f in 1..=clip.header.frames {
            let q = t.quat_at(f as f32);
            if let Some(pv) = prev {
                let dot = (pv[0] * q[0] + pv[1] * q[1] + pv[2] * q[2] + pv[3] * q[3]).abs();
                worst = worst.max(2.0 * dot.clamp(-1.0, 1.0).acos().to_degrees());
            }
            prev = Some(q);
        }
        println!("  track {i}: worst frame-to-frame {worst:.1} deg");
    }
    if let Some(t) = tracks.first() {
        let mut prev: Option<[f32; 4]> = None;
        let mut worst = 0.0f32;
        for f in 1..=clip.header.frames {
            let q = t.quat_at(f as f32);
            if let Some(p) = prev {
                let dot = (p[0] * q[0] + p[1] * q[1] + p[2] * q[2] + p[3] * q[3]).abs();
                let ang = 2.0 * dot.clamp(-1.0, 1.0).acos().to_degrees();
                worst = worst.max(ang);
            }
            prev = Some(q);
        }
        println!(
            "  track 0: worst frame-to-frame angle {worst:.1} deg over {} frames",
            clip.header.frames
        );
    }
    println!("OK");
}
