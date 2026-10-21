//! Effect chunks (`ECAT`, `ESHT`, `ECMP`, `ECSS`).

use std::fmt;

#[derive(Debug, Clone, Copy, Default)]
pub struct EffectHardpoint {
    pub hardpoint: u64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EffectTiming {
    pub start_time_offset: i64,
    pub duration_offset: i64,
    pub hardpoint: u64,
    pub float_one_a: f32,
    pub float_one_b: f32,
    pub unk: i64,
    pub index: i16,
    pub start_time: f32,
    pub duration: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EffectShaderSetup {
    pub unknown: u64,
    pub material: u64,
    pub material_data: u64,
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
            Error::MissingChunk => write!(f, "missing effect chunk"),
            Error::Malformed(m) => write!(f, "malformed effect chunk: {m}"),
        }
    }
}

impl std::error::Error for Error {}

fn i16_at(d: &[u8], at: usize) -> Option<i16> {
    d.get(at..at + 2).map(|b| i16::from_le_bytes([b[0], b[1]]))
}
fn i64_at(d: &[u8], at: usize) -> Option<i64> {
    d.get(at..at + 8)
        .map(|b| i64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}
fn u64_at(d: &[u8], at: usize) -> Option<u64> {
    d.get(at..at + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}
fn f32_at(d: &[u8], at: usize) -> Option<f32> {
    d.get(at..at + 4)
        .map(|b| f32::from_bits(u32::from_le_bytes([b[0], b[1], b[2], b[3]])))
}

pub fn parse_ecat(payload: &[u8]) -> Option<EffectHardpoint> {
    let hp = u64_at(payload, 0)?;
    Some(EffectHardpoint { hardpoint: hp })
}
pub fn parse_esht(payload: &[u8]) -> Option<EffectHardpoint> {
    parse_ecat(payload)
}
pub fn parse_ecss(payload: &[u8]) -> Option<EffectShaderSetup> {
    Some(EffectShaderSetup {
        unknown: u64_at(payload, 0)?,
        material: u64_at(payload, 8)?,
        material_data: u64_at(payload, 16)?,
    })
}
pub fn parse_ecmp(payload: &[u8]) -> Option<EffectTiming> {
    let start_time_offset = i64_at(payload, 0)?;
    let duration_offset = i64_at(payload, 8)?;
    let mut out = EffectTiming {
        start_time_offset,
        duration_offset,
        hardpoint: u64_at(payload, 16)?,
        float_one_a: f32_at(payload, 24)?,
        float_one_b: f32_at(payload, 28)?,
        unk: i64_at(payload, 32)?,
        index: i16_at(payload, 40)?,
        ..EffectTiming::default()
    };
    if out.start_time_offset != 0 {
        let off = out.start_time_offset as usize;
        if off + 4 <= payload.len() {
            out.start_time = f32_at(payload, off).unwrap_or(0.0);
        }
    }
    if out.duration_offset != 0 {
        let off = out.duration_offset as usize;
        if off + 4 <= payload.len() {
            out.duration = f32_at(payload, off).unwrap_or(0.0);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ecss_reads() {
        let mut d = vec![0u8; 24];
        for (i, v) in [1u64, 2, 3].iter().enumerate() {
            let bytes = v.to_le_bytes();
            d[i * 8..(i + 1) * 8].copy_from_slice(&bytes);
        }
        let e = parse_ecss(&d).unwrap();
        assert_eq!(e.material, 2);
    }
}
