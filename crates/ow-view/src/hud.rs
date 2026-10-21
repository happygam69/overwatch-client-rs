//! The play window's corner overlay: text baked into a small R8 texture and
//! drawn over the frame by `main.rs`'s HUD pass.
//!
//! Everything is authored here, in our own code: a 5x7 bitmap font covering
//! exactly the characters the HUD prints (digits, uppercase, the punctuation
//! `hud()` produces), a grid that turns lines of text into ink bytes, and
//! nothing else. Legibility is the only requirement — this is a debug HUD, not
//! a reading about the client, so no evidence class applies. Characters with
//! no glyph are skipped (left blank), which is safer than guessing.

/// Glyph width, in pixels.
const GW: usize = 5;
/// Glyph height, in rows.
const GH: usize = 7;
/// Cell width: the glyph plus its right spacing.
const CW: usize = 8;
/// Cell height: the glyph plus its bottom spacing.
const CH: usize = 8;
/// Columns of text. 64 x 8 px cells = a 512 px video row, itself a multiple of
/// 256, which is also what `write_texture` wants for its row pitch.
pub const COLS: usize = 64;
/// Rows of text.
pub const ROWS: usize = 4;
/// Texture width, pixels (= COLS * CW).
pub const WIDTH: usize = COLS * CW;
/// Canvas height, pixels (= ROWS * CH).
pub const HEIGHT: usize = ROWS * CH;

/// The overlay's placement: an 8 px margin, the canvas at 1:1 pixels.
pub(crate) const MARGIN: f32 = 8.0;

/// The GPU-side uniform for the overlay. `rect` is the quad in pixels, with
/// the x/y origin at the window's top-left; `viewport` is the surface size.
/// Both as 4-wide rows so the uniform block has no alignment subtleties.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Globals {
    pub(crate) rect: [f32; 4],
    pub(crate) viewport: [f32; 4],
}

/// Build the uniform: the quad at [`MARGIN`], the canvas, at this viewport.
pub(crate) fn globals(viewport_w: f32, viewport_h: f32) -> Globals {
    Globals {
        rect: [MARGIN, MARGIN, WIDTH as f32, HEIGHT as f32],
        viewport: [viewport_w, viewport_h, 0.0, 0.0],
    }
}

/// One glyph: [`GH`] rows, each a 5-bit mask (MSB = leftmost column).
fn glyph(c: u8) -> Option<&'static [u8; GH]> {
    let rows: &[u8; 7] = match c {
        b'0' => &[0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        b'1' => &[0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        b'2' => &[0x0E, 0x11, 0x01, 0x06, 0x08, 0x10, 0x1F],
        b'3' => &[0x0E, 0x11, 0x01, 0x06, 0x01, 0x11, 0x0E],
        b'4' => &[0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        b'5' => &[0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        b'6' => &[0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        b'7' => &[0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        b'8' => &[0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        b'9' => &[0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        b'A' => &[0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'B' => &[0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        b'C' => &[0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        b'D' => &[0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        b'E' => &[0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        b'F' => &[0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        b'G' => &[0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        b'H' => &[0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'I' => &[0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        b'J' => &[0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        b'K' => &[0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        b'L' => &[0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        b'M' => &[0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        b'N' => &[0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        b'O' => &[0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'P' => &[0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        b'Q' => &[0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        b'R' => &[0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        b'S' => &[0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        b'T' => &[0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b'U' => &[0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'V' => &[0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        b'W' => &[0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        b'X' => &[0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        b'Y' => &[0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        b'Z' => &[0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        b'(' => &[0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        b')' => &[0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        b',' => &[0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x08],
        b'-' => &[0x00, 0x00, 0x00, 0x0E, 0x00, 0x00, 0x00],
        b'.' => &[0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        b'/' => &[0x01, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10],
        b':' => &[0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        b'|' => &[0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        _ => return None,
    };
    Some(rows)
}

/// A text grid as R8 ink bytes, `WIDTH` x `HEIGHT`, 255 where a glyph pixel is.
#[derive(Clone)]
pub struct HudCanvas {
    cols: usize,
    rows: usize,
    data: Vec<u8>,
}

impl HudCanvas {
    /// The full text block: [`COLS`] columns by [`ROWS`] rows, blank.
    pub fn new() -> HudCanvas {
        HudCanvas {
            cols: COLS,
            rows: ROWS,
            data: vec![0; WIDTH * HEIGHT],
        }
    }

    pub fn dims(&self) -> (u32, u32) {
        (WIDTH as u32, HEIGHT as u32)
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Draw the given lines into the grid, replacing whatever was there.
    /// Lines beyond [`ROWS`] and glyphs beyond [`COLS`] are clipped, not an
    /// error — the HUD is a report, and a report that cannot break the frame
    /// is worth more than one that can. Lowercase is uppercased (the font is
    /// uppercase-only); a character without a glyph is left blank.
    pub fn lines(&mut self, lines: &[String]) {
        self.data.fill(0);
        for (i, line) in lines.iter().enumerate().take(self.rows) {
            for (j, c) in line
                .to_ascii_uppercase()
                .bytes()
                .take(self.cols)
                .enumerate()
            {
                let Some(rows) = glyph(c) else {
                    continue;
                };
                let base_x = j * CW;
                let base_y = i * CH;
                for (gy, bits) in rows.iter().enumerate() {
                    for gx in 0..GW {
                        if bits & (0x10 >> gx) != 0 {
                            self.data[(base_y + gy) * WIDTH + base_x + gx] = 255;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_seven_five_bit_rows() {
        for c in b'0'..=b'9' {
            let g = glyph(c).unwrap_or_else(|| panic!("no glyph for {}", c as char));
            assert!(g.iter().all(|bits| *bits <= 0x1F), "{:#x}", c);
        }
        for c in b'A'..=b'Z' {
            let g = glyph(c).unwrap_or_else(|| panic!("no glyph for {}", c as char));
            assert!(g.iter().all(|bits| *bits <= 0x1F), "{:#x}", c);
        }
    }

    #[test]
    fn a_digit_lands_in_the_expected_cells() {
        let mut hud = HudCanvas::new();
        hud.lines(&["1".to_string()]);
        // '1': rows [0x04, 0x0C, 0x04, ...] — column 2 inked in all seven.
        for gy in 0..GH {
            assert_eq!(hud.data()[gy * WIDTH + 2], 255, "row {gy}");
        }
        // The spacing columns and the rows below the glyph are blank.
        assert_eq!(hud.data()[2 * WIDTH + GW], 0);
        assert_eq!(hud.data()[GH * WIDTH + 2], 0);
        // Only line 0 is drawn; the other rows are untouched.
        assert!(hud.data()[HEIGHT * WIDTH / 2..].iter().all(|b| *b == 0));
    }

    #[test]
    fn lines_are_uppercased_clipped_and_unknowns_skipped() {
        let mut hud = HudCanvas::new();
        // 'a' uppercases to a glyph; '?', '+', which the font does not carry,
        // leave their cells blank rather than drawing something guessed.
        hud.lines(&["a?+b".to_string()]);
        assert_eq!(hud.data()[2], 255); // 'A' at cell 0
        assert_eq!(hud.data()[3 * CW + 2], 255); // 'B' at cell 3
                                                 // The unknown characters' cells are ink-free.
        assert_eq!(hud.data()[CW + 2], 0);
        assert_eq!(hud.data()[2 * CW + 2], 0);
        // A longer line clips at COLS: the last glyph reaches cell 63, and the
        // spacing columns after it are the row's last ink-free pixels. (A
        // glyph is GH pixel rows tall, so line 0 legitimately continues into
        // texture rows 1..GH — the blank check is the *text* row below, at
        // CH, not the next 512 bytes.)
        let long = "X".repeat(COLS * 2);
        hud.lines(&[long]);
        assert_eq!(hud.data()[(COLS - 1) * CW], 255);
        assert!(hud.data()[(COLS - 1) * CW + GW..COLS * CW]
            .iter()
            .all(|b| *b == 0));
        assert!(hud.data()[CH * WIDTH..].iter().all(|b| *b == 0));
    }

    #[test]
    fn lines_replace_what_was_there() {
        let mut hud = HudCanvas::new();
        hud.lines(&["X".repeat(COLS).to_string()]);
        hud.lines(&[]);
        assert!(hud.data().iter().all(|b| *b == 0));
    }
}
