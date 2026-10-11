//! Field names for STU classes, read from a reference-library checkout.
//!
//! A literal inside a state graph is meaningless on its own: a float of `250`
//! could be a speed, a range or a cooldown. What identifies it is the *field* it
//! arrived through, because an embedded instance records the field hash that
//! owns it. Joining `(owning class, field hash)` to a name turns "there is a
//! float that is 250" into "this weapon's projectile speed is 250".
//!
//! The generated types carry those names. Many fields are still unnamed — the
//! generated sources call them `m_1A2B3C4D` — and those are dropped here rather
//! than presented as names, so a caller can tell "named" from "not named yet"
//! instead of being handed a hash-shaped placeholder that looks authoritative.
//!
//! Nothing here is game data: it is the reference tooling's own naming of the
//! classes it has identified. See `docs/provenance.md`.

use crate::keytable::Sources;
use std::collections::HashMap;
use std::path::PathBuf;

/// Names keyed by the class that declares a field.
///
/// The key is `(class hash, field hash)` rather than the field hash alone,
/// because one field hash legitimately means different things on different
/// classes.
#[derive(Debug, Default)]
pub struct FieldNames {
    map: HashMap<(u32, u32), String>,
}

impl FieldNames {
    pub fn load_from_checkouts(sources: &Sources) -> FieldNames {
        let mut names = FieldNames::default();
        for dir in sources.class_source_dirs() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path: PathBuf = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("cs") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let Some(class_hash) = parse_class_hash(&text) else {
                    continue;
                };
                for (field_hash, name) in parse_named_fields(&text) {
                    names.map.insert((class_hash, field_hash), name);
                }
            }
        }
        names
    }

    /// The name of a field on a class, if the reference types name it.
    pub fn get(&self, class_hash: u32, field_hash: u32) -> Option<&str> {
        self.map.get(&(class_hash, field_hash)).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// The hash in a `[STU(0xHASH, size)]` attribute.
fn parse_class_hash(source: &str) -> Option<u32> {
    let at = source.find("[STU(0x")?;
    let rest = &source[at + "[STU(0x".len()..];
    let end = rest.find(|c: char| !c.is_ascii_hexdigit())?;
    u32::from_str_radix(&rest[..end], 16).ok()
}

/// `(field hash, name)` for every `[STUField(0xHASH, ...)] public <type> <name>;`
/// whose name is a real name rather than a hash placeholder.
fn parse_named_fields(source: &str) -> Vec<(u32, String)> {
    const MARKER: &str = "[STUField(0x";
    let mut out = Vec::new();
    let mut rest = source;

    while let Some(at) = rest.find(MARKER) {
        rest = &rest[at + MARKER.len()..];
        let Some(hash_end) = rest.find(|c: char| !c.is_ascii_hexdigit()) else {
            break;
        };
        let Ok(field_hash) = u32::from_str_radix(&rest[..hash_end], 16) else {
            continue;
        };
        // The declaration follows the attribute, ending at the semicolon.
        let Some(semi) = rest.find(';') else {
            break;
        };
        let declaration = &rest[hash_end..semi];
        rest = &rest[semi + 1..];

        let Some(name) = declaration.split_whitespace().last() else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || is_placeholder(name) {
            continue;
        }
        out.push((field_hash, name.to_string()));
    }
    out
}

/// Whether a declared name is really just the hash, as in `m_1A2B3C4D`.
fn is_placeholder(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("m_") else {
        return false;
    };
    rest.len() == 8 && rest.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_named_fields_and_drops_placeholders() {
        let source = "
            namespace TankLib.STU.Types {
                [STU(0xEED6D952, 696)]
                public class STUStatescriptStateWeaponVolley : STUStatescriptState
                {
                    [STUField(0x02F3F5D4, 448, ReaderType = typeof(EmbeddedInstanceFieldReader))] // size: 8
                    public STUConfigVar m_numShotsPerSecond;

                    [STUField(0x55E8761C, 296, ReaderType = typeof(EmbeddedInstanceFieldReader))] // size: 8
                    public STU_3A858EF7 m_55E8761C;
                }
            }";
        assert_eq!(parse_class_hash(source), Some(0xEED6_D952));

        let fields = parse_named_fields(source);
        assert_eq!(fields.len(), 1, "the placeholder must be dropped");
        assert_eq!(fields[0].0, 0x02F3_F5D4);
        assert_eq!(fields[0].1, "m_numShotsPerSecond");
    }

    #[test]
    fn placeholder_detection_is_strict() {
        assert!(is_placeholder("m_1A2B3C4D"));
        assert!(!is_placeholder("m_numShotsPerSecond"));
        // A real name that merely contains hex characters is not a placeholder.
        assert!(!is_placeholder("m_face"));
        assert!(!is_placeholder("m_projectileSpeed"));
    }
}
