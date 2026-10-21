//! Typed reads over a parsed asset.
//!
//! The raw reader in [`crate`] hands back bytes: a field is "8 bytes at +0x30".
//! To know that those bytes are an `f32` of 250 rather than an instance index,
//! something has to say what the field *is*, and that lives in the class
//! definitions — each class hash names a layout of fields, and each field hash
//! inside it has a type, an inheritance position and a default. Those
//! definitions are the reference library's generated `TankLib.STU.Types`
//! sources; this module only holds the shape they load into, so it stays free
//! of file IO and of any dependency on a particular checkout.
//!
//! With a [`ClassRegistry`] in hand:
//!
//! * [`Stu::typed_fields`] decodes every field an instance actually carries,
//! * [`Stu::typed_field`] decodes one field by hash,
//! * and both say **which** fields were absent from the payload and which
//!   values are simply the class default, because the two cases look identical
//!   in the bytes (an absent float reads as 0.0).
//!
//! ## Version 1 reads use the registry offsets; version 2 uses the file's bag
//!
//! In version 2 the file itself describes each instance's fields (hash, offset,
//! size), so typed reading only needs the registry for the *types*. Version 1
//! has no per-instance bag at all: the serialized field offsets are the ones
//! recorded on the class (`[STUField(hash, offset)]`), measured from the
//! instance's own start (including its 8-byte `hash, nextOffset` prefix), and a
//! field is only read if the class declares it. That is why a registry is
//! required for a version 1 read and is what information the two versions
//! actually carry.
//!
//! ## What "missing" and "defaulted" mean
//!
//! [`Value::Missing`] is a field the class declares that this instance does not
//! carry (only version 2 can express that; version 1 carries every declared
//! field). [`Value::Defaulted`] wraps a value whose bytes are exactly what the
//! class declaration says they should be when the field was never authored:
//! the initializer expression when there is one (`= 1f`), the zero of the type
//! when there is not, and null for the reference-like types. A caller that
//! wants "what did the author actually write" filters on `!is_defaulted()`.

use crate::Stu;
use std::collections::HashMap;
use std::fmt;

/// More elements than any real array or map holds; a guard so a malformed
/// count cannot ask for an unbounded allocation.
const MAX_ELEMENTS: u64 = 1 << 20;

// ---------------------------------------------------------------------------
// The shape of a class registry
// ---------------------------------------------------------------------------

/// A scalar's storage width and interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prim {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
}

impl Prim {
    pub fn size(self) -> usize {
        match self {
            Prim::I8 | Prim::U8 => 1,
            Prim::I16 | Prim::U16 => 2,
            Prim::I32 | Prim::U32 | Prim::F32 => 4,
            Prim::I64 | Prim::U64 | Prim::F64 => 8,
        }
    }

    /// Whether the value is signed when interpreted.
    pub fn signed(self) -> bool {
        matches!(self, Prim::I8 | Prim::I16 | Prim::I32 | Prim::I64)
    }

    /// The mask of bits a value of this width occupies.
    fn mask(self) -> u64 {
        if self.size() == 8 {
            u64::MAX
        } else {
            (1u64 << (8 * self.size())) - 1
        }
    }

    /// Raw little-endian bits of one value, zero-extended to a `u64`.
    fn read_at(self, data: &[u8], at: usize) -> Option<u64> {
        read_bits(data, at, self.size())
    }
}

/// The value-type structs that appear as STU fields.
///
/// The float-shaped ones (vectors, quaternions, colors) decode to their
/// components. The opaque ones do not: `teStructuredDataDateAndTime` is
/// explicitly unparsed in the reference ("todo: actually parse this") and the
/// rest are blobs as far as the reader is concerned, so they come back as raw
/// bytes rather than as components this code would have to invent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructKind {
    Vec2,
    Vec3,
    Vec3A,
    Vec4,
    Quat,
    ColorRGB,
    ColorRGBA,
    Mtx43A,
    Uuid,
    Dbid,
    DateAndTime,
}

impl StructKind {
    /// Byte size of the struct, from its field-by-field layout.
    pub fn size(self) -> usize {
        match self {
            StructKind::Vec2 => 8,
            StructKind::Vec3 => 12,
            StructKind::Vec3A | StructKind::Vec4 | StructKind::Quat => 16,
            StructKind::ColorRGB => 12,
            StructKind::ColorRGBA => 16,
            // Sixteen f32: four rows of four. The name says 4x3 but the
            // declaration carries M01..M16, so it is read as written.
            StructKind::Mtx43A => 64,
            StructKind::Uuid => 16,
            StructKind::Dbid => 16,
            StructKind::DateAndTime => 8,
        }
    }

    /// How many `f32` components the struct holds, when it is float-shaped.
    pub fn float_count(self) -> Option<usize> {
        match self {
            StructKind::Vec2 => Some(2),
            StructKind::Vec3 | StructKind::ColorRGB => Some(3),
            StructKind::Vec3A | StructKind::Vec4 | StructKind::Quat | StructKind::ColorRGBA => {
                Some(4)
            }
            StructKind::Mtx43A => Some(16),
            StructKind::Uuid | StructKind::Dbid | StructKind::DateAndTime => None,
        }
    }
}

/// How a field's bytes are read, beyond its type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderKind {
    /// Plain value, or an in-place instance for class-typed fields.
    Default,
    /// An index (V2) or record offset (V1) to an instance elsewhere.
    Embedded,
    /// An in-place instance payload sharing the parent's buffer.
    Inline,
}

/// What a field holds.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    Prim(Prim),
    String,
    AssetRef,
    /// Hash of the `[STUEnum]` definition.
    Enum(u32),
    Struct(StructKind),
    /// Hash of the STU class the field points at. Whether that is an index to
    /// an instance table (`ReaderKind::Embedded`) or an in-place payload
    /// (`Default`/`Inline`) is carried separately.
    Instance(u32),
    Array(Box<FieldType>),
    HashMap,
    /// A declared type the registry does not know how to read.
    Unknown,
}

/// The initializer expression from the class declaration, if any.
///
/// `None` is not "no value": for the value types it means the field's zero,
/// and for the reference-like types it means null. That is exactly C#'s
/// `default(T)`, which is what an unwritten field holds.
#[derive(Debug, Clone, PartialEq)]
pub enum DefaultValue {
    None,
    Int(i64),
    Float(f64),
    Bool(bool),
    Text(String),
    /// Member name of the field's enum, e.g. `xB`.
    EnumMember(String),
    /// Constructor arguments of a struct initializer, e.g. `new teVec2(1f, 1f)`.
    Struct(Vec<f32>),
}

/// One field declared on a class.
#[derive(Debug, Clone)]
pub struct FieldDef {
    pub hash: u32,
    /// The declared name as written, e.g. `m_projectileSpeed` or `m_1A2B3C4D`.
    pub name: String,
    /// The `[STUField(hash, offset)]` offset, used only by version 1 layouts.
    pub v1_offset: u32,
    /// The `// size: N` comment, when the generator wrote one. Version 1's
    /// layout widths, not what a version 2 payload will report.
    pub size_hint: Option<u32>,
    pub reader: ReaderKind,
    pub ty: FieldType,
    pub default: DefaultValue,
}

impl FieldDef {
    /// The declared name without its `m_` prefix, unless the name is really
    /// just the hash (`m_1A2B3C4D`), in which case there is no name to give.
    pub fn short_name(&self) -> Option<&str> {
        let rest = self.name.strip_prefix("m_")?;
        if rest.len() == 8 && rest.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        Some(rest)
    }
}

/// One `[STUEnum]` definition.
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub hash: u32,
    pub name: String,
    /// The enum's underlying integer type; determines the stored width.
    pub underlying: Prim,
    pub members: Vec<(String, i64)>,
}

impl EnumDef {
    pub fn member_value(&self, name: &str) -> Option<i64> {
        self.members
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| *v)
    }

    /// The member name for a raw value, if one matches.
    pub fn member_name(&self, raw: u64) -> Option<&str> {
        let mask = self.underlying.mask();
        self.members
            .iter()
            .find(|(_, v)| (*v as u64) & mask == raw & mask)
            .map(|(n, _)| n.as_str())
    }
}

/// One `[STU(hash, size)]` class definition.
#[derive(Debug, Clone)]
pub struct ClassDef {
    pub hash: u32,
    pub name: String,
    /// The class's serialized size, as declared.
    pub size: u32,
    pub base: Option<u32>,
    /// Fields **declared on this class**, in declaration order. Inherited
    /// fields are found by walking [`ClassDef::base`] through
    /// [`ClassRegistry::field`]; nothing is copied down, because the whole
    /// `STUConfigVar` family would multiply its base's fields several thousand
    /// times over.
    pub fields: Vec<FieldDef>,
    field_index: HashMap<u32, usize>,
}

impl ClassDef {
    pub fn new(hash: u32, name: String, size: u32, base: Option<u32>) -> ClassDef {
        ClassDef {
            hash,
            name,
            size,
            base,
            fields: Vec::new(),
            field_index: HashMap::new(),
        }
    }

    pub fn add_field(&mut self, def: FieldDef) {
        self.field_index.insert(def.hash, self.fields.len());
        self.fields.push(def);
    }

    /// A field declared directly on this class.
    pub fn own_field(&self, hash: u32) -> Option<&FieldDef> {
        self.field_index.get(&hash).map(|i| &self.fields[*i])
    }
}

/// Every class and enum the typed layer knows, keyed by hash.
#[derive(Debug, Default)]
pub struct ClassRegistry {
    classes: HashMap<u32, ClassDef>,
    enums: HashMap<u32, EnumDef>,
}

impl ClassRegistry {
    pub fn new() -> ClassRegistry {
        ClassRegistry::default()
    }

    pub fn add_class(&mut self, def: ClassDef) {
        self.classes.insert(def.hash, def);
    }

    pub fn add_enum(&mut self, def: EnumDef) {
        self.enums.insert(def.hash, def);
    }

    pub fn class(&self, hash: u32) -> Option<&ClassDef> {
        self.classes.get(&hash)
    }

    pub fn enum_def(&self, hash: u32) -> Option<&EnumDef> {
        self.enums.get(&hash)
    }

    pub fn class_count(&self) -> usize {
        self.classes.len()
    }

    /// Declared fields across every class, without inherited copies.
    pub fn field_count(&self) -> usize {
        self.classes.values().map(|c| c.fields.len()).sum()
    }

    pub fn enum_count(&self) -> usize {
        self.enums.len()
    }

    pub fn classes(&self) -> impl Iterator<Item = &ClassDef> {
        self.classes.values()
    }

    pub fn enums(&self) -> impl Iterator<Item = &EnumDef> {
        self.enums.values()
    }

    /// A field of a class, searching up the inheritance chain.
    ///
    /// A derived class that redeclares a hash shadows the base's field, which
    /// is the resolution the reference's reflection dictionary has: it is
    /// keyed by hash and overwritten by the derived declaration.
    pub fn field(&self, class_hash: u32, field_hash: u32) -> Option<&FieldDef> {
        let mut current = Some(class_hash);
        // Guard against a malformed (cyclic) chain; real chains are a handful
        // of levels.
        for _ in 0..64 {
            let hash = current?;
            let class = self.classes.get(&hash)?;
            if let Some(field) = class.own_field(field_hash) {
                return Some(field);
            }
            current = class.base;
        }
        None
    }

    /// Field hashes in version 1 read order: root base first, then each
    /// class's own declarations downward, deduplicated by first appearance.
    ///
    /// This is the order the reference walks for version 1 (`GetFieldOrder`
    /// concatenates parent fields then declared fields), and it only matters
    /// for version 1: version 2 order is the payload's own bag order.
    pub fn field_order(&self, class_hash: u32) -> Vec<u32> {
        let mut chain = Vec::new();
        let mut current = Some(class_hash);
        for _ in 0..64 {
            let Some(hash) = current else { break };
            let Some(class) = self.classes.get(&hash) else {
                break;
            };
            chain.push(hash);
            current = class.base;
        }

        let mut out = Vec::new();
        for hash in chain.into_iter().rev() {
            if let Some(class) = self.classes.get(&hash) {
                for field in &class.fields {
                    if !out.contains(&field.hash) {
                        out.push(field.hash);
                    }
                }
            }
        }
        out
    }

    pub fn class_name(&self, hash: u32) -> Option<&str> {
        self.classes.get(&hash).map(|c| c.name.as_str())
    }
}

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

/// A decoded field value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// The class declares the field; this instance does not carry it.
    Missing,
    /// The field is present but its bytes could not be read.
    Unreadable,
    /// A reference-like field with no value: string offset -1, empty/absent
    /// array or map, embedded index -1.
    Null,
    /// Bytes whose declared type the registry does not decode.
    Raw(Vec<u8>),
    /// A scalar's raw little-endian bits. The width is `Prim`.
    Prim(Prim, u64),
    Text(String),
    /// A de-obfuscated asset GUID.
    AssetRef(u64),
    Enum {
        raw: u64,
        member: Option<String>,
    },
    /// Components of a float-shaped struct.
    Struct(Vec<f32>),
    /// Index into the asset's instance table.
    Instance(usize),
    /// An in-place instance payload: `start` is where the payload's bag index
    /// sits. `bag >= 0` is a version 2 element (layout from the file);
    /// `bag == -1` is a version 1 payload, whose fields sit at the registry
    /// offsets relative to `start`.
    Inline {
        start: usize,
        bag: i32,
    },
    Array(Vec<Value>),
    /// `(key, instance index)` pairs; `None` is a key mapped to a null value.
    Map(Vec<(u64, Option<usize>)>),
    /// The stored bytes are exactly the class default; nothing was authored.
    Defaulted(Box<Value>),
}

impl Value {
    pub fn is_defaulted(&self) -> bool {
        matches!(self, Value::Defaulted(_))
    }

    /// The value with any [`Value::Defaulted`] wrapper removed.
    pub fn unwrap_defaulted(&self) -> &Value {
        match self {
            Value::Defaulted(inner) => inner,
            other => other,
        }
    }

    /// A short human form, for logs and probes.
    pub fn describe(&self) -> String {
        match self {
            Value::Missing => "<missing>".to_string(),
            Value::Unreadable => "<unreadable>".to_string(),
            Value::Null => "null".to_string(),
            Value::Raw(bytes) => format!("bytes[{}]", bytes.len()),
            Value::Prim(prim, bits) => match prim {
                Prim::F32 => format!("{:?}f", f32::from_bits(*bits as u32)),
                Prim::F64 => format!("{:?}", f64::from_bits(*bits)),
                Prim::I8 | Prim::I16 | Prim::I32 | Prim::I64 => {
                    format!("{}", sign_extend(*prim, *bits))
                }
                _ => format!("{bits}"),
            },
            Value::Text(text) => format!("{text:?}"),
            Value::AssetRef(guid) => format!("ref 0x{guid:016x}"),
            Value::Enum { raw, member } => match member {
                Some(name) => format!("{name}({raw})"),
                None => format!("enum {raw}"),
            },
            Value::Struct(components) => {
                let joined: Vec<String> = components.iter().map(|c| format!("{c:?}")).collect();
                format!("({})", joined.join(", "))
            }
            Value::Instance(index) => format!("instance #{index}"),
            Value::Inline { start, bag } => {
                if *bag >= 0 {
                    format!("inline @{start:#x} bag {bag}")
                } else {
                    format!("inline @{start:#x}")
                }
            }
            Value::Array(items) => format!("array[{}]", items.len()),
            Value::Map(entries) => format!("map[{}]", entries.len()),
            Value::Defaulted(inner) => format!("{} =default", inner.describe()),
        }
    }
}

/// A field hash paired with its decoded value.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedField {
    pub hash: u32,
    /// Declared name without its prefix, when it has a real one.
    pub name: Option<String>,
    pub value: Value,
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.describe())
    }
}

// ---------------------------------------------------------------------------
// Typed reading
// ---------------------------------------------------------------------------

impl Stu {
    /// Every field this instance carries, decoded.
    ///
    /// Version 2 walks the payload's own field bag, so a field the class
    /// declarations do not know still appears, as [`Value::Raw`]. Version 1
    /// walks the class's field order and reads each at its declared offset.
    pub fn typed_fields(&mut self, instance: usize, reg: &ClassRegistry) -> Vec<TypedField> {
        let Some(info) = self.instances.get(instance).copied() else {
            return Vec::new();
        };
        if self.is_v1() {
            let Some(record) = self.v1_record(instance) else {
                return Vec::new();
            };
            if record.offset <= 0 {
                return Vec::new();
            }
            return self.typed_fields_at(info.hash, record.offset as usize, -1, reg);
        }
        let start = self.payload_start(instance);
        let bag = self.bag_index_at(start);
        self.typed_fields_at(info.hash, start, bag, reg)
    }

    /// One field's value.
    ///
    /// `None` when neither the class nor the payload knows the hash. A hash
    /// the class declares but this instance does not carry gives
    /// `Some(Value::Missing)`; a version 1 instance whose record offset is
    /// non-positive reads as `Some(Value::Unreadable)`.
    pub fn typed_field(
        &mut self,
        instance: usize,
        field_hash: u32,
        reg: &ClassRegistry,
    ) -> Option<Value> {
        let info = self.instances.get(instance).copied()?;
        if self.is_v1() {
            let def = reg.field(info.hash, field_hash)?;
            let record = self.v1_record(instance)?;
            if record.offset <= 0 {
                return Some(Value::Unreadable);
            }
            let pos = record.offset as usize + def.v1_offset as usize;
            return Some(self.read_field(def, pos, -1, 0, false, reg));
        }
        let start = self.payload_start(instance);
        let bag = self.bag_index_at(start);
        if bag < 0 {
            return reg.field(info.hash, field_hash).map(|_| Value::Missing);
        }
        let layout = self.layout_at(start, bag);
        let Some(field) = layout.iter().find(|f| f.hash == field_hash).copied() else {
            return reg.field(info.hash, field_hash).map(|_| Value::Missing);
        };
        let declared_zero = self.declared_size(bag, field_hash) == Some(0);
        let value = match reg.field(info.hash, field_hash) {
            Some(def) => self.read_field(
                def,
                start + field.offset,
                bag,
                field.size,
                declared_zero,
                reg,
            ),
            None => self.raw_bytes(start + field.offset, field.size),
        };
        Some(value)
    }

    /// Every field of an instance payload that is not in the instance table:
    /// a version 2 inline element (`bag >= 0`) or a version 1 record
    /// (`bag == -1`, `start` at the record's own start, prefix included).
    pub fn typed_fields_at(
        &mut self,
        class_hash: u32,
        start: usize,
        bag: i32,
        reg: &ClassRegistry,
    ) -> Vec<TypedField> {
        let mut out = Vec::new();
        if bag >= 0 {
            for field in self.layout_at(start, bag) {
                let def = reg.field(class_hash, field.hash);
                let declared_zero = self.declared_size(bag, field.hash) == Some(0);
                let value = match def {
                    Some(def) => self.read_field(
                        def,
                        start + field.offset,
                        bag,
                        field.size,
                        declared_zero,
                        reg,
                    ),
                    None => self.raw_bytes(start + field.offset, field.size),
                };
                out.push(TypedField {
                    hash: field.hash,
                    name: def.and_then(|d| d.short_name().map(str::to_string)),
                    value,
                });
            }
        } else if reg.class(class_hash).is_some() {
            for hash in reg.field_order(class_hash) {
                let Some(def) = reg.field(class_hash, hash) else {
                    continue;
                };
                let value = self.read_field(def, start + def.v1_offset as usize, -1, 0, false, reg);
                out.push(TypedField {
                    hash,
                    name: def.short_name().map(str::to_string),
                    value,
                });
            }
        }
        out
    }

    /// One field of an out-of-table payload, the counterpart of
    /// [`Stu::typed_field`] for an [`Value::Inline`] result.
    pub fn typed_field_at(
        &mut self,
        class_hash: u32,
        start: usize,
        bag: i32,
        field_hash: u32,
        reg: &ClassRegistry,
    ) -> Option<Value> {
        if bag >= 0 {
            let layout = self.layout_at(start, bag);
            let field = layout.iter().find(|f| f.hash == field_hash).copied()?;
            let declared_zero = self.declared_size(bag, field_hash) == Some(0);
            let value = match reg.field(class_hash, field_hash) {
                Some(def) => self.read_field(
                    def,
                    start + field.offset,
                    bag,
                    field.size,
                    declared_zero,
                    reg,
                ),
                None => self.raw_bytes(start + field.offset, field.size),
            };
            Some(value)
        } else {
            let def = reg.field(class_hash, field_hash)?;
            let pos = start + def.v1_offset as usize;
            Some(self.read_field(def, pos, -1, 0, false, reg))
        }
    }

    /// The declared bag size of a version 2 field, before any inline size
    /// substitution. `Some(0)` marks the fields whose body carries its own
    /// size, which is also what routes arrays onto the in-place path.
    fn declared_size(&self, bag: i32, field_hash: u32) -> Option<i32> {
        let bag_index = usize::try_from(bag).ok()?;
        self.field_bags
            .get(bag_index)?
            .iter()
            .find(|(hash, _)| *hash == field_hash)
            .map(|(_, size)| *size)
    }

    fn read_field(
        &self,
        def: &FieldDef,
        pos: usize,
        bag: i32,
        size: usize,
        declared_zero: bool,
        reg: &ClassRegistry,
    ) -> Value {
        // Version 1 has no bag sizes; a fallback width is only needed by
        // fields whose declared type is unknown.
        let size = if bag >= 0 {
            size
        } else {
            def.size_hint.unwrap_or(0) as usize
        };
        let value = self.read_value(&def.ty, def.reader, def.hash, pos, size, declared_zero, reg);
        apply_default(def, value, reg)
    }

    fn read_value(
        &self,
        ty: &FieldType,
        reader: ReaderKind,
        field_hash: u32,
        pos: usize,
        size: usize,
        declared_zero: bool,
        reg: &ClassRegistry,
    ) -> Value {
        let v1 = self.is_v1();
        match ty {
            FieldType::Prim(prim) => match prim.read_at(&self.blob, pos) {
                Some(bits) => Value::Prim(*prim, bits),
                None => Value::Unreadable,
            },
            FieldType::Enum(hash) => {
                let width = reg
                    .enum_def(*hash)
                    .map(|e| e.underlying.size())
                    .unwrap_or(4);
                match read_bits(&self.blob, pos, width) {
                    Some(bits) => Value::Enum {
                        raw: bits,
                        member: reg
                            .enum_def(*hash)
                            .and_then(|e| e.member_name(bits))
                            .map(str::to_string),
                    },
                    None => Value::Unreadable,
                }
            }
            FieldType::Struct(kind) => self.read_struct(*kind, pos),
            FieldType::String => self.read_text(pos, v1),
            FieldType::AssetRef => {
                // Version 1 stores padding then the raw GUID; version 2 stores
                // the obfuscated GUID alone.
                if v1 {
                    match rd_u64(&self.blob, pos + 8) {
                        Some(guid) => Value::AssetRef(guid),
                        None => Value::Unreadable,
                    }
                } else {
                    match rd_u64(&self.blob, pos) {
                        Some(stored) => Value::AssetRef(crate::deobfuscate(
                            self.header_checksum,
                            field_hash,
                            stored,
                        )),
                        None => Value::Unreadable,
                    }
                }
            }
            FieldType::Instance(_) => match reader {
                ReaderKind::Embedded => {
                    if v1 {
                        // A record offset, relative to the file start.
                        match rd_i32(&self.blob, pos) {
                            Some(offset) if offset > 0 => self
                                .instance_at_v1_offset(offset)
                                .map(Value::Instance)
                                .unwrap_or(Value::Null),
                            _ => Value::Null,
                        }
                    } else {
                        match rd_i32(&self.blob, pos) {
                            Some(index) if index >= 0 => {
                                if (index as usize) < self.instances.len() {
                                    Value::Instance(index as usize)
                                } else {
                                    Value::Unreadable
                                }
                            }
                            _ => Value::Null,
                        }
                    }
                }
                // Default and Inline class-typed fields share the parent's
                // buffer; the payload begins at the field itself.
                _ => {
                    if v1 {
                        Value::Inline {
                            start: pos,
                            bag: -1,
                        }
                    } else {
                        match rd_i32(&self.blob, pos) {
                            Some(bag) if bag >= 0 => Value::Inline { start: pos, bag },
                            _ => Value::Null,
                        }
                    }
                }
            },
            FieldType::Array(inner) => {
                self.read_array(inner, reader, field_hash, pos, declared_zero, reg)
            }
            FieldType::HashMap => self.read_map(pos),
            FieldType::Unknown => self.raw_bytes(pos, size),
        }
    }

    fn read_struct(&self, kind: StructKind, pos: usize) -> Value {
        match kind.float_count() {
            Some(count) => {
                let mut out = Vec::with_capacity(count);
                for i in 0..count {
                    match rd_u32(&self.blob, pos + i * 4) {
                        Some(bits) => out.push(f32::from_bits(bits)),
                        None => return Value::Unreadable,
                    }
                }
                Value::Struct(out)
            }
            None => self.raw_bytes(pos, kind.size()),
        }
    }

    /// A `teString` at a field position. Version 2's field value is an `i32`
    /// offset into the dynamic block; version 1's is an `i64` file offset.
    /// Both point at `i32 size; u32 checksum; i64 text offset`, and the text
    /// offset is relative to the same section its field belonged to.
    fn read_text(&self, pos: usize, v1: bool) -> Value {
        let info = if v1 {
            match rd_i64(&self.blob, pos) {
                Some(info) => info,
                None => return Value::Unreadable,
            }
        } else {
            match rd_i32(&self.blob, pos) {
                Some(offset) => offset as i64,
                None => return Value::Unreadable,
            }
        };
        // -1 is null in both versions; version 1 also writes 0 for it.
        if info < 0 || (v1 && info == 0) {
            return Value::Null;
        }
        let info_at = if v1 {
            info as usize
        } else {
            self.dyn_start + info as usize
        };
        self.read_text_body(info_at, v1)
    }

    /// The `size, checksum, text offset` triple and the characters it points
    /// at.
    fn read_text_body(&self, info_at: usize, v1: bool) -> Value {
        let Some(size) = rd_i32(&self.blob, info_at) else {
            return Value::Unreadable;
        };
        if size < 0 {
            return Value::Unreadable;
        }
        if size == 0 {
            return Value::Text(String::new());
        }
        let Some(text_offset) = rd_i64(&self.blob, info_at + 8) else {
            return Value::Unreadable;
        };
        if text_offset < 0 {
            return Value::Unreadable;
        }
        let start = if v1 {
            text_offset as usize
        } else {
            self.dyn_start + text_offset as usize
        };
        let end = start.saturating_add(size as usize);
        // A version 2 text must stay inside the dynamic block; version 1's
        // offsets are file-absolute.
        if !v1 && end > self.dyn_start + self.dyn_len {
            return Value::Unreadable;
        }
        match self.blob.get(start..end) {
            Some(bytes) => Value::Text(String::from_utf8_lossy(bytes).into_owned()),
            None => Value::Unreadable,
        }
    }

    fn read_array(
        &self,
        inner: &FieldType,
        reader: ReaderKind,
        field_hash: u32,
        pos: usize,
        declared_zero: bool,
        reg: &ClassRegistry,
    ) -> Value {
        if self.is_v1() {
            return self.read_array_v1(inner, reader, field_hash, pos, reg);
        }
        if declared_zero || reader == ReaderKind::Inline {
            // In place: an i32 count then the elements.
            let Some(count) = rd_i32(&self.blob, pos) else {
                return Value::Unreadable;
            };
            if count == 0 {
                return Value::Null;
            }
            if count < 0 || count as u64 > MAX_ELEMENTS {
                return Value::Unreadable;
            }
            return self.read_elements(inner, reader, field_hash, pos + 4, count as usize, reg);
        }
        // In the dynamic block: i32 offset, then count, a u32, and an i64
        // data offset relative to the block.
        let Some(offset) = rd_i32(&self.blob, pos) else {
            return Value::Unreadable;
        };
        if offset < 0 {
            return Value::Null;
        }
        let base = self.dyn_start + offset as usize;
        let Some(count) = rd_i32(&self.blob, base) else {
            return Value::Unreadable;
        };
        if count == 0 {
            return Value::Null;
        }
        if count < 0 || count as u64 > MAX_ELEMENTS {
            return Value::Unreadable;
        }
        let Some(data_offset) = rd_i64(&self.blob, base + 8) else {
            return Value::Unreadable;
        };
        if data_offset < 0 {
            return Value::Unreadable;
        }
        let start = self.dyn_start + data_offset as usize;
        self.read_elements(inner, reader, field_hash, start, count as usize, reg)
    }

    fn read_array_v1(
        &self,
        inner: &FieldType,
        reader: ReaderKind,
        field_hash: u32,
        pos: usize,
        reg: &ClassRegistry,
    ) -> Value {
        // `i32 offset; i32 padding`; the array descriptor is `i64 count;
        // i64 data offset`, both relative to the file start.
        let Some(offset) = rd_i32(&self.blob, pos) else {
            return Value::Unreadable;
        };
        if offset <= 0 {
            return Value::Null;
        }
        let Some(count) = rd_i64(&self.blob, offset as usize) else {
            return Value::Unreadable;
        };
        let Some(data_offset) = rd_i64(&self.blob, offset as usize + 8) else {
            return Value::Unreadable;
        };
        if count == -1 || data_offset <= 0 {
            return Value::Null;
        }
        if count < 0 || count as u64 > MAX_ELEMENTS {
            return Value::Unreadable;
        }
        let start = data_offset as usize;
        self.read_elements(inner, reader, field_hash, start, count as usize, reg)
    }

    fn read_elements(
        &self,
        inner: &FieldType,
        reader: ReaderKind,
        field_hash: u32,
        start: usize,
        count: usize,
        reg: &ClassRegistry,
    ) -> Value {
        // Count was already capped at MAX_ELEMENTS by the callers, but that
        // is still up to 1M entries: reserve only a small prefix so a
        // malformed count cannot ask for a huge allocation up front. The
        // vector grows incrementally if the elements really are there.
        let mut out = Vec::with_capacity(count.min(1024));
        let mut at = start;
        for _ in 0..count {
            let (value, step) = self.read_element(inner, reader, field_hash, at, reg);
            if step == 0 {
                return Value::Unreadable;
            }
            out.push(value);
            let Some(next) = at.checked_add(step) else {
                return Value::Unreadable;
            };
            at = next;
        }
        Value::Array(out)
    }

    /// One array element at an absolute offset, and how many bytes it spans.
    ///
    /// Elements are read sequentially. Where a version 1 element is a record
    /// offset, a version 2 element is an instance index; where a version 2
    /// string's offsets are relative to the dynamic block, a version 1
    /// string's are relative to the file — which the shared `dyn_start = 0`
    /// fallback makes the same code path.
    fn read_element(
        &self,
        inner: &FieldType,
        reader: ReaderKind,
        field_hash: u32,
        at: usize,
        reg: &ClassRegistry,
    ) -> (Value, usize) {
        let v1 = self.is_v1();
        match inner {
            FieldType::Prim(prim) => match prim.read_at(&self.blob, at) {
                Some(bits) => (Value::Prim(*prim, bits), prim.size()),
                None => (Value::Unreadable, 0),
            },
            FieldType::Enum(hash) => {
                let width = reg
                    .enum_def(*hash)
                    .map(|e| e.underlying.size())
                    .unwrap_or(4);
                match read_bits(&self.blob, at, width) {
                    Some(bits) => (
                        Value::Enum {
                            raw: bits,
                            member: reg
                                .enum_def(*hash)
                                .and_then(|e| e.member_name(bits))
                                .map(str::to_string),
                        },
                        width,
                    ),
                    None => (Value::Unreadable, 0),
                }
            }
            FieldType::Struct(kind) => (self.read_struct(*kind, at), kind.size()),
            FieldType::String => {
                // `i64 offset; i64 mutability`, then the string triple at the
                // offset, in the section the offset is measured from.
                let Some(offset) = rd_i64(&self.blob, at) else {
                    return (Value::Unreadable, 0);
                };
                if offset < 0 {
                    return (Value::Null, 16);
                }
                let info_at = if v1 {
                    offset as usize
                } else {
                    self.dyn_start + offset as usize
                };
                (self.read_text_body(info_at, v1), 16)
            }
            FieldType::AssetRef => {
                // Padding then the GUID in both versions; only version 2
                // obfuscates.
                match rd_u64(&self.blob, at + 8) {
                    Some(guid) => {
                        let guid = if v1 {
                            guid
                        } else {
                            crate::deobfuscate(self.header_checksum, field_hash, guid)
                        };
                        (Value::AssetRef(guid), 16)
                    }
                    None => (Value::Unreadable, 0),
                }
            }
            FieldType::Instance(_) => match reader {
                ReaderKind::Embedded => {
                    if v1 {
                        match rd_i32(&self.blob, at) {
                            Some(offset) if offset > 0 => (
                                self.instance_at_v1_offset(offset)
                                    .map(Value::Instance)
                                    .unwrap_or(Value::Null),
                                8,
                            ),
                            _ => (Value::Null, 8),
                        }
                    } else {
                        match rd_i32(&self.blob, at) {
                            Some(index) if index >= 0 => {
                                if (index as usize) < self.instances.len() {
                                    (Value::Instance(index as usize), 8)
                                } else {
                                    (Value::Unreadable, 8)
                                }
                            }
                            _ => (Value::Null, 8),
                        }
                    }
                }
                // An in-place element: its own bag index then fields, sharing
                // the buffer around it. The stride is where its layout ends.
                _ => {
                    if v1 {
                        // Version 1 elements have no bag index; each starts
                        // directly on its fields, spaced by the class size.
                        let size = match inner {
                            FieldType::Instance(hash) => {
                                reg.class(*hash).map(|c| c.size as usize).unwrap_or(0)
                            }
                            _ => 0,
                        };
                        (Value::Inline { start: at, bag: -1 }, size)
                    } else {
                        match rd_i32(&self.blob, at) {
                            Some(bag) if bag >= 0 => {
                                let stride = self
                                    .layout_at(at, bag)
                                    .last()
                                    .map(|f| f.offset + f.size)
                                    .unwrap_or(4)
                                    .max(4);
                                (Value::Inline { start: at, bag }, stride)
                            }
                            _ => (Value::Null, 4),
                        }
                    }
                }
            },
            // Arrays of arrays and arrays of maps do not exist; a registry
            // that claims otherwise is wrong, and saying so beats inventing a
            // stride that would parse the rest of the array as garbage.
            _ => (Value::Unreadable, 0),
        }
    }

    fn read_map(&self, pos: usize) -> Value {
        // i32 offset; the descriptor is `u32 bucket count; u32 unknown;
        // i64 bucket offset; i64 data offset`. Offsets are relative to the
        // dynamic block in version 2 and to the file in version 1.
        let Some(offset) = rd_i32(&self.blob, pos) else {
            return Value::Unreadable;
        };
        if offset < 0 {
            // The reference returns an empty dictionary for -1.
            return Value::Map(Vec::new());
        }
        let base = if self.is_v1() {
            offset as usize
        } else {
            self.dyn_start + offset as usize
        };
        let Some(bucket_count) = rd_u32(&self.blob, base) else {
            return Value::Unreadable;
        };
        if bucket_count == 0 {
            return Value::Map(Vec::new());
        }
        if bucket_count as u64 > MAX_ELEMENTS {
            return Value::Unreadable;
        }
        let Some(bucket_offset) = rd_i64(&self.blob, base + 8) else {
            return Value::Unreadable;
        };
        let Some(data_offset) = rd_i64(&self.blob, base + 16) else {
            return Value::Unreadable;
        };
        if bucket_offset < 0 || data_offset < 0 {
            return Value::Unreadable;
        }
        let section = if self.is_v1() { 0 } else { self.dyn_start };
        let buckets = section + bucket_offset as usize;
        // The entry count is the last bucket's value.
        let Some(entry_count) = rd_u32(&self.blob, buckets + (bucket_count as usize - 1) * 4)
        else {
            return Value::Unreadable;
        };
        if entry_count as u64 > MAX_ELEMENTS {
            return Value::Unreadable;
        }
        let entries = section + data_offset as usize;
        // Same rule as read_elements: reserve a prefix, not the whole
        // attacker-sized count, and fail cleanly on address overflow.
        let entry_count_us = entry_count as usize;
        let mut out = Vec::with_capacity(entry_count_us.min(1024));
        for i in 0..entry_count_us {
            let Some(stride) = i.checked_mul(16) else {
                return Value::Unreadable;
            };
            let Some(at) = entries.checked_add(stride) else {
                return Value::Unreadable;
            };
            let Some(key) = rd_u64(&self.blob, at) else {
                return Value::Unreadable;
            };
            // The value is a u64 whose low 32 bits are the instance index;
            // the rest is padding for in-place deserialization.
            let Some(at8) = at.checked_add(8) else {
                return Value::Unreadable;
            };
            let Some(raw) = rd_u64(&self.blob, at8) else {
                return Value::Unreadable;
            };
            let index = (raw & 0xFFFF_FFFF) as u32 as i32;
            if index == -1 {
                out.push((key, None));
            } else if index < 0 || index as usize >= self.instances.len() {
                return Value::Unreadable;
            } else {
                out.push((key, Some(index as usize)));
            }
        }
        Value::Map(out)
    }

    fn raw_bytes(&self, pos: usize, size: usize) -> Value {
        // `size` comes from the file (a bag's declared field size), so a huge
        // value would clone a huge slice. Cap it: anything over 1 MiB is not a
        // real unknown field, it is a malformed asset.
        const MAX_RAW_BYTES: usize = 1 << 20;
        if size > MAX_RAW_BYTES {
            return Value::Unreadable;
        }
        let Some(end) = pos.checked_add(size) else {
            return Value::Unreadable;
        };
        match self.blob.get(pos..end) {
            Some(bytes) => Value::Raw(bytes.to_vec()),
            None => Value::Unreadable,
        }
    }
}

/// Whether a value's bytes are exactly the class's default.
///
/// `DefaultValue::None` is per-type: zero for the value types, but the
/// reference-like types (strings, references, arrays) hold a *null*, and a
/// decoded null is already visible as [`Value::Null`], so it is not wrapped —
/// wrapping would call every unauthored string "defaulted" and hide which
/// strings the file actually carries.
fn apply_default(def: &FieldDef, value: Value, reg: &ClassRegistry) -> Value {
    let defaulted = match &def.default {
        DefaultValue::None => match (&def.ty, &value) {
            (FieldType::Prim(prim), Value::Prim(read, bits)) => read == prim && *bits == 0,
            (FieldType::Enum(_), Value::Enum { raw, .. }) => *raw == 0,
            (FieldType::Struct(_), Value::Struct(components)) => {
                components.iter().all(|c| c.to_bits() == 0)
            }
            _ => false,
        },
        DefaultValue::Int(expected) => match (&def.ty, &value) {
            (FieldType::Prim(prim), Value::Prim(_, bits)) => {
                *bits == (*expected as u64) & prim.mask()
            }
            (FieldType::Enum(hash), Value::Enum { raw, .. }) => match reg.enum_def(*hash) {
                Some(def) => *raw == (*expected as u64) & def.underlying.mask(),
                None => false,
            },
            (FieldType::AssetRef, Value::AssetRef(guid)) => *guid == *expected as u64,
            _ => false,
        },
        DefaultValue::Float(expected) => match (&def.ty, &value) {
            (FieldType::Prim(Prim::F32), Value::Prim(_, bits)) => {
                *bits == (*expected as f32).to_bits() as u64
            }
            (FieldType::Prim(Prim::F64), Value::Prim(_, bits)) => *bits == expected.to_bits(),
            _ => false,
        },
        DefaultValue::Bool(expected) => match &value {
            Value::Prim(_, bits) => *bits == *expected as u64,
            _ => false,
        },
        DefaultValue::Text(expected) => match &value {
            Value::Text(text) => text == expected,
            _ => false,
        },
        DefaultValue::EnumMember(name) => match (&def.ty, &value) {
            (FieldType::Enum(hash), Value::Enum { raw, .. }) => match reg.enum_def(*hash) {
                Some(def) => def
                    .member_value(name)
                    .map(|v| (v as u64) & def.underlying.mask() == *raw)
                    .unwrap_or(false),
                None => false,
            },
            _ => false,
        },
        DefaultValue::Struct(expected) => match &value {
            Value::Struct(components) => {
                components.len() == expected.len()
                    && components
                        .iter()
                        .zip(expected)
                        .all(|(read, want)| read.to_bits() == want.to_bits())
            }
            _ => false,
        },
    };
    if defaulted {
        Value::Defaulted(Box::new(value))
    } else {
        value
    }
}

fn sign_extend(prim: Prim, bits: u64) -> i64 {
    let width = prim.size() * 8;
    if width == 64 {
        bits as i64
    } else {
        let shift = 64 - width;
        ((bits << shift) as i64) >> shift
    }
}

fn read_bits(data: &[u8], at: usize, width: usize) -> Option<u64> {
    let bytes = data.get(at..at.checked_add(width)?)?;
    let mut bits = 0u64;
    for (i, byte) in bytes.iter().enumerate() {
        bits |= (*byte as u64) << (8 * i);
    }
    Some(bits)
}

/// Little-endian reads that fail with `None` rather than an error: the typed
/// layer says "unreadable", it does not propagate parse failures.
fn rd_u32(data: &[u8], at: usize) -> Option<u32> {
    read_bits(data, at, 4).map(|bits| bits as u32)
}

fn rd_i32(data: &[u8], at: usize) -> Option<i32> {
    rd_u32(data, at).map(|v| v as i32)
}

fn rd_u64(data: &[u8], at: usize) -> Option<u64> {
    read_bits(data, at, 8)
}

fn rd_i64(data: &[u8], at: usize) -> Option<i64> {
    rd_u64(data, at).map(|v| v as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // A synthetic version 1 asset, laid out by hand.
    //
    //   0..8     header: magic, 3 records
    //   8..32    records {i32 offset, u32 flags}
    //   40..104  instance 0: class A, 64 bytes, fields at registry offsets
    //   104..120 instance 1: class B, 16 bytes
    //   120..136 instance 2: class B, 16 bytes
    //   200..     string info (size, checksum, i64 text offset); text at 228
    //   320..     array descriptor (i64 count, i64 data offset); data at 352
    // -----------------------------------------------------------------

    const CLASS_A: u32 = 0x0000_AAAA;
    const CLASS_B: u32 = 0x0000_BBBB;
    const F_STR: u32 = 0x0000_A001;
    const F_VAL: u32 = 0x0000_A002;
    const F_REF: u32 = 0x0000_A003;
    const F_EMB: u32 = 0x0000_A004;
    const F_ARR: u32 = 0x0000_A005;
    const G_X: u32 = 0x0000_B001;

    fn registry() -> ClassRegistry {
        let mut reg = ClassRegistry::new();

        let mut a = ClassDef::new(CLASS_A, "STU_A".to_string(), 64, None);
        a.add_field(FieldDef {
            hash: F_STR,
            name: "m_name".to_string(),
            v1_offset: 8,
            size_hint: Some(16),
            reader: ReaderKind::Default,
            ty: FieldType::String,
            default: DefaultValue::None,
        });
        a.add_field(FieldDef {
            hash: F_VAL,
            name: "m_speed".to_string(),
            // Deliberately past the naive packing: a real layout aligns
            // vectors, so the offset is not derivable from field sizes.
            v1_offset: 28,
            size_hint: Some(4),
            reader: ReaderKind::Default,
            ty: FieldType::Prim(Prim::F32),
            default: DefaultValue::None,
        });
        a.add_field(FieldDef {
            hash: F_REF,
            name: "m_1A2B3C4D".to_string(),
            v1_offset: 32,
            size_hint: Some(16),
            reader: ReaderKind::Default,
            ty: FieldType::AssetRef,
            default: DefaultValue::None,
        });
        a.add_field(FieldDef {
            hash: F_EMB,
            name: "m_child".to_string(),
            v1_offset: 48,
            size_hint: Some(8),
            reader: ReaderKind::Embedded,
            ty: FieldType::Instance(CLASS_B),
            default: DefaultValue::None,
        });
        a.add_field(FieldDef {
            hash: F_ARR,
            name: "m_values".to_string(),
            v1_offset: 56,
            size_hint: Some(8),
            reader: ReaderKind::Default,
            ty: FieldType::Array(Box::new(FieldType::Prim(Prim::U32))),
            default: DefaultValue::None,
        });
        reg.add_class(a);

        let mut b = ClassDef::new(CLASS_B, "STU_B".to_string(), 16, None);
        b.add_field(FieldDef {
            hash: G_X,
            name: "m_scale".to_string(),
            v1_offset: 8,
            size_hint: Some(4),
            reader: ReaderKind::Default,
            ty: FieldType::Prim(Prim::F32),
            default: DefaultValue::Float(0.5),
        });
        reg.add_class(b);
        reg
    }

    fn put(b: &mut [u8], at: usize, bytes: &[u8]) {
        b[at..at + bytes.len()].copy_from_slice(bytes);
    }

    fn synthetic_v1() -> Vec<u8> {
        let mut blob = vec![0u8; 400];
        put(&mut blob, 0, b"STUD");
        put(&mut blob, 4, &3u32.to_le_bytes());
        // records
        put(&mut blob, 8, &40i32.to_le_bytes());
        put(&mut blob, 12, &0u32.to_le_bytes());
        put(&mut blob, 16, &104i32.to_le_bytes());
        put(&mut blob, 20, &0u32.to_le_bytes());
        put(&mut blob, 24, &120i32.to_le_bytes());
        put(&mut blob, 28, &0u32.to_le_bytes());

        // instance 0: class A at 40
        put(&mut blob, 40, &CLASS_A.to_le_bytes());
        put(&mut blob, 44, &0u32.to_le_bytes());
        put(&mut blob, 48, &200i64.to_le_bytes()); // F_STR info offset
        put(&mut blob, 56, &0i64.to_le_bytes());
        put(&mut blob, 68, &2.0f32.to_le_bytes()); // F_VAL at 40 + 28
        put(&mut blob, 72, &0xFFFF_FFFF_FFFF_FFFFu64.to_le_bytes()); // F_REF padding
        put(&mut blob, 80, &0xDEAD_BEEF_0000_0042u64.to_le_bytes()); // F_REF guid
        put(&mut blob, 88, &104i32.to_le_bytes()); // F_EMB -> record 1
        put(&mut blob, 92, &0i32.to_le_bytes());
        put(&mut blob, 96, &320i32.to_le_bytes()); // F_ARR descriptor at 320
        put(&mut blob, 100, &0i32.to_le_bytes());

        // instance 1: class B at 104, scale 0.5 (the class default)
        put(&mut blob, 104, &CLASS_B.to_le_bytes());
        put(&mut blob, 112, &0.5f32.to_le_bytes());
        // instance 2: class B at 120, scale 0.25
        put(&mut blob, 120, &CLASS_B.to_le_bytes());
        put(&mut blob, 128, &0.25f32.to_le_bytes());

        // string: size 5 at 200, text offset 228
        put(&mut blob, 200, &5i32.to_le_bytes());
        put(&mut blob, 204, &0u32.to_le_bytes());
        put(&mut blob, 208, &228i64.to_le_bytes());
        put(&mut blob, 228, b"hello");

        // array: count 3 at 320, data at 352
        put(&mut blob, 320, &3i64.to_le_bytes());
        put(&mut blob, 328, &352i64.to_le_bytes());
        put(&mut blob, 352, &11u32.to_le_bytes());
        put(&mut blob, 356, &22u32.to_le_bytes());
        put(&mut blob, 360, &33u32.to_le_bytes());

        blob
    }

    #[test]
    fn v1_reads_fields_at_the_registry_offsets() {
        let reg = registry();
        let mut stu = Stu::parse(synthetic_v1()).expect("v1 parses");
        assert!(stu.is_v1());
        assert_eq!(stu.instances.len(), 3);
        assert_eq!(stu.instances[0].hash, CLASS_A);
        assert_eq!(stu.instances[1].hash, CLASS_B);

        // The raw layer has no per-instance bag in version 1.
        assert!(stu.fields(0).is_empty());

        let fields = stu.typed_fields(0, &reg);
        assert_eq!(fields.len(), 5, "all five declared fields");
        let by_hash = |hash: u32| fields.iter().find(|f| f.hash == hash).unwrap();

        // The string's text offset is a file offset, not a dyn-block one.
        assert_eq!(by_hash(F_STR).value, Value::Text("hello".to_string()));
        assert_eq!(by_hash(F_STR).name.as_deref(), Some("name"));
        // The float sits at the declared offset, not at a packed one.
        assert_eq!(
            by_hash(F_VAL).value,
            Value::Prim(Prim::F32, 2.0f32.to_bits() as u64)
        );
        // Version 1 asset references are stored raw, not obfuscated.
        assert_eq!(by_hash(F_REF).value, Value::AssetRef(0xDEAD_BEEF_0000_0042));
        assert_eq!(by_hash(F_REF).name, None, "a hash placeholder has no name");
        // The embedded field resolves through the record table.
        assert_eq!(by_hash(F_EMB).value, Value::Instance(1));
        assert_eq!(
            by_hash(F_ARR).value,
            Value::Array(vec![
                Value::Prim(Prim::U32, 11),
                Value::Prim(Prim::U32, 22),
                Value::Prim(Prim::U32, 33),
            ])
        );

        // The child whose float equals the class default says so; the other
        // does not.
        let child_default = stu.typed_field(1, G_X, &reg).unwrap();
        assert_eq!(
            child_default,
            Value::Defaulted(Box::new(Value::Prim(Prim::F32, 0.5f32.to_bits() as u64)))
        );
        let child_authored = stu.typed_field(2, G_X, &reg).unwrap();
        assert_eq!(
            child_authored,
            Value::Prim(Prim::F32, 0.25f32.to_bits() as u64)
        );
    }

    #[test]
    fn v1_unknown_hashes_stay_unknown() {
        let reg = registry();
        let mut stu = Stu::parse(synthetic_v1()).expect("v1 parses");
        assert_eq!(stu.typed_field(0, 0xDEAD_BEEF, &reg), None);
        assert!(stu.typed_fields(0, &ClassRegistry::new()).is_empty());
    }

    // -----------------------------------------------------------------
    // A synthetic version 2 asset.
    //
    //   0..36    header (instance bag, inline-array bag, field-bag array,
    //            dyn size/off, data off)
    //   40..     two instance records
    //   72..     one inner field bag header
    //   88..     eight field definitions
    //   180..    data buffer
    //   300..    dynamic block
    // -----------------------------------------------------------------

    const CLASS_C: u32 = 0x000C_1A55;
    const CLASS_D: u32 = 0x000C_2B00;
    const ENUM_E: u32 = 0x000E_0001;
    const C_F32: u32 = 0x000C_1001;
    const C_ENUM: u32 = 0x000C_1002;
    const C_STR: u32 = 0x000C_1003;
    const C_ARR: u32 = 0x000C_1004;
    const C_EMB: u32 = 0x000C_1005;
    const C_REF: u32 = 0x000C_1006;
    const C_HEX: u32 = 0x000C_1007;
    const C_MAP: u32 = 0x000C_1008;
    const C_ABSENT: u32 = 0x000C_1009;
    const D_X: u32 = 0x000C_2001;

    const REF_GUID: u64 = 0x1122_3344_5566_7788;

    fn registry_v2() -> ClassRegistry {
        let mut reg = ClassRegistry::new();
        reg.add_enum(EnumDef {
            hash: ENUM_E,
            name: "Enum_E".to_string(),
            underlying: Prim::I32,
            members: vec![
                ("xA".to_string(), 0),
                ("xB".to_string(), 1),
                ("xC".to_string(), 2),
            ],
        });

        let mut c = ClassDef::new(CLASS_C, "STU_C".to_string(), 44, None);
        c.add_field(FieldDef {
            hash: C_F32,
            name: "m_scale".to_string(),
            v1_offset: 8,
            size_hint: Some(4),
            reader: ReaderKind::Default,
            ty: FieldType::Prim(Prim::F32),
            default: DefaultValue::Float(1.0),
        });
        c.add_field(FieldDef {
            hash: C_ENUM,
            name: "m_mode".to_string(),
            v1_offset: 12,
            size_hint: Some(4),
            reader: ReaderKind::Default,
            ty: FieldType::Enum(ENUM_E),
            default: DefaultValue::EnumMember("xB".to_string()),
        });
        c.add_field(FieldDef {
            hash: C_STR,
            name: "m_name".to_string(),
            v1_offset: 16,
            size_hint: Some(16),
            reader: ReaderKind::Default,
            ty: FieldType::String,
            default: DefaultValue::None,
        });
        c.add_field(FieldDef {
            hash: C_ARR,
            name: "m_values".to_string(),
            v1_offset: 32,
            size_hint: Some(16),
            reader: ReaderKind::Default,
            ty: FieldType::Array(Box::new(FieldType::Prim(Prim::U32))),
            default: DefaultValue::None,
        });
        c.add_field(FieldDef {
            hash: C_EMB,
            name: "m_child".to_string(),
            v1_offset: 48,
            size_hint: Some(8),
            reader: ReaderKind::Embedded,
            ty: FieldType::Instance(CLASS_D),
            default: DefaultValue::None,
        });
        c.add_field(FieldDef {
            hash: C_REF,
            name: "m_ref".to_string(),
            v1_offset: 56,
            size_hint: Some(16),
            reader: ReaderKind::Default,
            ty: FieldType::AssetRef,
            default: DefaultValue::Int(REF_GUID as i64),
        });
        c.add_field(FieldDef {
            hash: C_HEX,
            name: "m_zero".to_string(),
            v1_offset: 72,
            size_hint: Some(4),
            reader: ReaderKind::Default,
            ty: FieldType::Prim(Prim::U32),
            default: DefaultValue::None,
        });
        c.add_field(FieldDef {
            hash: C_MAP,
            name: "m_map".to_string(),
            v1_offset: 76,
            size_hint: Some(24),
            reader: ReaderKind::Default,
            ty: FieldType::HashMap,
            default: DefaultValue::None,
        });
        // Declared by the class but not serialized into this payload.
        c.add_field(FieldDef {
            hash: C_ABSENT,
            name: "m_absent".to_string(),
            v1_offset: 100,
            size_hint: Some(4),
            reader: ReaderKind::Default,
            ty: FieldType::Prim(Prim::U32),
            default: DefaultValue::None,
        });
        reg.add_class(c);

        let mut d = ClassDef::new(CLASS_D, "STU_D".to_string(), 8, None);
        d.add_field(FieldDef {
            hash: D_X,
            name: "m_x".to_string(),
            v1_offset: 8,
            size_hint: Some(4),
            reader: ReaderKind::Default,
            ty: FieldType::Prim(Prim::U32),
            default: DefaultValue::None,
        });
        reg.add_class(d);
        reg
    }

    /// The obfuscation's inverse: the reader's `deobfuscate` XORs and then
    /// applies four byte swaps; the swaps are disjoint pairs, so applying the
    /// same swaps to a desired GUID and XORing gives the stored form.
    fn obfuscate(header_checksum: u64, field_hash: u32, guid: u64) -> u64 {
        let mut bytes = guid.to_le_bytes();
        bytes.swap(0, 3);
        bytes.swap(7, 1);
        bytes.swap(2, 6);
        bytes.swap(4, 5);
        let mixed = (field_hash as u64) | ((field_hash as u64) << 32);
        u64::from_le_bytes(bytes) ^ mixed ^ header_checksum
    }

    fn synthetic_v2() -> Vec<u8> {
        let mut blob = vec![0u8; 420];
        // header
        put(&mut blob, 0, &2i32.to_le_bytes()); // instance bag: 2 records
        put(&mut blob, 4, &40i32.to_le_bytes());
        put(&mut blob, 8, &0i32.to_le_bytes()); // no inline-array info
        put(&mut blob, 12, &0i32.to_le_bytes());
        put(&mut blob, 16, &1i32.to_le_bytes()); // field-bag array: 1 bag
        put(&mut blob, 20, &72i32.to_le_bytes());
        put(&mut blob, 24, &120i32.to_le_bytes()); // dyn size
        put(&mut blob, 28, &300i32.to_le_bytes()); // dyn offset
        put(&mut blob, 32, &180i32.to_le_bytes()); // data offset

        // instances: root class C, then child class D
        put(&mut blob, 40, &CLASS_C.to_le_bytes());
        put(&mut blob, 44, &0u32.to_le_bytes());
        put(&mut blob, 48, &(-1i32).to_le_bytes());
        put(&mut blob, 52, &44i32.to_le_bytes());
        put(&mut blob, 56, &CLASS_D.to_le_bytes());
        put(&mut blob, 60, &C_EMB.to_le_bytes());
        put(&mut blob, 64, &0i32.to_le_bytes());
        put(&mut blob, 68, &12i32.to_le_bytes());

        // field bag: 8 fields, definitions at 88
        put(&mut blob, 72, &8i32.to_le_bytes());
        put(&mut blob, 76, &88i32.to_le_bytes());
        let fields: [(u32, i32); 8] = [
            (C_F32, 4),
            (C_ENUM, 4),
            (C_STR, 4),
            (C_ARR, 4),
            (C_EMB, 8),
            (C_REF, 8),
            (C_HEX, 4),
            (C_MAP, 4),
        ];
        for (i, (hash, size)) in fields.iter().enumerate() {
            put(&mut blob, 88 + i * 8, &hash.to_le_bytes());
            put(&mut blob, 92 + i * 8, &size.to_le_bytes());
        }

        // data buffer at 180: bag index 0, then the field bodies
        let mut at = 180;
        put(&mut blob, at, &0i32.to_le_bytes());
        at += 4;
        put(&mut blob, at, &1.0f32.to_le_bytes()); // C_F32, the class default
        at += 4;
        put(&mut blob, at, &1u32.to_le_bytes()); // C_ENUM, member xB
        at += 4;
        put(&mut blob, at, &0i32.to_le_bytes()); // C_STR -> dyn offset 0
        at += 4;
        put(&mut blob, at, &32i32.to_le_bytes()); // C_ARR -> dyn offset 32
        at += 4;
        put(&mut blob, at, &1i64.to_le_bytes()); // C_EMB -> instance 1
        at += 8;
        let header_checksum = crate::crc::crc64(&blob[0..36]);
        put(
            &mut blob,
            at,
            &obfuscate(header_checksum, C_REF, REF_GUID).to_le_bytes(),
        );
        at += 8;
        put(&mut blob, at, &0u32.to_le_bytes()); // C_HEX, unauthored zero
        at += 4;
        put(&mut blob, at, &64i32.to_le_bytes()); // C_MAP -> dyn offset 64
        let _ = at;

        // Instance 1's payload is empty (bag index -1); it exists only to be
        // pointed at. Its data offset follows the root's 44-byte payload.
        put(&mut blob, 224, &(-1i32).to_le_bytes());

        // dynamic block
        // string: size 5 at +0, text at +16
        put(&mut blob, 300, &5i32.to_le_bytes());
        put(&mut blob, 304, &0u32.to_le_bytes());
        put(&mut blob, 308, &16i64.to_le_bytes());
        put(&mut blob, 316, b"hello");
        // array: count 3 at +32, data at +48
        put(&mut blob, 332, &3i32.to_le_bytes());
        put(&mut blob, 336, &0u32.to_le_bytes());
        put(&mut blob, 340, &48i64.to_le_bytes());
        put(&mut blob, 348, &7u32.to_le_bytes());
        put(&mut blob, 352, &8u32.to_le_bytes());
        put(&mut blob, 356, &9u32.to_le_bytes());
        // map: descriptor at +64, buckets at +88, entries at +96
        put(&mut blob, 364, &1u32.to_le_bytes());
        put(&mut blob, 368, &0u32.to_le_bytes());
        put(&mut blob, 372, &88i64.to_le_bytes());
        put(&mut blob, 380, &96i64.to_le_bytes());
        put(&mut blob, 388, &1u32.to_le_bytes());
        put(&mut blob, 396, &0xAABBu64.to_le_bytes());
        put(&mut blob, 404, &0xFFFF_FFFF_0000_0001u64.to_le_bytes());
        blob
    }

    #[test]
    fn v2_reads_every_field_with_types_and_defaults() {
        let reg = registry_v2();
        let mut stu = Stu::parse(synthetic_v2()).expect("v2 parses");
        assert!(!stu.is_v1());

        let fields = stu.typed_fields(0, &reg);
        assert_eq!(fields.len(), 8, "the bag's fields, in bag order");
        let by_hash = |hash: u32| fields.iter().find(|f| f.hash == hash).unwrap();

        assert_eq!(
            by_hash(C_F32).value,
            Value::Defaulted(Box::new(Value::Prim(Prim::F32, 1.0f32.to_bits() as u64)))
        );
        assert_eq!(
            by_hash(C_ENUM).value,
            Value::Defaulted(Box::new(Value::Enum {
                raw: 1,
                member: Some("xB".to_string())
            }))
        );
        assert_eq!(by_hash(C_STR).value, Value::Text("hello".to_string()));
        assert_eq!(
            by_hash(C_ARR).value,
            Value::Array(vec![
                Value::Prim(Prim::U32, 7),
                Value::Prim(Prim::U32, 8),
                Value::Prim(Prim::U32, 9),
            ])
        );
        assert_eq!(by_hash(C_EMB).value, Value::Instance(1));
        assert_eq!(
            by_hash(C_REF).value,
            Value::Defaulted(Box::new(Value::AssetRef(REF_GUID)))
        );
        assert_eq!(
            by_hash(C_HEX).value,
            Value::Defaulted(Box::new(Value::Prim(Prim::U32, 0)))
        );
        assert_eq!(by_hash(C_MAP).value, Value::Map(vec![(0xAABB, Some(1))]));

        // A hash the class declares but the payload omits.
        assert_eq!(stu.typed_field(0, C_ABSENT, &reg), Some(Value::Missing));
        // A hash neither knows.
        assert_eq!(stu.typed_field(0, 0xDEAD_BEEF, &reg), None);
        // A hash the class declares, on an instance whose payload is empty,
        // reads as missing rather than as bytes.
        assert_eq!(stu.typed_field(1, D_X, &reg), Some(Value::Missing));
    }

    #[test]
    fn v2_field_order_follows_base_then_declaration() {
        let mut reg = ClassRegistry::new();
        let mut base = ClassDef::new(0x10, "Base".to_string(), 8, None);
        base.add_field(FieldDef {
            hash: 0xAB,
            name: "m_a".to_string(),
            v1_offset: 8,
            size_hint: Some(4),
            reader: ReaderKind::Default,
            ty: FieldType::Prim(Prim::U32),
            default: DefaultValue::None,
        });
        reg.add_class(base);
        let mut derived = ClassDef::new(0x20, "Derived".to_string(), 16, Some(0x10));
        derived.add_field(FieldDef {
            hash: 0xCD,
            name: "m_b".to_string(),
            v1_offset: 12,
            size_hint: Some(4),
            reader: ReaderKind::Default,
            ty: FieldType::Prim(Prim::U32),
            default: DefaultValue::None,
        });
        reg.add_class(derived);

        assert_eq!(reg.field_order(0x20), vec![0xAB, 0xCD]);
        // Inherited lookup; the derived hash shadows the base's.
        assert!(reg.field(0x20, 0xAB).is_some());
        assert!(reg.field(0x20, 0xCD).is_some());
        assert_eq!(reg.field_count(), 2);
    }
}
