//! Cloth chunk (`CLTH`).

use std::fmt;

#[derive(Debug, Clone, Copy, Default)]
pub struct DominoClothData {
    pub positions_offset: i64,
    pub ow220: i64,
    pub section3: i64,
    pub section4: i64,
    pub section5: i64,
    pub section6: i64,
    pub section7: i64,
    pub section8: i64,
    pub vertex_parent_indices: i64,
    pub section10: i64,
    pub section11: i64,
    pub vertex_driver_weights: i64,
    pub vertex_driver_influences: i64,
    pub vertex_follower_bone_indices: i64,
    pub section15: i64,
    pub section16: i64,
    pub section17: i64,
    pub section18: i64,
    pub section19: i64,
    pub section20: i64,
    pub section21: i64,
    pub section22: i64,
    pub section23: i64,
    pub section24: i64,
    pub section25: i64,
    pub section26: i64,
    pub driver_to_bone_map: i64,
    pub name: [u8; 32],
    pub unknown1: i32,
    pub vertex_count: u16,
    pub unk2: u16,
    pub unk3: u16,
    pub unk4: u16,
    pub unk5: u16,
    pub unk6: u16,
    pub unk7: u16,
    pub unk8: u16,
    pub unk9: u16,
    pub unk10: u16,
    pub unk11: u16,
    pub bone_count: u16,
    pub unk13: u16,
    pub unk14: u16,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ClothDesc {
    pub domino: DominoClothData,
    pub offset28: i64,
    pub map_offset: i64,
    pub unknown_guid: u64,
    pub unknown2: i64,
    pub unknown3_guid: u64,
    pub ow225_new1: i64,
    pub unknown4: i64,
    pub unknown5: i64,
    pub unknown6: i64,
    pub ow225_new2: i64,
}

#[derive(Debug, Clone)]
pub struct VertexDriverWeight {
    pub driver_idx: i16,
    pub weight: f32,
}

#[derive(Debug, Clone)]
pub struct ClothVertex {
    pub follower_bone: i16,
    pub vertical_parent: i16,
    pub driver_weights: Vec<VertexDriverWeight>,
}

#[derive(Debug, Clone)]
pub struct ClothPiece {
    pub descriptor: ClothDesc,
    pub vertices: Vec<ClothVertex>,
    pub bone_to_driver_map: Vec<i16>,
}

#[derive(Debug, Clone, Default)]
pub struct Cloth {
    pub count: u64,
    pub offset: i64,
    pub pieces: Vec<ClothPiece>,
}

impl Cloth {
    /// The skeleton hierarchy with cloth's follower bones re-parented, plus the
    /// indices of the bones whose parent changed.
    ///
    /// Ported from the reference tooling's
    /// `teModelChunk_Cloth.CreateFakeHierarchy` / `GetEffectiveBoneIndex`
    /// (`E:/CASCReader/OWLib/TankLib/Chunks/teModelChunk_Cloth.cs`), which the
    /// exporters use to replace `skeleton.Hierarchy` whenever a model has a
    /// `CLTH` chunk (`ExportFormats/OverwatchModel.cs`, `RefPoseSkeleton.cs`).
    ///
    /// The rule, per cloth vertex:
    ///
    /// * A vertex whose vertical parent is **itself or -1** hangs off a
    ///   *driver*: take its highest-weight driver, look that driver's index up
    ///   in the piece's bone-to-driver map, and the position found is the
    ///   skeleton bone it hangs from. A highest-weight driver of -1 (or one not
    ///   in the map) answers -1.
    /// * Any other vertex answers whatever its vertical parent answers.
    /// * If the vertex also has a follower bone (`>= 0`) **and** the answer is
    ///   a real bone, the follower's parent in the hierarchy is rewritten to
    ///   that bone and the follower is recorded as re-parented. A follower
    ///   whose answer is -1 is left alone.
    ///
    /// Exactly as the reference: a highest-weight driver of -1 answers -1
    /// *before* the follower is considered, so that vertex re-parents nothing;
    /// the answer is memoised because the reference recomputes the same chain
    /// per caller, which is idempotent, so the results are identical.
    ///
    /// Guards where the reference would crash or hang, none of which should
    /// occur on real data: a vertical parent that is negative-but-not-`-1` or
    /// past the end of the vertex array takes the driver branch instead of
    /// indexing out of bounds; a vertex whose parent chain revisits a vertex
    /// (a cycle) answers -1; and a follower bone index outside the skeleton's
    /// parent array is left unwritten. All three are asserted by the synthetic
    /// test below.
    pub fn fake_hierarchy(&self, skeleton: &crate::skeleton::Skeleton) -> (Vec<i16>, Vec<usize>) {
        let mut hierarchy = skeleton.parents.clone();
        let mut reparented = std::collections::BTreeSet::new();
        for piece in &self.pieces {
            let mut memo: Vec<Option<i16>> = vec![None; piece.vertices.len()];
            let mut in_progress = vec![false; piece.vertices.len()];
            for vertex in 0..piece.vertices.len() {
                effective_bone(
                    piece,
                    &mut hierarchy,
                    &mut reparented,
                    &mut memo,
                    &mut in_progress,
                    vertex as i16,
                );
            }
        }
        (hierarchy, reparented.into_iter().collect())
    }
}

/// One vertex's effective bone, with the reference's side effect: a follower
/// bone is written into `hierarchy` and recorded. See [`Cloth::fake_hierarchy`].
fn effective_bone(
    piece: &ClothPiece,
    hierarchy: &mut [i16],
    reparented: &mut std::collections::BTreeSet<usize>,
    memo: &mut [Option<i16>],
    in_progress: &mut [bool],
    vertex: i16,
) -> i16 {
    let at = vertex as usize;
    if vertex < 0 || at >= piece.vertices.len() {
        return -1;
    }
    if let Some(answer) = memo[at] {
        return answer;
    }
    if in_progress[at] {
        // A cycle; the reference would recurse forever. Treat it as a root.
        return -1;
    }
    in_progress[at] = true;
    let v = &piece.vertices[at];
    let parent_vertex = v.vertical_parent;
    let parent_is_driver = parent_vertex < 0
        || parent_vertex as usize == at
        || parent_vertex as usize >= piece.vertices.len();
    let parent_bone = if parent_is_driver {
        // .NET's `MaxBy` keeps the *first* maximum, so ties keep the earlier
        // driver; a plain `>` comparison does the same and treats NaN as "not
        // greater", which never displaces the first entry.
        let mut best: Option<&VertexDriverWeight> = None;
        for w in &v.driver_weights {
            if best.is_none_or(|b| w.weight > b.weight) {
                best = Some(w);
            }
        }
        match best {
            Some(w) if w.driver_idx != -1 => piece
                .bone_to_driver_map
                .iter()
                .position(|m| *m == w.driver_idx)
                .map(|p| p as i16)
                .unwrap_or(-1),
            _ => {
                // The reference returns -1 here, before the follower is
                // considered, so this vertex does not reparent its follower.
                in_progress[at] = false;
                memo[at] = Some(-1);
                return -1;
            }
        }
    } else {
        effective_bone(
            piece,
            hierarchy,
            reparented,
            memo,
            in_progress,
            parent_vertex,
        )
    };
    in_progress[at] = false;
    let answer = if v.follower_bone == -1 {
        parent_bone
    } else {
        let follower = v.follower_bone as usize;
        if parent_bone != -1 && follower < hierarchy.len() {
            hierarchy[follower] = parent_bone;
            reparented.insert(follower);
        }
        v.follower_bone
    };
    memo[at] = Some(answer);
    answer
}

#[derive(Debug)]
pub enum Error {
    NotAChunkedAsset,
    MissingChunk,
    Malformed(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotAChunkedAsset => write!(f, "not a chunked asset"),
            Error::MissingChunk => write!(f, "model has no CLTH chunk"),
            Error::Malformed(m) => write!(f, "malformed cloth: {m}"),
        }
    }
}

impl std::error::Error for Error {}

fn i16_at(d: &[u8], at: usize) -> Option<i16> {
    d.get(at..at + 2).map(|b| i16::from_le_bytes([b[0], b[1]]))
}
fn i32_at(d: &[u8], at: usize) -> Option<i32> {
    d.get(at..at + 4)
        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn i64_at(d: &[u8], at: usize) -> Option<i64> {
    d.get(at..at + 8)
        .map(|b| i64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}
fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    d.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}
fn u64_at(d: &[u8], at: usize) -> Option<u64> {
    d.get(at..at + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}
fn f32_at(d: &[u8], at: usize) -> Option<f32> {
    d.get(at..at + 4)
        .map(|b| f32::from_bits(u32::from_le_bytes([b[0], b[1], b[2], b[3]])))
}

fn read_domino(d: &[u8], at: usize) -> Option<DominoClothData> {
    if at + 280 > d.len() {
        return None;
    }
    let mut out = DominoClothData::default();
    let mut p = at;
    out.positions_offset = i64_at(d, p)?;
    p += 8;
    out.ow220 = i64_at(d, p)?;
    p += 8;
    out.section3 = i64_at(d, p)?;
    p += 8;
    out.section4 = i64_at(d, p)?;
    p += 8;
    out.section5 = i64_at(d, p)?;
    p += 8;
    out.section6 = i64_at(d, p)?;
    p += 8;
    out.section7 = i64_at(d, p)?;
    p += 8;
    out.section8 = i64_at(d, p)?;
    p += 8;
    out.vertex_parent_indices = i64_at(d, p)?;
    p += 8;
    out.section10 = i64_at(d, p)?;
    p += 8;
    out.section11 = i64_at(d, p)?;
    p += 8;
    out.vertex_driver_weights = i64_at(d, p)?;
    p += 8;
    out.vertex_driver_influences = i64_at(d, p)?;
    p += 8;
    out.vertex_follower_bone_indices = i64_at(d, p)?;
    p += 8;
    out.section15 = i64_at(d, p)?;
    p += 8;
    out.section16 = i64_at(d, p)?;
    p += 8;
    out.section17 = i64_at(d, p)?;
    p += 8;
    out.section18 = i64_at(d, p)?;
    p += 8;
    out.section19 = i64_at(d, p)?;
    p += 8;
    out.section20 = i64_at(d, p)?;
    p += 8;
    out.section21 = i64_at(d, p)?;
    p += 8;
    out.section22 = i64_at(d, p)?;
    p += 8;
    out.section23 = i64_at(d, p)?;
    p += 8;
    out.section24 = i64_at(d, p)?;
    p += 8;
    out.section25 = i64_at(d, p)?;
    p += 8;
    out.section26 = i64_at(d, p)?;
    p += 8;
    out.driver_to_bone_map = i64_at(d, p)?;
    p += 8;
    if p + 32 > d.len() {
        return None;
    }
    out.name.copy_from_slice(&d[p..p + 32]);
    p += 32;
    out.unknown1 = i32_at(d, p)?;
    p += 4;
    out.vertex_count = u16_at(d, p)?;
    p += 2;
    out.unk2 = u16_at(d, p)?;
    p += 2;
    out.unk3 = u16_at(d, p)?;
    p += 2;
    out.unk4 = u16_at(d, p)?;
    p += 2;
    out.unk5 = u16_at(d, p)?;
    p += 2;
    out.unk6 = u16_at(d, p)?;
    p += 2;
    out.unk7 = u16_at(d, p)?;
    p += 2;
    out.unk8 = u16_at(d, p)?;
    p += 2;
    out.unk9 = u16_at(d, p)?;
    p += 2;
    out.unk10 = u16_at(d, p)?;
    p += 2;
    out.unk11 = u16_at(d, p)?;
    p += 2;
    out.bone_count = u16_at(d, p)?;
    p += 2;
    out.unk13 = u16_at(d, p)?;
    p += 2;
    out.unk14 = u16_at(d, p)?;
    Some(out)
}

fn read_i16_array(d: &[u8], off: i64, count: usize) -> Vec<i16> {
    let mut out = Vec::with_capacity(count);
    let base = off as usize;
    for i in 0..count {
        if let Some(v) = i16_at(d, base + i * 2) {
            out.push(v);
        } else {
            break;
        }
    }
    out
}

fn read_f32_array(d: &[u8], off: i64, count: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(count);
    let base = off as usize;
    for i in 0..count {
        if let Some(v) = f32_at(d, base + i * 4) {
            out.push(v);
        } else {
            break;
        }
    }
    out
}

pub fn parse(asset: &[u8]) -> Result<Cloth, Error> {
    use crate::chunks::Chunks;
    let chunks = Chunks::parse(asset).map_err(|_| Error::NotAChunkedAsset)?;
    let payload = chunks.payload(asset, b"CLTH").ok_or(Error::MissingChunk)?;
    parse_payload(payload)
}

pub fn parse_payload(payload: &[u8]) -> Result<Cloth, Error> {
    if payload.len() < 16 {
        return Err(Error::Malformed("truncated CLTH header"));
    }
    let count = u64::from_le_bytes(payload[0..8].try_into().unwrap());
    let offset = i64::from_le_bytes(payload[8..16].try_into().unwrap());
    let mut cloth = Cloth {
        count,
        offset,
        pieces: Vec::new(),
    };
    if offset <= 0 {
        return Ok(cloth);
    }
    let mut cursor = offset as usize;
    for _ in 0..count {
        if cursor + 360 > payload.len() {
            break;
        }
        let desc = read_desc(payload, cursor)?;
        let next_cursor = cursor + 360;
        let domino = desc.domino;
        let vc = domino.vertex_count as usize;
        let follower = read_i16_array(payload, domino.vertex_follower_bone_indices, vc);
        let parent = read_i16_array(payload, domino.vertex_parent_indices, vc);
        let weights_flat = read_f32_array(payload, domino.vertex_driver_weights, vc * 4);
        let indices_flat = read_i16_array(payload, domino.vertex_driver_influences, vc * 4);
        let bone_map = read_i16_array(
            payload,
            domino.driver_to_bone_map,
            domino.bone_count as usize,
        );
        let mut vertices = Vec::with_capacity(vc);
        for i in 0..vc {
            let mut dw = Vec::with_capacity(4);
            for k in 0..4 {
                let wi = i * 4 + k;
                let w = weights_flat.get(wi).copied().unwrap_or(0.0);
                let di = indices_flat.get(wi).copied().unwrap_or(-1);
                dw.push(VertexDriverWeight {
                    driver_idx: di,
                    weight: w,
                });
            }
            vertices.push(ClothVertex {
                follower_bone: follower.get(i).copied().unwrap_or(-1),
                vertical_parent: parent.get(i).copied().unwrap_or(-1),
                driver_weights: dw,
            });
        }
        cloth.pieces.push(ClothPiece {
            descriptor: desc,
            vertices,
            bone_to_driver_map: bone_map,
        });
        cursor = next_cursor;
    }
    Ok(cloth)
}

fn read_desc(d: &[u8], at: usize) -> Result<ClothDesc, Error> {
    let domino = read_domino(d, at).ok_or(Error::Malformed("cloth descriptor"))?;
    let mut p = at + 280;
    let mut desc = ClothDesc {
        domino,
        ..Default::default()
    };
    desc.offset28 = i64_at(d, p).ok_or(Error::Malformed("desc"))?;
    p += 8;
    desc.map_offset = i64_at(d, p).ok_or(Error::Malformed("desc"))?;
    p += 8;
    desc.unknown_guid = u64_at(d, p).ok_or(Error::Malformed("desc"))?;
    p += 8;
    desc.unknown2 = i64_at(d, p).ok_or(Error::Malformed("desc"))?;
    p += 8;
    desc.unknown3_guid = u64_at(d, p).ok_or(Error::Malformed("desc"))?;
    p += 8;
    desc.ow225_new1 = i64_at(d, p).ok_or(Error::Malformed("desc"))?;
    p += 8;
    desc.unknown4 = i64_at(d, p).ok_or(Error::Malformed("desc"))?;
    p += 8;
    desc.unknown5 = i64_at(d, p).ok_or(Error::Malformed("desc"))?;
    p += 8;
    desc.unknown6 = i64_at(d, p).ok_or(Error::Malformed("desc"))?;
    p += 8;
    desc.ow225_new2 = i64_at(d, p).ok_or(Error::Malformed("desc"))?;
    Ok(desc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skeleton::Skeleton;

    #[test]
    fn empty_header() {
        let data = vec![0u8; 16];
        let c = parse_payload(&data).unwrap();
        assert_eq!(c.count, 0);
        assert!(c.pieces.is_empty());
    }

    fn skeleton(parents: Vec<i16>) -> Skeleton {
        Skeleton {
            bones_abs: parents.len() as u16,
            bones_simple: parents.len() as u16,
            bones_cloth: 0,
            parents,
            ..Skeleton::default()
        }
    }

    /// A cloth vertex with the four driver slots the file always stores,
    /// padding unused ones the way an absent driver looks (`-1`, weight 0).
    fn vertex(follower: i16, parent: i16, drivers: &[(i16, f32)]) -> ClothVertex {
        let mut driver_weights: Vec<VertexDriverWeight> = drivers
            .iter()
            .map(|(d, w)| VertexDriverWeight {
                driver_idx: *d,
                weight: *w,
            })
            .collect();
        while driver_weights.len() < 4 {
            driver_weights.push(VertexDriverWeight {
                driver_idx: -1,
                weight: 0.0,
            });
        }
        ClothVertex {
            follower_bone: follower,
            vertical_parent: parent,
            driver_weights,
        }
    }

    fn piece(vertices: Vec<ClothVertex>, map: Vec<i16>) -> ClothPiece {
        ClothPiece {
            descriptor: ClothDesc::default(),
            vertices,
            bone_to_driver_map: map,
        }
    }

    /// The rule, on a synthetic chain of six bones and drivers the vertices
    /// name. Every expected value below is derived from the reference's
    /// `GetEffectiveBoneIndex`, not from this implementation's output: a vertex
    /// takes the driver branch only when its parent is itself or -1, otherwise
    /// it inherits whatever its parent vertex answers, and a vertex with a
    /// follower bone re-parents it and answers with the follower bone itself.
    #[test]
    fn fake_hierarchy_reparents_cloth_followers() {
        // 0 is the root, each bone is its predecessor's child.
        let sk = skeleton(vec![-1, 0, 1, 2, 3, 4]);
        // Drivers 10 and 11 map to bones 0 and 1, so `IndexOf` answers 0/1.
        let cloth = Cloth {
            count: 1,
            offset: 0,
            pieces: vec![piece(
                vec![
                    // A driver root: parent is itself, so the highest-weight
                    // driver (11 -> bone 1) is the answer. No follower here.
                    vertex(-1, 0, &[(11, 1.0)]),
                    // Hangs off vertex 0; follower 5 is re-parented to bone 1.
                    vertex(5, 0, &[(11, 1.0)]),
                    // Hangs off vertex 1, whose answer is the *follower bone* 5,
                    // so its own follower 4 ends up under bone 5.
                    vertex(4, 1, &[(11, 1.0)]),
                    // No follower: only propagates the answer (bone 4).
                    vertex(-1, 2, &[(11, 1.0)]),
                    // Parent is itself and the top driver is -1: the reference
                    // returns -1 *before* considering the follower, so bone 3
                    // is NOT re-parented by this vertex.
                    vertex(3, 4, &[(-1, 1.0)]),
                    // Hangs off vertex 4, whose answer is -1, so follower 2 is
                    // not re-parented either.
                    vertex(2, 4, &[(11, 1.0)]),
                    // An out-of-range parent takes the driver branch instead of
                    // indexing out of bounds (the reference would throw);
                    // follower 3 -> bone 0.
                    vertex(3, 999, &[(10, 1.0)]),
                    // Hangs off vertex 3, whose answer is bone 4; follower
                    // 2 -> bone 4.
                    vertex(2, 3, &[(10, 1.0)]),
                    // A tie between -1 and 11 keeps the *first* (.NET `MaxBy`
                    // keeps the first maximum), so this is another -1 and bone
                    // 2 keeps the bone 4 that vertex 7 gave it. A last-max port
                    // would move it to 1.
                    vertex(2, 8, &[(-1, 0.5), (11, 0.5), (11, 0.0)]),
                ],
                vec![10, 11],
            )],
        };
        let (hierarchy, reparented) = cloth.fake_hierarchy(&sk);
        // h5 = 1 (vertex 1), h4 = 5 (vertex 2), h3 = 0 (vertex 6), h2 = 4
        // (vertex 7).
        assert_eq!(hierarchy, vec![-1, 0, 4, 0, 5, 1]);
        assert_eq!(reparented, vec![2, 3, 4, 5]);
        // The input skeleton is not mutated.
        assert_eq!(sk.parents, vec![-1, 0, 1, 2, 3, 4]);
    }

    /// The three guards where the reference would crash or hang, none of which
    /// should occur on real data.
    #[test]
    fn fake_hierarchy_survives_cycles_and_bad_indices() {
        // A cycle between two vertices: the reference recurses forever.
        let sk = skeleton(vec![-1, 0]);
        let cyc = Cloth {
            count: 1,
            offset: 0,
            pieces: vec![piece(
                vec![vertex(-1, 1, &[(10, 1.0)]), vertex(-1, 0, &[(10, 1.0)])],
                vec![10],
            )],
        };
        let (hierarchy, reparented) = cyc.fake_hierarchy(&sk);
        assert_eq!(hierarchy, vec![-1, 0]);
        assert!(reparented.is_empty());

        // A follower bone outside the skeleton's parent array: no write, no
        // panic.
        let stray = Cloth {
            count: 1,
            offset: 0,
            pieces: vec![piece(vec![vertex(7, 0, &[(10, 1.0)])], vec![10])],
        };
        let (hierarchy, reparented) = stray.fake_hierarchy(&sk);
        assert_eq!(hierarchy, vec![-1, 0]);
        assert!(reparented.is_empty());
    }
}
