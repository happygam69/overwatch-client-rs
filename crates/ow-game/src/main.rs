//! The Bevy shell: the movement and combat sandbox on the `ow-sim` core.
//!
//! Bevy owns the window, input, rendering and HUD; every simulation number
//! comes from `ow-sim`, which stays engine-free (see `docs/rewrite-plan.md`,
//! "Bevy's role and boundary"). The shell samples input once per frame and
//! hands the snapshot to the sim, which runs whole fixed ticks — an input
//! timeline maps to ticks, which is what makes a replay well-defined.
//!
//! ```text
//! ow-game                          windowed sandbox (install map when present)
//! ow-game --range                  the flat test range instead of the map
//! ow-game --bots 4 --hero 2         bots and hero, by install index
//! ow-game --headless 5             scripted headless run, prints a HUD trace
//! ow-game --verify-determinism     scripted run twice; the two must be equal
//! ow-game --trace t.jsonl [--script s.txt]   replay a client trace, in the HUD
//! ```
//!
//! Per-shot damage is an explicit input (`--damage`): `docs/formats/weapons.md`
//! records the search that failed to find it in the files, so the sandbox
//! never dresses a guess as data.

mod install;
mod sandbox;
mod sim;

use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;

use ow_sim::movement::{Input as MoveInput, MoverSpec};
use ow_sim::{Combatant, FireMode, PoolSet, Weapon};

use sandbox::{BotIndex, Ground, Hud, PlayerCam};
use sim::Sim;

// ---- configuration ------------------------------------------------------

#[derive(Resource, Clone)]
struct Cfg {
    map: bool,
    bots: usize,
    hero: u32,
    damage: f32,
    mouse_sens: f32,
    /// Map-chunk selection flags, passed straight to `ow_scene::build`.
    scene_args: Vec<String>,
    divergence: Option<String>,
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
}

fn parse_cfg(args: &[String]) -> Cfg {
    let take_after = |name: &str| -> Vec<String> {
        // Keep `--chunks 0x..` / `--cluster N` / `--at x,y,z` in the game's own
        // argument list so the map builder sees them untouched.
        let mut out = Vec::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if a == name {
                out.push(a.clone());
                if let Some(v) = it.next() {
                    out.push(v.clone());
                }
            }
        }
        out
    };
    let mut scene_args = Vec::new();
    for name in [
        "--chunks",
        "--cluster",
        "--at",
        "--chunks-from",
        "--chunk-file",
    ] {
        scene_args.extend(take_after(name));
    }
    Cfg {
        map: !args.iter().any(|a| a == "--range"),
        bots: flag(args, "--bots")
            .and_then(|v| v.parse().ok())
            .unwrap_or(3),
        hero: flag(args, "--hero")
            .and_then(|v| v.parse().ok())
            .unwrap_or(2),
        damage: flag(args, "--damage")
            .and_then(|v| v.parse().ok())
            .unwrap_or(6.0),
        mouse_sens: flag(args, "--sens")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0022),
        scene_args,
        divergence: None,
    }
}

// ---- boot: everything that needs the install, done before Bevy ----------

#[derive(Resource)]
struct Boot {
    sim: Sim,
    /// Map geometry to bake, when the install answered.
    scene: Option<ow_scene::Scene>,
    note: String,
}

fn build_boot(cfg: &Cfg) -> Boot {
    let mut notes = Vec::new();

    // The install answers if it can; otherwise the sandbox still runs, on a
    // flat range with the wiki-checked defaults the install would have
    // replaced. The HUD says which world it is.
    let (spec, weapon, bots) = match install::open_data() {
        Ok(data) => match install::hero_spec_and_pools(&data, cfg.hero) {
            Ok((spec, _pools, _interval)) => {
                let stock = install::stock_bots(&data, cfg.bots).unwrap_or_default();
                let bots: Vec<Combatant> = stock
                    .into_iter()
                    .enumerate()
                    .map(|(i, (pools, _label))| {
                        let a = i as f32 / cfg.bots.max(1) as f32 * std::f32::consts::TAU;
                        Combatant::new([10.0 * a.cos(), 0.0, 10.0 * a.sin()], pools)
                    })
                    .collect();
                let (weapon, weapon_note) =
                    install::hero_weapon_and_note(&data, cfg.hero, cfg.damage);
                notes.push(format!("install: hero {} mover and pools read", cfg.hero));
                notes.push(weapon_note);
                (spec, weapon, bots)
            }
            Err(e) => {
                notes.push(format!("install read failed ({e}); synthetic defaults"));
                let (spec, interval, bots) = synthetic(cfg);
                (spec, weapon(cfg.damage, interval), bots)
            }
        },
        Err(e) => {
            notes.push(format!("{e}; synthetic defaults"));
            let (spec, interval, bots) = synthetic(cfg);
            (spec, weapon(cfg.damage, interval), bots)
        }
    };

    // Map geometry, when requested and available.
    let mut scene = None;
    if cfg.map {
        match ow_scene::build(&cfg.scene_args) {
            Ok(s) => {
                notes.push(format!(
                    "map: {} placements, {} models, {} triangles",
                    s.placements,
                    s.models_loaded,
                    s.triangle_count()
                ));
                scene = Some(s);
            }
            Err(e) => notes.push(format!("map not drawn ({e}); flat range")),
        }
    }

    let ground = match &scene {
        Some(s) => Ground::Map(ow_scene::Collider::build(s)),
        None => Ground::Flat(0.0),
    };
    notes.push(format!("surface: {}", ground.label()));
    let spawn = spawn_point(&ground, scene.as_ref());
    let mut sim = Sim::new(spec, spawn, 0.0, weapon, bots);
    sim.pitch = -0.02;

    Boot {
        sim,
        scene,
        note: notes.join("; "),
    }
}

fn synthetic(cfg: &Cfg) -> (MoverSpec, f32, Vec<Combatant>) {
    let bots = (0..cfg.bots)
        .map(|i| {
            let a = i as f32 / cfg.bots.max(1) as f32 * std::f32::consts::TAU;
            Combatant::new(
                [10.0 * a.cos(), 0.0, 10.0 * a.sin()],
                PoolSet {
                    health: 200.0,
                    armour: 0.0,
                    shields: 0.0,
                },
            )
        })
        .collect();
    (MoverSpec::from_base_speed(5.5), 0.5, bots)
}

fn weapon(damage: f32, interval: f32) -> Weapon {
    Weapon {
        name: "sandbox".into(),
        interval,
        pellets: 1,
        damage_per_projectile: damage,
        mode: FireMode::Hitscan { range: 200.0 },
        lifetime: None,
        spread_deg: 0.0,
        falloff: None,
        ammo: None,
        reload: None,
    }
}

/// A spawn point: the scene's own viewpoint when there is a map, the origin on
/// the flat range.
fn spawn_point(ground: &Ground, scene: Option<&ow_scene::Scene>) -> [f32; 3] {
    let (x, z) = match scene {
        Some(s) => (s.eye.x, s.eye.z),
        None => (0.0, 0.0),
    };
    let y = ground.at(x, z, 4.0);
    [x, y + sim::EYE_HEIGHT, z]
}

#[derive(Resource)]
struct GroundRes(Ground);

/// The map geometry to bake at startup, when there is one.
#[derive(Resource)]
struct MapScene(Option<ow_scene::Scene>);

// ---- the app ------------------------------------------------------------

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--verify-determinism") {
        std::process::exit(if verify_determinism(&args) { 0 } else { 1 });
    }
    if let Some(secs) = flag(&args, "--headless") {
        let seconds: f32 = secs.parse().unwrap_or(5.0);
        std::process::exit(headless(&args, seconds));
    }

    let mut cfg = parse_cfg(&args);
    cfg.divergence = divergence_report(&args);

    let mut boot = build_boot(&cfg);
    let note = boot.note.clone();
    let ground = match boot.scene.take() {
        Some(s) => Ground::Map(ow_scene::Collider::build(&s)),
        None => Ground::Flat(0.0),
    };
    // The scene itself is still needed for the bake in `setup`; keep it.
    let scene_for_setup = std::mem::take(&mut boot.scene);

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: format!("ow-game  |  {note}"),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(cfg)
        .insert_resource(GroundRes(ground))
        .insert_resource(boot)
        .insert_resource(MapScene(scene_for_setup))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (input_and_step, follow_camera, sync_bots, update_hud),
        )
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    scene: Res<MapScene>,
    ground: Res<GroundRes>,
    boot: Res<Boot>,
) {
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 220.0,
    });

    if let Some(scene) = scene.0.as_ref() {
        let placed = sandbox::bake_map(&mut commands, &mut meshes, &mut materials, scene);
        info!("map baked: {placed} placements");
    }

    // The flat range and its pillars are drawn when there is no map; on a map
    // the collider still reads the ground plane, so the floor is drawn there
    // too, under the geometry.
    let (floor, pillars) = sandbox::test_range_meshes();
    commands.spawn(PbrBundle {
        mesh: meshes.add(floor),
        material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.28, 0.30, 0.26),
            perceptual_roughness: 1.0,
            ..default()
        }),
        ..default()
    });
    if matches!(ground.0, Ground::Flat(_)) {
        let pillar_mesh = meshes.add(pillars);
        for (x, z) in [(-12.0f32, 0.0f32), (12.0, 0.0), (0.0, -12.0), (0.0, 12.0)] {
            commands.spawn(PbrBundle {
                mesh: pillar_mesh.clone(),
                material: materials.add(StandardMaterial {
                    base_color: Color::srgb(0.45, 0.42, 0.38),
                    perceptual_roughness: 0.9,
                    ..default()
                }),
                transform: Transform::from_xyz(x, 0.0, z),
                ..default()
            });
        }
    }

    // Bots: a box each, standing where the sim put them (hit shapes are Q8 of
    // the plan; until then the box is the stand-in).
    let bot_mesh = meshes.add(sandbox::bot_box());
    let bot_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.75, 0.3, 0.25),
        perceptual_roughness: 0.8,
        ..default()
    });
    let positions: Vec<[f32; 3]> = boot.sim.world.combatants.iter().map(|c| c.pos).collect();
    for (i, pos) in positions.iter().enumerate() {
        commands.spawn((
            PbrBundle {
                mesh: bot_mesh.clone(),
                material: bot_material.clone(),
                transform: Transform::from_xyz(pos[0], pos[1] + 0.9, pos[2]),
                ..default()
            },
            BotIndex(i),
        ));
    }

    // Camera at the eye, facing the mover's forward.
    let eye = boot.sim.eye_pos();
    let rot = Quat::from_rotation_y(boot.sim.controller.yaw - std::f32::consts::FRAC_PI_2)
        * Quat::from_rotation_x(boot.sim.pitch);
    commands.spawn((
        Camera3dBundle {
            transform: Transform::from_xyz(eye[0], eye[1], eye[2]).with_rotation(rot),
            ..default()
        },
        PlayerCam,
    ));

    commands.spawn(DirectionalLightBundle {
        directional_light: DirectionalLight {
            illuminance: 9_000.0,
            shadows_enabled: false,
            ..default()
        },
        transform: Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, -0.6, 0.0)),
        ..default()
    });

    // HUD.
    commands.spawn((
        TextBundle::from_section(
            "",
            TextStyle {
                font_size: 15.0,
                color: Color::WHITE,
                ..default()
            },
        )
        .with_style(Style {
            position_type: PositionType::Absolute,
            left: Val::Px(10.0),
            top: Val::Px(10.0),
            ..default()
        }),
        Hud,
    ));
}

// ---- systems ------------------------------------------------------------

fn input_and_step(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut motion: EventReader<MouseMotion>,
    cfg: Res<Cfg>,
    ground: Res<GroundRes>,
    mut boot: ResMut<Boot>,
    mut exit: EventWriter<AppExit>,
) {
    let dt = time.delta_seconds().clamp(0.0, 0.25);

    let mut look = Vec2::ZERO;
    for event in motion.read() {
        look += event.delta;
    }
    if look != Vec2::ZERO {
        boot.sim.controller.yaw -= look.x * cfg.mouse_sens;
        boot.sim.pitch = (boot.sim.pitch - look.y * cfg.mouse_sens).clamp(-1.45, 1.45);
    }

    if keys.just_pressed(KeyCode::Escape) {
        exit.send(AppExit::Success);
        return;
    }

    let input = MoveInput {
        forward: (keys.pressed(KeyCode::KeyW) as i32 - keys.pressed(KeyCode::KeyS) as i32) as f32,
        side: (keys.pressed(KeyCode::KeyD) as i32 - keys.pressed(KeyCode::KeyA) as i32) as f32,
        crouch: keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::KeyC),
        jump: keys.just_pressed(KeyCode::Space),
    };
    boot.sim.firing = buttons.pressed(MouseButton::Left);

    let boot = &mut *boot;
    boot.sim
        .advance(dt, input, |x, z, from| ground.0.at(x, z, from));
}

fn follow_camera(mut cam: Query<&mut Transform, With<PlayerCam>>, boot: Res<Boot>) {
    let Ok(mut transform) = cam.get_single_mut() else {
        return;
    };
    let eye = boot.sim.eye_pos();
    transform.translation = Vec3::new(eye[0], eye[1], eye[2]);
    transform.rotation =
        Quat::from_rotation_y(boot.sim.controller.yaw - std::f32::consts::FRAC_PI_2)
            * Quat::from_rotation_x(boot.sim.pitch);
}

fn sync_bots(boot: Res<Boot>, mut bots: Query<(&BotIndex, &mut Visibility)>) {
    for (index, mut visibility) in bots.iter_mut() {
        let alive = boot
            .sim
            .world
            .combatants
            .get(index.0)
            .map(|c| c.alive)
            .unwrap_or(false);
        *visibility = if alive {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

fn update_hud(boot: Res<Boot>, cfg: Res<Cfg>, mut hud: Query<&mut Text, With<Hud>>) {
    let Ok(mut text) = hud.get_single_mut() else {
        return;
    };
    let mut body = boot.sim.hud();
    if let Some(report) = &cfg.divergence {
        body.push('\n');
        body.push_str(report);
    }
    body.push_str(&format!("\nworld: {}", boot.note));
    text.sections[0].value = body;
}

// ---- headless -----------------------------------------------------------

/// The scripted schedule both headless modes share: walk, strafe-jump,
/// crouch-walk, backpedal, firing from the first tick. Deterministic by
/// construction — the same function of tick time on every run.
fn schedule(t: f32) -> (MoveInput, f32) {
    let input = if t < 1.0 {
        MoveInput {
            forward: 1.0,
            ..Default::default()
        }
    } else if t < 2.0 {
        MoveInput {
            forward: 1.0,
            side: 1.0,
            ..Default::default()
        }
    } else if t < 2.5 {
        MoveInput {
            forward: 1.0,
            jump: ((t - 2.0) * 60.0).floor() == 0.0,
            ..Default::default()
        }
    } else if t < 3.5 {
        MoveInput {
            forward: 1.0,
            crouch: true,
            ..Default::default()
        }
    } else {
        MoveInput {
            forward: -1.0,
            ..Default::default()
        }
    };
    (input, 0.0)
}

fn run_schedule(cfg: &Cfg, seconds: f32) -> (Boot, Vec<String>) {
    let mut boot = build_boot(cfg);
    let ground = match boot.scene.take() {
        Some(s) => Ground::Map(ow_scene::Collider::build(&s)),
        None => Ground::Flat(0.0),
    };
    boot.sim.firing = true;
    let mut lines = Vec::new();
    let ticks = (seconds * 60.0).round() as u64;
    let mut next_print = 0.0f32;
    for tick in 0..ticks {
        let t = tick as f32 * sim::DT;
        let (input, yaw) = schedule(t);
        boot.sim.controller.yaw = yaw;
        // One tick at a time: headless has no frames to accumulate.
        boot.sim
            .advance(sim::DT, input, |x, z, f| ground.at(x, z, f));
        if t + sim::DT >= next_print {
            next_print += 0.5;
            lines.push(boot.sim.hud());
        }
    }
    (boot, lines)
}

fn headless(args: &[String], seconds: f32) -> i32 {
    let mut cfg = parse_cfg(args);
    cfg.divergence = divergence_report(args);
    let (boot, lines) = run_schedule(&cfg, seconds);
    println!("{}", boot.note);
    if let Some(report) = &cfg.divergence {
        println!("{report}");
    }
    for line in &lines {
        println!("{line}\n---");
    }
    0
}

/// Run the schedule twice and compare every HUD line: the shell's own
/// determinism check (the core's is `trace_diff selftest`).
fn verify_determinism(args: &[String]) -> bool {
    let cfg = parse_cfg(args);
    let seconds = flag(args, "--headless")
        .and_then(|v| v.parse().ok())
        .unwrap_or(4.0f32);
    let (_, a) = run_schedule(&cfg, seconds);
    let (_, b) = run_schedule(&cfg, seconds);
    if a == b {
        println!(
            "determinism: PASS ({} HUD lines identical across two runs of {seconds}s)",
            a.len()
        );
        true
    } else {
        let first = a
            .iter()
            .zip(&b)
            .position(|(x, y)| x != y)
            .unwrap_or(a.len().min(b.len()));
        println!("determinism: FAIL at HUD line {first}");
        println!("first run:\n{}", a.get(first).cloned().unwrap_or_default());
        println!("second run:\n{}", b.get(first).cloned().unwrap_or_default());
        false
    }
}

/// Load a client trace and replay it through the core, reporting the gate's
/// verdict — for keyed traces the *same* engine `trace_diff keys --by-state`
/// runs (per write, with the frame-jitter band and the terrain/transition
/// screens), so the HUD and the CLI agree; scripted traces keep the older
/// bit-exact `diff_trace` engine.  Surfaced here so the M5 gate (the harness
/// runs from the game) is met.
fn divergence_report(args: &[String]) -> Option<String> {
    let path = flag(args, "--trace")?;
    let text = std::fs::read_to_string(path).ok()?;
    let trace = match ow_sim::trace::Trace::parse(&text) {
        Ok(t) => t,
        Err(e) => return Some(format!("trace {path}: {e}")),
    };
    let mut out = format!(
        "trace {path}: {} samples @ {:.0} Hz from {}",
        trace.samples.len(),
        trace.hz,
        trace.addr
    );
    // Keyed traces (the recorder's default) replay without a script.  The
    // older bit-exact `diff_trace_keys` (still on `trace_diff keys` without
    // `--by-state`, for the Q3 held-vs-edges comparison) reports the sub-frame
    // ramp-in as a "divergence", which is not what the movement claims are
    // arbitrated by.
    let has_keys = trace.samples.iter().all(|s| s.keys.is_some());
    if has_keys && flag(args, "--script").is_none() {
        let speed = flag(args, "--speed")
            .and_then(|v| v.parse().ok())
            .unwrap_or(5.5);
        match ow_sim::trace::diff_trace_keys_states(&trace, speed) {
            Ok(report) => match &report.first {
                Some(d) => out.push_str(&format!(
                    "
first divergence: sample {}  {}  client {:?}  model {:?}  ({} ulp)",
                    d.sample, d.field, d.expected, d.actual, d.ulp
                )),
                None => out.push_str(&format!(
                    "
no divergence across {} compared write(s): {} exact, {} in the client's \
frame-jitter band, {} settling, {} environment-affected, {} skipped at input \
transitions; max step deviation {:.7} m; max vertical step deviation {:.7} m; \
max vertical delta error {:.7} m; final ULP residual x/y/z = {:?}",
                    report.samples,
                    report.exact_writes,
                    report.jitter_writes,
                    report.settle_writes,
                    report.env_writes,
                    report.skipped,
                    report.max_step_err,
                    report.max_vstep_err,
                    report.max_vdelta_err,
                    report.residuals
                )),
            },
            Err(e) => out.push_str(&format!(
                "
key replay failed: {e}"
            )),
        }
        return Some(out);
    }
    if let Some(script_path) = flag(args, "--script") {
        let script = match std::fs::read_to_string(script_path) {
            Ok(s) => s,
            Err(e) => return Some(format!("{out}; script {script_path}: {e}")),
        };
        match ow_sim::trace::parse_script(&script) {
            Ok(steps) => {
                let align = flag(args, "--align-ms")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0);
                let speed = flag(args, "--speed")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(5.5);
                match ow_sim::trace::diff_trace(&trace, &steps, align, speed) {
                    Ok(report) => match report.first {
                        Some(d) => out.push_str(&format!(
                            "\nfirst divergence: sample {}  {}  client {:?}  model {:?}  ({} ulp)",
                            d.sample, d.field, d.expected, d.actual, d.ulp
                        )),
                        None => out.push_str(&format!(
                            "\nno position divergence across {} samples; final ULP residual x/y/z = {:?}",
                            report.samples, report.residuals
                        )),
                    },
                    Err(e) => out.push_str(&format!("\nreplay failed: {e}")),
                }
            }
            Err(e) => out.push_str(&format!("\nscript: {e}")),
        }
    }
    Some(out)
}
