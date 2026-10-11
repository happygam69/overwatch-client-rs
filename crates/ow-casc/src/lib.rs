//! Read-only access to an Overwatch installation's CASC store.
//!
//! The installation is the user's own copy, read in place and never modified.
//! This crate knows nothing about Overwatch's asset semantics; it only resolves
//! keys to bytes. See `ow-tank` for the asset layer.
//!
//! Layer order, outermost first:
//!
//! 1. `.build.info` names the build, CDN and keyring configs.
//! 2. those configs name the root and encoding files, and carry the Salsa20
//!    chunk keys.
//! 3. `data/<bucket>.idx` maps a 9-byte encoding key to an archive location.
//! 4. `data/data.NNN` holds the stored record.
//! 5. BLTE wraps the record, possibly compressed and possibly encrypted.
//! 6. the encoding file maps content keys to encoding keys.
//!
//! The formats were pinned down against a retail install and cross-checked with
//! overtools/TACTLib; see `docs/formats/casc.md` and `docs/provenance.md`.

pub mod blte;
pub mod container;
pub mod encoding;
pub mod index;
pub mod install;

pub use container::Store;
pub use install::Install;
