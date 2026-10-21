//! Throwaway probe: raw payload bytes and resolved layout of chosen STU
//! instances. Reads the install; writes nothing.
//!
//! usage: cargo run -p ow-tank --example stu_probe -- <graph hex> <instance>...

use ow_tank::{GameData, ManifestSelector};

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: stu_probe <graph hex guid> <instance>...");
        return std::process::ExitCode::from(2);
    }
    let graph = u64::from_str_radix(args[0].trim_start_matches("0x"), 16).unwrap();
    let want: Vec<usize> = args[1..].iter().map(|s| s.parse().unwrap()).collect();

    let root = ow_casc::install::detect_root().expect("no install");
    let sources = ow_tank::keytable::Sources::discover(&[root.clone()]);
    let data = GameData::open(
        &root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    )
    .expect("open failed");
    let mut stu = data.stu_of(graph).expect("no stu");

    for w in want {
        let info = &stu.instances[w];
        let start = stu.payload_start(w);
        let bag = stu.i32_at(start).unwrap_or(-1);
        println!(
            "inst {w}: hash=0x{:08X} embedder={} field=0x{:08X} size={} bag_index={bag}",
            info.hash, info.embedder_index, info.embedder_field, info.size
        );
        for f in stu.layout_at(start, bag) {
            println!("  field 0x{:08X} off={} size={}", f.hash, f.offset, f.size);
        }
        let bytes = stu.data(w).to_vec();
        let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
        println!("  payload[{}]: {}", bytes.len(), hex.join(" "));
    }
    std::process::ExitCode::SUCCESS
}
