//! Per-build manifest key material.
//!
//! A `.cmf` (content manifest) or `.trg` (resource graph) is AES-CBC encrypted
//! with a key and IV derived from a 512-byte table plus a few numbers from the
//! manifest's own header. The table differs for every game build — a retail
//! install's build selects exactly one table, and overtools/TACTLib carries one
//! source file per build holding it.
//!
//! This project deliberately does **not** vendor that key material. Instead it
//! reads the table for the build in front of it from a checkout of that library
//! which the user already has, or from a plain file of 512 bytes they supply.
//! The derivation algorithm lives in [`crate::manifest`]; only the constants
//! are borrowed, from their own MIT-licensed source, at runtime.
//!
//! See `docs/provenance.md` for why.

use std::fmt;
use std::path::{Path, PathBuf};

pub const TABLE_SIZE: usize = 512;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// No source was configured, or none of the configured ones had this build.
    NotFound {
        build: u32,
        tried: Vec<PathBuf>,
    },
    /// A source was found but did not yield 512 bytes.
    BadTable {
        source: PathBuf,
        found: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::NotFound { build, tried } => {
                write!(
                    f,
                    "no manifest key table for build {build}. Point OW_TACTLIB at a checkout of \
                     overtools/TACTLib, or OW_KEYTABLE at a 512-byte table file. Tried:"
                )?;
                for path in tried {
                    write!(f, "\n  {}", path.display())?;
                }
                Ok(())
            }
            Error::BadTable { source, found } => write!(
                f,
                "{} holds {found} bytes, expected {TABLE_SIZE}",
                source.display()
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// Where per-build key material, and supplementary names, may be found.
#[derive(Debug, Clone, Default)]
pub struct Sources {
    /// A checkout of overtools/TACTLib, whose `Core/Product/Tank/` holds one
    /// manifest key generator source file per build.
    pub tactlib_checkout: Option<PathBuf>,
    /// A checkout of overtools/OWLib, whose `TankLib/STU/Types/` holds the
    /// generated STU class definitions, used for diagnostic names.
    pub owlib_checkout: Option<PathBuf>,
    /// A plain file holding exactly 512 bytes.
    pub table_file: Option<PathBuf>,
}

impl Sources {
    /// Read the environment for configured sources.
    ///
    /// `OW_TACTLIB` names the container library checkout, `OW_OWLIB` the asset
    /// library checkout, and `OW_KEYTABLE` a table file. Either checkout's root
    /// works for either variable: the candidate paths are probed, so pointing
    /// both at the same repository checkout is fine.
    pub fn from_env() -> Sources {
        let env_path = |name: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|p| !p.as_os_str().is_empty())
        };
        Sources {
            tactlib_checkout: env_path("OW_TACTLIB"),
            owlib_checkout: env_path("OW_OWLIB"),
            table_file: env_path("OW_KEYTABLE"),
        }
    }

    /// Every directory that might hold the generated STU class sources.
    ///
    /// A checkout is often nested (`OWLib/TACTLib/TACTLib`), and the two
    /// libraries sit side by side, so several shapes are worth trying rather
    /// than requiring the caller to know which one is right.
    pub fn class_source_dirs(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for root in [&self.owlib_checkout, &self.tactlib_checkout]
            .into_iter()
            .flatten()
        {
            for suffix in [
                &["TankLib", "STU", "Types"][..],
                &["STU", "Types"][..],
                &["TankLib", "STU"][..],
            ] {
                let candidate = suffix
                    .iter()
                    .fold(root.clone(), |path, part| path.join(part));
                if candidate.is_dir() && !out.contains(&candidate) {
                    out.push(candidate);
                }
            }
            // also look one level up, for nested or sibling checkouts
            if let Some(parent) = root.parent() {
                for suffix in [&["STU", "Types"][..], &["TankLib", "STU", "Types"][..]] {
                    let candidate = suffix
                        .iter()
                        .fold(parent.to_path_buf(), |path, part| path.join(part));
                    if candidate.is_dir() && !out.contains(&candidate) {
                        out.push(candidate);
                    }
                }
            }
        }
        out
    }

    /// The 512-byte table for a build.
    pub fn table_for(&self, build: u32) -> Result<[u8; TABLE_SIZE], Error> {
        let mut tried = Vec::new();

        if let Some(path) = &self.table_file {
            tried.push(path.clone());
            if path.is_file() {
                let bytes = std::fs::read(path)?;
                return to_table(bytes, path);
            }
        }

        if let Some(root) = &self.tactlib_checkout {
            for kind in ["CMF", "TRG"] {
                for prefix in ["Core/Product/Tank", "Product/Tank"] {
                    let path = root
                        .join(prefix)
                        .join(kind)
                        .join(format!("Pro{kind}_{build}.cs"));
                    tried.push(path.clone());
                    if path.is_file() {
                        let bytes = std::fs::read_to_string(&path)?;
                        return to_table(parse_keytable_literal(&bytes), &path);
                    }
                }
            }
        }

        Err(Error::NotFound { build, tried })
    }
}

fn to_table(bytes: Vec<u8>, source: &Path) -> Result<[u8; TABLE_SIZE], Error> {
    if bytes.len() != TABLE_SIZE {
        return Err(Error::BadTable {
            source: source.to_path_buf(),
            found: bytes.len(),
        });
    }
    let mut out = [0u8; TABLE_SIZE];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// Pull the 512 hex bytes out of the `Keytable = { 0x.., ... }` literal.
///
/// The identifier also appears in the accessor code, so the declaration is the
/// last occurrence.
pub fn parse_keytable_literal(source: &str) -> Vec<u8> {
    let Some(decl) = source.rfind("Keytable") else {
        return Vec::new();
    };
    let rest = &source[decl..];
    let Some(open) = rest.find('{') else {
        return Vec::new();
    };
    let Some(close) = rest[open..].find('}') else {
        return Vec::new();
    };
    let body = &rest[open + 1..open + close];

    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'0' && (bytes[i + 1] == b'x' || bytes[i + 1] == b'X') {
            let start = i + 2;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_hexdigit() && end - start < 2 {
                end += 1;
            }
            if end - start == 2 {
                if let Ok(v) = u8::from_str_radix(&body[start..end], 16) {
                    out.push(v);
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_keytable_literal() {
        let source = r#"
            public byte[] Key(Header h, int length) { return Keytable[h.x]; }
            private static readonly byte[] Keytable =
            {
                0xBC, 0x63, 0x89, 0x14, 0x11, 0x19, 0x93, 0x89,
                0x51, 0xFB, 0x72, 0xCB, 0x04, 0x5C, 0x49, 0xA3,
            };
        "#;
        let table = parse_keytable_literal(source);
        assert_eq!(table.len(), 16);
        assert_eq!(table[0], 0xBC);
        assert_eq!(table[15], 0xA3);
    }
}
