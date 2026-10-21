//! Dump the first-person component and the model component of a hero's
//! gameplay/view-model entities, typed and with class field names.
//!
//! `STUFirstPersonComponent` (0x9D28963F) carries the FP offset struct
//! (STU_E43E9C08: m_position + three floats), the view-model entity ref,
//! the drag spring (STU_6AE5A305) and four floats (50.0, 1.25, 0.875, 0.8
//! on hero 3). The model component (0x191835F9) carries the model, look,
//! anim set and a STUSkeleton ref (m_9C3F817E) - the candidate reference
//! skeleton.
//!
//! usage: cargo run -p ow-tank --example fpcomp_probe -- [hero]

use ow_tank::{GameData, ManifestSelector};

fn main() -> std::process::ExitCode {
    let hero: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let root = ow_casc::install::detect_root().expect("no install");
    let sources = ow_tank::keytable::Sources::discover(&[root.clone()]);
    let data = GameData::open(
        &root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    )
    .expect("open failed");
    let registry = ow_tank::classreg::ClassRegistry::load_from_sources(&sources);
    let reg = registry.inner();
    let parts = ow_tank::entity::hero_parts(&data, hero).expect("hero parts");
    println!(
        "hero {hero}: entity {:#x}, view-model entity {:#x}, model {:#018x}, view model {:#018x}",
        parts.entity, parts.view_model_entity, parts.model, parts.view_model
    );
    for (label, ent) in [
        ("gameplay entity", parts.entity),
        ("view-model entity", parts.view_model_entity),
    ] {
        let Ok(mut stu) = data.stu_of(ent) else {
            println!("{label} {ent:#x}: no STU");
            continue;
        };
        for (i, inst) in stu.instances.clone().iter().enumerate() {
            if inst.hash != 0x9d28_963f && inst.hash != 0x1918_35f9 {
                continue;
            }
            println!(
                "\n{label} {ent:#x} inst {i}: class {:#010x} ({} fields)",
                inst.hash,
                data.classes.describe(inst.hash)
            );
            for f in stu.typed_fields(i, reg) {
                let name = reg
                    .field(inst.hash, f.hash)
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| format!("0x{:08X}", f.hash));
                println!("   {name} = {}", f.value.describe());
                if let ow_stu::types::Value::Inline { start, bag } = f.value {
                    if let Some(ow_stu::types::FieldType::Instance(child)) =
                        reg.field(inst.hash, f.hash).map(|d| &d.ty)
                    {
                        println!("     (inline class {child:#010x} {})", data.classes.describe(*child));
                        for g in stu.typed_fields_at(*child, start, bag, reg) {
                            let gname = reg
                                .field(*child, g.hash)
                                .map(|d| d.name.clone())
                                .unwrap_or_else(|| format!("0x{:08X}", g.hash));
                            println!("       {gname} = {}", g.value.describe());
                        }
                    }
                }
            }
        }
    }
    if let Some(p) = ow_tank::firstperson::read(&data, reg, parts.entity) {
        println!("\nparsed first-person params: {p:?}");
    }
    std::process::ExitCode::SUCCESS
}
