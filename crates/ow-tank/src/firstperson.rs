//! The client's first-person component data.
//!
//! `STUFirstPersonComponent` (`0x9D28963F`) lives on the hero's gameplay
//! entity and carries the per-hero view-model/camera parameters: an offset
//! struct, the view-model entity reference, the drag spring and four
//! scalars. The values are **read** (`docs/formats/animation.md`
//! 2026-10-20 (12)); which scalar the client uses as the FP-pass FOV, eye
//! height or scale is still unread, so consumers must treat the numbers as
//! data and the mapping as a hypothesis (`docs/fitted-ledger.md`).
//!
//! This module only parses the component. The viewer exposes the mapping as
//! explicit flags (`--fp-fov`, `--fp-eye`) until the consumer is read.

use ow_stu::types::{ClassRegistry, DefaultValue, Prim, TypedField, Value};

use crate::GameData;

/// The component's class hash.
pub const COMPONENT: u32 = 0x9D28_963F;
/// Scalar floats on the component.
pub const FOV_CANDIDATE: u32 = 0x4B75_D84E;
pub const EYE_HEIGHT_CANDIDATE: u32 = 0xE299_93A5;
pub const SCALE_A: u32 = 0x816B_830F;
pub const SCALE_B: u32 = 0xCF50_55F1;
/// The inline offset struct and its position field.
pub const OFFSET: u32 = 0xA02D_D607;
pub const OFFSET_STRUCT: u32 = 0xE43E_9C08;
pub const OFFSET_POSITION: u32 = 0x7DC1_550F;
/// The drag-spring struct and its three floats (64 / 16 / -1 on hero 3).
pub const SPRING: u32 = 0xF486_DCF2;
pub const SPRING_STRUCT: u32 = 0x6AE5_A305;
pub const SPRING_FIELDS: [u32; 3] = [0x823E_15BD, 0xDD79_8420, 0x53A9_AC14];
/// The view-model entity reference.
pub const VIEW_MODEL_ENTITY: u32 = 0xA83C_2C26;

/// Parsed `STUFirstPersonComponent` values for one hero.
#[derive(Debug, Clone, Copy, Default)]
pub struct Params {
    /// `0x4B75D84E` - 45..55 per hero; FP-pass FOV candidate.
    pub fov: f32,
    /// `0xE29993A5` - 1.25..1.5 per hero; eye-height candidate.
    pub eye_height: f32,
    /// `0x816B830F` - 0.875..1.085 per hero.
    pub scale_a: f32,
    /// `0xCF5055F1` - 0.8..1.0 per hero.
    pub scale_b: f32,
    /// `m_A02DD607.m_position` - the view-model offset candidate.
    pub offset: [f32; 3],
    /// `m_F486DCF2` - the drag spring (k, c, gain).
    pub spring: [f32; 3],
    /// The view-model entity the component names.
    pub view_model_entity: u64,
}

fn field<'a>(fields: &'a [TypedField], hash: u32) -> Option<&'a Value> {
    fields.iter().find(|f| f.hash == hash).map(|f| &f.value)
}

fn undefault(v: &Value) -> &Value {
    match v {
        Value::Defaulted(inner) => undefault(inner),
        other => other,
    }
}

fn as_f32(v: &Value) -> Option<f32> {
    match undefault(v) {
        Value::Prim(Prim::F32, bits) => Some(f32::from_bits(*bits as u32)),
        _ => None,
    }
}

fn as_asset(v: &Value) -> Option<u64> {
    match undefault(v) {
        Value::AssetRef(g) => Some(*g),
        _ => None,
    }
}

fn inline_child<'a>(
    reg: &'a ClassRegistry,
    class: u32,
    hash: u32,
) -> Option<&'a u32> {
    match reg.field(class, hash).map(|d| &d.ty) {
        Some(ow_stu::types::FieldType::Instance(child)) => Some(child),
        _ => None,
    }
}

/// A field's value in the instance, else the class default, else 0.
fn scalar(
    reg: &ClassRegistry,
    class: u32,
    fields: &[TypedField],
    hash: u32,
) -> f32 {
    if let Some(v) = field(fields, hash).and_then(as_f32) {
        return v;
    }
    match reg.field(class, hash).map(|d| &d.default) {
        Some(DefaultValue::Float(f)) => *f as f32,
        Some(DefaultValue::Int(i)) => *i as f32,
        _ => 0.0,
    }
}

/// Read the component from the hero's gameplay entity.
pub fn read(
    data: &GameData,
    reg: &ClassRegistry,
    entity: u64,
) -> Option<Params> {
    let mut stu = data.stu_of(entity).ok()?;
    let index = stu.instances.iter().position(|i| i.hash == COMPONENT)?;
    let fields = stu.typed_fields(index, reg);
    let mut params = Params::default();
    params.fov = scalar(reg, COMPONENT, &fields, FOV_CANDIDATE);
    params.eye_height = scalar(reg, COMPONENT, &fields, EYE_HEIGHT_CANDIDATE);
    params.scale_a = scalar(reg, COMPONENT, &fields, SCALE_A);
    params.scale_b = scalar(reg, COMPONENT, &fields, SCALE_B);
    params.view_model_entity = field(&fields, VIEW_MODEL_ENTITY)
        .and_then(as_asset)
        .unwrap_or(0);

    if let Some(Value::Inline { start, bag }) = field(&fields, OFFSET).map(undefault) {
        if let Some(child) = inline_child(reg, COMPONENT, OFFSET) {
            let inner = stu.typed_fields_at(*child, *start, *bag, reg);
            if let Some(Value::Struct(v)) =
                field(&inner, OFFSET_POSITION).map(undefault)
            {
                for i in 0..3.min(v.len()) {
                    params.offset[i] = v[i];
                }
            }
        }
    }
    for (i, hash) in SPRING_FIELDS.iter().enumerate() {
        params.spring[i] = scalar(reg, SPRING_STRUCT, &[], *hash);
    }
    if let Some(Value::Inline { start, bag }) = field(&fields, SPRING).map(undefault) {
        if let Some(child) = inline_child(reg, COMPONENT, SPRING) {
            let inner = stu.typed_fields_at(*child, *start, *bag, reg);
            for (i, hash) in SPRING_FIELDS.iter().enumerate() {
                if let Some(v) = field(&inner, *hash).and_then(as_f32) {
                    params.spring[i] = v;
                }
            }
        }
    }
    Some(params)
}
