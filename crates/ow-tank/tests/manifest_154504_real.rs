//! The encrypted build-154504 content manifest: opened with the client's own
//! key/IV derivation, checked against the plaintext `WinPrism` manifest of the
//! same build, and the type-0x079 records enumerated.
//!
//! The type-0x079 prefix is **not** `STUMapHeader` (that is class `0x02514B6A`
//! / tool type `0x09F`); this test pins the 25 records the earlier notes
//! mislabelled, and reports that their payloads are not local.
//!
//! Skipped when no install is present. Read-only.

use ow_casc::{Install, Store};
use ow_tank::guid;
use ow_tank::keytable::Sources;
use ow_tank::manifest::{ManifestKind, Registry};

fn parse_key(hex: &str) -> Option<[u8; 16]> {
    if hex.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn key_from_root(root_file: &str, name: &str) -> Option<[u8; 16]> {
    root_file.lines().find_map(|line| {
        let fields: Vec<&str> = line.split('|').map(str::trim).collect();
        if fields.len() < 7 {
            return None;
        }
        if fields[5].ends_with(name) {
            parse_key(fields[1])
        } else {
            None
        }
    })
}

fn type_079_records(registry: &Registry) -> Vec<ow_tank::manifest::Record> {
    registry
        .records
        .iter()
        .filter(|r| guid::type_of(r.guid) == 0x079)
        .copied()
        .collect()
}

#[test]
fn encrypted_154504_manifest_is_sorted_and_agrees_on_the_type_079_records() {
    let Some(root) = ow_casc::install::detect_root() else {
        eprintln!("note: no Overwatch install here; nothing to assert");
        return;
    };
    let sources = Sources::discover(std::slice::from_ref(&root));
    if sources.executable.is_none() && sources.table_file.is_none() {
        eprintln!(
            "note: no Overwatch.exe or OW_KEYTABLE under {}; nothing to assert",
            root.display()
        );
        return;
    }
    if let Some(executable) = &sources.executable {
        assert!(executable.is_file());
    }
    let table = sources
        .table_for(ow_tank::manifest::CMF_BUILD_154504)
        .expect("the installed client's own table");
    assert_eq!(table.len(), 512);

    let install = Install::open_with_support_keyring(&root, sources.support_keyring.as_deref())
        .expect("open install");
    let store = Store::open(install).expect("open store");
    let root_file = store.root_file().expect("root file");

    let candidates =
        Registry::find_manifest_candidates(&root_file, ManifestKind::Content, "Win", "RDEV", None);
    let (name, content_key) = candidates
        .iter()
        .find(|(name, _)| name == "Win_SPWin_RDEV_EExt.cmf")
        .expect("the encrypted RDEV content manifest is listed");
    let registry =
        Registry::open(&store, name, content_key, &sources).expect("decrypt the manifest");
    assert_eq!(registry.header.build_version, 154504);
    assert!(
        registry.records.windows(2).all(|w| w[0].guid <= w[1].guid),
        "records must stay GUID-sorted"
    );
    assert!(
        registry.records.len() > 1_000_000,
        "the encrypted manifest should hold the full registry, got {}",
        registry.records.len()
    );

    let encrypted_maps = type_079_records(&registry);
    assert_eq!(
        encrypted_maps.len(),
        25,
        "the build registers 25 type-0x079 records"
    );
    let first = registry
        .get(0x01e0_0000_0000_0001)
        .expect("type-0x079 record 0x01e0000000000001");
    assert_eq!(first.size, 85);
    assert_eq!(guid::type_of(first.guid), 0x079);

    // A second encrypted manifest with a different name and data count: the IV
    // must follow the name and the header, not a constant.
    let text_candidates = Registry::find_manifest_candidates(
        &root_file,
        ManifestKind::Text,
        "Win",
        "RDEV",
        Some("enUS"),
    );
    let (text_name, text_key) = text_candidates
        .iter()
        .find(|(name, _)| name == "Win_SPWin_RDEV_LenUS_text_EExt.cmf")
        .expect("the encrypted enUS text manifest is listed");
    let text =
        Registry::open(&store, text_name, text_key, &sources).expect("decrypt the text manifest");
    assert_eq!(text.header.build_version, 154504);
    assert!(
        text.records.windows(2).all(|w| w[0].guid <= w[1].guid),
        "text-manifest records must stay GUID-sorted"
    );
    assert!(
        text.records.len() > 100_000,
        "the text manifest should hold its registry, got {}",
        text.records.len()
    );

    // The plaintext manifest of the same build is the independent check on the
    // decryption: the 25 records must be identical field for field.
    let prism_name = "WinPrism_SPWin_RDEV_EExt.cmf";
    let prism_key = key_from_root(&root_file, prism_name).expect("WinPrism manifest key");
    let prism = Registry::open(&store, prism_name, &prism_key, &sources).expect("open WinPrism");
    let prism_maps = type_079_records(&prism);
    assert_eq!(encrypted_maps.len(), prism_maps.len());
    for (enc, plain) in encrypted_maps.iter().zip(prism_maps.iter()) {
        assert_eq!(enc.guid, plain.guid);
        assert_eq!(enc.size, plain.size);
        assert_eq!(enc.unknown, plain.unknown);
        assert_eq!(enc.content_key, plain.content_key);
    }

    // Report (do not assert) whether the payload bytes themselves are local;
    // this is a property of the install's content split, not of the reader.
    let absent = encrypted_maps
        .iter()
        .filter(|r| store.open_ckey(&r.content_key).is_err())
        .count();
    println!(
        "manifest {name}: {} records, {} type-0x079 records, {absent}/{} type-0x079 payloads not local",
        registry.records.len(),
        encrypted_maps.len(),
        encrypted_maps.len()
    );
}
