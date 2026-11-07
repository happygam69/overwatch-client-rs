//! The model skeleton: the model asset's `mskl` chunk.
//!
//! `docs/formats/animation.md` records what the *runtime* builds; this module
//! reads the same skeleton from the install, so bones are available without a
//! live client. The layout was taken from the TankLib reference checkout's
//! `Chunks/teModelChunk_Skeleton.cs` ("mskl: Defines model skeleton") and then
//! verified against the shipped bytes — the header's offsets land on arrays
//! whose shapes the counts predict, and the parent array is a forest:
//!
//! ```text
//! offset 0..=96  thirteen i64 offsets: hierarchy, matrices, inverted
//!                matrices, bind pose, bind-pose deltas, ids, names, remap,
//!                a second hierarchy
//! offset 104     i32 unknown
//! offset 108     u16 bones_abs    (the bone count everything is sized by)
//! offset 110     u16 bones_simple
//! offset 112     u16 bones_cloth
//! offset 114     u16 remap_count
//! offset 116     u16 id_count
//! ```
//!
//! Hierarchy entries are 6 bytes apart (4 bytes skipped, then an `i16` parent,
//! `-1` at a root); bind poses are 48-byte records (quaternion, scale, a pad,
//! translation, a pad); bone ids are `u32` hashes, not strings — the engine's
//! own vocabulary is `BoneLabelGUID`-keyed (`animation.md`).
//!
//! **The client's own reader (`MsklTableBuild`, `0x141b894d0`) sizes each
//! table**, and three of those sizes differ from what the offsets suggest:
//! the id array is `bones_simple` u32s (offset 110), not `id_count` (116); the
//! remap at offset 80 is `remap_count` (114) u16s; and the arrays at 88 and 96
//! are `bones_abs` u16s and `remap_count` 8-byte records. `bones_simple` and
//! `id_count` hold the same value on all 7,359 install models this crate
//! parses, so the two readings of the id count agree there; the client's is the
//! one used here. The array at offset 88 is the **inverse of the remap** — for
//! every bone, the mesh slot that names it, or `0xFFFF` for a bone no mesh
//! slot names (measured: the table is the exact inverse on all 7,328 skinned
//! install models, and `0xFFFF` sits exactly on the `bones_abs - remap_count`
//! unmapped bones; Tracer's character leaves 30 of its 229 bones unmapped). `ow-tank::skin` reads the forward table; the inverse is recorded
//! here rather than exposed, because nothing consumes it yet.

use std::fmt;

use crate::chunks::Chunks;

#[derive(Debug)]
pub enum Error {
    NotAChunkedAsset,
    MissingChunk(&'static str),
    Truncated(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotAChunkedAsset => write!(f, "not a chunked asset"),
            Error::MissingChunk(t) => write!(f, "model has no {t} chunk"),
            Error::Truncated(t) => write!(f, "malformed skeleton: {t}"),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

/// One bone's bind pose: orientation, scale and translation, as stored.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BindPose {
    pub orientation: [f32; 4],
    pub scale: [f32; 3],
    pub translation: [f32; 3],
}

/// The model skeleton, as the `mskl` chunk holds it.
#[derive(Debug, Clone, Default)]
pub struct Skeleton {
    pub bones_abs: u16,
    pub bones_simple: u16,
    pub bones_cloth: u16,
    /// One `-1` root sentinel per hierarchy entry; length = `bones_abs`.
    pub parents: Vec<i16>,
    /// Bind pose per bone; length = `bones_abs` (empty if the chunk omits it).
    pub bind_pose: Vec<BindPose>,
    /// The **inverted bind matrices**: one 4×4 per bone, from the header's
    /// third offset (byte 16), as the file stores them. `ow-tank::skin` checks
    /// them against the hierarchy rather than trusting the layout, and reads
    /// which of row/column major they are from the numbers.
    pub inverse_bind: Vec<[[f32; 4]; 4]>,
    /// File offset the inverted-bind array was read from, kept so a diagnostic
    /// can show the raw bytes behind a rejected layout.
    pub inverse_bind_offset: Option<usize>,
    /// Bone id hashes, one per `bones_simple` bone. The client copies
    /// `bones_simple` u32s (`MsklTableBuild`); the reference calls the count
    /// `IDCount` at 116 — measured equal on every install model.
    pub ids: Vec<u32>,
    /// The remap table (`remap_count` `u16`s), when present. This is the
    /// **mesh slot → bone** map: a vertex's raw blend index names a slot here,
    /// not a bone (`ow-tank::skin`). Its inverse, bone → slot with `0xFFFF`
    /// for an unskinned bone, is the array at file offset 88.
    pub remap: Vec<u16>,
}

impl Skeleton {
    pub fn roots(&self) -> Vec<usize> {
        self.parents
            .iter()
            .enumerate()
            .filter(|(_, p)| **p < 0)
            .map(|(i, _)| i)
            .collect()
    }

    /// Levels below the root, 0 for a root. `None` on a cycle or a bad parent.
    pub fn depth(&self, bone: usize) -> Option<usize> {
        let mut seen = 0usize;
        let mut at = bone;
        loop {
            let p = *self.parents.get(at)?;
            if p < 0 {
                return Some(seen);
            }
            let p = p as usize;
            if p >= self.parents.len() || p == at {
                return None;
            }
            seen += 1;
            at = p;
            if seen > self.parents.len() {
                return None;
            }
        }
    }

    /// A hierarchy that is a forest: every parent index is valid and smaller
    /// than its child (parents are stored before children), and every bone
    /// reaches a root. This is the check that tells a real skeleton from bytes
    /// that merely look like one.
    pub fn is_forest(&self) -> bool {
        if self.parents.is_empty() {
            return false;
        }
        self.parents
            .iter()
            .enumerate()
            .all(|(i, p)| (*p < 0 || (*p as usize) < i) && self.depth(i).is_some())
    }

    /// Children of each bone, in index order.
    pub fn children(&self) -> Vec<Vec<usize>> {
        let mut out = vec![Vec::new(); self.parents.len()];
        for (i, p) in self.parents.iter().enumerate() {
            if *p >= 0 && (*p as usize) < out.len() {
                out[*p as usize].push(i);
            }
        }
        out
    }
}

const HEADER_LEN: usize = 118;
const HIER_STEP: usize = 6;
const BIND_RECORD: usize = 48;

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    d.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}
fn i16_at(d: &[u8], at: usize) -> Option<i16> {
    u16_at(d, at).map(|v| v as i16)
}
fn i64_at(d: &[u8], at: usize) -> Option<i64> {
    d.get(at..at + 8)
        .map(|b| i64::from_le_bytes(b.try_into().unwrap()))
}
fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn f32_at(d: &[u8], at: usize) -> Option<f32> {
    u32_at(d, at).map(f32::from_bits)
}

/// Parse the `mskl` chunk of a model asset's bytes.
pub fn parse_model(asset: &[u8]) -> Result<Skeleton> {
    let chunks = Chunks::parse(asset).map_err(|_| Error::NotAChunkedAsset)?;
    let chunk = chunks
        .payload(asset, b"mskl")
        .ok_or(Error::MissingChunk("mskl"))?;
    parse(chunk)
}

/// Parse the `mskl` payload itself.
pub fn parse(d: &[u8]) -> Result<Skeleton> {
    if d.len() < HEADER_LEN {
        return Err(Error::Truncated("header"));
    }
    let hierarchy_off = i64_at(d, 0).unwrap_or(0).max(0) as usize;
    let matrices_off = i64_at(d, 8).unwrap_or(0).max(0) as usize;
    let ids_off = i64_at(d, 56).unwrap_or(0).max(0) as usize;
    let remap_off = i64_at(d, 80).unwrap_or(0).max(0) as usize;

    let bones_abs = u16_at(d, 108).unwrap_or(0);
    let bones_simple = u16_at(d, 110).unwrap_or(0);
    let bones_cloth = u16_at(d, 112).unwrap_or(0);
    let remap_count = u16_at(d, 114).unwrap_or(0);

    let mut parents = Vec::new();
    if hierarchy_off > 0 && bones_abs > 0 {
        for i in 0..bones_abs as usize {
            let at = hierarchy_off + i * HIER_STEP + 4;
            parents.push(i16_at(d, at).ok_or(Error::Truncated("hierarchy"))?);
        }
    }

    let mut bind_pose = Vec::new();
    let bind_off = matrices_off + bones_abs as usize * 64; // Matrix43 follows Matrix44? no:
    let _ = bind_off;
    // The bind pose lives at its own header offset (index 3, byte 24).
    let bind_off = i64_at(d, 24).unwrap_or(0).max(0) as usize;
    if bind_off > 0 && bones_abs > 0 {
        for i in 0..bones_abs as usize {
            let at = bind_off + i * BIND_RECORD;
            if at + BIND_RECORD > d.len() {
                return Err(Error::Truncated("bind pose"));
            }
            bind_pose.push(BindPose {
                orientation: [
                    f32_at(d, at).unwrap_or(0.0),
                    f32_at(d, at + 4).unwrap_or(0.0),
                    f32_at(d, at + 8).unwrap_or(0.0),
                    f32_at(d, at + 12).unwrap_or(0.0),
                ],
                scale: [
                    f32_at(d, at + 16).unwrap_or(0.0),
                    f32_at(d, at + 20).unwrap_or(0.0),
                    f32_at(d, at + 24).unwrap_or(0.0),
                ],
                translation: [
                    f32_at(d, at + 32).unwrap_or(0.0),
                    f32_at(d, at + 36).unwrap_or(0.0),
                    f32_at(d, at + 40).unwrap_or(0.0),
                ],
            });
        }
    }

    let mut inverse_bind = Vec::new();
    let inverse_off = i64_at(d, 16).unwrap_or(0).max(0) as usize;
    if inverse_off > 0 && bones_abs > 0 {
        for i in 0..bones_abs as usize {
            let at = inverse_off + i * 64;
            if at + 64 > d.len() {
                return Err(Error::Truncated("inverted bind matrices"));
            }
            let mut m = [[0.0f32; 4]; 4];
            for (row, r) in m.iter_mut().enumerate() {
                for (col, c) in r.iter_mut().enumerate() {
                    *c = f32_at(d, at + (row * 4 + col) * 4).unwrap_or(0.0);
                }
            }
            inverse_bind.push(m);
        }
    }

    let mut ids = Vec::new();
    if ids_off > 0 && bones_simple > 0 {
        // `MsklTableBuild` copies `bones_simple` ids from offset 56.
        for i in 0..bones_simple as usize {
            ids.push(u32_at(d, ids_off + i * 4).ok_or(Error::Truncated("ids"))?);
        }
    }

    let mut remap = Vec::new();
    if remap_off > 0 && remap_count > 0 {
        for i in 0..remap_count as usize {
            remap.push(u16_at(d, remap_off + i * 2).ok_or(Error::Truncated("remap"))?);
        }
    }

    Ok(Skeleton {
        bones_abs,
        bones_simple,
        bones_cloth,
        parents,
        bind_pose,
        inverse_bind,
        inverse_bind_offset: (inverse_off > 0 && bones_abs > 0).then_some(inverse_off),
        ids,
        remap,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic `mskl` payload built here — no game bytes. Three bones in a
    /// chain plus a root, ids, and a remap, so every field is exercised.
    fn synth() -> Vec<u8> {
        let bones = 4usize;
        let hierarchy_off = 0x80usize;
        let matrices_off = hierarchy_off + bones * HIER_STEP;
        let bind_off = matrices_off + bones * 64;
        let ids_off = bind_off + bones * BIND_RECORD;
        let remap_off = ids_off + bones * 4;
        let mut d = vec![0u8; remap_off + bones * 2];
        for (slot, value) in [
            (0usize, hierarchy_off),
            (8, matrices_off),
            (16, 0),
            (24, bind_off),
            (32, 0),
            (40, 0),
            (48, 0),
            (56, ids_off),
            (64, 0),
            (72, 0),
            (80, remap_off),
            (88, 0),
            (96, 0),
        ] {
            d[slot..slot + 8].copy_from_slice(&(value as i64).to_le_bytes());
        }
        d[104..108].copy_from_slice(&0i32.to_le_bytes());
        for (slot, v) in [
            (108usize, bones as u16),
            (110, bones as u16),
            (112, 2),
            (114, bones as u16),
            (116, bones as u16),
        ] {
            d[slot..slot + 2].copy_from_slice(&v.to_le_bytes());
        }
        // parents: -1, 0, 1, 1
        for (i, p) in [-1i16, 0, 1, 1].iter().enumerate() {
            let at = hierarchy_off + i * HIER_STEP + 4;
            d[at..at + 2].copy_from_slice(&p.to_le_bytes());
        }
        // bind poses: bone i at height i
        for i in 0..bones {
            let at = bind_off + i * BIND_RECORD;
            let q = [0.0f32, 0.0, 0.0, 1.0];
            for (k, v) in q.iter().enumerate() {
                d[at + k * 4..at + k * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            for (k, v) in [1.0f32, 1.0, 1.0].iter().enumerate() {
                d[at + 16 + k * 4..at + 16 + k * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            let t = [0.0f32, i as f32 * 0.25, 0.0];
            for (k, v) in t.iter().enumerate() {
                d[at + 32 + k * 4..at + 32 + k * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        for i in 0..bones {
            d[ids_off + i * 4..ids_off + i * 4 + 4]
                .copy_from_slice(&(0xABCD_0000u32 + i as u32).to_le_bytes());
        }
        for i in 0..bones {
            d[remap_off + i * 2..remap_off + i * 2 + 2].copy_from_slice(&(i as u16).to_le_bytes());
        }
        d
    }

    #[test]
    fn reads_the_synthetic_skeleton() {
        let d = synth();
        let s = parse(&d).expect("parses");
        assert_eq!(s.bones_abs, 4);
        assert_eq!(s.bones_simple, 4);
        assert_eq!(s.bones_cloth, 2);
        assert_eq!(s.parents, vec![-1, 0, 1, 1]);
        assert_eq!(s.ids.len(), 4);
        assert_eq!(s.ids[2], 0xABCD_0002);
        assert_eq!(s.remap.len(), 4);
        assert!(s.is_forest());
        assert_eq!(s.roots(), vec![0]);
        assert_eq!(s.depth(0), Some(0));
        assert_eq!(s.depth(2), Some(2));
        assert_eq!(s.depth(3), Some(2));
        assert_eq!(s.bind_pose.len(), 4);
        assert_eq!(s.bind_pose[2].translation[1], 0.5);
        assert_eq!(s.children()[1], vec![2, 3]);
    }

    #[test]
    fn a_parent_after_its_child_is_not_a_forest() {
        let mut d = synth();
        let at = 0x80 + HIER_STEP + 4; // bone 1's parent slot
        d[at..at + 2].copy_from_slice(&3i16.to_le_bytes()); // parent 3 > index 1
        let s = parse(&d).expect("parses");
        assert!(!s.is_forest());
        assert_eq!(s.depth(1), None);
    }
}
