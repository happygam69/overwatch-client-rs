//! The encoding file's client-defined section layout, walked over the
//! install's real blob.
//!
//! `EncodingSectionsRead` (`0x1424d1c60`) places five sections in a fixed
//! order; `ow-casc` computes them with the client's 32-bit arithmetic and
//! slices the decoded stream. This test opens the build's encoding manifest
//! through the store, parses it with [`EncodingFile`], then walks every page
//! index and every page body: 16,025 ckey->ekey pages and 10,519 ekey->espec
//! pages on the installed build.
//!
//! The lookup itself is `FUN_142498880` (the page-index binary search),
//! `FUN_142498540` (the ckey page scan) and `FUN_142498610` (the ekey page
//! scan); `FUN_142498700` is the ckey -> decoded-size/ekey/espec/stored-size
//! resolve built on them. None of those functions reads the page's 16-byte
//! index hash: the binary search uses the hash bytes only as part of its
//! `width + 16` entry stride. The MD5 relation is therefore a measured format
//! fact here, not a client check.
//!
//! Skipped when no install is present. Read-only. The load is ~0.6 s and the
//! walk ~3 s in a debug build.

use std::time::Instant;

use ow_casc::blte::md5_digest;
use ow_casc::encoding::{
    page_index_search, EncodingFile, EncodingHeader, Section, SectionLayout, PAGE_HASH_SIZE,
    PAGE_INDEX_ENTRY,
};
use ow_casc::{Install, Store};

fn hex_key(hex: &str) -> Option<[u8; 16]> {
    if hex.len() != 32 || !hex.is_ascii() {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Open the build's encoding manifest through the same two-key path
/// `Store::ensure_encoding` uses.
fn load_encoding() -> Option<Vec<u8>> {
    let root = ow_casc::install::detect_root()?;
    let install = Install::open(&root).ok()?;
    let values = install.build_config.get("encoding")?.to_vec();
    let store = Store::open(install).ok()?;
    let order: Vec<&String> = if values.len() >= 2 {
        vec![&values[1], &values[0]]
    } else {
        values.iter().collect()
    };
    for value in order {
        let Some(key) = hex_key(value) else { continue };
        if let Ok(blob) = store.open_ekey(&key) {
            return Some(blob);
        }
    }
    None
}

fn first_key(index: &[u8], page: usize) -> [u8; 16] {
    index[page * PAGE_INDEX_ENTRY..page * PAGE_INDEX_ENTRY + 16]
        .try_into()
        .expect("16-byte first key")
}

fn page_hash(index: &[u8], page: usize) -> [u8; 16] {
    let at = page * PAGE_INDEX_ENTRY + 16;
    index[at..at + PAGE_HASH_SIZE]
        .try_into()
        .expect("16-byte page hash")
}

/// A key strictly between `low` and `high`, or `None` when they are
/// adjacent. The client's page rule is "the last first key that does not
/// exceed the key", so this key must resolve to `low`'s page.
fn between(low: &[u8; 16], high: &[u8; 16]) -> Option<[u8; 16]> {
    for i in 0..16 {
        if low[i] == high[i] {
            continue;
        }
        assert!(low[i] < high[i], "first keys must increase");
        let mut key = *high;
        key[i] -= 1;
        for byte in &mut key[i + 1..] {
            *byte = 0xFF;
        }
        return (key > *low && key < *high).then_some(key);
    }
    None
}

/// Just above `key`, or `None` when it is the all-ones key.
fn above(key: &[u8; 16]) -> Option<[u8; 16]> {
    let mut out = *key;
    for byte in out.iter_mut().rev() {
        if *byte != 0xFF {
            *byte += 1;
            return Some(out);
        }
        *byte = 0;
    }
    None
}

fn assert_page_rule(index: &[u8], pages: usize, what: &str) {
    assert_eq!(
        page_index_search(index, pages as u32, 16, &[0u8; 16], 16),
        u32::MAX,
        "{what}: a key below the first page must miss"
    );
    let mut exact = 0usize;
    let mut previous = 0usize;
    let mut gaps = 0usize;
    for page in 0..pages {
        let key = first_key(index, page);
        assert_eq!(
            page_index_search(index, pages as u32, 16, &key, 16),
            page as u32,
            "{what}: page {page}'s first key must resolve to page {page}"
        );
        exact += 1;
        if page != 0 {
            let before = first_key(index, page - 1);
            assert_eq!(
                page_index_search(index, pages as u32, 16, &before, 16),
                (page - 1) as u32,
                "{what}: page {}'s first key must resolve to page {}",
                page - 1,
                page - 1
            );
            previous += 1;
            if let Some(key) = between(&before, &key) {
                assert_eq!(
                    page_index_search(index, pages as u32, 16, &key, 16),
                    (page - 1) as u32,
                    "{what}: a key between pages {} and {page} must resolve to page {}",
                    page - 1,
                    page - 1
                );
                gaps += 1;
            }
        }
    }
    let last = first_key(index, pages - 1);
    if let Some(key) = above(&last) {
        assert_eq!(
            page_index_search(index, pages as u32, 16, &key, 16),
            (pages - 1) as u32,
            "{what}: a key above the last first key must resolve to the last page"
        );
    }
    eprintln!(
        "{what}: {exact} exact first-key hits, {previous} previous-page hits, {gaps} between-page keys"
    );
}

#[test]
fn page_boundary_rule_holds_on_every_install_page() {
    let Some(blob) = load_encoding() else {
        eprintln!("note: no Overwatch install here; nothing to assert");
        return;
    };
    let encoding = EncodingFile::parse(&blob).expect("parse");
    let layout = *encoding.layout();
    let ck_index =
        &blob[layout.ckey_page_index.offset as usize..layout.ckey_page_index.end() as usize];
    let es_index =
        &blob[layout.ekey_page_index.offset as usize..layout.ekey_page_index.end() as usize];

    assert_page_rule(ck_index, 16_025, "ck");
    assert_page_rule(es_index, 10_519, "es");
}

#[test]
fn first_record_sizes_match_the_installed_file() {
    let Some(blob) = load_encoding() else {
        eprintln!("note: no Overwatch install here; nothing to assert");
        return;
    };
    let encoding = EncodingFile::parse(&blob).expect("parse");
    let layout = *encoding.layout();
    let ck_bodies =
        &blob[layout.ckey_page_bodies.offset as usize..layout.ckey_page_bodies.end() as usize];
    let body = &ck_bodies[..4096];
    assert_eq!(body[0], 1, "the first ckey page has one ekey");
    let decoded_size = u32::from_be_bytes([body[2], body[3], body[4], body[5]]);
    let ckey: [u8; 16] = body[6..22].try_into().unwrap();
    let ekey: [u8; 16] = body[22..38].try_into().unwrap();
    assert_eq!(hex(&ckey), "000029571a8ec20b5b56ba7719fbeb6f");
    assert_eq!(hex(&ekey), "9c34bebaac56e41a108d1953ee6c9199");
    assert_eq!(decoded_size, 524_304);

    assert_eq!(encoding.ekeys_for_ckey(&ckey), vec![ekey]);
    assert_eq!(encoding.decoded_size(&ckey), Some(524_304));
    assert_eq!(encoding.stored_size(&ekey), Some(3_530));
    assert_eq!(encoding.ekey_record_bytes(&ekey).map(|r| r.len()), Some(25));

    let root = ow_casc::install::detect_root().unwrap();
    let install = Install::open(&root).unwrap();
    let store = Store::open(install).unwrap();
    assert_eq!(store.decoded_size(&ckey).unwrap(), Some(524_304));
    assert_eq!(store.stored_size(&ekey).unwrap(), Some(3_530));
    let data = store.open_ekey(&ekey).expect("the first record opens");
    assert_eq!(
        data.len(),
        decoded_size as usize,
        "the ckey record's u32 is the decoded length"
    );
    assert_ne!(
        data.len(),
        3_530,
        "the ekey tail is the stored length, not the decoded one"
    );
}

#[test]
fn sampled_ckeys_resolve_to_their_page_records() {
    let Some(blob) = load_encoding() else {
        eprintln!("note: no Overwatch install here; nothing to assert");
        return;
    };
    let encoding = EncodingFile::parse(&blob).expect("parse");
    let layout = *encoding.layout();
    let ck_bodies =
        &blob[layout.ckey_page_bodies.offset as usize..layout.ckey_page_bodies.end() as usize];

    let mut sampled = 0usize;
    let mut multi = 0usize;
    let mut pages: Vec<usize> = (0..16_025).step_by(500).collect();
    pages.extend([7664, 13330]);
    for page in pages {
        let body = &ck_bodies[page * 4096..(page + 1) * 4096];
        let mut at = 0usize;
        while at + 22 <= body.len() {
            let count = body[at] as usize;
            if count == 0 {
                break;
            }
            let ckey: [u8; 16] = body[at + 6..at + 22].try_into().unwrap();
            let ekeys: Vec<[u8; 16]> = body[at + 22..at + 22 + count * 16]
                .chunks(16)
                .map(|c| c.try_into().unwrap())
                .collect();
            assert_eq!(
                encoding.ekeys_for_ckey(&ckey),
                ekeys,
                "ck page {page} record at {at}"
            );
            assert_eq!(
                encoding.decoded_size(&ckey),
                Some(u64::from(u32::from_be_bytes([
                    body[at + 2],
                    body[at + 3],
                    body[at + 4],
                    body[at + 5]
                ]))),
                "ck page {page} record at {at}: decoded size"
            );
            for ekey in &ekeys {
                let record = encoding.ekey_record_bytes(ekey).unwrap_or_else(|| {
                    panic!("ck page {page}: ekey {ekey:?} is not in the es pages")
                });
                let size = u32::from_be_bytes([record[21], record[22], record[23], record[24]]);
                assert_eq!(
                    encoding.stored_size(ekey),
                    Some(u64::from(size)),
                    "ck page {page}: stored size of {ekey:?}"
                );
                sampled += 1;
            }
            if count > 1 {
                multi += 1;
            }
            at += 22 + count * 16;
        }
    }
    eprintln!(
        "resolved {sampled} ekeys from {} sampled ckey pages ({multi} multi-ekey records)",
        sampled
    );
    assert!(sampled >= 35);
    assert_eq!(multi, 2);
}

#[test]
fn a_missing_ckey_is_absent() {
    let Some(blob) = load_encoding() else {
        eprintln!("note: no Overwatch install here; nothing to assert");
        return;
    };
    let encoding = EncodingFile::parse(&blob).expect("parse");
    assert!(encoding.ekeys_for_ckey(&[0u8; 16]).is_empty());
    assert!(encoding.ekeys_for_ckey(&[0xFFu8; 16]).is_empty());
    assert_eq!(encoding.decoded_size(&[0u8; 16]), None);
    assert_eq!(encoding.stored_size(&[0xFFu8; 16]), None);
}

#[test]
fn sections_and_pages_match_the_install() {
    let Some(blob) = load_encoding() else {
        eprintln!("note: no Overwatch install here; nothing to assert");
        return;
    };

    let header = EncodingHeader::from_bytes(&blob).expect("client-accepted header");
    assert_eq!(header.ckey_width(), 16);
    assert_eq!(header.ekey_width(), 16);
    assert_eq!(header.ckey_to_ekey_page_bytes(), 4096);
    assert_eq!(header.ekey_to_espec_page_bytes(), 4096);
    assert_eq!(header.ckey_to_ekey_page_count(), 16_025);
    assert_eq!(header.ekey_to_espec_page_count(), 10_519);
    assert_eq!(header.espec_block_size(), 1_484_507);

    let layout = SectionLayout::of(header.fields());
    assert_eq!(
        layout,
        SectionLayout {
            espec: Section {
                offset: 0x16,
                size: 1_484_507,
            },
            ckey_page_index: Section {
                offset: 1_484_529,
                size: 512_800,
            },
            ckey_page_bodies: Section {
                offset: 1_997_329,
                size: 65_638_400,
            },
            ekey_page_index: Section {
                offset: 67_635_729,
                size: 336_608,
            },
            ekey_page_bodies: Section {
                offset: 67_972_337,
                size: 43_085_824,
            },
        }
    );
    assert_eq!(layout.ekey_page_bodies.end(), 111_058_161);
    assert_eq!(blob.len(), 111_058_223);
    // The 62 bytes after the last section are the builder's own debug string
    // for the section sizes. The client reads only the declared sections.
    assert_eq!(
        &blob[111_058_161..],
        b"b:{22=n,1484507=z,512800=n,65638400=n,336608=n,43085824=n,*=z}"
    );

    let started = Instant::now();
    let encoding = EncodingFile::parse(&blob).expect("the shipped reader opens the install");
    eprintln!("parse took {:?}", started.elapsed());
    assert_eq!(encoding.layout(), &layout);
    assert_eq!(encoding.ckey_page_count(), 16_025);
    assert_eq!(encoding.ekey_page_count(), 10_519);
    assert_eq!(encoding.espec_entry_count(), 29_694);
    assert_eq!(encoding.espec().last(), Some(&0));

    let ck_index =
        &blob[layout.ckey_page_index.offset as usize..layout.ckey_page_index.end() as usize];
    let ck_bodies =
        &blob[layout.ckey_page_bodies.offset as usize..layout.ckey_page_bodies.end() as usize];
    let es_index =
        &blob[layout.ekey_page_index.offset as usize..layout.ekey_page_index.end() as usize];
    let es_bodies =
        &blob[layout.ekey_page_bodies.offset as usize..layout.ekey_page_bodies.end() as usize];
    assert_eq!(ck_index.len(), 16_025 * PAGE_INDEX_ENTRY);
    assert_eq!(ck_bodies.len(), 16_025 * 4096);
    assert_eq!(es_index.len(), 10_519 * PAGE_INDEX_ENTRY);
    assert_eq!(es_bodies.len(), 10_519 * 4096);

    let started = Instant::now();
    let mut ck_entries = 0usize;
    let mut ck_ekeys = 0usize;
    let mut ck_decoded_bytes = 0u64;
    let mut full_ck_pages = Vec::new();
    let mut multi_ekey = Vec::new();
    let mut previous: Option<[u8; 16]> = None;
    for page in 0..16_025 {
        let page_key = first_key(ck_index, page);
        if let Some(previous) = previous {
            assert!(
                previous < page_key,
                "ck page {page}: first keys are not strictly increasing"
            );
        }
        previous = Some(page_key);

        let body = &ck_bodies[page * 4096..(page + 1) * 4096];
        assert_eq!(
            page_hash(ck_index, page),
            md5_digest(body),
            "ck page {page}: index hash is not MD5 of the body"
        );
        let mut at = 0usize;
        let mut body_first: Option<&[u8]> = None;
        let mut entries_on_page = 0usize;
        let mut saw_zero = false;
        while at + 22 <= body.len() {
            // The client reads the low byte as the count and terminator; the
            // high byte is never read.
            let count = body[at] as usize;
            assert_eq!(
                body[at + 1],
                0,
                "ck page {page}: record at {at} uses the high count byte"
            );
            if count == 0 {
                saw_zero = true;
                break;
            }
            assert!(
                count > 0 && at + 22 + count * 16 <= body.len(),
                "ck page {page}: entry at {at} with {count} ekeys overruns the page"
            );
            if body_first.is_none() {
                body_first = Some(&body[at + 6..at + 22]);
            }
            ck_decoded_bytes += u64::from(u32::from_be_bytes([
                body[at + 2],
                body[at + 3],
                body[at + 4],
                body[at + 5],
            ]));
            if count > 1 {
                multi_ekey.push((page, entries_on_page, count));
            }
            at += 22 + count * 16;
            entries_on_page += 1;
            ck_ekeys += count;
        }
        assert_eq!(
            body_first,
            Some(&page_key[..]),
            "ck page {page}: index first key does not match the first record"
        );
        if !saw_zero {
            assert!(
                at + 22 > body.len(),
                "ck page {page}: stopped before the end without a zero count"
            );
            full_ck_pages.push((page, at));
        }
        ck_entries += entries_on_page;
    }
    assert_eq!(ck_entries, 1_714_571);
    assert_eq!(ck_ekeys, 1_714_573);
    assert_eq!(ck_decoded_bytes, 193_863_820_123);
    assert_eq!(full_ck_pages, vec![(7664, 4082), (13330, 4082)]);
    assert_eq!(multi_ekey, vec![(7664, 32, 2), (13330, 82, 2)]);

    let mut es_entries = 0usize;
    let mut terminated_pages = Vec::new();
    let mut espec_index_seen = vec![false; encoding.espec_entry_count()];
    let mut stored_bytes = 0u64;
    previous = None;
    for page in 0..10_519 {
        let page_key = first_key(es_index, page);
        if let Some(previous) = previous {
            assert!(
                previous < page_key,
                "es page {page}: first keys are not strictly increasing"
            );
        }
        previous = Some(page_key);

        let body = &es_bodies[page * 4096..(page + 1) * 4096];
        assert_eq!(
            page_hash(es_index, page),
            md5_digest(body),
            "es page {page}: index hash is not MD5 of the body"
        );
        let mut at = 0usize;
        let mut body_first: Option<&[u8]> = None;
        while at + 25 <= body.len() {
            if body_first.is_none() {
                body_first = Some(&body[at..at + 16]);
            }
            let espec =
                u32::from_be_bytes([body[at + 16], body[at + 17], body[at + 18], body[at + 19]]);
            // `FUN_142498700` reads bytes 21..25, the low four bytes of the
            // u40 tail; the fifth byte at 20 is not read.
            let size =
                u32::from_be_bytes([body[at + 21], body[at + 22], body[at + 23], body[at + 24]]);
            assert_eq!(
                body[at + 20],
                0,
                "es page {page}: record at {at} has a nonzero fifth size byte"
            );
            at += 25;
            if espec == 0xFFFF_FFFF {
                terminated_pages.push(page);
                break;
            }
            let index = espec as usize;
            assert!(
                index < espec_index_seen.len(),
                "es page {page}: espec index {index} is outside the 29,694-string block"
            );
            espec_index_seen[index] = true;
            stored_bytes += u64::from(size);
            es_entries += 1;
        }
        assert_eq!(
            body_first,
            Some(&page_key[..]),
            "es page {page}: index first key does not match the first record"
        );
    }
    assert_eq!(es_entries, 1_714_573);
    assert_eq!(terminated_pages, vec![10_518]);
    assert!(
        espec_index_seen.iter().all(|seen| *seen),
        "the ekey->espec pages do not reference every string in the espec block"
    );
    assert_eq!(stored_bytes, 99_356_850_082);

    eprintln!(
        "walked 16025 ck pages ({ck_entries} entries, {ck_ekeys} ekeys, \
         {ck_decoded_bytes} decoded bytes), 10519 es pages ({es_entries} entries, \
         {stored_bytes} stored bytes), 29694 espec strings, in {:?}",
        started.elapsed()
    );
    eprintln!(
        "first page: ckey {} hash {}; first es key {}",
        hex(&first_key(ck_index, 0)),
        hex(&page_hash(ck_index, 0)),
        hex(&first_key(es_index, 0))
    );
}
