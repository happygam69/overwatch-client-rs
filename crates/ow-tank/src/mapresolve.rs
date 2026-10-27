//! Map headers, map variations and the chunk set of one map asset.
//!
//! The chain read from the client:
//!
//! ```text
//! STUMapHeader (type 0x09F, class 0x02514B6A)
//!   +0x0A0  m_map              the base map asset (type 0x002)
//!   +0x0D0  m_supportedGameModes
//!   +0x0F0  m_D97BC44F         one mode entry per variation
//!   +0x110  m_78715D57         one STU_7FB10A24 per variation
//!   +0x150  m_364ADD04         the layer key shared by the variations
//! ```
//!
//! The field offsets are the ones the client registers at
//! `RegisterStuMapHeaderFields` (`0x14143b5c0`) and `RegisterStuMapHeaderTail`
//! (`0x14143bbe5`); the class is registered at `RegisterStuMapHeader`
//! (`0x14031c580`). The two arrays are paired **by index**: on this install
//! `m_78715D57` and `m_D97BC44F` always have the same length (the practice
//! header 3/3, Cairo 10/10, header `0x07900000000000d4` 63/63), while
//! `m_C0CA4671` is a different, shorter display list (21 entries for that
//! 63-variation header; Cairo's 10 are in another order). Each variation's own
//! map asset is `m_BF231F12`, a raw `u64` naming a registered type-`0x002`
//! asset; `m_364ADD04` is the layer key (the same value as the header's for a
//! map whose variations share one layer, a different value when they do not).
//!
//! The map asset itself is not STU. The client's loader `LoadMapAssetChunks`
//! (`0x141ff5c80`) reads four stored chunk references and then synthesises the
//! rest:
//!
//! ```text
//! +0x1B0  u32  count of the extra GUID list
//! +0x1B4  u32  offset of the GUID list, relative to the map asset's start
//! +0x1B8  u64  a chunk GUID (kind 0x11 on every map seen)
//! +0x1C0  u64  a chunk GUID (kind 0x12 on every map seen)
//! +0x1C8  u64  a chunk GUID (kind 0x23 on every map seen)
//! ```
//!
//! It requests, in this order: `+0x1B8`, the `+0x1B0` list, `+0x1C0`,
//! `+0x1C8`, then the client's table slots `0x1F`, `0x20`, `0x23`, then
//! `1..=0x1C`. The last 31 GUIDs are the ones [`crate::mapguid`] synthesises
//! from the map asset's low 32 bits. A stored GUID whose value is zero is
//! skipped by the client (`FUN_141fd3950`, `0x141fd3950`), and a chunk the
//! manifest does not carry simply does not open.
//!
//! This is the whole "second chunk path": kinds `0x10` (the `+0x1B0` list),
//! `0x11` (`+0x1B8`), `0x12` (`+0x1C0`) and `0x23` (`+0x1C8`) are stored in the
//! map asset, not derived from the 31 table slots. The training-room practice
//! variant stores the base map's collision/pathing chunks there, which is why
//! its scene is complete while the base map asset alone has no visual chunks.
//!
//! **Open:** the client function that *selects* a variation for a game mode is
//! not read yet. The pairing above is the file's own invariant and the query
//! [`Header::variants_for_mode`] only reports the entries that name a mode; it
//! does not invent a default. See `docs/open-questions.md` item 11b.

use std::fmt;

use ow_stu::Stu;

use crate::mapchunk::{Kind, MapChunk};
use crate::mapguid;
use crate::{guid, text, GameData};

/// `STUMapHeader` (registered at `0x14031c580`), size `0x240`.
pub const MAP_HEADER_CLASS: u32 = 0x0251_4B6A;
/// The map header's asset type (`STUMapHeader`).
pub const MAP_HEADER_TYPE: u16 = 0x09F;
/// `m_map` on `STUMapHeader`.
pub const FIELD_MAP: u32 = 0x4E87_690F;
/// `m_mapName` on `STUMapHeader`.
pub const FIELD_MAP_NAME: u32 = 0x506F_A8D8;
/// `m_D97BC44F`: the mode entry array paired with the variations.
pub const FIELD_VARIATION_MODES: u32 = 0xD97B_C44F;
/// `m_78715D57`: the variation (map layer) array.
pub const FIELD_VARIATIONS: u32 = 0x7871_5D57;
/// `m_364ADD04`: the header's layer key.
pub const FIELD_LAYER: u32 = 0x364A_DD04;
/// `m_gameMode` on a `STU_71B2D30A` mode entry.
pub const FIELD_GAME_MODE: u32 = 0xEB4F_2408;
/// `m_A9253C68` on a `STU_71B2D30A` mode entry.
pub const FIELD_MODE_VARIANT: u32 = 0xA925_3C68;
/// `m_364ADD04` on a `STU_7FB10A24` variation: the variation's layer key.
pub const FIELD_VARIATION_LAYER: u32 = 0x364A_DD04;
/// `m_BF231F12` on a `STU_7FB10A24` variation: the variation's map asset.
pub const FIELD_VARIATION_MAP: u32 = 0xBF23_1F12;
/// `m_resourceKey` on a `STU_7FB10A24` variation.
pub const FIELD_VARIATION_RESOURCE_KEY: u32 = 0x38BF_B46C;

/// `STUGameMode` (`0x494C33C4`), the asset `m_gameMode` names.
pub const GAME_MODE_CLASS: u32 = 0x494C_33C4;
/// `displayName` on `STUGameMode` (a `teString` asset reference).
pub const FIELD_GAME_MODE_DISPLAY_NAME: u32 = 0x5DB9_1CE2;

/// The training-room practice header's `m_mapName`, byte for byte.
pub const PRACTICE_MAP_NAME: &str = "Maps\\TrainingRoom\\Training Room - Practice";
/// The mode display name a practice-range launch selects by.
pub const PRACTICE_MODE_NAME: &str = "Practice Range";

/// The map asset fields the loader reads, all relative to the asset's start.
const MAP_EXTRA_COUNT: usize = 0x1B0;
const MAP_EXTRA_OFFSET: usize = 0x1B4;
const MAP_CHUNK_A: usize = 0x1B8;
const MAP_CHUNK_B: usize = 0x1C0;
const MAP_CHUNK_C: usize = 0x1C8;
const MAP_ASSET_MIN: usize = 0x1D0;
const MAX_EXTRA: u32 = 4096;

/// The map loader's table-slot order: `0x1F`, `0x20`, `0x23`, then `1..=0x1C`.
pub const LOADER_SLOT_ORDER: [usize; 31] = [
    0x1F, 0x20, 0x23, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21,
    22, 23, 24, 25, 26, 27, 28,
];

#[derive(Debug)]
pub enum Error {
    Asset(crate::Error),
    /// The asset opened but does not carry `STUMapHeader`.
    NotAMapHeader(u64),
    /// The GUID is not a type-`0x002` map asset.
    NotAMapAsset(u64),
    /// The map asset is shorter than the fixed fields the loader reads.
    MapTooShort {
        len: usize,
    },
    /// The extra-GUID list's count/offset does not fit the map asset.
    BadExtraList {
        count: u32,
        offset: u32,
        len: usize,
    },
    /// The mode and variation arrays are not the same length.
    UnpairedVariations {
        modes: usize,
        variations: usize,
    },
    /// No type-`0x09F` header carries the requested `m_mapName`.
    NoMapNamed {
        name: String,
        scanned: usize,
        unreadable: usize,
    },
    /// More than one type-`0x09F` header carries the requested `m_mapName`.
    DuplicateMapName {
        name: String,
        headers: Vec<u64>,
    },
    /// The header has no variation whose mode display name is the requested one.
    NoVariationNamed {
        header: u64,
        map_name: String,
        wanted: String,
        modes: Vec<String>,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Asset(e) => write!(f, "{e}"),
            Error::NotAMapHeader(guid) => {
                write!(f, "0x{guid:016x} does not carry STUMapHeader (0x02514B6A)")
            }
            Error::NotAMapAsset(guid) => {
                write!(f, "0x{guid:016x} is not a type-0x002 map asset")
            }
            Error::MapTooShort { len } => {
                write!(
                    f,
                    "map asset is {len} B, shorter than the 0x1D0 the loader reads"
                )
            }
            Error::BadExtraList { count, offset, len } => write!(
                f,
                "map extra-GUID list is {count} entries at offset {offset} in a {len}-byte asset"
            ),
            Error::UnpairedVariations { modes, variations } => write!(
                f,
                "map header has {modes} mode entries but {variations} variation entries"
            ),
            Error::NoMapNamed {
                name,
                scanned,
                unreadable,
            } => write!(
                f,
                "no type-0x09F map header names {name:?} (scanned {scanned}, {unreadable} unreadable)"
            ),
            Error::DuplicateMapName { name, headers } => write!(
                f,
                "{} type-0x09F map headers name {name:?}: {}",
                headers.len(),
                headers
                    .iter()
                    .map(|g| format!("0x{g:016x}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Error::NoVariationNamed {
                header,
                map_name,
                wanted,
                modes,
            } => write!(
                f,
                "map header 0x{header:016x} ({map_name:?}) has no variation whose mode display name is {wanted:?}; modes: {}",
                if modes.is_empty() {
                    "none readable".to_string()
                } else {
                    modes.join(", ")
                }
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<crate::Error> for Error {
    fn from(e: crate::Error) -> Self {
        Error::Asset(e)
    }
}

type Result<T> = std::result::Result<T, Error>;

fn rd_u32(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn rd_u64(bytes: &[u8], at: usize) -> Option<u64> {
    bytes
        .get(at..at + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

/// One selectable variation of a map header.
///
/// `game_mode` comes from the paired `m_D97BC44F` entry; `layer`, `map` and
/// `resource_key` come from `m_78715D57[index]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Variation {
    pub index: usize,
    pub game_mode: u64,
    pub layer: u64,
    pub map: u64,
    pub resource_key: u64,
}

/// A parsed `STUMapHeader`.
pub struct Header {
    stu: Stu,
    instance: usize,
}

impl Header {
    /// Open a header through the client's asset path and find its instance.
    pub fn open(data: &GameData, guid: u64) -> Result<Header> {
        let stu = data.stu_of(guid)?;
        let instance = stu
            .instance(MAP_HEADER_CLASS)
            .ok_or(Error::NotAMapHeader(guid))?;
        Ok(Header { stu, instance })
    }

    pub fn map_name(&mut self) -> Option<String> {
        self.stu.read_string(self.instance, FIELD_MAP_NAME)
    }

    /// The base map asset named by `m_map`.
    pub fn map(&mut self) -> Option<u64> {
        self.stu.read_asset_ref(self.instance, FIELD_MAP)
    }

    /// The header's own layer key (`m_364ADD04`).
    pub fn layer(&mut self) -> Option<u64> {
        self.stu.read_asset_ref(self.instance, FIELD_LAYER)
    }

    /// The mode and variation arrays paired by index.
    ///
    /// The client's file keeps one `m_D97BC44F` entry per `m_78715D57` entry;
    /// a header whose lengths differ is reported instead of truncated.
    pub fn variations(&self) -> Result<Vec<Variation>> {
        let modes = self
            .stu
            .inline_elements(self.instance, FIELD_VARIATION_MODES);
        let variants = self.stu.inline_elements(self.instance, FIELD_VARIATIONS);
        if modes.len() != variants.len() {
            return Err(Error::UnpairedVariations {
                modes: modes.len(),
                variations: variants.len(),
            });
        }
        let mut out = Vec::with_capacity(variants.len());
        for (index, ((mode_bag, mode_at), (variant_bag, variant_at))) in
            modes.iter().zip(variants.iter()).enumerate()
        {
            out.push(Variation {
                index,
                game_mode: self
                    .stu
                    .read_asset_ref_at(*mode_at, *mode_bag, FIELD_GAME_MODE)
                    .unwrap_or(0),
                layer: self
                    .stu
                    .read_asset_ref_at(*variant_at, *variant_bag, FIELD_VARIATION_LAYER)
                    .unwrap_or(0),
                map: self
                    .stu
                    .read_u64_at(*variant_at, *variant_bag, FIELD_VARIATION_MAP)
                    .unwrap_or(0),
                resource_key: self
                    .stu
                    .read_asset_ref_at(*variant_at, *variant_bag, FIELD_VARIATION_RESOURCE_KEY)
                    .unwrap_or(0),
            });
        }
        Ok(out)
    }

    /// Every variation whose paired mode entry names `game_mode`.
    ///
    /// This is a query over the header's own arrays, not a default: a mode may
    /// name several variations (Cairo names 63 variations for 10 modes), and
    /// the client's choice among them is not read yet.
    pub fn variations_for_mode(&self, game_mode: u64) -> Result<Vec<Variation>> {
        Ok(self
            .variations()?
            .into_iter()
            .filter(|v| v.game_mode == game_mode)
            .collect())
    }
}

/// The display name of a `STUGameMode` asset (`m_gameMode`'s asset).
///
/// The name is the `displayName` field's `teString` asset, read through the
/// client's own open. `None` when the mode asset or its text is not readable.
pub fn game_mode_display_name(data: &GameData, game_mode: u64) -> Option<String> {
    let mut mode = data.stu_of(game_mode).ok()?;
    let instance = mode.instance(GAME_MODE_CLASS)?;
    let text_guid = mode.read_asset_ref(instance, FIELD_GAME_MODE_DISPLAY_NAME)?;
    if text_guid == 0 {
        return None;
    }
    let bytes = data.open_asset(text_guid).ok()?;
    text::parse_display_text(&bytes).ok().map(|t| t.value)
}

/// One variation as `--list-maps` reports it.
#[derive(Debug, Clone)]
pub struct VariationSummary {
    pub index: usize,
    pub game_mode: u64,
    pub mode_name: Option<String>,
    pub scene_map: u64,
}

/// One type-`0x09F` header as `--list-maps` reports it.
#[derive(Debug, Clone)]
pub struct MapSummary {
    pub header: u64,
    pub map_name: String,
    pub base_map: Option<u64>,
    pub variations: Vec<VariationSummary>,
}

/// Every readable type-`0x09F` header and its paired variation modes.
///
/// A header that does not open or carries no `mapName` is left out; the caller
/// gets the readable set, not a guessed entry.
pub fn list_maps(data: &GameData) -> Vec<MapSummary> {
    let mut guids = data.guids_of_type(MAP_HEADER_TYPE);
    guids.sort_unstable();
    let mut mode_names: std::collections::HashMap<u64, Option<String>> =
        std::collections::HashMap::new();
    let mut out = Vec::new();
    for guid in guids {
        let Ok(mut header) = Header::open(data, guid) else {
            continue;
        };
        let Some(map_name) = header.map_name() else {
            continue;
        };
        let base_map = header.map();
        let Ok(variations) = header.variations() else {
            continue;
        };
        let mut listed = Vec::with_capacity(variations.len());
        for v in variations {
            let mode_name = mode_names
                .entry(v.game_mode)
                .or_insert_with(|| game_mode_display_name(data, v.game_mode))
                .clone();
            listed.push(VariationSummary {
                index: v.index,
                game_mode: v.game_mode,
                mode_name,
                scene_map: v.map,
            });
        }
        out.push(MapSummary {
            header: guid,
            map_name,
            base_map,
            variations: listed,
        });
    }
    out.sort_by(|a, b| {
        a.map_name
            .cmp(&b.map_name)
            .then_with(|| a.header.cmp(&b.header))
    });
    out
}

/// A type-`0x09F` header found by its `m_mapName`, with its paired modes.
#[derive(Debug)]
pub struct NamedMap {
    pub header: u64,
    pub map_name: String,
    pub base_map: Option<u64>,
    pub variations: Vec<Variation>,
    /// The paired mode's display name, index for index with `variations`.
    pub mode_names: Vec<Option<String>>,
}

/// Every readable type-`0x09F` header whose `m_mapName` is exactly `name`.
///
/// The result is one header or an error: zero, or more than one, is never
/// silently narrowed to a guess.
pub fn find_map_by_name(data: &GameData, name: &str) -> Result<NamedMap> {
    let guids = data.guids_of_type(MAP_HEADER_TYPE);
    let scanned = guids.len();
    let mut unreadable = 0usize;
    let mut matches = Vec::new();
    for guid in guids {
        let Ok(mut header) = Header::open(data, guid) else {
            unreadable += 1;
            continue;
        };
        let Some(map_name) = header.map_name() else {
            unreadable += 1;
            continue;
        };
        if map_name != name {
            continue;
        }
        let base_map = header.map();
        let variations = header.variations()?;
        let mode_names: Vec<Option<String>> = variations
            .iter()
            .map(|v| game_mode_display_name(data, v.game_mode))
            .collect();
        matches.push(NamedMap {
            header: guid,
            map_name,
            base_map,
            variations,
            mode_names,
        });
    }

    match matches.len() {
        0 => Err(Error::NoMapNamed {
            name: name.to_string(),
            scanned,
            unreadable,
        }),
        1 => Ok(matches.pop().expect("one match")),
        _ => {
            let mut headers: Vec<u64> = matches.iter().map(|m| m.header).collect();
            headers.sort_unstable();
            Err(Error::DuplicateMapName {
                name: name.to_string(),
                headers,
            })
        }
    }
}

/// The header and variation a practice-range launch opens.
#[derive(Debug, Clone)]
pub struct PracticeRange {
    pub header: u64,
    pub map_name: String,
    pub base_map: Option<u64>,
    pub variation: Variation,
    /// The header's total variation count, so a caller can say "variation 0 of
    /// N" without re-opening the header.
    pub variation_count: usize,
    pub mode_name: String,
    pub scene_map: u64,
    /// Every variation whose mode display name is [`PRACTICE_MODE_NAME`], in
    /// header order. The launch uses the first; the client's own selector is
    /// still open, so the rest are reported rather than hidden.
    pub matching_variations: Vec<usize>,
}

/// Find the practice-range header by `m_mapName` and its first
/// `"Practice Range"` variation in header order.
///
/// This is a data query over the install's own headers and mode assets. It does
/// not use a GUID seed: the header is the one whose `m_mapName` is
/// [`PRACTICE_MAP_NAME`], and the variation is the first whose paired mode's
/// `STUGameMode.displayName` is exactly [`PRACTICE_MODE_NAME`]. A missing or
/// duplicate header, or a header with no such variation, is an error.
pub fn find_practice_range(data: &GameData) -> Result<PracticeRange> {
    let named = find_map_by_name(data, PRACTICE_MAP_NAME)?;
    let variation_count = named.variations.len();
    let matching_variations: Vec<usize> = named
        .variations
        .iter()
        .zip(named.mode_names.iter())
        .filter(|(_, name)| name.as_deref() == Some(PRACTICE_MODE_NAME))
        .map(|(v, _)| v.index)
        .collect();
    if matching_variations.is_empty() {
        return Err(Error::NoVariationNamed {
            header: named.header,
            map_name: named.map_name,
            wanted: PRACTICE_MODE_NAME.to_string(),
            modes: named
                .mode_names
                .iter()
                .map(|n| n.clone().unwrap_or_else(|| "<unreadable>".to_string()))
                .collect(),
        });
    }
    let index = matching_variations[0];
    let variation = named.variations[index];
    Ok(PracticeRange {
        header: named.header,
        map_name: named.map_name,
        base_map: named.base_map,
        variation,
        variation_count,
        mode_name: PRACTICE_MODE_NAME.to_string(),
        scene_map: variation.map,
        matching_variations,
    })
}

/// A practice-range launch: the selected header/variation and its resolved
/// chunk scene.
pub struct PracticeScene {
    pub range: PracticeRange,
    pub scene: Scene,
}

/// Resolve the practice-range scene through the header and map loader.
pub fn resolve_practice_range(data: &GameData, kinds: &[u64]) -> Result<PracticeScene> {
    let range = find_practice_range(data)?;
    let scene = resolve_scene(data, range.scene_map, kinds)?;
    Ok(PracticeScene { range, scene })
}

/// A type-`0x002` map asset, read as `FUN_141ff5c80` reads it.
pub struct MapAsset {
    pub guid: u64,
    bytes: Vec<u8>,
}

impl MapAsset {
    pub fn open(data: &GameData, guid: u64) -> Result<MapAsset> {
        if !is_map_asset(guid) {
            return Err(Error::NotAMapAsset(guid));
        }
        let bytes = data.open_asset(guid)?;
        if bytes.len() < MAP_ASSET_MIN {
            return Err(Error::MapTooShort { len: bytes.len() });
        }
        Ok(MapAsset { guid, bytes })
    }

    /// The four stored references in the loader's order:
    /// `+0x1B8`, the `+0x1B0` list, `+0x1C0`, `+0x1C8`.
    ///
    /// Zero is the client's "nothing here" value and is not returned.
    pub fn stored_chunks(&self) -> Result<Vec<u64>> {
        let count = rd_u32(&self.bytes, MAP_EXTRA_COUNT).unwrap_or(0);
        let offset = rd_u32(&self.bytes, MAP_EXTRA_OFFSET).unwrap_or(0) as usize;
        if count > MAX_EXTRA {
            return Err(Error::BadExtraList {
                count,
                offset: offset as u32,
                len: self.bytes.len(),
            });
        }
        let need = (count as usize)
            .checked_mul(8)
            .and_then(|n| offset.checked_add(n))
            .ok_or(Error::BadExtraList {
                count,
                offset: offset as u32,
                len: self.bytes.len(),
            })?;
        if count > 0 && need > self.bytes.len() {
            return Err(Error::BadExtraList {
                count,
                offset: offset as u32,
                len: self.bytes.len(),
            });
        }

        let mut out = Vec::with_capacity(count as usize + 3);
        let a = rd_u64(&self.bytes, MAP_CHUNK_A).filter(|g| *g != 0);
        let b = rd_u64(&self.bytes, MAP_CHUNK_B).filter(|g| *g != 0);
        let c = rd_u64(&self.bytes, MAP_CHUNK_C).filter(|g| *g != 0);
        out.extend(a);
        for i in 0..count as usize {
            if let Some(g) = rd_u64(&self.bytes, offset + i * 8).filter(|g| *g != 0) {
                out.push(g);
            }
        }
        out.extend(b);
        out.extend(c);
        Ok(out)
    }

    /// Every chunk GUID the loader requests for this map, in its order.
    ///
    /// `kinds` is the client's `0x24`-entry chunk-kind table
    /// ([`crate::keytable::Sources::chunk_kinds`]); a slot the table does not
    /// cover is skipped, as the loader checks the index before calling the
    /// formula.
    pub fn requested_chunks(&self, kinds: &[u64]) -> Result<Vec<u64>> {
        let mut out = self.stored_chunks()?;
        for slot in LOADER_SLOT_ORDER {
            let Some(kind) = kinds.get(slot) else {
                continue;
            };
            out.push(mapguid::chunk_guid(self.guid, *kind));
        }
        let mut seen = std::collections::HashSet::new();
        out.retain(|g| seen.insert(*g));
        Ok(out)
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// One requested chunk and what the install supplied for it.
pub struct SceneChunk {
    pub guid: u64,
    /// The kind byte in the asset GUID's bits 32..39.
    pub kind: u8,
    pub size: usize,
    /// The placeable walk; `None` when the chunk is not a placeable chunk or
    /// its body did not parse.
    pub chunk: Option<MapChunk>,
    /// Why the chunk did not parse, when it opened but the body did not.
    pub parse_error: Option<String>,
}

impl SceneChunk {
    pub fn placeables(&self) -> usize {
        self.chunk.as_ref().map(|c| c.placeables.len()).unwrap_or(0)
    }
}

/// A map's chunk set, opened and walked.
pub struct Scene {
    pub map: u64,
    /// Every GUID the loader requests, in the loader's order.
    pub requested: Vec<u64>,
    /// The requested chunks the install supplied, in request order.
    pub chunks: Vec<SceneChunk>,
    /// The requested GUIDs the manifest does not register.
    pub unregistered: Vec<u64>,
    /// The requested GUIDs that are registered but did not open.
    pub unopened: Vec<u64>,
    pub placeables: usize,
    pub models: usize,
    pub entities: usize,
    /// Min/max of the placeable translations that differ from the zero default,
    /// when any chunk was walked.
    pub bounds: Option<([f32; 3], [f32; 3])>,
}

/// Open a map asset and every chunk the client's loader requests for it.
///
/// A chunk that does not open is recorded in [`Scene::unregistered`] or
/// [`Scene::unopened`], never invented. A placeable body that does not parse is
/// recorded on its [`SceneChunk`].
pub fn resolve_scene(data: &GameData, map: u64, kinds: &[u64]) -> Result<Scene> {
    let asset = MapAsset::open(data, map)?;
    let requested = asset.requested_chunks(kinds)?;
    let mut chunks = Vec::new();
    let mut unregistered = Vec::new();
    let mut unopened = Vec::new();
    let mut placeables = 0usize;
    let mut models = 0usize;
    let mut entities = 0usize;
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    let mut have_bounds = false;

    for guid in &requested {
        if data.registry().get(*guid).is_none() {
            unregistered.push(*guid);
            continue;
        }
        let bytes = match data.open_asset(*guid) {
            Ok(b) => b,
            Err(_) => {
                unopened.push(*guid);
                continue;
            }
        };
        let kind = ((guid >> 32) & 0xFF) as u8;
        let declared = Kind::from_type_id(kind).unwrap_or(Kind::Unknown);
        let (chunk, parse_error) = match MapChunk::parse(&bytes, declared) {
            Ok(c) => (Some(c), None),
            Err(e) => match MapChunk::parse(&bytes, Kind::Unknown) {
                Ok(c) => (Some(c), Some(e.to_string())),
                Err(e2) => (None, Some(e2.to_string())),
            },
        };
        if let Some(c) = &chunk {
            placeables += c.placeables.len();
            for p in &c.placeables {
                if p.model.is_some() {
                    models += 1;
                }
                if p.entity.is_some() {
                    entities += 1;
                }
                if p.translation != [0.0, 0.0, 0.0] {
                    have_bounds = true;
                    for (i, v) in p.translation.iter().enumerate() {
                        min[i] = min[i].min(*v);
                        max[i] = max[i].max(*v);
                    }
                }
            }
        }
        chunks.push(SceneChunk {
            guid: *guid,
            kind,
            size: bytes.len(),
            chunk,
            parse_error,
        });
    }

    Ok(Scene {
        map,
        requested,
        chunks,
        unregistered,
        unopened,
        placeables,
        models,
        entities,
        bounds: have_bounds.then_some((min, max)),
    })
}

/// Convenience: the map asset a GUID names, validated as type `0x002`.
pub fn is_map_asset(guid: u64) -> bool {
    guid::type_of(guid) == 0x002
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_loader_order_is_the_clients() {
        // 0x1F, 0x20, 0x23, then 1..=0x1C -- exactly the order the four
        // FUN_141b32e20 call sites and the loop are issued in.
        assert_eq!(LOADER_SLOT_ORDER.len(), 31);
        assert_eq!(&LOADER_SLOT_ORDER[..3], &[0x1f, 0x20, 0x23]);
        assert_eq!(LOADER_SLOT_ORDER[3], 1);
        assert_eq!(LOADER_SLOT_ORDER[30], 0x1c);
    }

    #[test]
    fn the_extra_list_is_bounded_and_read_in_the_loader_order() {
        // A synthetic map asset with the four stored fields and a two-entry
        // list: the order must be +0x1B8, list[0], list[1], +0x1C0, +0x1C8.
        let mut bytes = vec![0u8; 0x200];
        bytes[0x1b0..0x1b4].copy_from_slice(&2u32.to_le_bytes());
        bytes[0x1b4..0x1b8].copy_from_slice(&0x1e0u32.to_le_bytes());
        bytes[0x1b8..0x1c0].copy_from_slice(&0x0dd0_0011_0000_0688u64.to_le_bytes());
        bytes[0x1c0..0x1c8].copy_from_slice(&0x0dd0_0012_0000_0688u64.to_le_bytes());
        bytes[0x1c8..0x1d0].copy_from_slice(&0x0dd0_0023_0000_0688u64.to_le_bytes());
        bytes[0x1e0..0x1e8].copy_from_slice(&0x0dd0_0010_0000_0688u64.to_le_bytes());
        bytes[0x1e8..0x1f0].copy_from_slice(&0x0dd0_0028_0000_0688u64.to_le_bytes());
        let asset = MapAsset {
            guid: 0x0800_0000_0000_0688,
            bytes,
        };
        assert_eq!(
            asset.stored_chunks().unwrap(),
            vec![
                0x0dd0_0011_0000_0688,
                0x0dd0_0010_0000_0688,
                0x0dd0_0028_0000_0688,
                0x0dd0_0012_0000_0688,
                0x0dd0_0023_0000_0688,
            ]
        );
    }

    #[test]
    fn a_zero_stored_reference_is_skipped() {
        let bytes = vec![0u8; 0x1d0];
        let asset = MapAsset {
            guid: 0x0800_0000_0000_0688,
            bytes,
        };
        assert!(asset.stored_chunks().unwrap().is_empty());
    }
}
