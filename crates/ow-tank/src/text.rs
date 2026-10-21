//! Display strings and subtitle tables (`teString`, `teSubtitleThing`).

use std::fmt;

#[derive(Debug, Clone, Default)]
pub struct DisplayText {
    pub unknown: u8,
    pub param_count: u8,
    pub value: String,
}

#[derive(Debug, Clone, Default)]
pub struct SubtitleThing {
    pub strings: Vec<String>,
}

#[derive(Debug)]
pub enum Error {
    Malformed(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Malformed(m) => write!(f, "malformed text: {m}"),
        }
    }
}

impl std::error::Error for Error {}

pub fn parse_display_text(data: &[u8]) -> Result<DisplayText, Error> {
    if data.len() < 2 {
        return Err(Error::Malformed("truncated display text header"));
    }
    let mut out = DisplayText {
        unknown: data[0],
        param_count: data[1],
        ..Default::default()
    };
    // Characters run from after the header to one byte before the end.
    if data.len() > 3 {
        let bytes = &data[2..data.len() - 1];
        out.value = String::from_utf8_lossy(bytes)
            .trim_end_matches('\0')
            .to_string();
    }
    Ok(out)
}

pub fn parse_subtitle_thing(data: &[u8]) -> SubtitleThing {
    let mut out = SubtitleThing::default();
    if data.len() < 8 {
        return out;
    }
    let mut offsets = [0u16; 4];
    for (i, slot) in offsets.iter_mut().enumerate() {
        let at = i * 2;
        if at + 2 <= data.len() {
            *slot = u16::from_le_bytes([data[at], data[at + 1]]);
        }
    }
    for off in offsets {
        if off == 0 {
            continue;
        }
        let start = off as usize;
        if start >= data.len() {
            continue;
        }
        let mut end = start;
        while end < data.len() && data[end] != 0 {
            end += 1;
        }
        out.strings
            .push(String::from_utf8_lossy(&data[start..end]).to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtitle_reads_four() {
        // offsets at 0: 8, 10, 0, 12; strings follow
        let mut data = vec![0u8; 20];
        data[0..2].copy_from_slice(&8u16.to_le_bytes());
        data[2..4].copy_from_slice(&11u16.to_le_bytes());
        data[4..6].copy_from_slice(&0u16.to_le_bytes());
        data[6..8].copy_from_slice(&14u16.to_le_bytes());
        data[8..11].copy_from_slice(b"hi\0");
        data[11..14].copy_from_slice(b"ok\0");
        data[14..18].copy_from_slice(b"bye\0");
        let s = parse_subtitle_thing(&data);
        assert_eq!(s.strings, vec!["hi", "ok", "bye"]);
    }
}
