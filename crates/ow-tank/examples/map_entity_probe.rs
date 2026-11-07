//! The practice range's entity placeables and their component ids.
//!
//! An entity placeable carries a table of component-instance-data records
//! (`ow_tank::mapchunk::InstanceDataRef`): a small `u32` component id and an
//! offset to the component's STU stream. The STU asset at the offset names the
//! component's data class, so this probe lists the distinct ids the range uses,
//! resolves each root class against the reference checkout's STU class list,
//! and counts the entities that carry the model component.
//!
//! Read-only. Run with:
//!
//! ```text
//! cargo run -p ow-tank --example map_entity_probe
//! ```

use std::collections::BTreeMap;

use ow_tank::classreg::ClassRegistry;
use ow_tank::keytable::Sources;
use ow_tank::mapchunk::Body;
use ow_tank::{mapresolve, GameData, ManifestSelector};

fn main() -> std::io::Result<()> {
    let root = ow_casc::install::detect_root().expect("the install root");
    let sources = Sources::discover(std::slice::from_ref(&root));
    let data =
        GameData::open(&root, &ManifestSelector::default(), &sources).expect("the install opens");
    let classreg = ClassRegistry::load_from_sources(&sources);
    let reg = classreg.inner();
    let kinds = sources.chunk_kinds().expect("the chunk-kind table");
    let scene = mapresolve::resolve_practice_range(&data, &kinds).expect("the scene resolves");

    for chunk in &scene.scene.chunks {
        println!(
            "  chunk {:#018x} kind {:#04x} {} B placeables {} parsed {}",
            chunk.guid,
            chunk.kind,
            chunk.size,
            chunk.placeables(),
            chunk.chunk.is_some()
        );
    }
    let mut entities = 0usize;
    let mut with_model: BTreeMap<u32, usize> = BTreeMap::new();
    let mut with_model_total = 0usize;
    let mut records = 0usize;
    let mut types: BTreeMap<u32, (usize, usize, Option<(u32, String)>)> = BTreeMap::new();
    let mut chunks = 0usize;
    for chunk in &scene.scene.chunks {
        let Some(parsed) = &chunk.chunk else { continue };
        let Ok(bytes) = data.open_asset(chunk.guid) else {
            continue;
        };
        chunks += 1;
        for placeable in &parsed.placeables {
            let Body::Entity(body) = &placeable.body else {
                continue;
            };
            entities += 1;
            let ids: Vec<u32> = body.instance_data.iter().map(|r| r.type_hash).collect();
            let has_model = ids.contains(&2);
            if has_model {
                with_model_total += 1;
            }
            for id in &ids {
                if has_model {
                    *with_model.entry(*id).or_insert(0) += 1;
                }
            }
            for record in &body.instance_data {
                records += 1;
                let entry = types.entry(record.type_hash).or_insert((0, 0, None));
                entry.0 += 1;
                entry.1 = entry.1.max(bytes.len().saturating_sub(record.offset));
                if entry.2.is_none() {
                    if let Some(stream) = bytes.get(record.offset..) {
                        if let Ok(stu) = ow_stu::Stu::parse(stream.to_vec()) {
                            if let Some(root) = stu.instances.first() {
                                let name = reg
                                    .class(root.hash)
                                    .map(|c| c.name.clone())
                                    .unwrap_or_default();
                                entry.2 = Some((root.hash, name));
                            }
                        }
                    }
                }
            }
        }
    }

    println!(
        "{chunks} chunk(s), {entities} entity placeable(s), {records} instance-data record(s), \
         {} distinct type word(s)",
        types.len()
    );
    println!(
        "{} STU class(es) in the reference list; {with_model_total} entity placeable(s) carry component id 2 (the model component)",
        reg.class_count()
    );
    for (id, (count, tail, root)) in &types {
        let co = with_model.get(id).copied().unwrap_or(0);
        match root {
            Some((hash, name)) if !name.is_empty() => println!(
                "  id {id:#04x} x{count:<4} root {hash:#010x} {name}                  (with model {co}, stream tail {tail} byte(s))"
            ),
            Some((hash, _)) => println!(
                "  id {id:#04x} x{count:<4} root {hash:#010x} unresolved                  (with model {co}, stream tail {tail} byte(s))"
            ),
            None => println!(
                "  id {id:#04x} x{count:<4} stream not an STU asset                  (stream tail {tail} byte(s))"
            ),
        }
    }
    Ok(())
}
