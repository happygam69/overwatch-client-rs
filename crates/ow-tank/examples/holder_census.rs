//! What the practice range's models carry for collision, by shape mode.
//!
//! The movement world's builder selects a holder with
//! `model_body::holder_selected` and then builds the shape its mode names. A
//! mesh-only world (descriptor type 3) therefore drops every point-cloud,
//! capsule and sphere holder; this prints how many of each the range has, per
//! model and weighted by placements, so the gap is a number rather than a guess.
//!
//! Read-only over the install; skipped when no install is present.
//!
//! ```text
//! cargo run --release -p ow-tank --example holder_census
//! ```

use std::collections::BTreeMap;

use ow_tank::classreg::ClassRegistry as Classreg;
use ow_tank::keytable::Sources;
use ow_tank::range_world;
use ow_tank::{GameData, ManifestSelector};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(root) = ow_casc::install::detect_root() else {
        println!("no install present; nothing to census");
        return Ok(());
    };
    let sources = Sources::discover(std::slice::from_ref(&root));
    let data = GameData::open(&root, &ManifestSelector::default(), &sources)?;
    let classreg = Classreg::load_from_sources(&sources);
    let rows = range_world::holder_rows(&data, &sources, classreg.inner())?;
    // A named model: print every holder it carries, so a specific placement's
    // collision can be traced.
    for arg in std::env::args().skip(1) {
        let text = arg.trim_start_matches("0x");
        let Ok(guid) = u64::from_str_radix(text, 16) else {
            continue;
        };
        let mine: Vec<_> = rows.iter().filter(|r| r.model == guid).collect();
        println!("model {guid:#x}: {} holder(s)", mine.len());
        for row in mine {
            println!(
                "  shape mode {} byte {} mesh {} selected {} descriptor {}",
                row.shape_mode,
                row.mode_byte,
                row.has_mesh,
                row.selected,
                row.shape.map_or(-1, |s| s.descriptor_type() as i32)
            );
        }
    }
    let placements = range_world::placements(&data, &sources)?;
    let mut instances: BTreeMap<u64, usize> = BTreeMap::new();
    for p in &placements {
        *instances.entry(p.model).or_default() += 1;
    }

    // Placeables by body kind: the scene instances models and model groups, so
    // any other kind's model (an entity's, say) is geometry the renderer never
    // draws and the collision world never had.
    let kinds = sources.chunk_kinds()?;
    let scene = ow_tank::mapresolve::resolve_practice_range(&data, &kinds)?;
    let mut by_kind: BTreeMap<&'static str, (usize, usize, usize)> = BTreeMap::new();
    for chunk in &scene.scene.chunks {
        let Some(c) = &chunk.chunk else { continue };
        for placeable in &c.placeables {
            let name = match &placeable.body {
                ow_tank::mapchunk::Body::Model(_) => "model",
                ow_tank::mapchunk::Body::SingleModel(_) => "single model",
                ow_tank::mapchunk::Body::ModelGroup(_) => "model group",
                ow_tank::mapchunk::Body::Entity(_) => "entity",
                ow_tank::mapchunk::Body::Light(_) => "light",
                ow_tank::mapchunk::Body::ReflectionPoint(_) => "reflection",
                ow_tank::mapchunk::Body::Text(_) => "text",
                ow_tank::mapchunk::Body::Sound(_) => "sound",
                ow_tank::mapchunk::Body::Effect(_) => "effect",
                _ => "other",
            };
            let entry = by_kind.entry(name).or_default();
            entry.0 += 1;
            if placeable.model.is_some() {
                entry.1 += 1;
            }
            if placeable.entity.is_some() {
                entry.2 += 1;
            }
        }
    }
    println!();
    println!(
        "{:<14} {:>10} {:>8} {:>8}",
        "kind", "placeables", "model", "entity"
    );
    for (name, (n, models, entities)) in &by_kind {
        println!("{name:<14} {n:>10} {models:>8} {entities:>8}");
    }

    let mut by_mode: BTreeMap<(i32, u8, bool, bool), (usize, usize, usize)> = BTreeMap::new();
    // (shape mode, mode byte, has mesh, selected) -> (models, holders, placed)
    for row in &rows {
        let placed = instances.get(&row.model).copied().unwrap_or(0);
        let entry = by_mode
            .entry((row.shape_mode, row.mode_byte, row.has_mesh, row.selected))
            .or_insert((0, 0, 0));
        entry.0 += 1;
        entry.1 += placed;
    }
    println!(
        "{} holder(s) over {} distinct model(s); {} placement(s)",
        rows.len(),
        instances.len(),
        placements.len()
    );
    println!(
        "{:>10} {:>9} {:>8} {:>8} {:>7} {:>8} {:>8}",
        "shape", "mode byte", "mesh", "selected", "holders", "instances", "descriptor"
    );
    let mut totals: BTreeMap<&'static str, (usize, usize)> = BTreeMap::new();
    for ((mode, byte, mesh, selected), (holders, selected_models, placed)) in &by_mode {
        let shape = ow_tank::model_body::shape_for(*mode);
        let name = match shape {
            Some(ow_tank::model_body::Shape::Mesh) => "mesh",
            Some(ow_tank::model_body::Shape::PointCloud) => "point cloud",
            Some(ow_tank::model_body::Shape::Capsule) => "capsule",
            Some(ow_tank::model_body::Shape::Sphere) => "sphere",
            None => "skipped",
        };
        let selected_flag = if *selected { "yes" } else { "no" };
        let _ = selected_models;
        println!(
            "{name:>10} {byte:>9} {mesh:>8} {selected_flag:>8} {holders:>7} {placed:>8} {:>8}",
            shape.map_or(-1, |s| s.descriptor_type() as i32)
        );
        if *selected {
            let entry = totals.entry(name).or_insert((0, 0));
            entry.0 += holders;
            entry.1 += placed;
        }
    }
    println!();
    for (name, (holders, placed)) in &totals {
        println!("selected {name}: {holders} holder(s), {placed} placed instance(s)");
    }

    // Visible geometry covered by a collision holder, weighted by the model's
    // own triangle count and by how often the scene places it.
    let mut with = (0usize, 0usize, 0usize);
    let mut without = (0usize, 0usize, 0usize);
    let mut unreadable = 0usize;
    let mut missing: Vec<(usize, u64, usize)> = Vec::new();
    let mut has_holder: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();
    for row in &rows {
        if row.selected && row.has_mesh {
            has_holder.insert(row.model);
        }
    }
    for (model, count) in &instances {
        let triangles = match data
            .open_asset(*model)
            .ok()
            .and_then(|bytes| ow_tank::model::parse(&bytes, &|_| None).ok())
        {
            Some(parsed) => parsed.triangle_count(),
            None => {
                unreadable += 1;
                continue;
            }
        };
        let entry = if has_holder.contains(model) {
            &mut with
        } else {
            missing.push((*count, *model, triangles));
            &mut without
        };
        entry.0 += 1;
        entry.1 += count;
        entry.2 += count * triangles;
    }
    println!();
    println!(
        "models with a collision holder: {} model(s), {} placement(s), {} triangles placed",
        with.0, with.1, with.2
    );
    println!(
        "models without: {} model(s), {} placement(s), {} triangles placed ({unreadable} unreadable)",
        without.0, without.1, without.2
    );
    missing.sort_unstable();
    missing.reverse();
    println!("top models without collision (placements, model, triangles):");
    for (count, model, triangles) in missing.iter().take(12) {
        println!("  {count:>4} x {model:#018x} {triangles} tri");
    }
    Ok(())
}
