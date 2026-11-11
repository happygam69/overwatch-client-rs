//! The walker's behaviour, measured against the install.
//!
//! `ow-client` is a binary crate, so this runs the built binary and reads the
//! numbers it prints. The install is needed: without it the binary cannot
//! build the range's collision world, and the test skips (the project's rule
//! for the install-reading tests).
//!
//! ```text
//! cargo test --release -p ow-client --test walk_real -- --nocapture
//! ```
//!
//! What is pinned here:
//!
//! - the walk sweeps the **client's own shape**, the vertical capsule
//!   (`radius 0.5`, height `1.5`, the install's common hero), and the walker's
//!   position is the capsule's **origin**, which is at its feet: the spawn
//!   resolves to `y = -4.006`, the floor the room stands on;
//! - the **level collider** world (`MAP_CHUNK_COLLISION`, one shape, 343,474
//!   triangles) has the spawn room's floor, so the walker is grounded and the
//!   forward leg is the exact speed: 5.500 m in one second at 5.5 m/s, with
//!   no drift and no idle motion;
//! - the **per-model** world (3,017 holder meshes) has the strafe free (the
//!   exact 5.500 m) but **no floor** under the spawn room, so the walker's
//!   floor probe finds one 13.9 m away and the body floats;
//! - the two together are what the client has, and the walk does not survive
//!   them: the forward leg drifts 0.628 m and the strafe collapses to 1.902 m.
//!
//! That last row is the measured reason the walk's **response** is still a
//! stand-in (`docs/formats/character-controller.md`): the client's own solver
//! resolves the position to a 0.005 fixed point over the character's Domino
//! pair list (`0x142cf3f80`), and that resolve is unread. The capsule is the
//! shape; the resolve is what is missing.

use std::process::Command;

/// The spawn room's floor, as the viewer resolves it.
const SPAWN: [&str; 3] = ["71.169", "-3.656", "53.110"];

/// The room's floor height, the capsule's origin (its feet) at the spawn.
const FLOOR_Y: f32 = -4.006;

struct Leg {
    label: String,
    dx: f32,
    dz: f32,
    travelled: f32,
    grounded: bool,
}

struct Walk {
    legs: Vec<Leg>,
    contacts: u32,
    shapes: usize,
    start: [f32; 3],
}

/// Run the walk test and parse its legs.
fn walk(source: &str) -> Option<Walk> {
    let out = Command::new(env!("CARGO_BIN_EXE_ow-client"))
        .args(["--walk-test", "--no-hero", "--collision", source, "--spawn"])
        .args(SPAWN)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if !text.contains("walk test: start") {
        // No install here (or no world): the project's rule is to skip.
        eprintln!("note: the walk test did not run here:\n{text}");
        return None;
    }
    let mut legs = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        for label in ["forward", "idle", "strafe"] {
            let Some(rest) = line.strip_prefix(&format!("{label}: travelled (")) else {
                continue;
            };
            let mut parts = rest.split(')');
            let xy = parts.next()?;
            let mut nums = xy.split(", ");
            let dx: f32 = nums.next()?.parse().ok()?;
            let dz: f32 = nums.next()?.parse().ok()?;
            let after = parts.next()?;
            let travelled: f32 = after
                .trim_start_matches(" = ")
                .split(' ')
                .next()?
                .parse()
                .ok()?;
            let grounded = line.contains("grounded true");
            legs.push(Leg {
                label: label.to_string(),
                dx,
                dz,
                travelled,
                grounded,
            });
        }
    }
    if legs.is_empty() {
        return None;
    }
    // The header line carries the shape count and the walk's start; the
    // summary line carries the contact count.
    let header = text.lines().find(|l| l.contains("walk test: start"));
    let shapes = header
        .and_then(|l| l.split("shape(s)").next())
        .and_then(|s| s.split(", ").last())
        .and_then(|n| n.trim().parse::<usize>().ok())
        .unwrap_or(0);
    let contacts = text
        .lines()
        .find(|l| l.contains("contact(s)"))
        .and_then(|l| l.split("contact(s)").next())
        .and_then(|s| s.split_whitespace().last())
        .and_then(|n| n.parse::<u32>().ok())
        .unwrap_or(0);
    // The walk's start: the capsule's origin, at its feet.
    let start = header
        .and_then(|l| l.strip_prefix("walk test: start ("))
        .and_then(|rest| rest.split(')').next())
        .and_then(|rest| {
            let mut nums = rest.split(", ");
            let x: f32 = nums.next()?.parse().ok()?;
            let y: f32 = nums.next()?.parse().ok()?;
            let z: f32 = nums.next()?.parse().ok()?;
            Some([x, y, z])
        })
        .unwrap_or([0.0; 3]);
    Some(Walk {
        legs,
        contacts,
        shapes,
        start,
    })
}

fn leg<'a>(legs: &'a [Leg], label: &str) -> &'a Leg {
    legs.iter()
        .find(|l| l.label == label)
        .unwrap_or_else(|| panic!("the walk printed no {label} leg"))
}

/// The walker's position is the capsule's **origin**, which is at its feet.
///
/// The sphere proxy's position was the sphere's *centre*, a radius above the
/// floor. With the capsule the position is the floor itself, so the room's
/// floor height is the spawn's own `y`.
#[test]
fn the_walker_stands_on_the_floor_with_its_feet() {
    let Some(walk) = walk("level") else {
        return;
    };
    assert!(
        (walk.start[1] - FLOOR_Y).abs() < 0.01,
        "the capsule's origin rests on the floor at y {}, not {}",
        walk.start[1],
        FLOOR_Y
    );
}

#[test]
fn the_level_collider_walks_the_spawn_room_exactly() {
    let Some(walk) = walk("level") else {
        return;
    };
    let legs = &walk.legs;
    assert_eq!(walk.shapes, 1, "the level collider is one shape");
    assert!(walk.contacts > 0, "the walk reports its contacts");
    let forward = leg(legs, "forward");
    assert!(
        (forward.travelled - 5.5).abs() < 0.01,
        "the forward leg covered {} m in one second at 5.5 m/s",
        forward.travelled
    );
    assert!(
        forward.dx.abs() < 0.001,
        "the forward leg drifted {} m off the straight line",
        forward.dx
    );
    assert!(
        forward.grounded,
        "the walker is grounded on the level collider"
    );
    let idle = leg(legs, "idle");
    assert!(
        idle.travelled < 0.001,
        "the idle leg drifted {} m",
        idle.travelled
    );
    assert!(idle.grounded, "the walker stays grounded while idle");
    // The capsule is 0.5 m wider than the sphere proxy was, so the room's
    // wall face stops the strafe a little earlier: 4.519 m, against 4.540 m
    // for the sphere.
    let strafe = leg(legs, "strafe");
    assert!(
        (strafe.travelled - 4.519).abs() < 0.01,
        "the strafe leg covered {} m - the level collider's wall face stops it",
        strafe.travelled
    );
}

#[test]
fn the_per_model_meshes_have_the_strafe_but_not_the_floor() {
    let Some(walk) = walk("models") else {
        return;
    };
    let legs = &walk.legs;
    assert_eq!(walk.shapes, 3017, "the range's per-model holder meshes");
    let forward = leg(legs, "forward");
    // The capsule is the client's shape and it is wider than the proxy, so it
    // meets the coarse holders the proxy passed. The number is the measured
    // one for this world, not a target.
    assert!(
        (forward.travelled - 4.978).abs() < 0.05,
        "the forward leg covered {} m",
        forward.travelled
    );
    assert!(
        (forward.dx - 0.764).abs() < 0.05,
        "the forward leg drifted {} m",
        forward.dx
    );
    let strafe = leg(legs, "strafe");
    assert!(
        (strafe.travelled - 5.5).abs() < 0.05,
        "the strafe leg covered {} m",
        strafe.travelled
    );
    assert!(
        strafe.dz.abs() < 0.05,
        "the strafe leg drifted {} m along the forward axis",
        strafe.dz
    );
    assert!(
        !forward.grounded,
        "the per-model meshes carry no floor under the spawn room: the walker's \
         floor probe finds one 13.9 m away"
    );
}

/// The two copies together are what the client has, and the walk's **response**
/// does not survive them: the forward leg drifts 0.628 m and the strafe
/// collapses to 1.902 m.
///
/// This row is pinned so the gap cannot be forgotten and cannot be papered
/// over: the client's own resolve (`0x142cf3f80`, the Domino pair-list walk
/// with its 0.005 fixed point) is unread, and until it is, the walk's slide
/// and floor stage are a declared stand-in.
#[test]
fn the_two_copies_together_still_break_the_walk() {
    let Some(walk) = walk("both") else {
        return;
    };
    let legs = &walk.legs;
    assert_eq!(
        walk.shapes, 3018,
        "the level collider plus the holder meshes"
    );
    let forward = leg(legs, "forward");
    assert!(
        (forward.travelled - 4.999).abs() < 0.05,
        "the forward leg covered {} m",
        forward.travelled
    );
    assert!(
        (forward.dx - 0.628).abs() < 0.05,
        "the forward leg drifted {} m",
        forward.dx
    );
    let strafe = leg(legs, "strafe");
    assert!(
        (strafe.travelled - 1.902).abs() < 0.05,
        "the strafe leg covered {} m",
        strafe.travelled
    );
}
