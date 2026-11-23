//! Best-effort fonts for styled Page view, separate from exact paint sessions.
//!
//! Coverage is checked before drawing. Missing characters produce a visible box,
//! never a terminal session error. This is scalar rasterization, not shaping:
//! complex-script joining, bidi and ligatures are not implemented here.

use fontdue::{Font, FontSettings};

const MAX_SIZE: u32 = 256;

/// A raster mask with baseline-relative ink coordinates.
#[derive(Clone, Debug)]
pub struct PageGlyph {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub advance: u32,
    pub coverage: Vec<u8>,
    pub missing: bool,
}

/// Regular and bold faces, optional true italic cuts, and coverage fallbacks.
/// No glyph cache is retained, so a long page cannot grow session memory.
pub struct PageFontSession {
    regular: Font,
    bold: Font,
    italic: Option<Font>,
    bold_italic: Option<Font>,
    fallback: Vec<Font>,
}

impl PageFontSession {
    /// Load the bundled licensed faces. Complex scripts need an added face.
    ///
    /// # Errors
    /// Returns the font parser's error if bundled bytes are damaged.
    pub fn bundled() -> Result<Self, &'static str> {
        Self::from_bytes(crate::TEXT_FONT, crate::DISPLAY_FONT)
    }

    /// Load caller-provided regular and bold cuts.
    ///
    /// # Errors
    /// Returns the font parser's error for malformed face bytes.
    pub fn from_bytes(regular: &[u8], bold: &[u8]) -> Result<Self, &'static str> {
        Ok(Self {
            regular: Font::from_bytes(regular, FontSettings::default())?,
            bold: Font::from_bytes(bold, FontSettings::default())?,
            italic: None,
            bold_italic: None,
            fallback: Vec::new(),
        })
    }

    /// Install real italic faces, without synthetic shear.
    ///
    /// # Errors
    /// Returns the font parser's error. The session is unchanged on failure.
    pub fn set_italic(&mut self, italic: &[u8], bold_italic: &[u8]) -> Result<(), &'static str> {
        let italic = Font::from_bytes(italic, FontSettings::default())?;
        let bold_italic = Font::from_bytes(bold_italic, FontSettings::default())?;
        self.italic = Some(italic);
        self.bold_italic = Some(bold_italic);
        Ok(())
    }

    /// Add one coverage fallback. At most eight faces may be retained.
    ///
    /// # Errors
    /// Returns an error for malformed bytes or an exceeded face budget.
    pub fn add_fallback(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        if self.fallback.len() >= 8 {
            return Err("fallback face budget exceeded");
        }
        self.fallback
            .push(Font::from_bytes(bytes, FontSettings::default())?);
        Ok(())
    }

    fn primary(&self, bold: bool, italic: bool) -> &Font {
        match (bold, italic) {
            (true, true) => self.bold_italic.as_ref().unwrap_or(&self.bold),
            (false, true) => self.italic.as_ref().unwrap_or(&self.regular),
            (true, false) => &self.bold,
            (false, false) => &self.regular,
        }
    }

    fn face(&self, ch: char, bold: bool, italic: bool) -> Option<&Font> {
        let primary = self.primary(bold, italic);
        if primary.lookup_glyph_index(ch) != 0 {
            return Some(primary);
        }
        self.fallback
            .iter()
            .find(|face| face.lookup_glyph_index(ch) != 0)
    }

    /// Whether a real glyph, rather than a placeholder, will be used.
    #[must_use]
    pub fn has_glyph(&self, ch: char, bold: bool, italic: bool) -> bool {
        self.face(ch, bold, italic).is_some()
    }

    /// Whether a true italic cut is installed for this weight.
    #[must_use]
    pub fn has_italic(&self, bold: bool) -> bool {
        if bold {
            self.bold_italic.is_some()
        } else {
            self.italic.is_some()
        }
    }

    /// Natural strut, enlarged to accommodate every installed fallback face.
    #[must_use]
    pub fn line_metrics(&self, px: u32, bold: bool, italic: bool) -> Option<(u32, i32)> {
        if !(1..=MAX_SIZE).contains(&px) {
            return None;
        }
        let mut ascent = 0.0_f32;
        let mut descent = 0.0_f32;
        let mut gap = 0.0_f32;
        for font in std::iter::once(self.primary(bold, italic)).chain(self.fallback.iter()) {
            let metrics = font.horizontal_line_metrics(px as f32)?;
            ascent = ascent.max(metrics.ascent);
            descent = descent.max(-metrics.descent);
            gap = gap.max(metrics.line_gap);
        }
        let height = (ascent + descent + gap).ceil();
        if !height.is_finite() || height <= 0.0 || height > 4096.0 {
            return None;
        }
        #[allow(clippy::cast_sign_loss)] // Positive and capped above.
        let height = height as u32;
        Some((height, ascent.ceil() as i32))
    }

    /// Rounded scalar advance. Combining marks may have a zero advance.
    #[must_use]
    pub fn advance(&self, ch: char, px: u32, bold: bool, italic: bool) -> Option<u32> {
        if !(1..=MAX_SIZE).contains(&px) {
            return None;
        }
        Some(self.face(ch, bold, italic).map_or_else(
            || (px / 2).max(3),
            |font| {
                #[allow(clippy::cast_sign_loss)] // Negative advances clamp to zero.
                let width = font
                    .metrics(ch, px as f32)
                    .advance_width
                    .round()
                    .clamp(0.0, 4096.0) as u32;
                width
            },
        ))
    }

    /// Draw a real glyph or a hollow missing-glyph box. No sticky failures.
    #[must_use]
    pub fn raster(&self, ch: char, px: u32, bold: bool, italic: bool) -> Option<PageGlyph> {
        let advance = self.advance(ch, px, bold, italic)?;
        let Some(font) = self.face(ch, bold, italic) else {
            let width = advance.max(3);
            let height = px.max(4);
            let mut coverage = vec![0; (width * height) as usize];
            for y in 0..height {
                for x in 0..width {
                    if x == 0 || x + 1 == width || y == 0 || y + 1 == height {
                        coverage[(y * width + x) as usize] = 255;
                    }
                }
            }
            return Some(PageGlyph {
                left: 0,
                top: -i32::try_from(height).ok()?,
                width,
                height,
                advance,
                coverage,
                missing: true,
            });
        };
        let (m, coverage) = font.rasterize(ch, px as f32);
        Some(PageGlyph {
            left: m.xmin,
            top: -m.ymin - i32::try_from(m.height).ok()?,
            width: u32::try_from(m.width).ok()?,
            height: u32::try_from(m.height).ok()?,
            advance,
            coverage,
            missing: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::PageFontSession;

    #[test]
    fn missing_scripts_draw_boxes_without_poisoning_later_text() {
        let session = PageFontSession::bundled().unwrap();
        for ch in "हिन्दी Русский 日本語 © €".chars() {
            let glyph = session.raster(ch, 20, false, false).unwrap();
            assert_eq!(glyph.coverage.len(), (glyph.width * glyph.height) as usize);
            if !session.has_glyph(ch, false, false) {
                assert!(glyph.missing);
                assert!(glyph.coverage.contains(&255));
            }
        }
        assert!(!session.raster('A', 20, false, false).unwrap().missing);
    }

    #[test]
    fn bold_uses_a_real_different_cut() {
        let session = PageFontSession::bundled().unwrap();
        assert_ne!(
            session.raster('A', 24, false, false).unwrap().coverage,
            session.raster('A', 24, true, false).unwrap().coverage
        );
        assert!(!session.has_italic(false));
    }

    #[test]
    fn a_missing_glyph_is_not_transliterated() {
        let session = PageFontSession::bundled().unwrap();
        assert!(session.has_glyph('é', false, false));
        assert_ne!(
            session.raster('é', 24, false, false).unwrap().coverage,
            session.raster('e', 24, false, false).unwrap().coverage
        );
        assert!(session.advance('A', 0, false, false).is_none());
        assert!(session.raster('A', 257, false, false).is_none());
    }
}
