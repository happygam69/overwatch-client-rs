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
    /// A supplementary keyring, found in the asset-library checkout.
    pub support_keyring: Option<PathBuf>,
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
            support_keyring: env_path("OW_SUPPORT_KEYRING"),
        }
    }

    /// Environment first, then a search for the checkouts on disk.
    ///
    /// Requiring two environment variables before the tool will start is a poor
    /// first impression, and it made the tool look broken on a machine that had
    /// both checkouts sitting one directory away. The search is structural
    /// rather than a list of guessed paths: it looks for the *signature* of each
    /// checkout, so it finds one wherever it happens to live.
    pub fn discover(extra_roots: &[PathBuf]) -> Sources {
        let mut sources = Sources::from_env();
        if sources.tactlib_checkout.is_some() && sources.owlib_checkout.is_some() {
            return sources;
        }

        let mut roots: Vec<PathBuf> = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            roots.push(cwd.clone());
            // A checkout is often a sibling of the repository, so walk up.
            let mut up = cwd;
            for _ in 0..3 {
                if !up.pop() {
                    break;
                }
                roots.push(up.clone());
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            let mut dir = exe;
            for _ in 0..3 {
                if !dir.pop() {
                    break;
                }
                roots.push(dir.clone());
            }
        }
        roots.extend(extra_roots.iter().cloned());

        let (tactlib, owlib) = find_checkouts(&roots);
        if sources.tactlib_checkout.is_none() {
            sources.tactlib_checkout = tactlib;
        }
        if sources.owlib_checkout.is_none() {
            sources.owlib_checkout = owlib;
        }
        if sources.support_keyring.is_none() {
            // The asset-library checkout ships a supplementary keyring beside
            // its generated types. Read that copy rather than one of our own.
            sources.support_keyring = sources
                .owlib_checkout
                .iter()
                .map(|root| root.join("TankLib").join("Overwatch.keyring"))
                .find(|p| p.is_file());
        }
        sources
    }

    /// Every place discovery looked, for an error message worth reading.
    pub fn searched(&self) -> Vec<PathBuf> {
        self.tactlib_checkout
            .iter()
            .chain(self.owlib_checkout.iter())
            .cloned()
            .collect()
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

/// Directory names never worth descending into during a search.
const SKIP: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "bin",
    "obj",
    ".vs",
    ".idea",
    "packages",
    "data",
    "cache",
    "Cache",
    "System Volume Information",
    "$RECYCLE.BIN",
];

/// Find the two checkouts by looking for what makes each one recognisable.
///
/// * an asset-library checkout contains `TankLib/STU/Types`
/// * a container-library checkout contains `Core/Product/Tank`
///
/// Breadth-first, so a shallow match wins over a deep one, and bounded in both
/// depth and breadth so a search from a drive root cannot run away. The
/// signatures are checked at every directory visited, including each root, which
/// is what lets a checkout be found wherever it lives rather than at a guessed
/// path.
fn find_checkouts(roots: &[PathBuf]) -> (Option<PathBuf>, Option<PathBuf>) {
    // A checkout can be nested: `OWLib/TACTLib/TACTLib` is four levels below a
    // drive root, and the reference checkout on this machine is exactly that
    // shape. Three levels found the outer library and missed the inner one.
    const MAX_DEPTH: usize = 5;
    const MAX_DIRS: usize = 1_500;
    const MAX_CHILDREN: usize = 200;

    /// Names that suggest a directory could hold a source checkout. Past the
    /// first two levels the search only descends into these, which is what keeps
    /// a walk from a drive root to a couple of hundred milliseconds instead of
    /// half a minute. A checkout with an unrelated name is still found through
    /// `OW_TACTLIB` or `OW_OWLIB`.
    fn looks_relevant(name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        [
            "owlib",
            "tactlib",
            "tanklib",
            "casc",
            "overwatch",
            "repos",
            "src",
            "projects",
            "code",
            "dev",
            "tools",
            "git",
            "work",
            "games",
        ]
        .iter()
        .any(|needle| name.contains(needle))
    }

    let is_owlib = |dir: &std::path::Path| dir.join("TankLib").join("STU").join("Types").is_dir();
    let is_tactlib = |dir: &std::path::Path| dir.join("Core").join("Product").join("Tank").is_dir();

    let mut tactlib = None;
    let mut owlib = None;
    let mut queue: std::collections::VecDeque<(PathBuf, usize)> = std::collections::VecDeque::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    for root in roots {
        if root.is_dir() && seen.insert(root.clone()) {
            queue.push_back((root.clone(), 0));
        }
    }

    let mut visited = 0usize;
    while let Some((dir, depth)) = queue.pop_front() {
        visited += 1;
        if visited > MAX_DIRS {
            break;
        }

        if owlib.is_none() && is_owlib(&dir) {
            owlib = Some(dir.clone());
        }
        if tactlib.is_none() && is_tactlib(&dir) {
            tactlib = Some(dir.clone());
        }
        if tactlib.is_some() && owlib.is_some() {
            break;
        }
        if depth >= MAX_DEPTH {
            continue;
        }

        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        // Relevant names first, so a shallow match is reached before the
        // breadth-first walk spends its budget on unrelated trees.
        let mut children: Vec<(bool, PathBuf)> = Vec::new();
        for (count, entry) in entries.flatten().enumerate() {
            if count >= MAX_CHILDREN {
                break;
            }
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !path.is_dir() || name.starts_with('.') || SKIP.contains(&name) {
                continue;
            }
            if depth >= 1 && !looks_relevant(name) {
                continue;
            }
            children.push((!looks_relevant(name), path));
        }
        children.sort_by_key(|(unrelated, _)| *unrelated);
        for (_, path) in children {
            if seen.insert(path.clone()) {
                queue.push_back((path, depth + 1));
            }
        }
    }

    (tactlib, owlib)
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
