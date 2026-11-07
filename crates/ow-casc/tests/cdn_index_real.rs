//! The client's CDN index footer and page layout against the install's real
//! index files.
//!
//! `FUN_1424d9af0` (`0x1424d9af0`) parses the last 20 bytes of an
//! `indices/<hash>.index`: version 0/1, checksum size <= 16, page size in KiB,
//! entry stride `offset + size + key`, `pageCount = ceil(elements /
//! entriesPerPage)`, and a total length of
//! `checksumSize + 20 + (checksumSize + keySize + pageSize) * pageCount`.
//! `FUN_1424d9c90` (`0x1424d9c90`) additionally checks the terminal checksum
//! and MD5(terminal + footer) against the index's own 16-byte key.
//!
//! This test opens the four indexes the install's CDN config names and pins
//! their exact geometry. It reads every page to compare the per-page MD5
//! array (`FUN_1424d9f20`), so the big archive group reads 260 MB. Skipped when
//! no install is present. Read-only.

use ow_casc::container::inspect_index;
use ow_casc::install::{detect_root, Install};

fn key(hex: &str) -> [u8; 16] {
    assert_eq!(hex.len(), 32);
    let mut out = [0u8; 16];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("hex key");
    }
    out
}

fn index_path(root: &std::path::Path, hex: &str) -> std::path::PathBuf {
    root.join("data")
        .join("casc")
        .join("indices")
        .join(format!("{hex}.index"))
}

#[test]
fn the_cdn_index_footer_and_page_layout_match_the_install() {
    let Some(root) = detect_root() else {
        eprintln!("note: no Overwatch install here; nothing to assert");
        return;
    };
    let install = Install::open(&root).expect("open install");
    let config = install.cdn_config.as_ref().expect("CDN config");

    // A staged patch makes the CDN config name an index that belongs to a build
    // the install has not activated yet (the agent writes the new index before
    // it swaps the executable). Comparing the pinned geometry against such an
    // index would pin a build that is not installed, so the pinned values are
    // only checked while the named indexes are no newer than the executable.
    let exe_mtime = [
        root.join("_retail_").join("Overwatch.exe"),
        root.join("Overwatch.exe"),
    ]
    .iter()
    .filter_map(|p| std::fs::metadata(p).ok())
    .filter_map(|m| m.modified().ok())
    .max();
    let mut staged: Vec<String> = Vec::new();

    // (config key, expected geometry). The four index files the config names
    // are the group, the patch group and the two loose-file indexes.
    let cases = [
        (
            "archive-group",
            260_173_908u64,
            6u8,
            4_096u32,
            26u32,
            157u32,
            63_149u32,
            9_914_328u32,
            157u32,
            92u32,
            "000109e353bd670d693a06737245b539",
            "fffffffb31ca64be552c4cc1f44456da",
        ),
        (
            "patch-archive-group",
            48_616_028,
            5,
            4_096,
            25,
            163,
            11_800,
            1_923_358,
            163,
            121,
            "00059826be04a848e36df3bfa6cc34f7",
            "fffffdfa3530b1764499cbfe44212070",
        ),
        (
            "file-index",
            576_828,
            0,
            4_096,
            20,
            204,
            140,
            28_374,
            204,
            18,
            "01c0ce1762fd4408b5909d4c4003cc1a",
            "ffff7c29a7f2e4adacce0ea2fc7f3414",
        ),
        (
            "patch-file-index",
            90_668,
            0,
            4_096,
            20,
            204,
            22,
            4_426,
            204,
            142,
            "0cbc31ef94a4e2df3afcfef3039a7f2d",
            "ffe466d253b2e46ee6dcc9bda0ac92e1",
        ),
    ];

    for (
        config_key,
        file_len,
        offset_bytes,
        page_size,
        entry_size,
        entries_per_page,
        page_count,
        element_count,
        first_page_entries,
        last_page_entries,
        first_key,
        last_key,
    ) in cases
    {
        let hex = config
            .get_str(config_key)
            .unwrap_or_else(|_| panic!("config has {config_key}"));
        let path = index_path(&root, hex);
        let stats =
            inspect_index(&path, Some(&key(hex))).unwrap_or_else(|e| panic!("{config_key}: {e}"));

        if let (Some(exe_mtime), Ok(meta)) = (exe_mtime, std::fs::metadata(&path)) {
            if let Ok(index_mtime) = meta.modified() {
                if index_mtime > exe_mtime {
                    staged.push(format!(
                        "{config_key} ({hex}) is newer than the executable: {index_mtime:?} > \
                         {exe_mtime:?}"
                    ));
                    continue;
                }
            }
        }

        assert_eq!(stats.file_len, file_len, "{config_key} file length");
        assert_eq!(
            stats.offset_bytes, offset_bytes,
            "{config_key} offset bytes"
        );
        assert_eq!(stats.size_bytes, 4, "{config_key} size bytes");
        assert_eq!(stats.key_bytes, 16, "{config_key} key bytes");
        assert_eq!(stats.checksum_size, 8, "{config_key} checksum bytes");
        assert_eq!(stats.page_size, page_size, "{config_key} page size");
        assert_eq!(stats.entry_size, entry_size, "{config_key} entry stride");
        assert_eq!(
            stats.entries_per_page, entries_per_page,
            "{config_key} entries per page"
        );
        assert_eq!(stats.page_count, page_count, "{config_key} page count");
        assert_eq!(stats.element_count, element_count, "{config_key} elements");
        assert_eq!(
            stats.first_page_entries, first_page_entries,
            "{config_key} first page entries"
        );
        assert_eq!(
            stats.last_page_entries, last_page_entries,
            "{config_key} last page entries"
        );
        assert_eq!(
            stats.first_last_key,
            key(first_key),
            "{config_key} first last-key"
        );
        assert_eq!(
            stats.final_last_key,
            key(last_key),
            "{config_key} final last-key"
        );
        assert!(
            stats.page_checksums_ok,
            "{config_key}: every page MD5 must match its stored checksum"
        );
        eprintln!(
            "{config_key}: {file_len} B, offsetBytes {offset_bytes}, pageSize {page_size}, \
             stride {entry_size}, {entries_per_page} entries/page, {page_count} pages, \
             {element_count} entries, last page {last_page_entries}"
        );
    }

    if !staged.is_empty() {
        eprintln!(
            "note: the install is mid-patch; {} of the named indexes belong to a build that is \
             not active yet, so their pinned geometry was not checked: {}",
            staged.len(),
            staged.join("; ")
        );
    }
}
