//! Probe the typed STU layer: the class registry and per-instance decoding.
//!
//! ```text
//! ow-tank-stu --registry                  # registry load stats
//! ow-tank-stu --class 0x7C3457DC          # one class's declaration
//! ow-tank-stu --guid 0x0ca0000000000126 --fields
//! ow-tank-stu --guid 0x02e0000000000002 --instance 1 --fields --deep
//! ```
//!
//! Read-only: opens the install the same way the rest of the tree does.

use ow_stu::types::{ClassRegistry, FieldType, TypedField, Value};
use ow_tank::classreg::ClassRegistry as TankRegistry;
use ow_tank::{guid, GameData, ManifestSelector};
use std::path::PathBuf;

fn parse_hex(text: &str) -> Option<u64> {
    u64::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok()
}

fn print_registry_stats(registry: &TankRegistry) {
    let reg = registry.inner();
    println!("files read:    {}", registry.files);
    println!("classes:       {}", reg.class_count());
    println!("enums:         {}", reg.enum_count());
    println!("fields:        {}", reg.field_count());
    if !registry.unknown_types.is_empty() {
        let mut counts: Vec<(&String, &usize)> = registry.unknown_types.iter().collect();
        counts.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        println!("unknown types: {} distinct", counts.len());
        for (name, count) in counts.iter().take(20) {
            println!("  {count:>6}  {name}");
        }
    }
    if !registry.unknown_bases.is_empty() {
        println!("unknown bases: {:?}", registry.unknown_bases);
    }
    if !registry.unparsed_defaults.is_empty() {
        println!("unparsed defaults: {}", registry.unparsed_defaults.len());
        for line in registry.unparsed_defaults.iter().take(10) {
            println!("  {line}");
        }
    }
}

fn dump_class(registry: &TankRegistry, hash: u32) {
    let reg = registry.inner();
    let Some(class) = reg.class(hash) else {
        println!("0x{hash:08X}: not in the registry");
        return;
    };
    let base = match class.base.and_then(|b| reg.class(b)) {
        Some(base) => format!("0x{:08X} {}", base.hash, base.name),
        None => "none".to_string(),
    };
    println!(
        "0x{:08X} {} size {} base {base}",
        class.hash, class.name, class.size
    );
    println!("{} own field(s)", class.fields.len());
    for field in &class.fields {
        println!(
            "  +{:<5} 0x{:08X} {:<30} {:?} size={:?} {:?} default={:?}",
            field.v1_offset,
            field.hash,
            field.name,
            field.reader,
            field.size_hint,
            field.ty,
            field.default
        );
    }
}

/// Print typed fields, recursing into in-place instances (`--deep`) so the
/// nested declarations are visible rather than an `inline @...` marker.
fn print_fields(
    stu: &mut ow_stu::Stu,
    reg: &ClassRegistry,
    class_hash: u32,
    fields: Vec<TypedField>,
    indent: usize,
    depth: usize,
) {
    let pad = " ".repeat(indent);
    for field in fields {
        println!(
            "{pad}h 0x{:08X} {:<32} {}",
            field.hash,
            field.name.as_deref().unwrap_or("-"),
            field.value.describe()
        );
        if depth == 0 {
            continue;
        }
        if let Value::Array(items) = &field.value {
            for (k, item) in items.iter().enumerate() {
                let extra = match item {
                    Value::Instance(idx) => {
                        let info = &stu.instances[*idx];
                        let name = reg.class(info.hash).map(|c| c.name.as_str()).unwrap_or("?");
                        format!(
                            "  <- 0x{:08X} {name} embed field 0x{:08X} embed index {}",
                            info.hash, info.embedder_field, info.embedder_index
                        )
                    }
                    _ => String::new(),
                };
                println!("{pad}  [{k}] {}{extra}", item.describe());
            }
            continue;
        }
        let Value::Inline { start, bag } = &field.value else {
            continue;
        };
        if let Some(FieldType::Instance(child)) = reg.field(class_hash, field.hash).map(|d| &d.ty) {
            let nested = stu.typed_fields_at(*child, *start, *bag, reg);
            print_fields(stu, reg, *child, nested, indent + 2, depth - 1);
        }
    }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if args.is_empty() {
        eprintln!(
            "usage: ow-tank-stu --registry | --class 0xHASH | --guid 0xGUID [--instance N] [--fields] [--deep]"
        );
        return std::process::ExitCode::from(2);
    }

    let root = ow_casc::install::detect_root();
    let extra: Vec<PathBuf> = root.iter().cloned().collect();
    let sources = ow_tank::keytable::Sources::discover(&extra);
    let registry = TankRegistry::load_from_sources(&sources);
    if registry.is_empty() {
        eprintln!("no STU class checkout found; set OW_OWLIB to a checkout of overtools/OWLib");
    }

    if args.iter().any(|a| a == "--registry") {
        print_registry_stats(&registry);
        if flag("--guid").is_none() {
            return std::process::ExitCode::SUCCESS;
        }
    }

    if let Some(class) = flag("--class") {
        match parse_hex(&class) {
            Some(hash) => dump_class(&registry, hash as u32),
            None => {
                eprintln!("--class wants a hex hash");
                return std::process::ExitCode::from(2);
            }
        }
        return std::process::ExitCode::SUCCESS;
    }

    let guid_text = flag("--guid").or_else(|| {
        args.iter()
            .find(|a| a.starts_with("0x"))
            .filter(|a| !a.trim_start_matches("0x").is_empty())
            .cloned()
    });
    let Some(guid_text) = guid_text else {
        eprintln!("select --registry, --class 0xHASH or --guid 0xGUID");
        return std::process::ExitCode::from(2);
    };
    let Some(g) = parse_hex(&guid_text) else {
        eprintln!("--guid wants a hex GUID");
        return std::process::ExitCode::from(2);
    };
    let Some(root) = root else {
        eprintln!("no Overwatch install found; set OW_INSTALL");
        return std::process::ExitCode::from(2);
    };
    let data = match GameData::open(
        root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    ) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("open failed: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    let mut stu = match data.stu_of(g) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("0x{g:016x}: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    let only: Option<usize> = flag("--instance").and_then(|v| v.parse().ok());
    let show_fields = args.iter().any(|a| a == "--fields");
    let depth: usize = if args.iter().any(|a| a == "--deep") {
        1
    } else {
        0
    };
    println!(
        "0x{g:016x} type 0x{:03x} index {}: {} instance(s), {}",
        guid::type_of(g),
        guid::index_of(g),
        stu.instances.len(),
        if stu.is_v1() {
            "version 1"
        } else {
            "version 2"
        }
    );
    for i in 0..stu.instances.len() {
        if only.is_some_and(|n| n != i) {
            continue;
        }
        let hash = stu.instances[i].hash;
        let name = registry
            .inner()
            .class(hash)
            .map(|c| c.name.as_str())
            .unwrap_or("?");
        let record = match stu.v1_record(i) {
            Some(r) => format!(" record +{} flags 0x{:08x}", r.offset, r.flags),
            None => String::new(),
        };
        println!(
            "[{i:>4}] 0x{hash:08X} {name}{record}  payload {} B",
            stu.data(i).len()
        );
        if show_fields {
            let fields = stu.typed_fields(i, registry.inner());
            print_fields(&mut stu, registry.inner(), hash, fields, 10, depth);
        }
    }
    std::process::ExitCode::SUCCESS
}
