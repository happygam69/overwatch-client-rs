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
    map: HashMap<(u32, u32), FieldName>,
}

/// A field's name and declared type.
///
/// The type is not decoration: a field declared as a `STUConfigVar` holds the
/// *index of an instance* rather than a value, so reading its bytes as a number
/// yields a denormal float that looks like data and is not. That mistake is easy
/// to make and hard to notice, so the declared type travels with the name.
#[derive(Debug, Clone)]
pub struct FieldName {
    pub name: String,
    /// Declared C# type, e.g. `STUConfigVar` or `float`.
    pub type_name: String,
}

impl FieldName {
    /// Whether this field points at another instance rather than holding a
    /// value.
    pub fn is_reference(&self) -> bool {
        self.type_name.starts_with("STUConfigVar") || self.type_name.starts_with("STU_")
    }

    /// The name without its Hungarian prefix.
    pub fn short_name(&self) -> &str {
        self.name.trim_start_matches("m_")
    }
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
                for (field_hash, field) in parse_named_fields(&text) {
                    names.map.insert((class_hash, field_hash), field);
                }
            }
        }
        names
    }

    pub fn get(&self, class_hash: u32, field_hash: u32) -> Option<&FieldName> {
        self.map.get(&(class_hash, field_hash))
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

/// `(field hash, name and declared type)` for every
/// `[STUField(0xHASH, ...)] public <type> <name>;` whose name is a real name
/// rather than a hash placeholder.
fn parse_named_fields(source: &str) -> Vec<(u32, FieldName)> {
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

        // `public <type> <name>`, with the type possibly carrying generic
        // arguments or an array suffix that contains spaces is not expected, so
        // splitting is enough.
        let words: Vec<&str> = declaration.split_whitespace().collect();
        let Some(name) = words.last() else {
            continue;
        };
        let name = name.trim().trim_end_matches(';');
        if name.is_empty() || is_placeholder(name) {
            continue;
        }
        // The token before the name is the type, minus any `public` keyword.
        let type_name = words
            .iter()
            .rev()
            .nth(1)
            .map(|t| t.trim_end_matches(';').to_string())
            .unwrap_or_default();
        out.push((
            field_hash,
            FieldName {
                name: name.to_string(),
                type_name,
            },
        ));
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
        assert_eq!(fields[0].1.name, "m_numShotsPerSecond");
        assert_eq!(fields[0].1.type_name, "STUConfigVar");
        // Declared as a ConfigVar, so the bytes at that field are an instance
        // index rather than a value.
        assert!(fields[0].1.is_reference());
    }

    #[test]
    fn a_plain_number_field_is_not_a_reference() {
        let source = "
            [STU(0x11111111, 8)]
            public class Thing { [STUField(0x22222222, 0)] public float m_speed; }";
        let fields = parse_named_fields(source);
        assert_eq!(fields[0].1.name, "m_speed");
        assert_eq!(fields[0].1.type_name, "float");
        assert!(!fields[0].1.is_reference());
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
