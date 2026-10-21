//! The typed STU registry: classes, fields and enums, in the shape
//! `ow_stu::types` reads with.
//!
//! The typed layer needs more than hash-to-name: for every class it needs the
//! declared size, the base class, and each field's declared type, reader kind,
//! version-1 offset and initializer. All of that is in the generated sources as
//! C# declarations:
//!
//! ```csharp
//! [STU(0x7C3457DC, 912)]
//! public class STUHero : STUInstance
//! {
//!     [STUField(0x950BBA06, 172, ReaderType = typeof(EmbeddedInstanceFieldReader))] // size: 8
//!     public STUEntityDefinition m_gameplayEntity;
//! }
//! ```
//!
//! This module translates those into [`ow_stu::types`] definitions. Nothing is
//! carried in this repository: the definitions are the reference tooling's own
//! published reconstruction, read from the operator's checkout at runtime. See
//! `docs/provenance.md`.

use crate::keytable::Sources;
use ow_stu::types::{
    ClassDef, ClassRegistry as StuRegistry, DefaultValue, EnumDef, FieldDef, FieldType, Prim,
    ReaderKind, StructKind,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

const CLASS_MARKER: &str = "[STU(0x";
const ENUM_MARKER: &str = "[STUEnum(0x";
const FIELD_MARKER: &str = "[STUField(0x";
/// The base every hierarchy roots at; it is not itself a generated class, so it
/// has no hash to resolve to.
const ROOT_CLASS: &str = "STUInstance";

/// Every class and enum the checkout describes, plus what the loader could not
/// make sense of (which should be nothing).
#[derive(Debug, Default)]
pub struct ClassRegistry {
    reg: StuRegistry,
    /// `.cs` files read.
    pub files: usize,
    /// Initializer expressions the loader did not recognize, as
    /// `<type> <name> = <expr>`.
    pub unparsed_defaults: Vec<String>,
    /// Declared type names that mapped to `FieldType::Unknown`, by name and
    /// occurrence count.
    pub unknown_types: BTreeMap<String, usize>,
    /// Base class names that resolved to nothing, by name and occurrence count.
    pub unknown_bases: BTreeMap<String, usize>,
}

impl ClassRegistry {
    /// Load every class and enum under the configured checkout's STU source
    /// directories. An empty registry when there is no checkout.
    pub fn load_from_sources(sources: &Sources) -> ClassRegistry {
        let mut files: Vec<PathBuf> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        for dir in sources.class_source_dirs() {
            collect_cs_files(&dir, &mut files, &mut seen);
        }
        // The directory list can name both a directory and its parent, hence
        // the dedupe above; the sort is for a deterministic parse order.
        files.sort();

        let mut raw_classes = Vec::new();
        let mut raw_enums = Vec::new();
        let mut registry = ClassRegistry::default();
        for path in &files {
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            registry.files += 1;
            parse_cs_file(&text, &mut raw_classes, &mut raw_enums);
        }
        registry.build(raw_classes, raw_enums);
        registry
    }

    /// The registry proper, for the typed readers.
    pub fn inner(&self) -> &StuRegistry {
        &self.reg
    }

    pub fn is_empty(&self) -> bool {
        self.reg.class_count() == 0
    }

    /// Turn the parsed declarations into registry definitions.
    ///
    /// Two passes in effect: names must all be known before any base or field
    /// type can resolve, so the raw forms keep names and this resolves them.
    fn build(&mut self, raw_classes: Vec<RawClass>, raw_enums: Vec<RawEnum>) {
        let class_hash: HashMap<String, u32> = raw_classes
            .iter()
            .map(|c| (c.name.clone(), c.hash))
            .collect();
        let enum_hash: HashMap<String, u32> =
            raw_enums.iter().map(|e| (e.name.clone(), e.hash)).collect();

        for raw in raw_enums {
            self.reg.add_enum(EnumDef {
                hash: raw.hash,
                name: raw.name,
                underlying: raw.underlying,
                members: raw.members,
            });
        }

        for raw in raw_classes {
            let base = if raw.base_name == ROOT_CLASS {
                None
            } else {
                match class_hash.get(raw.base_name.as_str()) {
                    Some(hash) => Some(*hash),
                    None => {
                        *self.unknown_bases.entry(raw.base_name.clone()).or_insert(0) += 1;
                        None
                    }
                }
            };
            let mut def = ClassDef::new(raw.hash, raw.name, raw.size, base);
            for field in raw.fields {
                let ty = map_type(
                    &field.type_name,
                    &class_hash,
                    &enum_hash,
                    &mut self.unknown_types,
                );
                let default = match field.default_expr.as_deref() {
                    None => DefaultValue::None,
                    Some(expr) => parse_default(expr).unwrap_or_else(|| {
                        self.unparsed_defaults
                            .push(format!("{} {} = {expr}", field.type_name, field.name));
                        DefaultValue::None
                    }),
                };
                def.add_field(FieldDef {
                    hash: field.hash,
                    name: field.name,
                    v1_offset: field.v1_offset,
                    size_hint: field.size_hint,
                    reader: field.reader,
                    ty,
                    default,
                });
            }
            self.reg.add_class(def);
        }
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// A class as written, before hashes and type names are resolved.
struct RawClass {
    hash: u32,
    size: u32,
    name: String,
    base_name: String,
    fields: Vec<RawField>,
}

/// A field as written.
struct RawField {
    hash: u32,
    v1_offset: u32,
    size_hint: Option<u32>,
    reader: ReaderKind,
    type_name: String,
    name: String,
    default_expr: Option<String>,
}

/// An enum as written.
struct RawEnum {
    hash: u32,
    name: String,
    underlying: Prim,
    members: Vec<(String, i64)>,
}

/// Every `.cs` file under `dir`, recursively (enums live in a subdirectory),
/// deduplicated across overlapping roots.
fn collect_cs_files(dir: &Path, files: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_cs_files(&path, files, seen);
        } else if path.extension().and_then(|e| e.to_str()) == Some("cs") {
            let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if seen.insert(key) {
                files.push(path);
            }
        }
    }
}

fn parse_cs_file(text: &str, classes: &mut Vec<RawClass>, enums: &mut Vec<RawEnum>) {
    parse_classes(text, classes);
    parse_enums(text, enums);
}

/// Hex digits starting at `at` (the `0x` is already consumed by the marker),
/// plus the position just after them.
fn read_hex(text: &str, at: usize) -> Option<(u32, usize)> {
    let rest = text.get(at..)?;
    let end = rest.find(|c: char| !c.is_ascii_hexdigit())?;
    if end == 0 {
        return None;
    }
    let value = u32::from_str_radix(&rest[..end], 16).ok()?;
    Some((value, at + end))
}

/// Position after a C# identifier starting at `at`.
fn scan_ident(text: &str, at: usize) -> usize {
    text[at..]
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .map(|offset| at + offset)
        .unwrap_or(text.len())
}

/// The index of the `)` that closes an attribute whose opening `(` was just
/// consumed. Parentheses opened inside the attribute (a `typeof(...)`) nest.
fn find_attr_end(text: &str, from: usize) -> Option<usize> {
    let mut depth = 1usize;
    for (offset, c) in text[from..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(from + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether the position sits after a `//` on its line. The generated sources
/// comment out a few whole field declarations, attribute line included; a
/// marker or `public` inside a comment is not a declaration.
fn in_line_comment(text: &str, pos: usize) -> bool {
    let line_start = text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    text[line_start..pos].contains("//")
}

/// The index of the `;` that ends a declaration, skipping string literals so a
/// stray semicolon inside a default cannot end it early.
fn find_decl_end(text: &str, from: usize) -> Option<usize> {
    let mut in_str = false;
    let mut escaped = false;
    for (offset, c) in text[from..].char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
        } else if c == ';' {
            return Some(from + offset);
        }
    }
    None
}

/// The index of the `}` matching the `{` at `open`, ignoring braces inside
/// comments and string literals (a default string may well contain both).
fn matching_brace(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let bytes = text.as_bytes();
    let mut at = open;
    while at < bytes.len() {
        let c = bytes[at];
        if in_line_comment {
            if c == b'\n' {
                in_line_comment = false;
            }
        } else if in_block_comment {
            if c == b'*' && bytes.get(at + 1) == Some(&b'/') {
                in_block_comment = false;
                at += 1;
            }
        } else if in_str {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_str = false;
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'/' if bytes.get(at + 1) == Some(&b'/') => in_line_comment = true,
                b'/' if bytes.get(at + 1) == Some(&b'*') => in_block_comment = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(at);
                    }
                }
                _ => {}
            }
        }
        at += 1;
    }
    None
}

/// `[STU(0xHASH, size)] public class NAME : BASE { ... }` blocks.
fn parse_classes(text: &str, out: &mut Vec<RawClass>) {
    let mut at = 0;
    while let Some(found) = text[at..].find(CLASS_MARKER) {
        let marker_end = at + found + CLASS_MARKER.len();
        if in_line_comment(text, at + found) {
            at = marker_end;
            continue;
        }
        let Some((hash, pos)) = read_hex(text, marker_end) else {
            at = marker_end;
            continue;
        };
        let Some(comma) = text[pos..].find(',') else {
            break;
        };
        let after_comma = pos + comma + 1;
        let digits = text[after_comma..].trim_start();
        let digits_at = after_comma + (text[after_comma..].len() - digits.len());
        let digits_end = digits
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(digits.len());
        let Some(size) = digits[..digits_end].parse::<u32>().ok() else {
            at = after_comma;
            continue;
        };
        let after_size = digits_at + digits_end;

        let Some(class_at) = text[after_size..].find("public class ") else {
            break;
        };
        let name_start = after_size + class_at + "public class ".len();
        let name_end = scan_ident(text, name_start);
        let name = &text[name_start..name_end];

        let Some(colon) = text[name_end..].find(':') else {
            at = name_end;
            continue;
        };
        let base_start = name_end + colon + 1;
        let base_rest = text[base_start..].trim_start();
        let base_at = base_start + (text[base_start..].len() - base_rest.len());
        let base_end = scan_ident(text, base_at);
        let base_name = &text[base_at..base_end];

        let Some(open_rel) = text[base_end..].find('{') else {
            break;
        };
        let open = base_end + open_rel;
        let Some(close) = matching_brace(text, open) else {
            break;
        };

        out.push(RawClass {
            hash,
            size,
            name: name.to_string(),
            base_name: base_name.to_string(),
            fields: parse_fields(&text[open + 1..close]),
        });
        at = close + 1;
    }
}

/// `[STUField(0xHASH, offset [, ReaderType = typeof(...)])] // size: N` plus the
/// declaration that follows, within one class body.
fn parse_fields(body: &str) -> Vec<RawField> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = body[at..].find(FIELD_MARKER) {
        let marker_end = at + found + FIELD_MARKER.len();
        if in_line_comment(body, at + found) {
            at = marker_end;
            continue;
        }
        let Some((hash, pos)) = read_hex(body, marker_end) else {
            at = marker_end;
            continue;
        };
        let Some(attr_end) = find_attr_end(body, pos) else {
            break;
        };
        let attr = &body[pos..attr_end];
        at = attr_end + 1;

        // The attribute tail is `, offset` and optionally
        // `, ReaderType = typeof(XFieldReader)`.
        let Some(offset) = attr
            .split(',')
            .nth(1)
            .and_then(|part| part.trim().parse::<u32>().ok())
        else {
            continue;
        };
        let reader = if attr.contains("EmbeddedInstanceFieldReader") {
            ReaderKind::Embedded
        } else if attr.contains("InlineInstanceFieldReader") {
            ReaderKind::Inline
        } else {
            ReaderKind::Default
        };

        let Some(semi) = find_decl_end(body, at) else {
            break;
        };
        let decl_at = at;
        let decl = &body[decl_at..semi];
        at = semi + 1;

        // `// size: N`, written between the attribute and the declaration.
        let size_hint = decl.find("// size:").and_then(|hint| {
            let rest = decl[hint + "// size:".len()..].trim_start();
            let end = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            rest[..end].parse::<u32>().ok()
        });

        let Some(public_at) = decl.find("public ") else {
            continue;
        };
        // A commented-out declaration still says `public`, so the line itself
        // decides whether the field exists.
        if in_line_comment(body, decl_at + public_at) {
            continue;
        }
        let decl = decl[public_at + "public ".len()..].trim_start();
        let Some(space) = decl.find(char::is_whitespace) else {
            continue;
        };
        let type_name = &decl[..space];
        let rest = decl[space..].trim_start();
        let name_end = rest
            .find(|c: char| c.is_whitespace() || c == '=' || c == ';')
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        let default_expr = rest[name_end..]
            .trim_start()
            .strip_prefix('=')
            .map(|expr| expr.trim().to_string())
            .filter(|expr| !expr.is_empty());

        out.push(RawField {
            hash,
            v1_offset: offset,
            size_hint,
            reader,
            type_name: type_name.to_string(),
            name: name.to_string(),
            default_expr,
        });
    }
    out
}

/// `[STUEnum(0xHASH)] public enum NAME : underlying { [STUField...] M = V, }`
/// blocks.
fn parse_enums(text: &str, out: &mut Vec<RawEnum>) {
    let mut at = 0;
    while let Some(found) = text[at..].find(ENUM_MARKER) {
        let marker_end = at + found + ENUM_MARKER.len();
        if in_line_comment(text, at + found) {
            at = marker_end;
            continue;
        }
        let Some((hash, pos)) = read_hex(text, marker_end) else {
            at = marker_end;
            continue;
        };
        let Some(close) = text[pos..].find(')') else {
            break;
        };
        let mut after = pos + close + 1;

        let Some(enum_at) = text[after..].find("public enum ") else {
            break;
        };
        let name_start = after + enum_at + "public enum ".len();
        let name_end = scan_ident(text, name_start);
        let name = &text[name_start..name_end];

        let Some(colon) = text[name_end..].find(':') else {
            at = name_end;
            continue;
        };
        let under_start = name_end + colon + 1;
        let under_rest = text[under_start..].trim_start();
        let under_at = under_start + (text[under_start..].len() - under_rest.len());
        let under_end = scan_ident(text, under_at);
        let Some(underlying) = prim_from_name(&text[under_at..under_end]) else {
            at = under_end;
            continue;
        };

        let Some(open_rel) = text[under_end..].find('{') else {
            break;
        };
        let open = under_end + open_rel;
        let Some(close) = matching_brace(text, open) else {
            break;
        };

        out.push(RawEnum {
            hash,
            name: name.to_string(),
            underlying,
            members: parse_enum_members(&text[open + 1..close]),
        });
        after = close + 1;
        at = after;
    }
}

/// `[STUField(0xHASH)] Name = value,` member declarations within an enum body.
fn parse_enum_members(body: &str) -> Vec<(String, i64)> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = body[at..].find(FIELD_MARKER) {
        let marker_end = at + found + FIELD_MARKER.len();
        if in_line_comment(body, at + found) {
            at = marker_end;
            continue;
        }
        let Some((_hash, pos)) = read_hex(body, marker_end) else {
            at = marker_end;
            continue;
        };
        let Some(close) = body[pos..].find(']') else {
            break;
        };
        let start = pos + close + 1;
        let Some(end) = body[start..].find([',', '}']).map(|offset| start + offset) else {
            break;
        };
        let member = body[start..end].trim();
        at = end + 1;

        let Some((name, value)) = member.split_once('=') else {
            continue;
        };
        let Some(value) = parse_int_literal(value.trim()) else {
            continue;
        };
        out.push((name.trim().to_string(), value));
    }
    out
}

// ---------------------------------------------------------------------------
// Type and default mapping
// ---------------------------------------------------------------------------

fn prim_from_name(name: &str) -> Option<Prim> {
    Some(match name {
        "sbyte" => Prim::I8,
        "byte" => Prim::U8,
        "short" => Prim::I16,
        "ushort" => Prim::U16,
        "int" => Prim::I32,
        "uint" => Prim::U32,
        "long" => Prim::I64,
        "ulong" => Prim::U64,
        "float" => Prim::F32,
        "double" => Prim::F64,
        _ => return None,
    })
}

fn struct_kind(name: &str) -> Option<StructKind> {
    Some(match name {
        "teVec2" => StructKind::Vec2,
        "teVec3" => StructKind::Vec3,
        "teVec3A" => StructKind::Vec3A,
        "teVec4" => StructKind::Vec4,
        "teQuat" => StructKind::Quat,
        "teColorRGB" => StructKind::ColorRGB,
        "teColorRGBA" => StructKind::ColorRGBA,
        "teMtx43A" => StructKind::Mtx43A,
        "teUUID" => StructKind::Uuid,
        "DBID" => StructKind::Dbid,
        "teStructuredDataDateAndTime" => StructKind::DateAndTime,
        _ => return None,
    })
}

fn map_type(
    type_name: &str,
    classes: &HashMap<String, u32>,
    enums: &HashMap<String, u32>,
    unknown: &mut BTreeMap<String, usize>,
) -> FieldType {
    if let Some(inner) = type_name.strip_suffix("[]") {
        return FieldType::Array(Box::new(map_scalar_type(inner, classes, enums, unknown)));
    }
    map_scalar_type(type_name, classes, enums, unknown)
}

fn map_scalar_type(
    type_name: &str,
    classes: &HashMap<String, u32>,
    enums: &HashMap<String, u32>,
    unknown: &mut BTreeMap<String, usize>,
) -> FieldType {
    if type_name == "teString" {
        return FieldType::String;
    }
    if type_name.starts_with("teStructuredDataAssetRef<") {
        return FieldType::AssetRef;
    }
    if type_name.starts_with("teStructuredDataHashMap<") {
        return FieldType::HashMap;
    }
    // One `uint` in the `teEntityID` wrapper, read as that.
    if type_name == "teEntityID" {
        return FieldType::Prim(Prim::U32);
    }
    if let Some(prim) = prim_from_name(type_name) {
        return FieldType::Prim(prim);
    }
    if let Some(kind) = struct_kind(type_name) {
        return FieldType::Struct(kind);
    }
    if let Some(hash) = enums.get(type_name) {
        return FieldType::Enum(*hash);
    }
    if let Some(hash) = classes.get(type_name) {
        return FieldType::Instance(*hash);
    }
    *unknown.entry(type_name.to_string()).or_insert(0) += 1;
    FieldType::Unknown
}

/// Parse one initializer expression. Every form the generator writes is
/// covered: plain literals (`0x1E`, `-1`, `0.5f`), enum members
/// (`Enum_X.xNAME`, `TeamIndex.xName`), enum casts (`(Enum_X)0x3`, read as the
/// numeric literal), struct constructors (`new teVec2(1f, 1f)`), and strings.
fn parse_default(expr: &str) -> Option<DefaultValue> {
    let expr = expr.trim();
    if let Some(text) = string_literal(expr) {
        return Some(DefaultValue::Text(text));
    }
    if let Some(ctor) = expr.strip_prefix("new ") {
        let open = ctor.find('(')?;
        let close = ctor.rfind(')')?;
        let mut parts = Vec::new();
        for arg in ctor[open + 1..close].split(',') {
            parts.push(parse_float_literal(arg)?);
        }
        return Some(DefaultValue::Struct(parts));
    }
    if expr.starts_with('(') {
        let close = expr.find(')')?;
        return parse_default(expr[close + 1..].trim());
    }
    if expr.starts_with("0x") || expr.starts_with("0X") {
        if let Ok(value) = u64::from_str_radix(&expr[2..], 16) {
            return Some(DefaultValue::Int(value as i64));
        }
    }
    if let Some(magnitude) = expr.strip_suffix(['f', 'F']) {
        if let Ok(value) = magnitude.parse::<f64>() {
            return Some(DefaultValue::Float(value));
        }
    }
    if let Ok(value) = expr.parse::<i64>() {
        return Some(DefaultValue::Int(value));
    }
    if let Ok(value) = expr.parse::<f64>() {
        return Some(DefaultValue::Float(value));
    }
    if let Some((_, member)) = expr.split_once('.') {
        if !member.is_empty()
            && member
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Some(DefaultValue::EnumMember(member.to_string()));
        }
    }
    None
}

fn parse_float_literal(text: &str) -> Option<f32> {
    let text = text.trim();
    let text = text.strip_suffix(['f', 'F']).unwrap_or(text);
    text.parse::<f32>().ok()
}

fn parse_int_literal(text: &str) -> Option<i64> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).ok()
    } else {
        text.parse::<i64>().ok()
    }
}

fn string_literal(expr: &str) -> Option<String> {
    let mut chars = expr.strip_prefix('"')?.chars();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => out.push(chars.next()?),
            _ => out.push(c),
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
    [STU(0x11111111, 64)]
    public class STUBase : STUInstance
    {
        [STUField(0xAAAA0001, 0)] // size: 4
        public float m_health = 100f;

        [STUField(0xAAAA0002, 4)] // size: 8
        public ulong m_owner = 0x1234;
    }

    [STU(0x22222222, 128)]
    public class STUChild : STUBase
    {
        // } this brace is inside a comment
        [STUField(0xBBBB0001, 8, ReaderType = typeof(EmbeddedInstanceFieldReader))] // size: 8
        public STUBase m_friend;

        [STUField(0xBBBB0002, 16, ReaderType = typeof(InlineInstanceFieldReader))] // size: 16
        public teStructuredDataAssetRef<STUHero>[] m_heroes;

        [STUField(0xBBBB0003, 32)]
        public Enum_DEAD0001 m_kind = Enum_DEAD0001.x2;

        [STUField(0xBBBB0004, 36)]
        public Enum_DEAD0001 m_other = (Enum_DEAD0001)0x3;

        [STUField(0xBBBB0005, 40)]
        public teVec2 m_offset = new teVec2(0.5f, -0.5f);

        [STUField(0xBBBB0006, 48)]
        public teString m_note = "size: {not a comment}";

        [STUField(0xBBBB0007, 56)]
        public teEntityID m_source;

        [STUField(0xBBBB0008, 60)]
        public teStructuredDataHashMap<STUChild> m_map;

        [STUField(0xBBBB0009, 64)]
        public MysteryType m_mystery;

        [STUField(0xBBBB000A, 68)]
        public float m_bad = 5 + 3;

        // [STUField(0xBBBB000B, 72)] // size: 4
        // public float m_commented;
    }

    [STUEnum(0xDEAD0001)]
    public enum Enum_DEAD0001 : byte
    {
        [STUField(0x01)] x1 = 0,
        [STUField(0x02)] x2 = 0x1,
        [STUField(0x03)] x3 = 0x2,
    }
    "#;

    fn sample_registry() -> ClassRegistry {
        let mut classes = Vec::new();
        let mut enums = Vec::new();
        parse_cs_file(SAMPLE, &mut classes, &mut enums);
        let mut registry = ClassRegistry::default();
        registry.build(classes, enums);
        registry
    }

    #[test]
    fn loads_class_headers_and_inheritance() {
        let registry = sample_registry();
        let reg = registry.inner();
        assert_eq!(reg.class_count(), 2);
        assert_eq!(reg.enum_count(), 1);

        let base = reg.class(0x11111111).expect("base class");
        assert_eq!(base.name, "STUBase");
        assert_eq!(base.size, 64);
        assert_eq!(base.base, None);

        let child = reg.class(0x22222222).expect("child class");
        assert_eq!(child.base, Some(0x11111111));
        assert_eq!(child.size, 128);
        assert!(child.own_field(0xAAAA0001).is_none());

        let inherited = reg.field(0x22222222, 0xAAAA0001).expect("inherited field");
        assert_eq!(inherited.default, DefaultValue::Float(100.0));
        assert_eq!(
            reg.field(0x22222222, 0xAAAA0002).unwrap().default,
            DefaultValue::Int(0x1234)
        );

        // Version 1 order: the root's fields first, then the child's.
        let order = reg.field_order(0x22222222);
        assert_eq!(&order[..2], &[0xAAAA0001, 0xAAAA0002]);
    }

    #[test]
    fn maps_field_types_and_readers() {
        let registry = sample_registry();
        let reg = registry.inner();
        let field = |hash| reg.field(0x22222222, hash).expect("field");

        let friend = field(0xBBBB0001);
        assert_eq!(friend.reader, ReaderKind::Embedded);
        assert_eq!(friend.ty, FieldType::Instance(0x11111111));
        assert_eq!(friend.size_hint, Some(8));
        assert_eq!(friend.v1_offset, 8);

        let heroes = field(0xBBBB0002);
        assert_eq!(heroes.reader, ReaderKind::Inline);
        assert_eq!(heroes.ty, FieldType::Array(Box::new(FieldType::AssetRef)));

        let kind = field(0xBBBB0003);
        assert_eq!(kind.ty, FieldType::Enum(0xDEAD0001));
        assert_eq!(kind.default, DefaultValue::EnumMember("x2".to_string()));

        assert_eq!(field(0xBBBB0004).default, DefaultValue::Int(3));

        let offset = field(0xBBBB0005);
        assert_eq!(offset.ty, FieldType::Struct(StructKind::Vec2));
        assert_eq!(offset.default, DefaultValue::Struct(vec![0.5, -0.5]));

        let note = field(0xBBBB0006);
        assert_eq!(note.ty, FieldType::String);
        assert_eq!(
            note.default,
            DefaultValue::Text("size: {not a comment}".to_string())
        );

        assert_eq!(field(0xBBBB0007).ty, FieldType::Prim(Prim::U32));
        assert_eq!(field(0xBBBB0008).ty, FieldType::HashMap);
        assert_eq!(field(0xBBBB0009).ty, FieldType::Unknown);

        // A commented-out declaration is not a field.
        assert!(reg.field(0x22222222, 0xBBBB000B).is_none());
        assert_eq!(registry.inner().class(0x22222222).unwrap().fields.len(), 10);

        assert_eq!(registry.unknown_types.get("MysteryType"), Some(&1));
        assert!(registry.unknown_bases.is_empty());
        assert_eq!(registry.unparsed_defaults.len(), 1);
    }

    #[test]
    fn loads_enum_members() {
        let registry = sample_registry();
        let def = registry.inner().enum_def(0xDEAD0001).expect("enum");
        assert_eq!(def.name, "Enum_DEAD0001");
        assert_eq!(def.underlying, Prim::U8);
        assert_eq!(
            def.members,
            vec![
                ("x1".to_string(), 0),
                ("x2".to_string(), 1),
                ("x3".to_string(), 2),
            ]
        );
        assert_eq!(def.member_name(1), Some("x2"));
        assert_eq!(def.member_value("x3"), Some(2));
    }

    #[test]
    fn parses_every_default_form_seen_in_the_generated_sources() {
        assert_eq!(parse_default("0"), Some(DefaultValue::Int(0)));
        assert_eq!(parse_default("-1"), Some(DefaultValue::Int(-1)));
        assert_eq!(parse_default("0x1E"), Some(DefaultValue::Int(30)));
        assert_eq!(parse_default("0x1f"), Some(DefaultValue::Int(31)));
        assert_eq!(parse_default("0.5f"), Some(DefaultValue::Float(0.5)));
        assert_eq!(parse_default("1f"), Some(DefaultValue::Float(1.0)));
        assert_eq!(parse_default("-1f"), Some(DefaultValue::Float(-1.0)));
        assert_eq!(parse_default("0.0035"), Some(DefaultValue::Float(0.0035)));
        assert_eq!(
            parse_default("(Enum_539C28BF)0x3"),
            Some(DefaultValue::Int(3))
        );
        assert_eq!(
            parse_default("Enum_1E9100B8.xA2915D6E"),
            Some(DefaultValue::EnumMember("xA2915D6E".to_string()))
        );
        assert_eq!(
            parse_default("TeamIndex.xTeam1"),
            Some(DefaultValue::EnumMember("xTeam1".to_string()))
        );
        assert_eq!(
            parse_default("new teVec3A(0f, -1f, 0f, 0f)"),
            Some(DefaultValue::Struct(vec![0.0, -1.0, 0.0, 0.0]))
        );
        assert_eq!(
            parse_default("\"\""),
            Some(DefaultValue::Text(String::new()))
        );
        assert_eq!(
            parse_default("\"Damage Zone\""),
            Some(DefaultValue::Text("Damage Zone".to_string()))
        );
        assert_eq!(parse_default("5 + 3"), None);
        assert_eq!(parse_default("0x"), None);
    }
}
