//! Evaluation of the first-person animation blend tree.
//!
//! The tree asset and the item's slots/clips are read by
//! [`ow_tank::blendtree`]; this module turns them into a per-bone pose.
//!
//! **What is read** (`docs/formats/animation.md` 2026-10-20 (5)-(7)):
//! the node graph (BranchByCategory children by category asset, LayerChild
//! parent/child/additive links, Animation clip indices, Pose2d clip index
//! and driver parameter slots), the slot values, and the client's
//! per-child sampler rule - vec4 lanes are *winner-take-all above weight
//! 0.5*, scalars are lerped (`FUN_7ff7c6e84610`).
//!
//! **What is fitted** (`docs/fitted-ledger.md`): how the two Pose2d driver
//! inputs combine into an applied weight, and the layer gain (the client
//! writes the LayerChild weight from dataflow, which is still unread; the
//! file default is 0.0). At the default inputs (x = y = 0, layer gain 1,
//! layer weight 0) the tree yields the branch's base clip, which is what
//! the client's authored data says the idle is.

use glam::{Quat, Vec3};
use ow_tank::blendtree::{BlendTree, Node};
use ow_tank::clip::BlendPose;

/// One step of a bone's local transform: the clip record that animates it
/// (if any) and the bind local that fills missing channels.
#[derive(Clone, Copy)]
pub struct ChainLink {
    pub record: Option<usize>,
    pub bind: (Quat, Vec3),
}

/// Build per-bone record chains for a clip on the view-model rig.
///
/// The clip is authored on the **character** rig; the FP rig is a subset
/// whose hierarchies mostly match, but where the character chain drops
/// intermediate bones the FP rig folded their transforms into the child's
/// bind (`bind_fp(child) = bind_char(missing) * bind_char(child)`, verified
/// exactly for the weapon hooks 284*44 / 285*74). So for each FP bone the
/// chain is `[dropped ancestor ..., own bone]`, composed parent to child;
/// for ordinary bones it is just `[own bone]` and the behaviour is the same
/// as before.
pub fn build_chains(
    source: &BlendPose,
    fp_ids: &[u32],
    fp_parents: &[i16],
    char_ids: &[u32],
    char_parents: &[i16],
    char_local: &[glam::Mat4],
) -> Vec<Vec<ChainLink>> {
    let mut record_of = std::collections::HashMap::new();
    for r in 0..source.records() {
        if let Some(id) = source.bone_id(r) {
            record_of.insert(id, r);
        }
    }
    let split = |m: &glam::Mat4| {
        let (_, q, t) = m.to_scale_rotation_translation();
        (q, t)
    };
    let mut out = vec![Vec::new(); fp_ids.len()];
    for (b, id) in fp_ids.iter().enumerate() {
        let parent_id = {
            let p = fp_parents.get(b).copied().unwrap_or(-1);
            if p < 0 {
                None
            } else {
                fp_ids.get(p as usize).copied()
            }
        };
        let Some(ci) = char_ids.iter().position(|x| x == id) else {
            continue;
        };
        let mut chain_ids = vec![*id];
        let mut cur = ci;
        loop {
            let p = char_parents.get(cur).copied().unwrap_or(-1);
            if p < 0 {
                break;
            }
            let pid = char_ids[p as usize];
            // Stop where the FP rig takes over: its own parent (or any bone
            // the FP rig carries) is applied by the FP hierarchy itself.
            if Some(pid) == parent_id || fp_ids.contains(&pid) {
                break;
            }
            chain_ids.push(pid);
            cur = p as usize;
        }
        chain_ids.reverse();
        let links: Vec<ChainLink> = chain_ids
            .iter()
            .map(|cid| {
                let idx = char_ids.iter().position(|x| x == cid).unwrap_or(ci);
                ChainLink {
                    record: record_of.get(cid).copied(),
                    bind: split(&char_local[idx]),
                }
            })
            .collect();
        out[b] = links;
    }
    out
}

/// Parent-to-child composition of two local transforms.
fn compose(a: (Quat, Vec3), b: (Quat, Vec3)) -> (Quat, Vec3) {
    (a.0 * b.0, a.1 + a.0 * b.1)
}

/// Sample one bone's chain at `t`, composing parent to child.
pub fn sample_chain(
    source: &BlendPose,
    links: &[ChainLink],
    t: f32,
) -> Option<(Quat, Vec3)> {
    if links.is_empty() || !links.iter().any(|l| l.record.is_some()) {
        return None;
    }
    let mut acc: Option<(Quat, Vec3)> = None;
    for link in links {
        let local = match link.record {
            Some(record) => {
                let (r, p) = source.sample_record(record, t)?;
                (r.unwrap_or(link.bind.0), p.unwrap_or(link.bind.1))
            }
            None => link.bind,
        };
        acc = Some(match acc {
            None => local,
            Some(a) => compose(a, local),
        });
    }
    acc
}

/// One clip from the item, ready to sample per skin bone.
pub struct ClipLayer {
    pub guid: u64,
    pub source: BlendPose,
    /// `bone_chain[bone]` = the char-chain links to compose for this bone
    /// (see [`build_chains`]). Empty = the clip does not drive the bone.
    pub bone_chain: Vec<Vec<ChainLink>>,
}

impl ClipLayer {
    fn sample(&self, bone: usize, t: f32) -> Option<(Quat, Vec3)> {
        sample_chain(&self.source, self.bone_chain.get(bone)?, t)
    }

    fn drives(&self, bone: usize) -> bool {
        self.bone_chain
            .get(bone)
            .map(|c| c.iter().any(|l| l.record.is_some()))
            .unwrap_or(false)
    }
}

/// The idle tree for one hero, decoded once.
pub struct TreeAnim {
    pub tree: BlendTree,
    pub slots: Vec<f32>,
    pub clips: Vec<Option<ClipLayer>>,
    /// Category asset to follow (`0x0280000000000005` = category 5).
    pub category: u64,
    /// Pose2d driver inputs (fitted stand-in for the dataflow values).
    pub x: f32,
    pub y: f32,
    /// Multiplier on the LayerChild weight (the file default is 0.0).
    pub layer_gain: f32,
    /// Explicit LayerChild weight override (`--tree-layer`), bypassing the
    /// file default - an experiment knob until the dataflow writer is read.
    pub layer_weight: Option<f32>,
}

impl TreeAnim {
    /// Resolve the branch child for the selected category.
    fn branch(&self) -> Option<(usize, usize)> {
        // The first BranchByCategory node is the tree's root branch.
        let branch = self
            .tree
            .nodes
            .iter()
            .position(|n| matches!(n, Node::Branch { .. }))?;
        let Node::Branch { children, .. } = &self.tree.nodes[branch] else {
            return None;
        };
        let mut fallback = None;
        for &child_inst in children {
            if let Some(Node::BranchChild { category, .. }) = self.tree.nodes.get(child_inst) {
                if *category == self.category {
                    return Some((branch, child_inst));
                }
                if fallback.is_none() {
                    fallback = Some((branch, child_inst));
                }
            }
        }
        fallback
    }

    pub fn base_clip(&self) -> Option<(usize, u64)> {
        let (_, child) = self.branch()?;
        let animation = match self.tree.nodes.get(child)? {
            Node::BranchChild { child, .. } => *child,
            _ => return None,
        };
        let clip = match self.tree.nodes.get(animation)? {
            Node::Animation { clip } => *clip,
            _ => return None,
        };
        let guid = *self.clips.get(clip)?.as_ref().map(|c| &c.guid)?;
        Some((clip, guid))
    }

    /// Per-layer applied weights as `(clip_index, weight)`.
    pub fn layer_weights(&self) -> Vec<(usize, f32)> {
        let Some((branch, _)) = self.branch() else {
            return Vec::new();
        };
        let Node::Branch { layers, .. } = &self.tree.nodes[branch] else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for &layer in layers {
            let Some(Node::LayerChild { child, weight, .. }) = self.tree.nodes.get(layer) else {
                continue;
            };
            let Some(Node::Pose2d { clip, x, y }) = self.tree.nodes.get(*child) else {
                continue;
            };
            // Fitted: the file's per-axis weight amplitudes scaled by the
            // (unknown) driver inputs, then the LayerChild weight.
            let wx = x.weight.resolved(&self.slots) * self.x.abs();
            let wy = y.weight.resolved(&self.slots) * self.y.abs();
            let w = (wx + wy).clamp(0.0, 1.0);
            let base = self.layer_weight.unwrap_or_else(|| weight.resolved(&self.slots));
            let applied = base * self.layer_gain * w;
            out.push((*clip, applied));
        }
        out
    }

    /// One frame: per-bone `(rotation, translation)`, `None` = bind.
    pub fn evaluate(&self, t: f32, bones: usize) -> Vec<Option<(Quat, Vec3)>> {
        let mut pose: Vec<Option<(Quat, Vec3)>> = vec![None; bones];
        let Some((base_index, _)) = self.base_clip() else {
            return pose;
        };
        if let Some(Some(base)) = self.clips.get(base_index) {
            for bone in 0..bones {
                if let Some(s) = base.sample(bone, t) {
                    pose[bone] = Some(s);
                }
            }
        }
        for (clip, weight) in self.layer_weights() {
            let Some(Some(layer)) = self.clips.get(clip) else {
                continue;
            };
            // The sampler's vec4 rule: at or below the 0.5 gate the child's
            // lanes do not apply; above it they replace the current pose.
            if weight <= 0.5 {
                continue;
            }
            for bone in 0..bones {
                if layer.drives(bone) {
                    if let Some(s) = layer.sample(bone, t) {
                        pose[bone] = Some(s);
                    }
                }
            }
        }
        pose
    }

    /// Diagnostics for a failed resolution: what the tree actually holds.
    pub fn debug_report(&self) -> String {
        let branch = self
            .tree
            .nodes
            .iter()
            .position(|n| matches!(n, Node::Branch { .. }));
        let mut detail = format!(
            "nodes {} root {} anim_nodes {:?} clips {} slots {}",
            self.tree.nodes.len(),
            self.tree.root,
            self.tree.anim_nodes,
            self.clips.len(),
            self.slots.len()
        );
        if let Some(b) = branch {
            if let Node::Branch { layers, children } = &self.tree.nodes[b] {
                detail.push_str(&format!("; branch {b} layers {layers:?}"));
                for &c in children {
                    if let Some(Node::BranchChild { category, child }) = self.tree.nodes.get(c) {
                        detail.push_str(&format!(
                            "; child {c} cat {category:#x} -> {child}"
                        ));
                        if let Some(Node::Animation { clip }) = self.tree.nodes.get(*child) {
                            detail.push_str(&format!(
                                "; animation clip{} decode {}",
                                clip + 1,
                                match self.clips.get(*clip) {
                                    Some(Some(_)) => "ok",
                                    Some(None) => "failed",
                                    None => "out of range",
                                }
                            ));
                        }
                    } else {
                        detail.push_str(&format!("; child {c} unparsed"));
                    }
                }
            }
        }
        detail
    }

    /// One-line report of what the tree is doing, for the HUD/log.
    pub fn summary(&self) -> String {
        let (clip_index, guid) = match self.base_clip() {
            Some(v) => v,
            None => return format!("tree: no base clip ({})", self.debug_report()),
        };
        let layers = self
            .layer_weights()
            .iter()
            .map(|(c, w)| format!("clip{}:{w:.2}", c + 1))
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            "tree cat {:#x}: base clip{} {guid:#018x}; layers [{layers}]; x {:.2} y {:.2} gain {:.2}",
            self.category,
            clip_index + 1,
            self.x,
            self.y,
            self.layer_gain
        )
    }
}
