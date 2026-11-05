//! PROTOTYPE (isolated branch, not adopted): bundled Regular + Bold native
//! font pair behind one sticky-error session.
//!
//! Faces: the licensed `AtkinsonHyperlegible` Regular ([`crate::TEXT_FONT`])
//! and `AtkinsonHyperlegible` Bold from `fonts/` (both OFL 1.1, see
//! LICENSE-AtkinsonHyperlegible.txt). Each weight is its OWN real face: a
//! bold request is never answered with regular metrics or bitmaps, and a
//! missing face refuses. This is NOT the canonical masked profile: the
//! canonical scorer keeps its own face set and is not replaced by this.
//!
//! Scope and honest limits, enforced as typed refusals (never a quiet
//! ASCII-only completion): ASCII graphic + SPACE + NBSP only. Any other
//! character (non-Latin, accented Latin, emoji, controls) refuses with
//! [`NativeGlyphError::ShapingRefused`], first error sticky for the whole
//! preparation. No shaping, no ligatures, no kerning, no fallback chain,
//! no complete-Unicode claim. Extension point: replace `check_character`
//! with a per-face coverage plus fallback policy and a shaper; the pair
//! type and the sticky/caps contract stay.
//!
//! Caps: the same strict diagnostic caps apply to the PAIR AS A WHOLE
//! (total calls across both faces, counting every attempt including those
//! after a terminal error, and total generated mask bytes across both
//! faces) and, as an inner guard, to each face session separately. The
//! pair's aggregate is therefore no larger than one regular session's.
//! They are not a device budget.

use crate::diagnostic_font_session::{
    NativeFontLimits, NativeFontSession, NativeFontSnapshot, NativeGlyph, NativeGlyphError,
};
use std::cell::Cell;

/// Licensed bold face bytes (OFL 1.1).
pub const BOLD_FONT: &[u8] = include_bytes!("../fonts/AtkinsonHyperlegible-Bold.ttf");

pub struct BundledFacePair {
    regular: NativeFontSession,
    bold: NativeFontSession,
    limits: NativeFontLimits,
    calls: Cell<u64>,
    first_error: Cell<Option<NativeGlyphError>>,
}

impl BundledFacePair {
    /// # Errors
    /// [`NativeGlyphError::Construction`] when either face does not parse;
    /// [`NativeGlyphError::ExcessiveLimits`] when a cap exceeds its ceiling.
    pub fn new() -> Result<Self, NativeGlyphError> {
        Self::with_limits(NativeFontLimits::default())
    }

    /// # Errors
    /// As [`Self::new`]. Tightening only.
    pub fn with_limits(limits: NativeFontLimits) -> Result<Self, NativeGlyphError> {
        let regular = NativeFontSession::with_limits(limits)?;
        let bold = NativeFontSession::construct_from(BOLD_FONT, limits)?;
        Ok(Self {
            regular,
            bold,
            limits,
            calls: Cell::new(0),
            first_error: Cell::new(None),
        })
    }

    fn face(&self, bold: bool) -> &NativeFontSession {
        if bold {
            &self.bold
        } else {
            &self.regular
        }
    }

    fn record(&self, error: NativeGlyphError) -> NativeGlyphError {
        if let Some(first) = self.first_error.get() {
            first
        } else {
            self.first_error.set(Some(error));
            error
        }
    }

    fn sticky<T>(
        &self,
        result: impl FnOnce() -> Result<T, NativeGlyphError>,
    ) -> Result<T, NativeGlyphError> {
        // Every attempt counts, including those after a terminal error.
        self.calls.set(self.calls.get().saturating_add(1));
        if let Some(error) = self.first_error.get() {
            return Err(error);
        }
        if self.calls.get() > self.limits.max_calls {
            return Err(self.record(NativeGlyphError::CallBudget));
        }
        let value = result().map_err(|error| self.record(error))?;
        let total = self
            .regular
            .snapshot()
            .generated_mask_bytes
            .saturating_add(self.bold.snapshot().generated_mask_bytes);
        if total > self.limits.max_total_glyph_bytes {
            return Err(self.record(NativeGlyphError::TotalGlyphByteBudget));
        }
        Ok(value)
    }

    /// # Errors
    /// See [`NativeGlyphError`]; first error sticky across both faces.
    pub fn advance(&self, c: char, px: u32, bold: bool) -> Result<u32, NativeGlyphError> {
        self.sticky(|| self.face(bold).advance(c, px, false))
    }

    /// # Errors
    /// See [`NativeGlyphError`]; first error sticky across both faces.
    pub fn line_height(&self, px: u32, bold: bool) -> Result<u32, NativeGlyphError> {
        self.sticky(|| self.face(bold).line_height(px, false))
    }

    /// # Errors
    /// See [`NativeGlyphError`]; first error sticky across both faces.
    pub fn baseline_offset(&self, px: u32, bold: bool) -> Result<i32, NativeGlyphError> {
        self.sticky(|| self.face(bold).baseline_offset(px, false))
    }

    /// Rasterizes one glyph. The remaining PAIR byte budget is passed into
    /// the face's own preflight, so an over-budget glyph is refused BEFORE any
    /// buffer is generated and generated bytes never exceed the cap.
    ///
    /// # Errors
    /// See [`NativeGlyphError`]; first error sticky across both faces.
    pub fn raster(&self, c: char, px: u32, bold: bool) -> Result<NativeGlyph, NativeGlyphError> {
        self.sticky(|| {
            let used = self
                .regular
                .snapshot()
                .generated_mask_bytes
                .saturating_add(self.bold.snapshot().generated_mask_bytes);
            let remaining = self.limits.max_total_glyph_bytes.saturating_sub(used);
            self.face(bold).raster_within(c, px, false, remaining)
        })
    }

    #[must_use]
    pub fn snapshot(&self) -> NativeFontSnapshot {
        let (a, b) = (self.regular.snapshot(), self.bold.snapshot());
        NativeFontSnapshot {
            calls: self.calls.get(),
            generated_mask_bytes: a
                .generated_mask_bytes
                .saturating_add(b.generated_mask_bytes),
            first_error: self.first_error.get(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> BundledFacePair {
        BundledFacePair::new().expect("bundled faces parse")
    }

    #[test]
    fn bold_is_its_own_face_not_regular_metrics() {
        let p = pair();
        let mut differs = 0;
        for c in ['A', 'g', 'W', 'm'] {
            let r = p.raster(c, 24, false).unwrap();
            let b = p.raster(c, 24, true).unwrap();
            if r.coverage != b.coverage {
                differs += 1;
            }
        }
        assert_eq!(differs, 4, "every sampled bold glyph differs from regular");
        assert!(
            p.advance('m', 24, true).unwrap() >= p.advance('m', 24, false).unwrap(),
            "bold advance is the bold face advance"
        );
        assert_eq!(p.snapshot().first_error, None);
    }

    #[test]
    fn bold_matches_fontdue_on_the_bold_bytes() {
        let p = pair();
        let reference =
            fontdue::Font::from_bytes(BOLD_FONT, fontdue::FontSettings::default()).unwrap();
        for px in [12u32, 16, 33] {
            for c in ['A', 'g', 'j', '1'] {
                let g = p.raster(c, px, true).unwrap();
                let (m, cov) = reference.rasterize(c, px as f32);
                assert_eq!(
                    (g.width as usize, g.height as usize, g.left),
                    (m.width, m.height, m.xmin)
                );
                assert_eq!(g.coverage, cov, "{c:?}@{px}");
                assert_eq!(i64::from(g.advance), m.advance_width.round() as i64);
            }
        }
    }

    #[test]
    fn non_ascii_and_non_latin_refuse_typed_and_stick() {
        for c in ['é', 'я', '日', '\u{1F600}', '\n', '\u{200D}'] {
            let p = pair();
            assert_eq!(
                p.advance(c, 16, false),
                Err(NativeGlyphError::ShapingRefused),
                "{c:?}"
            );
            assert_eq!(
                p.advance('A', 16, true),
                Err(NativeGlyphError::ShapingRefused),
                "sticky"
            );
            assert_eq!(
                p.snapshot().first_error,
                Some(NativeGlyphError::ShapingRefused)
            );
        }
        // refusal on the bold face too
        let p = pair();
        assert_eq!(
            p.raster('日', 16, true),
            Err(NativeGlyphError::ShapingRefused)
        );
    }

    #[test]
    fn caps_tighten_only_and_apply_to_the_pair_as_a_whole() {
        let mut limits = NativeFontLimits::default();
        limits.max_total_glyph_bytes += 1;
        assert!(matches!(
            BundledFacePair::with_limits(limits),
            Err(NativeGlyphError::ExcessiveLimits)
        ));
        let tight = NativeFontLimits {
            max_calls: 3,
            ..NativeFontLimits::default()
        };
        let p = BundledFacePair::with_limits(tight).unwrap();
        assert!(p.advance('A', 16, false).is_ok());
        assert!(p.advance('A', 16, true).is_ok());
        assert!(p.advance('A', 16, true).is_ok());
        assert_eq!(p.advance('A', 16, true), Err(NativeGlyphError::CallBudget));
        assert_eq!(p.advance('A', 16, false), Err(NativeGlyphError::CallBudget));
        // every attempt counted, including the post-terminal ones
        assert_eq!(p.snapshot().calls, 5);
    }

    #[test]
    fn aggregate_mask_bytes_are_capped_across_both_faces() {
        let one = BundledFacePair::new()
            .unwrap()
            .raster('W', 64, false)
            .unwrap();
        let bytes = one.coverage.len();
        let tight = NativeFontLimits {
            max_total_glyph_bytes: bytes + bytes / 2,
            ..NativeFontLimits::default()
        };
        let p = BundledFacePair::with_limits(tight).unwrap();
        assert!(p.raster('W', 64, false).is_ok());
        assert_eq!(
            p.raster('W', 64, true),
            Err(NativeGlyphError::TotalGlyphByteBudget),
            "bold bytes count against the same total"
        );
        // PREFLIGHT: the refused bold glyph generated NOTHING; generated bytes
        // never exceed the pair cap and equal exactly the one admitted glyph.
        let snap = p.snapshot();
        assert!(snap.generated_mask_bytes <= bytes + bytes / 2);
        assert_eq!(snap.generated_mask_bytes, bytes);
        assert_eq!(
            p.advance('A', 16, false),
            Err(NativeGlyphError::TotalGlyphByteBudget)
        );
    }

    #[test]
    fn overflow_refusal_never_generates_bytes_in_either_order() {
        let bytes = BundledFacePair::new()
            .unwrap()
            .raster('W', 64, true)
            .unwrap()
            .coverage
            .len();
        let cap = bytes + bytes / 2;
        for first_bold in [false, true] {
            let p = BundledFacePair::with_limits(NativeFontLimits {
                max_total_glyph_bytes: cap,
                ..NativeFontLimits::default()
            })
            .unwrap();
            assert!(p.raster('W', 64, first_bold).is_ok());
            let before = p.snapshot().generated_mask_bytes;
            assert_eq!(
                p.raster('W', 64, !first_bold),
                Err(NativeGlyphError::TotalGlyphByteBudget)
            );
            assert_eq!(p.snapshot().generated_mask_bytes, before);
            assert!(p.snapshot().generated_mask_bytes <= cap);
        }
    }

    #[test]
    fn pair_cap_boundary_is_exact() {
        let bytes = BundledFacePair::new()
            .unwrap()
            .raster('W', 64, false)
            .unwrap()
            .coverage
            .len();
        let at = |cap: usize| {
            BundledFacePair::with_limits(NativeFontLimits {
                max_total_glyph_bytes: cap,
                ..NativeFontLimits::default()
            })
            .unwrap()
        };
        // cap == bytes admits exactly the one glyph and fills the budget
        let p = at(bytes);
        assert!(p.raster('W', 64, false).is_ok());
        assert_eq!(p.snapshot().generated_mask_bytes, bytes);
        // cap == bytes - 1 refuses it with nothing generated
        let q = at(bytes - 1);
        assert_eq!(
            q.raster('W', 64, false),
            Err(NativeGlyphError::TotalGlyphByteBudget)
        );
        assert_eq!(q.snapshot().generated_mask_bytes, 0);
    }
}
