//! Best-effort fonts for styled Page view, separate from exact paint sessions.
//!
//! Coverage is checked before drawing. Missing characters produce a visible box,
//! never a terminal session error. Optional OpenType cluster shaping handles
//! left-to-right scripts. Bidi paragraph layout remains unsupported.

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

/// One shaped input cluster. `start` and `end` are UTF-8 byte boundaries.
#[derive(Clone, Debug)]
pub struct PageCluster {
    pub start: usize,
    pub end: usize,
    pub glyph: PageGlyph,
}

/// Regular and bold faces, optional true italic cuts, and coverage fallbacks.
/// No glyph cache is retained, so a long page cannot grow session memory.
pub struct PageFontSession {
    regular: std::sync::Arc<Font>,
    bold: Font,
    italic: Option<Font>,
    bold_italic: Option<Font>,
    fallback: Vec<Font>,
    regular_bytes: Vec<u8>,
    bold_bytes: Vec<u8>,
    italic_bytes: Option<Vec<u8>>,
    bold_italic_bytes: Option<Vec<u8>>,
    fallback_bytes: Vec<Vec<u8>>,
    fallback_styles: Vec<(bool, bool)>,
}

impl PageFontSession {
    /// Load bundled Latin, Cyrillic and Devanagari faces. CJK needs an added face.
    ///
    /// # Errors
    /// Returns the font parser's error if bundled bytes are damaged.
    pub fn bundled() -> Result<Self, &'static str> {
        let mut session = Self::from_bytes(crate::TEXT_FONT, crate::DISPLAY_FONT)?;
        session.add_fallback(include_bytes!("../fonts/NotoSans-Cyrillic.ttf"))?;
        session.add_fallback(include_bytes!("../fonts/NotoSansDevanagari.ttf"))?;
        session.add_fallback(include_bytes!("../fonts/KoboPage-arabic.ttf"))?;
        session.add_fallback(include_bytes!("../fonts/KoboPage-hebrew.ttf"))?;
        session.add_styled_fallback(
            include_bytes!("../fonts/KoboPage-arabic-bold.ttf"),
            true,
            false,
        )?;
        session.add_styled_fallback(
            include_bytes!("../fonts/KoboPage-hebrew-bold.ttf"),
            true,
            false,
        )?;
        session.add_styled_fallback(
            include_bytes!("../fonts/KoboPage-cyrillic-bold.ttf"),
            true,
            false,
        )?;
        session.add_styled_fallback(
            include_bytes!("../fonts/KoboPage-cyrillic-italic.ttf"),
            false,
            true,
        )?;
        session.add_styled_fallback(
            include_bytes!("../fonts/KoboPage-cyrillic-bolditalic.ttf"),
            true,
            true,
        )?;
        session.add_styled_fallback(
            include_bytes!("../fonts/KoboPage-devanagari-bold.ttf"),
            true,
            false,
        )?;
        Ok(session)
    }

    /// Load caller-provided regular and bold cuts.
    ///
    /// # Errors
    /// Returns the font parser's error for malformed face bytes.
    pub fn from_bytes(regular: &[u8], bold: &[u8]) -> Result<Self, &'static str> {
        Ok(Self {
            regular: crate::shared_font(regular)?,
            bold: Font::from_bytes(bold, FontSettings::default())?,
            italic: None,
            bold_italic: None,
            fallback: Vec::new(),
            regular_bytes: regular.to_vec(),
            bold_bytes: bold.to_vec(),
            italic_bytes: None,
            bold_italic_bytes: None,
            fallback_bytes: Vec::new(),
            fallback_styles: Vec::new(),
        })
    }

    /// Install real italic faces, without synthetic shear.
    ///
    /// # Errors
    /// Returns the font parser's error. The session is unchanged on failure.
    pub fn set_italic(&mut self, italic: &[u8], bold_italic: &[u8]) -> Result<(), &'static str> {
        let italic_bytes = italic;
        let bold_italic_bytes = bold_italic;
        let italic = Font::from_bytes(italic, FontSettings::default())?;
        let bold_italic = Font::from_bytes(bold_italic, FontSettings::default())?;
        self.italic_bytes = Some(italic_bytes.to_vec());
        self.bold_italic_bytes = Some(bold_italic_bytes.to_vec());
        self.italic = Some(italic);
        self.bold_italic = Some(bold_italic);
        Ok(())
    }

    /// Add one coverage fallback. At most sixteen faces may be retained.
    ///
    /// # Errors
    /// Returns an error for malformed bytes or an exceeded face budget.
    pub fn add_fallback(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        self.add_styled_fallback(bytes, false, false)
    }

    /// Add a fallback face with its actual weight and slant, never synthetic styles.
    ///
    /// # Errors
    /// Returns an error for malformed bytes or an exceeded face budget.
    pub fn add_styled_fallback(
        &mut self,
        bytes: &[u8],
        bold: bool,
        italic: bool,
    ) -> Result<(), &'static str> {
        if self.fallback.len() >= 16 {
            return Err("fallback face budget exceeded");
        }
        self.fallback
            .push(Font::from_bytes(bytes, FontSettings::default())?);
        self.fallback_bytes.push(bytes.to_vec());
        self.fallback_styles.push((bold, italic));
        Ok(())
    }

    fn primary(&self, bold: bool, italic: bool) -> &Font {
        match (bold, italic) {
            (true, true) => self.bold_italic.as_ref().unwrap_or(&self.bold),
            (false, true) => self.italic.as_ref().unwrap_or(self.regular.as_ref()),
            (true, false) => &self.bold,
            (false, false) => &self.regular,
        }
    }

    fn face(&self, ch: char, bold: bool, italic: bool) -> Option<&Font> {
        let primary = self.primary(bold, italic);
        if primary.lookup_glyph_index(ch) != 0 {
            return Some(primary);
        }
        self.fallback_index(|face| face.lookup_glyph_index(ch) != 0, bold, italic)
            .map(|i| &self.fallback[i])
    }

    fn fallback_index(
        &self,
        covers: impl Fn(&Font) -> bool,
        bold: bool,
        italic: bool,
    ) -> Option<usize> {
        self.fallback
            .iter()
            .enumerate()
            .filter(|(_, font)| covers(font))
            .min_by_key(|(i, _)| {
                let (b, it) = self.fallback_styles[*i];
                u8::from(b != bold) + 2 * u8::from(it != italic)
            })
            .map(|(i, _)| i)
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

    /// Natural strut of the primary face only, independent of installed fallbacks.
    #[must_use]
    pub fn line_metrics(&self, px: u32, bold: bool, italic: bool) -> Option<(u32, i32)> {
        Self::metrics_for_fonts(px, std::iter::once(self.primary(bold, italic)))
    }

    /// Strut for a primary face and the fallback actually used by this character.
    /// The painter takes maxima only over the characters on each laid-out line.
    #[must_use]
    pub fn character_line_metrics(
        &self,
        ch: char,
        px: u32,
        bold: bool,
        italic: bool,
    ) -> Option<(u32, i32)> {
        Self::metrics_for_fonts(
            px,
            std::iter::once(self.primary(bold, italic)).chain(self.face(ch, bold, italic)),
        )
    }

    fn metrics_for_fonts<'a>(px: u32, fonts: impl Iterator<Item = &'a Font>) -> Option<(u32, i32)> {
        if !(1..=MAX_SIZE).contains(&px) {
            return None;
        }
        let mut ascent = 0.0_f32;
        let mut descent = 0.0_f32;
        let mut gap = 0.0_f32;
        for font in fonts {
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

    /// Shape text in whitespace-delimited font spans, keeping input cluster boundaries.
    /// Indic substitutions and positioning use the selected face's OpenType tables.
    /// Clusters return in logical source order; the painter reorders each line.
    #[must_use]
    #[allow(clippy::too_many_lines)] // One bounded cluster raster transaction.
    pub fn shape(&self, text: &str, px: u32, bold: bool, italic: bool) -> Option<Vec<PageCluster>> {
        if !(1..=MAX_SIZE).contains(&px) || text.len() > 65_536 {
            return None;
        }
        let primary = self.primary(bold, italic);
        let primary_bytes = match (bold, italic) {
            (true, true) => self
                .bold_italic_bytes
                .as_deref()
                .unwrap_or(&self.bold_bytes),
            (false, true) => self.italic_bytes.as_deref().unwrap_or(&self.regular_bytes),
            (true, false) => &self.bold_bytes,
            (false, false) => &self.regular_bytes,
        };
        let mut output = Vec::new();
        let mut mask_bytes = 0_usize;
        let mut base = 0;
        // Separate ASCII punctuation from joining-script words so a bracket
        // absent from the fallback face cannot force a whole word to scalar paint.
        let mut spans = Vec::new();
        let mut start = 0;
        for (i, ch) in text.char_indices() {
            if ch.is_ascii_punctuation() {
                if start < i {
                    spans.push(&text[start..i]);
                }
                spans.push(&text[i..i + ch.len_utf8()]);
                start = i + ch.len_utf8();
            } else if ch.is_whitespace() {
                spans.push(&text[start..i + ch.len_utf8()]);
                start = i + ch.len_utf8();
            }
        }
        if start < text.len() {
            spans.push(&text[start..]);
        }
        for word in spans {
            let covers = |font: &Font| {
                word.chars()
                    .all(|ch| ch.is_whitespace() || font.lookup_glyph_index(ch) != 0)
            };
            let selected = if covers(primary) {
                Some((primary, primary_bytes))
            } else {
                self.fallback_index(covers, bold, italic)
                    .map(|i| (&self.fallback[i], self.fallback_bytes[i].as_slice()))
            };
            let Some((font, bytes)) = selected else {
                for (offset, ch) in word.char_indices() {
                    output.push(PageCluster {
                        start: base + offset,
                        end: base + offset + ch.len_utf8(),
                        glyph: self.raster(ch, px, bold, italic)?,
                    });
                }
                base += word.len();
                continue;
            };
            let face = rustybuzz::Face::from_slice(bytes, 0)?;
            let scale = px as f32 / face.units_per_em() as f32;
            let mut buffer = rustybuzz::UnicodeBuffer::new();
            buffer.push_str(word);
            buffer.guess_segment_properties();
            let shaped = rustybuzz::shape(&face, &[], buffer);
            // The shaper returns RTL clusters in visual order. Store clusters
            // in logical source order; paragraph layout reorders each line later.
            let mut order: Vec<usize> = (0..shaped.glyph_infos().len()).collect();
            order.sort_by_key(|&i| shaped.glyph_infos()[i].cluster);
            let infos: Vec<_> = order.iter().map(|&i| shaped.glyph_infos()[i]).collect();
            let positions: Vec<_> = order.iter().map(|&i| shaped.glyph_positions()[i]).collect();
            let mut index = 0;
            while index < infos.len() {
                let start = usize::try_from(infos[index].cluster).ok()?;
                let mut end_index = index + 1;
                while end_index < infos.len() && infos[end_index].cluster == infos[index].cluster {
                    end_index += 1;
                }
                let end = if end_index < infos.len() {
                    usize::try_from(infos[end_index].cluster).ok()?
                } else {
                    word.len()
                };
                if end <= start || !word.is_char_boundary(start) || !word.is_char_boundary(end) {
                    return None;
                }
                let mut ink = Vec::new();
                let mut pen = 0_f32;
                let mut left = 0_i32;
                let mut right = 0_i32;
                let mut top = 0_i32;
                let mut bottom = 0_i32;
                for j in index..end_index {
                    let glyph_id = u16::try_from(infos[j].glyph_id).ok()?;
                    let m = font.metrics_indexed(glyph_id, px as f32);
                    if m.width > 512 || m.height > 512 {
                        return None;
                    }
                    let (_, coverage) = font.rasterize_indexed(glyph_id, px as f32);
                    let x = (pen + positions[j].x_offset as f32 * scale).round() as i32 + m.xmin;
                    let y = -(positions[j].y_offset as f32 * scale).round() as i32
                        - m.ymin
                        - i32::try_from(m.height).ok()?;
                    left = left.min(x);
                    right = right.max(x + i32::try_from(m.width).ok()?);
                    top = top.min(y);
                    bottom = bottom.max(y + i32::try_from(m.height).ok()?);
                    ink.push((x, y, m.width, m.height, coverage));
                    pen += positions[j].x_advance as f32 * scale;
                }
                let width = u32::try_from(right - left).ok()?;
                let height = u32::try_from(bottom - top).ok()?;
                let count = usize::try_from(width).ok()?.checked_mul(height as usize)?;
                mask_bytes = mask_bytes.checked_add(count)?;
                if count > 262_144 || mask_bytes > 4_194_304 {
                    return None;
                }
                let mut coverage = vec![0_u8; count];
                for (x, y, w, h, mask) in ink {
                    for row in 0..h {
                        for col in 0..w {
                            let dst = usize::try_from(y - top)
                                .ok()?
                                .checked_add(row)?
                                .checked_mul(width as usize)?
                                .checked_add(usize::try_from(x - left).ok()?)?
                                .checked_add(col)?;
                            coverage[dst] = coverage[dst].max(mask[row * w + col]);
                        }
                    }
                }
                #[allow(clippy::cast_sign_loss)]
                // Negative advances refuse below; rounded nonnegative px.
                let advance = pen.round().max(0.0) as u32;
                output.push(PageCluster {
                    start: base + start,
                    end: base + end,
                    glyph: PageGlyph {
                        left,
                        top,
                        width,
                        height,
                        advance,
                        coverage,
                        missing: false,
                    },
                });
                index = end_index;
            }
            base += word.len();
        }
        Some(output)
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
        let measured = font.metrics(ch, px as f32);
        if measured.width > 512
            || measured.height > 512
            || measured.width.checked_mul(measured.height)? > 262_144
        {
            return None;
        }
        let (m, coverage) = font.rasterize(ch, px as f32);
        if m.width != measured.width || m.height != measured.height {
            return None;
        }
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
    fn indic_shaping_clusters_cover_the_input_without_byte_splits() {
        let session = PageFontSession::bundled().unwrap();
        let text = "हिन्दी पाठ";
        let shaped = session.shape(text, 24, false, false).unwrap();
        assert!(!shaped.is_empty());
        assert_eq!(shaped.first().unwrap().start, 0);
        assert_eq!(shaped.last().unwrap().end, text.len());
        for pair in shaped.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
        }
        for c in &shaped {
            assert!(text.is_char_boundary(c.start));
            assert!(text.is_char_boundary(c.end));
            assert!(!c.glyph.missing);
        }
        assert!(
            shaped.len() < text.chars().count(),
            "Indic syllables form clusters"
        );
        assert!(session.shape(text, 0, false, false).is_none());
        assert!(session
            .shape(&"x".repeat(65_537), 24, false, false)
            .is_none());
    }

    #[test]
    fn bundled_fallbacks_cover_russian_and_devanagari_but_not_japanese() {
        let session = PageFontSession::bundled().unwrap();
        assert!("Русский हिन्दी"
            .chars()
            .all(|ch| session.has_glyph(ch, false, false)));
        assert!(!session.has_glyph('日', false, false));
    }

    #[test]
    fn fallback_weight_and_slant_select_actual_cuts() {
        let session = PageFontSession::bundled().unwrap();
        let regular = session.shape("Русский", 24, false, false).unwrap();
        let bold = session.shape("Русский", 24, true, false).unwrap();
        let italic = session.shape("Русский", 24, false, true).unwrap();
        assert_ne!(regular[0].glyph.coverage, bold[0].glyph.coverage);
        assert_ne!(regular[0].glyph.coverage, italic[0].glyph.coverage);
        let regular = session.shape("हिन्दी", 24, false, false).unwrap();
        let bold = session.shape("हिन्दी", 24, true, false).unwrap();
        assert_ne!(regular[0].glyph.coverage, bold[0].glyph.coverage);
    }

    #[test]
    fn unused_fallback_never_inflates_latin_line_metrics() {
        let plain = PageFontSession::from_bytes(crate::TEXT_FONT, crate::DISPLAY_FONT).unwrap();
        let bundled = PageFontSession::bundled().unwrap();
        assert_eq!(
            plain.line_metrics(24, false, false),
            bundled.line_metrics(24, false, false)
        );
        assert_eq!(
            plain.character_line_metrics('A', 24, false, false),
            bundled.character_line_metrics('A', 24, false, false)
        );
        let latin = bundled
            .character_line_metrics('A', 24, false, false)
            .unwrap();
        let indic = bundled
            .character_line_metrics('ह', 24, false, false)
            .unwrap();
        assert!(indic.0 >= latin.0);
        assert_ne!(indic, latin, "used Devanagari may enlarge only its line");
    }

    #[test]
    fn arabic_and_hebrew_shape_to_logical_clusters_with_full_coverage() {
        let session = PageFontSession::bundled().unwrap();
        for text in ["مرحبا بالعالم", "שלום עולם"] {
            let shaped = session.shape(text, 28, false, false).unwrap();
            assert_eq!(shaped.first().unwrap().start, 0);
            assert_eq!(shaped.last().unwrap().end, text.len());
            for pair in shaped.windows(2) {
                assert_eq!(pair[0].end, pair[1].start);
            }
            assert!(text.chars().all(|ch| session.has_glyph(ch, false, false)));
            assert!(shaped.iter().all(|c| !c.glyph.missing));
        }
    }

    #[test]
    fn arabic_and_hebrew_bold_use_distinct_real_cuts() {
        let session = PageFontSession::bundled().unwrap();
        for text in ["مرحبا", "שלום"] {
            let regular = session.shape(text, 28, false, false).unwrap();
            let bold = session.shape(text, 28, true, false).unwrap();
            assert_ne!(regular[0].glyph.coverage, bold[0].glyph.coverage);
        }
    }

    #[test]
    fn punctuation_does_not_disable_joining_script_shaping() {
        let session = PageFontSession::bundled().unwrap();
        let plain = session.shape("بالعالم", 28, false, false).unwrap();
        let bracketed = session.shape("(بالعالم)", 28, false, false).unwrap();
        assert_eq!(bracketed.len(), plain.len() + 2);
        for (a, b) in plain.iter().zip(bracketed.iter().skip(1)) {
            assert_eq!(a.glyph.coverage, b.glyph.coverage);
        }
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

#[cfg(test)]
mod shared_mono_tests {
    use super::*;
    #[test]
    fn shared_mono_identity_and_all_mapped_glyphs_match_uncached() {
        let ui = crate::shared_font(crate::MONO_FONT).unwrap();
        let page = PageFontSession::from_bytes(crate::MONO_FONT,
            include_bytes!("../fonts/DejaVuSansMono-Bold.ttf")).unwrap();
        assert!(std::sync::Arc::ptr_eq(&ui, &page.regular));
        let fresh = Font::from_bytes(crate::MONO_FONT, FontSettings::default()).unwrap();
        let face = rustybuzz::ttf_parser::Face::parse(crate::MONO_FONT, 0).unwrap();
        let mut count = 0;
        for sub in face.tables().cmap.unwrap().subtables {
            sub.codepoints(|cp| {
                if let Some(ch) = char::from_u32(cp) {
                    assert_eq!(ui.lookup_glyph_index(ch), fresh.lookup_glyph_index(ch));
                    for px in [12.0, 18.0, 24.0, 32.0] {
                        assert_eq!(ui.rasterize(ch, px), fresh.rasterize(ch, px));
                    }
                    count += 1;
                }
            });
        }
        assert!(count > 1000);
    }
}
