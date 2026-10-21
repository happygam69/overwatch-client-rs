//! The animation blend-tree asset (`STUAnimBlendTree`), read from the
//! install.
//!
//! Evidence: `docs/formats/animation.md` 2026-10-20 (5)-(7). The tree is a
//! plain STU asset referenced by a blend-set item's `0xC0214513` field; its
//! instances are the typed nodes (BranchByCategory / LayerChild /
//! AnimationPose2d / Animation / LayerAdditive). Every parameter object
//! (`STU_ABD8FE73` and friends) carries `m_63DEDAC0`, which is a **1-based
//! index into the item's `0xC486B05D` float array** - the same field on an
//! animation wrapper is the 1-based clip index.
//!
//! This module only *reads* the asset. The per-node runtime maths
//! (Pose2d driver evaluation, layer compositing) is not here; what is
//! known and what is fitted is declared in `docs/fitted-ledger.md`.

use ow_stu::types::{ClassRegistry, Prim, TypedField, Value};

use crate::GameData;

pub const SET_ROOT: u32 = 0xFC47_A2ED;
pub const SET_ITEM: u32 = 0xDDD1_8945;
pub const TREE: u32 = 0x6786_6D38;
pub const BRANCH: u32 = 0xEEC7_A51C;
pub const LAYER_CHILD: u32 = 0x4027_4C18;
pub const POSE2D: u32 = 0xC8B1_D2D7;
pub const ANIMATION: u32 = 0xCB60_35BA;
pub const LAYER_ADDITIVE: u32 = 0xA719_E03A;
pub const BRANCH_CHILD: u32 = 0xC9E2_FF36;
pub const ANIM_WRAPPER: u32 = 0x215B_E8F3;
pub const DRIVER: u32 = 0xE7BE_FDCE;
pub const DRIVER_DETAIL: u32 = 0x6754_CB2C;
pub const PARAM: u32 = 0xABD8_FE73;
const CATEGORY_REF: u32 = 0x4831_B16F;

const F_SET_ITEMS: u32 = 0x6AFC_D1A5;
const F_TREE: u32 = 0xC021_4513;
const F_ITEM_DATA: u32 = 0xF6E6_D4B1;
const F_CLIPS: u32 = 0x5AD9_27D3;
const F_SLOTS: u32 = 0xC486_B05D;
const F_PARAM_ID: u32 = 0x63DE_DAC0;
const F_VALUE: u32 = 0x07DD_813E;

const F_ANIM_NODES: u32 = 0x0B15_B894;
const F_ROOT_NODE: u32 = 0xD649_7916;
const F_LAYERS: u32 = 0x0DE1_BA16;
const F_CHILDREN: u32 = 0x134E_E5BB;
const F_PARENT: u32 = 0x3868_F518;
const F_CHILD: u32 = 0xAF63_2ACD;
const F_ADDITIVE: u32 = 0xF2D8_BF6C;
const F_CATEGORY: u32 = 0x2C54_AEAF;
const F_ANIMATION: u32 = 0x3F5B_86A4;
const F_LAYER_DETAIL: u32 = 0x541B_8505;
const F_DRIVER_X: u32 = 0xE5E3_09D5;
const F_DRIVER_Y: u32 = 0xEB39_1735;
const F_DRIVER_DETAIL: u32 = 0xFC93_13F1;
const F_DRV_WEIGHT: u32 = 0x3B68_CA1F;
const F_DRV_SCALE: u32 = 0x87B8_E1D6;
const F_DRV_DAMPING: u32 = 0x14F9_5E1B;

/// One parameter object: an authored float plus the slot that supplies the
/// runtime value (`m_63DEDAC0`, 1-based into the item's float array).
#[derive(Debug, Clone, Copy, Default)]
pub struct Param {
    pub value: f32,
    pub slot: Option<u32>,
}

impl Param {
    pub fn resolved(&self, slots: &[f32]) -> f32 {
        match self.slot {
            Some(n) if n > 0 => slots.get(n as usize - 1).copied().unwrap_or(self.value),
            _ => self.value,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Driver {
    pub weight: Param,
    pub scale: Param,
    pub damping: Param,
}

#[derive(Debug, Clone)]
pub enum Node {
    Tree,
    Branch {
        layers: Vec<usize>,
        children: Vec<usize>,
    },
    LayerChild {
        parent: usize,
        child: usize,
        additive: usize,
        weight: Param,
    },
    Pose2d {
        clip: usize,
        x: Driver,
        y: Driver,
    },
    Animation {
        clip: usize,
    },
    LayerAdditive,
    BranchChild {
        category: u64,
        child: usize,
    },
    Unknown,
}

#[derive(Debug, Clone, Default)]
pub struct BlendTree {
    pub nodes: Vec<Node>,
    pub root: usize,
    pub anim_nodes: Vec<usize>,
}

/// A blend-set item: its clips, its float slot array and the tree it binds.
#[derive(Debug, Clone, Default)]
pub struct TreeItem {
    pub clips: Vec<u64>,
    pub slots: Vec<f32>,
    pub tree: u64,
}

fn field(fields: &[TypedField], hash: u32) -> Option<&Value> {
    fields.iter().find(|f| f.hash == hash).map(|f| &f.value)
}

fn undefault(v: &Value) -> &Value {
    match v {
        Value::Defaulted(inner) => undefault(inner),
        other => other,
    }
}

fn as_instance(v: &Value) -> Option<usize> {
    match undefault(v) {
        Value::Instance(i) => Some(*i),
        _ => None,
    }
}

fn as_array(v: &Value) -> Option<&[Value]> {
    match undefault(v) {
        Value::Array(a) => Some(a),
        _ => None,
    }
}

fn as_asset(v: &Value) -> Option<u64> {
    match undefault(v) {
        Value::AssetRef(g) => Some(*g),
        _ => None,
    }
}

fn as_f32(v: &Value) -> Option<f32> {
    match undefault(v) {
        Value::Prim(Prim::F32, bits) => Some(f32::from_bits(*bits as u32)),
        _ => None,
    }
}

fn as_i32(v: &Value) -> Option<i32> {
    match undefault(v) {
        Value::Prim(Prim::I32, bits) => Some(*bits as i32),
        Value::Prim(Prim::U32, bits) => Some(*bits as i32),
        _ => None,
    }
}

/// Read one parameter object at an inline position.
fn param_at(
    stu: &mut ow_stu::Stu,
    reg: &ClassRegistry,
    class: u32,
    start: usize,
    bag: i32,
) -> Option<Param> {
    let fields = stu.typed_fields_at(class, start, bag, reg);
    // An unauthored m_value is the class default (0.0) and is not carried;
    // the slot id is still present and is what the runtime uses.
    Some(Param {
        value: field(&fields, F_VALUE).and_then(as_f32).unwrap_or(0.0),
        slot: field(&fields, F_PARAM_ID)
            .and_then(as_i32)
            .map(|v| v as u32),
    })
}

fn param_inline(
    stu: &mut ow_stu::Stu,
    reg: &ClassRegistry,
    fields: &[TypedField],
    hash: u32,
) -> Param {
    let Some(Value::Inline { start, bag }) = field(fields, hash).map(undefault) else {
        return Param::default();
    };
    param_at(stu, reg, PARAM, *start, *bag).unwrap_or_default()
}

fn driver_at(stu: &mut ow_stu::Stu, reg: &ClassRegistry, start: usize, bag: i32) -> Driver {
    let fields = stu.typed_fields_at(DRIVER, start, bag, reg);
    let mut out = Driver::default();
    let Some(Value::Inline { start, bag }) = field(&fields, F_DRIVER_DETAIL).map(undefault) else {
        return out;
    };
    let detail = stu.typed_fields_at(DRIVER_DETAIL, *start, *bag, reg);
    out.weight = param_inline(stu, reg, &detail, F_DRV_WEIGHT);
    out.scale = param_inline(stu, reg, &detail, F_DRV_SCALE);
    out.damping = param_inline(stu, reg, &detail, F_DRV_DAMPING);
    out
}

fn clip_index_of(
    stu: &mut ow_stu::Stu,
    reg: &ClassRegistry,
    fields: &[TypedField],
) -> Option<usize> {
    let Value::Inline { start, bag } = field(fields, F_ANIMATION).map(undefault)? else {
        return None;
    };
    let wrapper = stu.typed_fields_at(ANIM_WRAPPER, *start, *bag, reg);
    let n = as_i32(field(&wrapper, F_PARAM_ID)?)?;
    if n > 0 {
        Some(n as usize - 1)
    } else {
        None
    }
}

/// Load the tree asset for a blend-set item.
pub fn read_tree(
    data: &GameData,
    reg: &ClassRegistry,
    tree_guid: u64,
) -> Result<BlendTree, String> {
    let mut stu = data
        .stu_of(tree_guid)
        .map_err(|e| format!("tree {tree_guid:#018x}: {e}"))?;
    let hashes: Vec<u32> = stu.instances.iter().map(|i| i.hash).collect();
    let mut tree = BlendTree {
        nodes: vec![Node::Unknown; hashes.len()],
        ..BlendTree::default()
    };
    for (i, hash) in hashes.iter().enumerate() {
        let node = match *hash {
            TREE => {
                let fields = stu.typed_fields(i, reg);
                tree.anim_nodes = field(&fields, F_ANIM_NODES)
                    .and_then(as_array)
                    .map(|a| a.iter().filter_map(as_instance).collect())
                    .unwrap_or_default();
                tree.root = field(&fields, F_ROOT_NODE)
                    .and_then(as_instance)
                    .unwrap_or(0);
                Node::Tree
            }
            BRANCH => {
                let fields = stu.typed_fields(i, reg);
                Node::Branch {
                    layers: field(&fields, F_LAYERS)
                        .and_then(as_array)
                        .map(|a| a.iter().filter_map(as_instance).collect())
                        .unwrap_or_default(),
                    children: field(&fields, F_CHILDREN)
                        .and_then(as_array)
                        .map(|a| a.iter().filter_map(as_instance).collect())
                        .unwrap_or_default(),
                }
            }
            LAYER_CHILD => {
                let fields = stu.typed_fields(i, reg);
                let mut weight = Param::default();
                if let Some(Value::Inline { start, bag }) =
                    field(&fields, F_LAYER_DETAIL).map(undefault)
                {
                    let detail = stu.typed_fields_at(DRIVER_DETAIL, *start, *bag, reg);
                    weight = param_inline(&mut stu, reg, &detail, F_DRV_WEIGHT);
                }
                Node::LayerChild {
                    parent: field(&fields, F_PARENT).and_then(as_instance).unwrap_or(0),
                    child: field(&fields, F_CHILD).and_then(as_instance).unwrap_or(0),
                    additive: field(&fields, F_ADDITIVE)
                        .and_then(as_instance)
                        .unwrap_or(0),
                    weight,
                }
            }
            POSE2D => {
                let fields = stu.typed_fields(i, reg);
                let clip = clip_index_of(&mut stu, reg, &fields).unwrap_or(0);
                let x = match field(&fields, F_DRIVER_X).map(undefault) {
                    Some(Value::Inline { start, bag }) => driver_at(&mut stu, reg, *start, *bag),
                    _ => Driver::default(),
                };
                let y = match field(&fields, F_DRIVER_Y).map(undefault) {
                    Some(Value::Inline { start, bag }) => driver_at(&mut stu, reg, *start, *bag),
                    _ => Driver::default(),
                };
                Node::Pose2d { clip, x, y }
            }
            ANIMATION => {
                let fields = stu.typed_fields(i, reg);
                Node::Animation {
                    clip: clip_index_of(&mut stu, reg, &fields).unwrap_or(0),
                }
            }
            LAYER_ADDITIVE => Node::LayerAdditive,
            BRANCH_CHILD => {
                let fields = stu.typed_fields(i, reg);
                // m_category is an inline STU_4831B16F whose m_value is the
                // category asset ref (0x028...0005 / ...0009).
                let category = match field(&fields, F_CATEGORY).map(undefault) {
                    Some(Value::Inline { start, bag }) => {
                        let inner = stu.typed_fields_at(CATEGORY_REF, *start, *bag, reg);
                        field(&inner, F_VALUE).and_then(as_asset).unwrap_or(0)
                    }
                    Some(v) => as_asset(v).unwrap_or(0),
                    None => 0,
                };
                Node::BranchChild {
                    category,
                    child: field(&fields, F_CHILD).and_then(as_instance).unwrap_or(0),
                }
            }
            _ => Node::Unknown,
        };
        tree.nodes[i] = node;
    }
    Ok(tree)
}

/// Load blend-set item `index` (0-based) from the set `set_guid`.
pub fn read_set_item(
    data: &GameData,
    reg: &ClassRegistry,
    set_guid: u64,
    index: usize,
) -> Result<TreeItem, String> {
    let mut stu = data
        .stu_of(set_guid)
        .map_err(|e| format!("set {set_guid:#018x}: {e}"))?;
    let root = stu
        .instances
        .iter()
        .position(|i| i.hash == SET_ROOT)
        .ok_or("set: no root instance")?;
    let fields = stu.typed_fields(root, reg);
    let items = field(&fields, F_SET_ITEMS)
        .and_then(as_array)
        .ok_or("set: no m_animNodes container")?;
    let Some(Value::Inline { start, bag }) = items.get(index).map(undefault) else {
        return Err(format!("set: item {index} is not inline"));
    };
    let item = stu.typed_fields_at(SET_ITEM, *start, *bag, reg);
    let tree = field(&item, F_TREE).and_then(as_asset).unwrap_or(0);
    let Some(data_inst) = field(&item, F_ITEM_DATA).and_then(as_instance) else {
        return Err("set: item has no data instance".into());
    };
    let dfields = stu.typed_fields(data_inst, reg);
    let clips = field(&dfields, F_CLIPS)
        .and_then(as_array)
        .map(|a| a.iter().filter_map(as_asset).collect())
        .unwrap_or_default();
    let slots = field(&dfields, F_SLOTS)
        .and_then(as_array)
        .map(|a| a.iter().filter_map(as_f32).collect())
        .unwrap_or_default();
    Ok(TreeItem { clips, slots, tree })
}
