//! The Overwatch asset layer on top of the container.
//!
//! `ow-casc` turns keys into bytes; this crate knows what those bytes are.
//! It provides:
//!
//! * [`guid`] ??? asset identifiers and their type numbers.
//! * [`manifest`] ??? the content manifest, which is the GUID to content-key
//!   registry, and the encryption wrapped around it.
//! * [`keytable`] ??? where the per-build manifest key material is read from.
//! * [`classes`] ??? STU instance type hashes for gameplay classes.
//! * [`GameData`] ??? the assembled view: an install, its registry, and typed
//!   access to game assets.
//!
//! No game content is carried in this repository: every byte comes from the
//! user's own installation at runtime.

pub mod assoc;
pub mod blendtree;
pub mod bundle;
pub mod chunks;
pub mod classes;
pub mod classreg;
pub mod clip;
pub mod cloth;
pub mod effect;
pub mod entity;
pub mod fieldnames;
pub mod firstperson;
pub mod graph;
pub mod guid;
pub mod hardpoint;
pub mod keytable;
pub mod lighting;
pub mod manifest;
pub mod mapchunk;
pub mod mapguid;
pub mod mapresolve;
pub mod material;
pub mod model;
pub mod mstu;
pub mod packages;
pub mod replay;
pub mod settings;
pub mod shader;
pub mod skeleton;
pub mod skin;
pub mod sound;
pub mod text;
pub mod texture;
pub mod trg;
pub mod unlocks;
pub mod volley;
pub mod weapons;

use ow_casc::Store;
use ow_stu::Stu;

use std::cell::OnceCell;

#[derive(Debug)]
pub enum Error {
    Install(ow_casc::install::Error),
    Casc(ow_casc::container::Error),
    Manifest(manifest::Error),
    Stu(ow_stu::Error),
    /// The assembly does not ship content manifests this build can read.
    NoManifest,
    /// Manifests matched the request, but none of them opened. Carries why.
    NoManifestUsable(String),
    NotFound(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Install(e) => write!(f, "{e}"),
            Error::Casc(e) => write!(f, "{e}"),
            Error::Manifest(e) => write!(f, "{e}"),
            Error::Stu(e) => write!(f, "{e}"),
            Error::NoManifest => write!(
                f,
                "this installation's root file lists no content manifest the tool can open"
            ),
            Error::NoManifestUsable(detail) => {
                write!(f, "no content manifest could be opened. Tried:\n  {detail}")
            }
            Error::NotFound(what) => write!(f, "not found: {what}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<ow_casc::install::Error> for Error {
    fn from(e: ow_casc::install::Error) -> Self {
        Error::Install(e)
    }
}
impl From<ow_casc::container::Error> for Error {
    fn from(e: ow_casc::container::Error) -> Self {
        Error::Casc(e)
    }
}
impl From<manifest::Error> for Error {
    fn from(e: manifest::Error) -> Self {
        Error::Manifest(e)
    }
}
impl From<ow_stu::Error> for Error {
    fn from(e: ow_stu::Error) -> Self {
        Error::Stu(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Platform, region and locale tags used to pick manifests out of the root file.
///
/// The defaults match a retail Windows install on the global region with English
/// text. A text manifest is loaded alongside the base one because the base does
/// not cover everything: on the reference install it carries 67 assets of the
/// settings type and the enUS manifest adds another 52. `content_manifest` can
/// force a particular content variant; the install ships more than one.
#[derive(Debug, Clone)]
pub struct ManifestSelector {
    pub platform: String,
    pub region: String,
    pub text_locale: String,
    /// Force one content manifest by a substring of its file name.
    ///
    /// `None` opens the first candidate that works, in the order
    /// [`manifest::Registry::find_manifest_candidates`] returns. A caller that
    /// needs a particular variant — the Prism renderer's assets, for example —
    /// names it here instead of relying on the root file's order.
    pub content_manifest: Option<String>,
}

impl Default for ManifestSelector {
    fn default() -> Self {
        ManifestSelector {
            platform: "Win".to_string(),
            region: "RDEV".to_string(),
            text_locale: "enUS".to_string(),
            content_manifest: None,
        }
    }
}

/// A loaded installation plus its asset registry.
pub struct GameData {
    store: Store,
    registry: manifest::Registry,
    /// Names of the manifests whose records the registry holds.
    pub manifests: Vec<String>,
    pub classes: classes::ClassMap,
    /// Field names, read from a reference checkout when one is configured.
    /// Empty otherwise, in which case a graph's parameters cannot be named.
    pub fields: fieldnames::FieldNames,
    /// The bundle index, built on the first bundle-hosted asset opened.
    bundle_index: OnceCell<std::result::Result<bundle::BundleIndex, String>>,
}

impl GameData {
    /// Open an installation and build its asset registry.
    ///
    /// Key material for the manifests is read from `sources`; see
    /// [`keytable::Sources`].
    pub fn open(
        install_root: impl AsRef<std::path::Path>,
        selector: &ManifestSelector,
        sources: &keytable::Sources,
    ) -> Result<GameData> {
        let install = ow_casc::Install::open_with_support_keyring(
            install_root,
            sources.support_keyring.as_deref(),
        )?;
        let store = Store::open(install)?;

        let root_file = store.root_file()?;

        // The base manifest is authoritative; the text manifest adds what it
        // does not carry. Speech is not loaded -- nothing here reads audio.
        //
        // More than one manifest can match, and they are not interchangeable:
        // on a recent install the plain `Win_` root manifest is encrypted under
        // a derivation that is not public while the `WinPrism_` one is in the
        // clear. So try them in order and keep the first that opens, recording
        // why the others were passed over rather than hiding it.
        let candidates = manifest::Registry::find_manifest_candidates(
            &root_file,
            manifest::ManifestKind::Content,
            &selector.platform,
            &selector.region,
            None,
        );
        if candidates.is_empty() {
            return Err(Error::NoManifest);
        }

        // An explicit manifest, for measuring what each variant covers. The
        // selector wins over the environment; both match a substring so a
        // partial name is enough.
        let forced = selector
            .content_manifest
            .clone()
            .or_else(|| std::env::var("OW_MANIFEST").ok());
        let mut opened = None;
        let mut skipped: Vec<String> = Vec::new();
        for (name, content_key) in &candidates {
            if let Some(want) = &forced {
                if !name.contains(want.as_str()) {
                    continue;
                }
            }
            match manifest::Registry::open(&store, name, content_key, sources) {
                Ok(registry) => {
                    opened = Some((registry, name.clone()));
                    break;
                }
                Err(e) => skipped.push(format!("{name}: {e}")),
            }
        }
        let Some((mut registry, name)) = opened else {
            let detail = skipped.join("\n  ");
            return Err(Error::NoManifestUsable(detail));
        };
        if !skipped.is_empty() {
            // Say *why* each candidate was passed over: the reason is the whole
            // difference between "this install does not ship it" and "our key
            // derivation does not reach it".
            eprintln!(
                "note: {name} chosen; {} other manifest(s) for this region did not open:",
                skipped.len()
            );
            for line in &skipped {
                eprintln!("  {line}");
            }
        }
        let mut manifests = vec![name];

        for (text_name, text_key) in manifest::Registry::find_manifest_candidates(
            &root_file,
            manifest::ManifestKind::Text,
            &selector.platform,
            &selector.region,
            Some(&selector.text_locale),
        ) {
            // A missing text manifest is not fatal: the base data still reads.
            match manifest::Registry::open(&store, &text_name, &text_key, sources) {
                Ok(text) => {
                    registry.merge_missing(&text);
                    manifests.push(text_name);
                    break;
                }
                Err(_) => continue,
            }
        }

        let mut classes = classes::ClassMap::builtin();
        classes.load_from_checkouts(sources);
        let fields = fieldnames::FieldNames::load_from_checkouts(sources);

        Ok(GameData {
            store,
            registry,
            manifests,
            classes,
            fields,
            bundle_index: OnceCell::new(),
        })
    }

    /// The root file, as text, for the layers that read it themselves.
    pub fn root_file(&self) -> Result<String> {
        self.store.root_file().map_err(Error::Casc)
    }

    /// Open an asset by content key, for layers above the manifest.
    pub fn open_ckey(&self, key: &[u8; 16]) -> Result<Vec<u8>> {
        self.store.open_ckey(key).map_err(Error::Casc)
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn registry(&self) -> &manifest::Registry {
        &self.registry
    }

    /// Asset GUIDs of a type, in registry order.
    pub fn guids_of_type(&self, type_id: u16) -> Vec<u64> {
        self.registry.guids_of_type(type_id)
    }

    /// Count how many assets exist per type.
    pub fn type_histogram(&self) -> std::collections::BTreeMap<u16, usize> {
        let mut out = std::collections::BTreeMap::new();
        for record in &self.registry.records {
            *out.entry(guid::type_of(record.guid)).or_insert(0) += 1;
        }
        out
    }

    /// Raw bytes of an asset, from the encoded store only.
    ///
    /// This is the path for assets whose CMF record byte has bit 7 clear. For
    /// the client's full open — which falls back to a bundle — use
    /// [`GameData::open_asset`]. Ordinary assets are identical through both.
    pub fn bytes_of(&self, guid: u64) -> Result<Vec<u8>> {
        let record = self
            .registry
            .get(guid)
            .ok_or_else(|| Error::NotFound(format!("asset 0x{guid:016x}")))?;
        Ok(self.store.open_ckey(&record.content_key)?)
    }

    /// The client's asset open: the encoded store, or the asset's bundle.
    ///
    /// The CMF record byte at `+12` decides. With bit 7 clear the payload is a
    /// content key in the store (`Store::open_ckey`), which is what
    /// [`GameData::bytes_of`] does. With bit 7 set the payload is an entry of a
    /// type-`0x09C` bundle, and the client reads
    /// `payload + offset` for the record's declared size (`FUN_141fd5820`).
    ///
    /// Reading a bundle-hosted asset builds the bundle index first: the
    /// manifest's type-`0x09C` records are opened and their entry tables read.
    /// That is the one costly step here, and it happens once per [`GameData`].
    pub fn open_asset(&self, guid: u64) -> Result<Vec<u8>> {
        let record = self
            .registry
            .get(guid)
            .ok_or_else(|| Error::NotFound(format!("asset 0x{guid:016x}")))?;
        if record.unknown & bundle::RECORD_IN_BUNDLE == 0 {
            return Ok(self.store.open_ckey(&record.content_key)?);
        }

        let index = self
            .bundle_index()
            .map_err(|e| Error::NotFound(format!("asset 0x{guid:016x}: {e}")))?;
        let location = index.get(guid).ok_or_else(|| {
            Error::NotFound(format!(
                "asset 0x{guid:016x} is marked as bundle-hosted but no bundle holds it \
                 ({} bundles read, {} entries)",
                index.bundles, index.entries_seen
            ))
        })?;
        let payload = self.bytes_of(location.bundle)?;
        let parsed = bundle::Bundle::parse(&payload)
            .map_err(|e| Error::NotFound(format!("bundle 0x{:016x}: {e}", location.bundle)))?;
        let bytes = parsed.span(location.offset, record.size).map_err(|e| {
            Error::NotFound(format!(
                "asset 0x{guid:016x} in bundle 0x{:016x}: {e}",
                location.bundle
            ))
        })?;
        Ok(bytes.to_vec())
    }

    /// The bundle index, built on first use.
    ///
    /// Every type-`0x09C` record is a bundle; its payload is the entry table the
    /// client reads in `FUN_141fd5820`. A bundle that cannot be opened is
    /// skipped and counted, never fatal: the install is allowed to be partial.
    pub fn bundle_index(&self) -> std::result::Result<&bundle::BundleIndex, &str> {
        let cell = self.bundle_index.get_or_init(|| {
            let guids: Vec<u64> = self
                .registry
                .records
                .iter()
                .map(|r| r.guid)
                .filter(|g| guid::type_of(*g) == bundle::BUNDLE_TYPE)
                .collect();
            let index = bundle::BundleIndex::build(guids.into_iter(), |bundle_guid| {
                self.registry
                    .get(bundle_guid)
                    .and_then(|r| self.store.open_ckey(&r.content_key).ok())
            });
            if index.bundles == 0 {
                Err("no type-0x09C bundle payload parsed".to_string())
            } else {
                Ok(index)
            }
        });
        match cell {
            Ok(index) => Ok(index),
            Err(e) => Err(e.as_str()),
        }
    }

    /// Parse an asset as structured data.
    ///
    /// Uses the client's full open, so a bundle-hosted asset is reachable here
    /// too. See [`GameData::open_asset`].
    pub fn stu_of(&self, guid: u64) -> Result<Stu> {
        let bytes = self.open_asset(guid)?;
        Ok(Stu::parse(bytes)?)
    }
}
