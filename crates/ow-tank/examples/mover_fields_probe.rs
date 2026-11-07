//! Throwaway probe: the mover component's declared field table (offset, hash,
//! type, class default) next to the instance's own values. Reads the install;
//! writes nothing.
//!
//! usage: cargo run -p ow-tank --example mover_fields_probe -- [entity-hex]

use ow_tank::classes;
use ow_tank::classreg::ClassRegistry;
use ow_tank::keytable::Sources;
use ow_tank::{GameData, ManifestSelector};

fn main() -> std::process::ExitCode {
    let entity = std::env::args()
        .nth(1)
        .map(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(1))
        .unwrap_or(0x0400_0000_0000_0001);
    let root = match ow_casc::install::detect_root() {
        Some(r) => r,
        None => {
            eprintln!("no install");
            return std::process::ExitCode::from(2);
        }
    };
    let sources = Sources::discover(std::slice::from_ref(&root));
    let data = match GameData::open(
        root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    ) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("open failed: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    let reg = ClassRegistry::load_from_sources(&sources);
    println!("class registry empty: {}", reg.is_empty());
    let ty = classes::CHARACTER_MOVER_COMPONENT;
    let Some(def) = reg.inner().class(ty) else {
        eprintln!("no class {ty:#X}");
        return std::process::ExitCode::from(1);
    };
    println!(
        "class {} hash {:#X} size {:#X}, {} fields",
        def.name,
        def.hash,
        def.size,
        def.fields.len()
    );
    let mut stu = match data.stu_of(entity) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("stu {entity:#X}: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    if let Some(i) = stu.instance(ty) {
        for (name, hash) in [
            ("gravity 0xC2C40969", ow_tank::entity::mover_fields::GRAVITY),
            (
                "fall    0xB74245E8",
                ow_tank::entity::mover_fields::FALL_TERMINAL,
            ),
            (
                "rise    0x975F21A5",
                ow_tank::entity::mover_fields::RISE_TERMINAL,
            ),
            (
                "takeoff 0xEC289987",
                ow_tank::entity::mover_fields::TAKEOFF_LANE,
            ),
            ("forward 0x19452F15", ow_tank::entity::mover_fields::FORWARD),
        ] {
            println!("  typed {name}: {:?}", stu.read_f32(i, hash));
        }
    }
    let inst = stu.instance(ty);
    let payload = inst.map(|i| stu.payload_start(i));
    let mut rows: Vec<_> = def.fields.iter().collect();
    rows.sort_by_key(|f| f.v1_offset);
    for f in rows {
        let mut live = String::from("-");
        if let Some(start) = payload {
            let at = start + f.v1_offset as usize;
            if let Some(raw) = stu.u32_at(at) {
                live = format!("{:#010X} f32 {:?}", raw, f32::from_bits(raw));
            }
        }
        println!(
            "  slot 0x{:03X} (comp 0x{:03X}) hash 0x{:08X} {:<28} {:?} default {:?} live {}",
            f.v1_offset + 4,
            f.v1_offset,
            f.hash,
            f.short_name().unwrap_or("-"),
            f.ty,
            f.default,
            live
        );
    }
    std::process::ExitCode::SUCCESS
}
