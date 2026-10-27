//! Sample the map's ground height along a straight path.
//!
//! Built to check a traced descent against the geometry it crossed. The default
//! scene is the resolved practice range; `--chunks-from` is an explicit chunk
//! diagnostic, not a map. `ground_at`'s fallback floor (2.0) shows up as a
//! constant 2.000 and means the point is outside the loaded chunks.
//!
//! Usage:
//!     cargo run -p ow-scene --bin ground_probe -- --from X,Z --to X,Z
//!         [--steps N] [--chunks-from <guid list>] [--no-map]

use glam::Vec3;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |n: &str| -> Option<String> {
        args.iter()
            .position(|a| a == n)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let pair = |s: String| -> Option<(f32, f32)> {
        let mut it = s.split(',');
        Some((
            it.next()?.trim().parse().ok()?,
            it.next()?.trim().parse().ok()?,
        ))
    };
    let (Some(from), Some(to)) = (flag("--from").and_then(pair), flag("--to").and_then(pair))
    else {
        eprintln!("usage: ground_probe --from X,Z --to X,Z [--steps N] [--chunks-from list]");
        return std::process::ExitCode::from(2);
    };
    let steps: usize = flag("--steps").and_then(|v| v.parse().ok()).unwrap_or(40);
    let scene = match ow_scene::build(&args) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(1);
        }
    };
    let collider = ow_scene::Collider::build(&scene);
    println!(
        "scene: {} triangles, {} collider cells",
        scene.triangle_count(),
        collider.cell_count()
    );
    let mut prev: Option<Vec3> = None;
    for k in 0..=steps {
        let t = k as f32 / steps as f32;
        let x = from.0 + (to.0 - from.0) * t;
        let z = from.1 + (to.1 - from.1) * t;
        let y = collider.ground_at(x, z, 100.0);
        let mut grade = 0.0f32;
        if let Some(p) = prev {
            let d = ((x - p.x).powi(2) + (z - p.z).powi(2)).sqrt();
            if d > 1e-3 {
                grade = (y - p.y) / d;
            }
        }
        println!(
            "  t={:.2}  ({:8.2},{:8.2})  ground y {:8.3}   grade {:+.3} ({:+.1} deg)",
            t,
            x,
            z,
            y,
            grade,
            grade.atan().to_degrees()
        );
        prev = Some(Vec3::new(x, y, z));
    }
    std::process::ExitCode::SUCCESS
}
