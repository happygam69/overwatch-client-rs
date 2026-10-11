//! The Overwatch asset layer on top of the container.
//!
//! `ow-casc` turns keys into bytes; this crate knows what those bytes are.
//! It provides:
//!
//! * [`guid`] — asset identifiers and their type numbers.
//! * [`manifest`] — the content manifest, which is the GUID to content-key
//!   registry, and the encryption wrapped around it.
//! * [`keytable`] — where the per-build manifest key material is read from.
//! * [`classes`] — STU instance type hashes for gameplay classes.
//! * [`GameData`] — the assembled view: an install, its registry, and typed
//!   access to game assets.
//!
//! No game content is carried in this repository: every byte comes from the
//! user's own installation at runtime.

pub mod classes;
pub mod entity;
pub mod guid;
pub mod keytable;
pub mod manifest;
pub mod settings;

use ow_casc::Store;
use ow_stu::Stu;

#[derive(Debug)]
pub enum Error {
    Install(ow_casc::install::Error),
    Casc(ow_casc::container::Error),
    Manifest(manifest::Error),
    Stu(ow_stu::Error),
    /// The assembly does not ship content manifests this build can read.
    NoManifest,
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
/// settings type and the enUS manifest adds another 52.
#[derive(Debug, Clone)]
pub struct ManifestSelector {
    pub platform: String,
    pub region: String,
    pub text_locale: String,
}

impl Default for ManifestSelector {
    fn default() -> Self {
        ManifestSelector {
            platform: "Win".to_string(),
            region: "RDEV".to_string(),
            text_locale: "enUS".to_string(),
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
        let install = ow_casc::Install::open(install_root)?;
        let store = Store::open(install)?;

        let root_file = store.root_file()?;

        // The base manifest is authoritative; the text manifest adds what it
        // does not carry. Speech is not loaded -- nothing here reads audio.
        let (name, content_key) = manifest::Registry::find_manifest_name(
            &root_file,
            manifest::ManifestKind::Content,
            &selector.platform,
            &selector.region,
            None,
        )
        .ok_or(Error::NoManifest)?;

        let mut registry = manifest::Registry::open(&store, &name, &content_key, sources)?;
        let mut manifests = vec![name];

        if let Some((text_name, text_key)) = manifest::Registry::find_manifest_name(
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
                }
                Err(e) => eprintln!("warning: could not open text manifest {text_name}: {e}"),
            }
        }

        let mut classes = classes::ClassMap::builtin();
        classes.load_from_checkouts(sources);

        Ok(GameData {
            store,
            registry,
            manifests,
            classes,
        })
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

    /// Raw bytes of an asset.
    pub fn bytes_of(&self, guid: u64) -> Result<Vec<u8>> {
        let record = self
            .registry
            .get(guid)
            .ok_or_else(|| Error::NotFound(format!("asset 0x{guid:016x}")))?;
        Ok(self.store.open_ckey(&record.content_key)?)
    }

    /// Parse an asset as structured data.
    pub fn stu_of(&self, guid: u64) -> Result<Stu> {
        let bytes = self.bytes_of(guid)?;
        Ok(Stu::parse(bytes)?)
    }
}
