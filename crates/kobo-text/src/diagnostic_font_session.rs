//! TEST/DIAGNOSTIC ONLY: a native font session for the paint-proof lane.
//!
//! Built from the licensed AtkinsonHyperlegible-Regular bytes compiled into
//! this crate ([`crate::TEXT_FONT`], OFL 1.1). This is the same native
//! implementation (fontdue data and raster path), NOT the installed
//! typesetter instance, which is unreachable behind the boxed global. The
//! NORMAL, no-feature graph carries nothing of this module; the browser
//! reaches it through a dev-dependency feature and a `cfg(test)` adapter.
//!
//! Contract (114 v3 + 115 review): integer CSS px only, no ambient size
//! conversion; ASCII graphic plus SPACE/NBSP only; regular only (bold is one
//! failed operation, sticky and terminal); real fontdue metrics with no
//! fallback; per-glyph rounded advance, not reader run accumulation; strict
//! host caps that are NOT a target budget and may only ever be tightened,
//! never raised; no cache; first error sticky and terminal for the whole
//! preparation.

use fontdue::{Font, FontSettings};
use std::cell::Cell;

/// Largest accepted integer pixel size (host diagnostic cap).
pub const MAX_PX: u32 = 256;
/// Largest accepted glyph bitmap edge (host diagnostic cap).
pub const MAX_GLYPH_DIMENSION: u32 = 512;
/// Largest accepted per-glyph mask size in bytes (host diagnostic cap).
pub const MAX_GLYPH_MASK_BYTES: usize = 262_144;
/// Total public method calls, success or failure, across all four methods.
pub const MAX_TOTAL_CALLS: u64 = 65_536;
/// Cumulative generated mask bytes per session (host diagnostic cap derived
/// from the engine's `MAX_GLYPH_BYTES`; a different cap from `MAX_PIXELS`,
/// which bounds surface area, not bytes). Intentionally strict: not a
/// display-retention or device policy.
pub const MAX_TOTAL_GLYPH_BYTES: usize = 1_048_576;

/// 2^32 as f32, exactly representable: the exclusive upper bound for any
/// value that must fit a u32.
const U32_BOUND: f32 = 4_294_967_296.0;
/// 2^31 as f32, exactly representable: the EXCLUSIVE upper bound for any
/// value cast to i32. `i32::MAX` (2^31 - 1) is not representable as f32 -
/// the literal rounds UP to 2^31 - so an inclusive check against it would
/// admit exactly 2^31, whose cast then saturates.
const I32_BOUND: f32 = 2_147_483_648.0;

/// Pure boundary predicate: a baseline is castable to i32 only when finite
/// and inside `0..2^31` (exclusive). Kept free of session state so the
/// boundary tests exercise the exact predicate directly.
fn baseline_fits_i32(baseline: f32) -> bool {
    baseline.is_finite() && (0.0..I32_BOUND).contains(&baseline)
}

/// Why a session operation failed. The first failure is sticky: it ends the
/// whole preparation and is returned by every later call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeGlyphError {
    /// More than the configured total public calls were attempted.
    CallBudget,
    /// px outside 1..=[`MAX_PX`].
    InvalidSize,
    /// Character outside the v1 profile: ASCII graphic, SPACE or NBSP only.
    ShapingRefused,
    /// The face carries no glyph for the character (glyph index 0).
    MissingGlyph,
    /// The face reported no line metrics; no fallback is invented.
    NoLineMetrics,
    /// A metric was non-finite or unrepresentable, or the computed height
    /// was not positive, or the baseline fell outside 0..height.
    NonFiniteMetric,
    /// The raw or rounded advance was not positive or not representable.
    InvalidAdvance,
    /// A glyph bitmap edge exceeded the dimension cap.
    DimensionBudget,
    /// A glyph mask exceeded the per-glyph byte cap.
    MaskByteBudget,
    /// Cumulative mask bytes would exceed the session total.
    TotalGlyphByteBudget,
    /// The rasterized result disagreed with its preflight metrics.
    InvalidBitmap,
    /// A diagnostic cap was set above its hard ceiling; caps may only
    /// tighten, never raise the host limits.
    ExcessiveLimits,
    /// Bold was requested; the v1 session is regular only. One failed
    /// operation, no glyph work performed.
    BoldUnsupported,
    /// The face bytes did not parse.
    Construction,
}

/// One rasterized glyph: ink bounds and row-major 8-bit coverage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeGlyph {
    /// fontdue `xmin`.
    pub left: i32,
    /// `-(ymin + height)`, checked arithmetic.
    pub top: i32,
    pub width: u32,
    pub height: u32,
    /// Per-glyph rounded advance; not reader run accumulation.
    pub advance: u32,
    /// Row-major coverage; length equals `width * height`.
    pub coverage: Vec<u8>,
}

/// Session state at a moment, for test assertions. Reading it is free: it
/// is outside the four-method call budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeFontSnapshot {
    pub calls: u64,
    pub generated_mask_bytes: usize,
    pub first_error: Option<NativeGlyphError>,
}

/// The session's caps. Defaults are the host diagnostic constants; smaller
/// values exist so boundary tests can reach them. The public constructor
/// TIGHTENS only: any cap above its hard ceiling is rejected before
/// construction. Diagnostic construction only: production code never sees
/// this module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeFontLimits {
    pub max_calls: u64,
    pub max_glyph_dimension: u32,
    pub max_glyph_mask_bytes: usize,
    pub max_total_glyph_bytes: usize,
}

impl Default for NativeFontLimits {
    fn default() -> Self {
        Self {
            max_calls: MAX_TOTAL_CALLS,
            max_glyph_dimension: MAX_GLYPH_DIMENSION,
            max_glyph_mask_bytes: MAX_GLYPH_MASK_BYTES,
            max_total_glyph_bytes: MAX_TOTAL_GLYPH_BYTES,
        }
    }
}

impl NativeFontLimits {
    /// Tightening check: EACH cap must be at or below its hard ceiling.
    /// Pure and font-free, so rejection is exercised before any
    /// construction or document work.
    fn tightening_violation(&self) -> Option<NativeGlyphError> {
        let ceilings = Self::default();
        if self.max_calls > ceilings.max_calls
            || self.max_glyph_dimension > ceilings.max_glyph_dimension
            || self.max_glyph_mask_bytes > ceilings.max_glyph_mask_bytes
            || self.max_total_glyph_bytes > ceilings.max_total_glyph_bytes
        {
            Some(NativeGlyphError::ExcessiveLimits)
        } else {
            None
        }
    }
}

/// A regular-only native font session over the compiled-in licensed bytes.
/// The face is immutable and stateless; call counters and the sticky error
/// are single-threaded `Cell` session state. No cache.
pub struct NativeFontSession {
    face: Font,
    limits: NativeFontLimits,
    calls: Cell<u64>,
    generated_mask_bytes: Cell<usize>,
    first_error: Cell<Option<NativeGlyphError>>,
}

impl NativeFontSession {
    /// Constructs the session from the licensed bytes with the host
    /// diagnostic caps. Construction is outside the call budget; its memory
    /// is real but unmeasured (stated, not hidden).
    ///
    /// # Errors
    /// [`NativeGlyphError::Construction`] when the bytes do not parse.
    pub fn new() -> Result<Self, NativeGlyphError> {
        Self::with_limits(NativeFontLimits::default())
    }

    /// TIGHTENING-ONLY diagnostic construction with explicit caps: every
    /// cap must be at or below its hard ceiling, and any cap above it is
    /// rejected BEFORE the font is constructed or any document is touched.
    /// Public only because the cross-crate browser adapter (dev-dependency
    /// feature) needs lower caps in its own tests; in-crate boundary tests
    /// use the private constructor instead. Outside the call budget.
    ///
    /// # Errors
    /// [`NativeGlyphError::ExcessiveLimits`] when any cap exceeds its hard
    /// ceiling; [`NativeGlyphError::Construction`] when the bytes do not
    /// parse.
    pub fn with_limits(limits: NativeFontLimits) -> Result<Self, NativeGlyphError> {
        if let Some(error) = limits.tightening_violation() {
            return Err(error);
        }
        Self::construct(limits)
    }

    /// Private construction with no tightening check: the in-crate test
    /// override for states the public API refuses on purpose (for example
    /// a `u64::MAX` call cap, to prove the counter saturates at the real
    /// boundary rather than being extrapolated from a small cap).
    fn construct(limits: NativeFontLimits) -> Result<Self, NativeGlyphError> {
        Self::construct_from(crate::TEXT_FONT, limits)
    }

    /// Same construction over caller-chosen licensed face bytes. Crate
    /// private: only the bundled pair prototype names the Bold bytes.
    pub(crate) fn construct_from(
        bytes: &[u8],
        limits: NativeFontLimits,
    ) -> Result<Self, NativeGlyphError> {
        let face = Font::from_bytes(bytes, FontSettings::default())
            .map_err(|_| NativeGlyphError::Construction)?;
        Ok(Self {
            face,
            limits,
            calls: Cell::new(0),
            generated_mask_bytes: Cell::new(0),
            first_error: Cell::new(None),
        })
    }

    /// Test-only counter seed, so saturation is exercised at the real u64
    /// boundary instead of inferred from a handful of calls.
    #[cfg(test)]
    fn seed_calls(&self, calls: u64) {
        self.calls.set(calls);
    }

    /// Session state for test assertions. Outside the call budget.
    #[must_use]
    pub fn snapshot(&self) -> NativeFontSnapshot {
        NativeFontSnapshot {
            calls: self.calls.get(),
            generated_mask_bytes: self.generated_mask_bytes.get(),
            first_error: self.first_error.get(),
        }
    }

    /// Counts one public operation first, then enforces sticky termination
    /// and the call budget. Post-terminal calls do no work and no
    /// allocation; the count saturates; the first error is preserved. All
    /// four methods share this one budget and this one sticky slot.
    fn enter(&self) -> Result<(), NativeGlyphError> {
        self.calls.set(self.calls.get().saturating_add(1));
        if let Some(error) = self.first_error.get() {
            return Err(error);
        }
        if self.calls.get() > self.limits.max_calls {
            return Err(self.fail(NativeGlyphError::CallBudget));
        }
        Ok(())
    }

    /// Records the first error and returns the sticky one.
    fn fail(&self, error: NativeGlyphError) -> NativeGlyphError {
        if let Some(first) = self.first_error.get() {
            first
        } else {
            self.first_error.set(Some(error));
            error
        }
    }

    fn check_size(&self, px: u32) -> Result<(), NativeGlyphError> {
        if (1..=MAX_PX).contains(&px) {
            Ok(())
        } else {
            Err(self.fail(NativeGlyphError::InvalidSize))
        }
    }

    fn check_character(&self, character: char) -> Result<(), NativeGlyphError> {
        let ok = character.is_ascii_graphic() || character == ' ' || character == '\u{00A0}';
        if ok {
            Ok(())
        } else {
            Err(self.fail(NativeGlyphError::ShapingRefused))
        }
    }

    fn check_bold(&self, bold: bool) -> Result<(), NativeGlyphError> {
        if bold {
            // One failed operation, no glyph work performed.
            Err(self.fail(NativeGlyphError::BoldUnsupported))
        } else {
            Ok(())
        }
    }

    fn check_coverage(&self, character: char) -> Result<(), NativeGlyphError> {
        if self.face.lookup_glyph_index(character) != 0 {
            Ok(())
        } else {
            Err(self.fail(NativeGlyphError::MissingGlyph))
        }
    }

    /// The rounded per-glyph advance: the RAW value must be finite,
    /// positive and below the u32 bound; the ROUNDED value must also be
    /// positive and representable before the cast. Identical checks on
    /// every path that yields an advance.
    fn checked_advance(&self, advance_width: f32) -> Result<u32, NativeGlyphError> {
        if !advance_width.is_finite() {
            return Err(self.fail(NativeGlyphError::NonFiniteMetric));
        }
        if advance_width <= 0.0 || advance_width >= U32_BOUND {
            return Err(self.fail(NativeGlyphError::InvalidAdvance));
        }
        let rounded = advance_width.round();
        if !(1.0..U32_BOUND).contains(&rounded) {
            return Err(self.fail(NativeGlyphError::InvalidAdvance));
        }
        // Checked finite, positive and representable above.
        u32::try_from(rounded as i64).map_err(|_| self.fail(NativeGlyphError::InvalidAdvance))
    }

    /// Line metrics with no fallback: every metric finite, ceil height over
    /// the signed descent, positive and representable BEFORE the cast;
    /// ceil baseline checked inside 0..height and representable as i32
    /// BEFORE the cast.
    fn line_metrics(&self, px: u32) -> Result<(u32, i32), NativeGlyphError> {
        let line = self
            .face
            .horizontal_line_metrics(px as f32)
            .ok_or_else(|| self.fail(NativeGlyphError::NoLineMetrics))?;
        if !(line.ascent.is_finite() && line.descent.is_finite() && line.line_gap.is_finite()) {
            return Err(self.fail(NativeGlyphError::NonFiniteMetric));
        }
        // Signed descent: fontdue reports descent below the baseline as a
        // negative value, so the natural height is ascent - descent + gap.
        let height = (line.ascent - line.descent + line.line_gap).ceil();
        let baseline = line.ascent.ceil();
        if !(height.is_finite() && baseline.is_finite()) {
            return Err(self.fail(NativeGlyphError::NonFiniteMetric));
        }
        if height <= 0.0 || height >= U32_BOUND {
            return Err(self.fail(NativeGlyphError::NonFiniteMetric));
        }
        if !baseline_fits_i32(baseline) || baseline >= height {
            return Err(self.fail(NativeGlyphError::NonFiniteMetric));
        }
        // Checked finite and representable above.
        let height = u32::try_from(height as i64)
            .map_err(|_| self.fail(NativeGlyphError::NonFiniteMetric))?;
        Ok((height, baseline as i32))
    }

    /// Per-glyph advance in integer CSS px.
    ///
    /// # Errors
    /// See [`NativeGlyphError`]; the first error is sticky and terminal.
    pub fn advance(&self, character: char, px: u32, bold: bool) -> Result<u32, NativeGlyphError> {
        self.enter()?;
        self.check_bold(bold)?;
        self.check_size(px)?;
        self.check_character(character)?;
        self.check_coverage(character)?;
        let metrics = self.face.metrics(character, px as f32);
        self.checked_advance(metrics.advance_width)
    }

    /// Natural line height: ceil(ascent - descent + `line_gap`).
    ///
    /// # Errors
    /// See [`NativeGlyphError`]; the first error is sticky and terminal.
    pub fn line_height(&self, px: u32, bold: bool) -> Result<u32, NativeGlyphError> {
        self.enter()?;
        self.check_bold(bold)?;
        self.check_size(px)?;
        Ok(self.line_metrics(px)?.0)
    }

    /// Distance from the top of the line to the baseline: ceil(ascent),
    /// checked inside 0..height.
    ///
    /// # Errors
    /// See [`NativeGlyphError`]; the first error is sticky and terminal.
    pub fn baseline_offset(&self, px: u32, bold: bool) -> Result<i32, NativeGlyphError> {
        self.enter()?;
        self.check_bold(bold)?;
        self.check_size(px)?;
        Ok(self.line_metrics(px)?.1)
    }

    /// Rasterizes one glyph. Every refusal check runs BEFORE the byte
    /// reservation and before rasterize; the reservation is made before
    /// rasterize and never refunded; the returned metrics are verified
    /// against the preflight ones - all of width, height, xmin, ymin and
    /// advance - and the offsets derive from the verified values.
    ///
    /// # Errors
    /// See [`NativeGlyphError`]; the first error is sticky and terminal.
    pub fn raster(
        &self,
        character: char,
        px: u32,
        bold: bool,
    ) -> Result<NativeGlyph, NativeGlyphError> {
        self.raster_within(character, px, bold, usize::MAX)
    }

    /// [`Self::raster`] with an extra caller budget: `remaining` is how many
    /// more mask bytes the CALLER can still accept. The check runs in the
    /// same preflight as the session caps, BEFORE the byte reservation and
    /// before any buffer is generated, so a refused glyph allocates nothing.
    ///
    /// # Errors
    /// As [`Self::raster`]; [`NativeGlyphError::TotalGlyphByteBudget`] when
    /// the mask would exceed `remaining`.
    pub(crate) fn raster_within(
        &self,
        character: char,
        px: u32,
        bold: bool,
        remaining: usize,
    ) -> Result<NativeGlyph, NativeGlyphError> {
        self.enter()?;
        self.check_bold(bold)?;
        self.check_size(px)?;
        self.check_character(character)?;
        self.check_coverage(character)?;
        let preflight = self.face.metrics(character, px as f32);
        let advance = self.checked_advance(preflight.advance_width)?;
        let dimension = self.limits.max_glyph_dimension as usize;
        if preflight.width > dimension || preflight.height > dimension {
            return Err(self.fail(NativeGlyphError::DimensionBudget));
        }
        let mask_bytes = preflight
            .width
            .checked_mul(preflight.height)
            .ok_or_else(|| self.fail(NativeGlyphError::MaskByteBudget))?;
        if mask_bytes > self.limits.max_glyph_mask_bytes {
            return Err(self.fail(NativeGlyphError::MaskByteBudget));
        }
        let generated = self.generated_mask_bytes.get();
        if mask_bytes > remaining
            || generated
                .checked_add(mask_bytes)
                .is_none_or(|total| total > self.limits.max_total_glyph_bytes)
        {
            return Err(self.fail(NativeGlyphError::TotalGlyphByteBudget));
        }
        // Reserved before rasterize; never refunded, even if verification
        // below fails.
        self.generated_mask_bytes.set(generated + mask_bytes);
        let (metrics, coverage) = self.face.rasterize(character, px as f32);
        if metrics.width != preflight.width
            || metrics.height != preflight.height
            || metrics.xmin != preflight.xmin
            || metrics.ymin != preflight.ymin
            || metrics.advance_width.to_bits() != preflight.advance_width.to_bits()
            || coverage.len() != mask_bytes
        {
            return Err(self.fail(NativeGlyphError::InvalidBitmap));
        }
        let top = i32::try_from(metrics.height)
            .ok()
            .and_then(|height| metrics.ymin.checked_add(height))
            .and_then(i32::checked_neg)
            .ok_or_else(|| self.fail(NativeGlyphError::InvalidBitmap))?;
        Ok(NativeGlyph {
            left: metrics.xmin,
            top,
            width: u32::try_from(metrics.width)
                .map_err(|_| self.fail(NativeGlyphError::InvalidBitmap))?,
            height: u32::try_from(metrics.height)
                .map_err(|_| self.fail(NativeGlyphError::InvalidBitmap))?,
            advance,
            coverage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> NativeFontSession {
        NativeFontSession::new().expect("the compiled-in licensed bytes parse")
    }

    fn reference() -> Font {
        Font::from_bytes(crate::TEXT_FONT, FontSettings::default()).expect("font")
    }

    fn limited(update: impl Fn(&mut NativeFontLimits)) -> NativeFontSession {
        let mut limits = NativeFontLimits::default();
        update(&mut limits);
        NativeFontSession::with_limits(limits).expect("font")
    }

    /// The reference is fontdue itself with the recorded default settings:
    /// every session metric must equal the value fontdue reports for the
    /// same bytes, size and character, under the contract's rounding.
    #[test]
    fn advance_and_metrics_match_fontdue_at_fixed_px() {
        let session = session();
        let reference = reference();
        for px in [8, 16, 33, 256] {
            for ch in ['A', 'x', '1', ' '] {
                let metrics = reference.metrics(ch, px as f32);
                let expected = metrics.advance_width.round();
                assert!(expected > 0.0, "fixture assumption: positive advance");
                assert_eq!(
                    session.advance(ch, px, false).expect("advance"),
                    u32::try_from(expected as i64).expect("fixture advance fits"),
                    "advance mismatch for {ch:?} at {px}px"
                );
                let line = reference
                    .horizontal_line_metrics(px as f32)
                    .expect("line metrics");
                let height = (line.ascent - line.descent + line.line_gap).ceil();
                assert_eq!(
                    session.line_height(px, false).expect("line height"),
                    u32::try_from(height as i64).expect("fixture height fits"),
                    "height mismatch at {px}px"
                );
                assert_eq!(
                    session.baseline_offset(px, false).expect("baseline"),
                    line.ascent.ceil() as i32,
                    "baseline mismatch at {px}px"
                );
            }
        }
        let snap = session.snapshot();
        // 4 sizes * 4 chars * 3 methods.
        assert_eq!(snap.calls, 48);
        assert_eq!(snap.first_error, None);
    }

    /// Full mask and offset identity with fontdue across several sizes and
    /// shapes: cap-height, descender, dot-and-descender, space, NBSP.
    #[test]
    fn raster_masks_and_offsets_match_fontdue_across_px_and_shapes() {
        let session = session();
        let reference = reference();
        for px in [8, 16, 24, 96, 256] {
            for ch in ['A', 'g', 'j', ' ', '\u{00A0}'] {
                let glyph = session.raster(ch, px, false).expect("raster");
                let (metrics, coverage) = reference.rasterize(ch, px as f32);
                assert_eq!(glyph.left, metrics.xmin, "left for {ch:?} at {px}px");
                assert_eq!(
                    glyph.top,
                    -(metrics.ymin + i32::try_from(metrics.height).expect("dims")),
                    "top for {ch:?} at {px}px"
                );
                assert_eq!(
                    glyph.width as usize, metrics.width,
                    "width for {ch:?} at {px}px"
                );
                assert_eq!(
                    glyph.height as usize, metrics.height,
                    "height for {ch:?} at {px}px"
                );
                assert_eq!(glyph.coverage, coverage, "coverage for {ch:?} at {px}px");
                assert_eq!(
                    glyph.advance,
                    u32::try_from(metrics.advance_width.round() as i64).expect("fits"),
                    "advance for {ch:?} at {px}px"
                );
            }
        }
    }

    #[test]
    fn space_is_a_zero_mask_with_positive_advance_and_nbsp_is_accepted() {
        let session = session();
        let space = session.raster(' ', 16, false).expect("space");
        assert_eq!((space.width, space.height), (0, 0));
        assert!(space.coverage.is_empty());
        assert!(space.advance > 0);
        assert!(session.advance('\u{00A0}', 16, false).expect("nbsp") > 0);
    }

    #[test]
    fn bold_is_one_failed_operation_and_terminal() {
        let session = session();
        assert_eq!(
            session.advance('A', 16, true),
            Err(NativeGlyphError::BoldUnsupported)
        );
        let snap = session.snapshot();
        assert_eq!(snap.calls, 1);
        assert_eq!(snap.first_error, Some(NativeGlyphError::BoldUnsupported));
        // Terminal: no further work, first error preserved.
        assert_eq!(
            session.advance('A', 16, false),
            Err(NativeGlyphError::BoldUnsupported)
        );
        assert_eq!(
            session.raster('A', 16, false),
            Err(NativeGlyphError::BoldUnsupported)
        );
        assert_eq!(
            session.line_height(16, false),
            Err(NativeGlyphError::BoldUnsupported)
        );
        assert_eq!(
            session.baseline_offset(16, false),
            Err(NativeGlyphError::BoldUnsupported)
        );
        assert_eq!(session.snapshot().generated_mask_bytes, 0);
    }

    #[test]
    fn invalid_size_and_shaping_fail_with_their_reasons() {
        assert_eq!(
            session().advance('A', 0, false),
            Err(NativeGlyphError::InvalidSize)
        );
        assert_eq!(
            session().advance('A', MAX_PX + 1, false),
            Err(NativeGlyphError::InvalidSize)
        );
        assert_eq!(
            session().line_height(0, false),
            Err(NativeGlyphError::InvalidSize)
        );
        assert_eq!(
            session().advance('\t', 16, false),
            Err(NativeGlyphError::ShapingRefused)
        );
        assert_eq!(
            session().advance('\u{0301}', 16, false),
            Err(NativeGlyphError::ShapingRefused)
        );
        assert_eq!(
            session().advance('\n', 16, false),
            Err(NativeGlyphError::ShapingRefused)
        );
    }

    #[test]
    fn a_character_the_face_lacks_is_missing_glyph() {
        // Atkinson Hyperlegible carries no U+2011 non-breaking hyphen; the
        // private coverage check names it directly (the public gate refuses
        // it earlier as shaping-sensitive).
        let session = session();
        assert_eq!(
            session.check_coverage('\u{2011}'),
            Err(NativeGlyphError::MissingGlyph)
        );
    }

    #[test]
    fn sticky_first_error_wins_across_all_methods() {
        let session = session();
        assert_eq!(
            session.line_height(0, false),
            Err(NativeGlyphError::InvalidSize)
        );
        // Different failure classes and different methods still report the
        // first error, and no generation happens after termination.
        assert_eq!(
            session.advance('A', 16, false),
            Err(NativeGlyphError::InvalidSize)
        );
        assert_eq!(
            session.raster('A', 16, false),
            Err(NativeGlyphError::InvalidSize)
        );
        assert_eq!(
            session.baseline_offset(16, false),
            Err(NativeGlyphError::InvalidSize)
        );
        assert_eq!(session.snapshot().generated_mask_bytes, 0);
    }

    /// The public constructor TIGHTENS only: exact ceilings are accepted,
    /// each cap one over its hard ceiling (and a `u64::MAX` call cap, which
    /// could never be safely exhausted) is rejected BEFORE any font
    /// construction. Tightened caps in every field at once are accepted.
    #[test]
    fn with_limits_accepts_exact_ceilings_and_rejects_each_cap_one_over() {
        let ceilings = NativeFontLimits::default();
        NativeFontSession::with_limits(ceilings).expect("exact ceilings accepted");
        NativeFontSession::with_limits(NativeFontLimits {
            max_calls: 1,
            max_glyph_dimension: 1,
            max_glyph_mask_bytes: 1,
            max_total_glyph_bytes: 1,
        })
        .expect("tightened in every field");
        for limits in [
            NativeFontLimits {
                max_calls: ceilings.max_calls + 1,
                ..ceilings
            },
            NativeFontLimits {
                max_calls: u64::MAX,
                ..ceilings
            },
            NativeFontLimits {
                max_glyph_dimension: ceilings.max_glyph_dimension + 1,
                ..ceilings
            },
            NativeFontLimits {
                max_glyph_mask_bytes: ceilings.max_glyph_mask_bytes + 1,
                ..ceilings
            },
            NativeFontLimits {
                max_total_glyph_bytes: ceilings.max_total_glyph_bytes + 1,
                ..ceilings
            },
        ] {
            assert!(
                matches!(
                    NativeFontSession::with_limits(limits),
                    Err(NativeGlyphError::ExcessiveLimits)
                ),
                "cap above the hard ceiling must be rejected: {limits:?}"
            );
        }
    }

    /// The counter saturates at the REAL u64 boundary: seeded one below
    /// `u64::MAX` through the private test override (the public tightening
    /// constructor rejects this cap on purpose), one call reaches the
    /// maximum, and later calls pin the counter there instead of wrapping
    /// to zero - before and after a terminal error.
    #[test]
    fn call_counter_saturates_at_the_real_u64_boundary() {
        let session = NativeFontSession::construct(NativeFontLimits {
            max_calls: u64::MAX,
            ..NativeFontLimits::default()
        })
        .expect("font");
        session.seed_calls(u64::MAX - 1);
        session
            .advance('A', 16, false)
            .expect("call at u64::MAX - 1");
        assert_eq!(session.snapshot().calls, u64::MAX);
        session
            .advance('A', 16, false)
            .expect("pinned counter still works");
        session
            .line_height(16, false)
            .expect("pinned counter still works");
        assert_eq!(session.snapshot().calls, u64::MAX);
        assert_eq!(session.snapshot().first_error, None);
        assert_eq!(
            session.advance('A', 0, false),
            Err(NativeGlyphError::InvalidSize)
        );
        for _ in 0..10 {
            let _ = session.raster('A', 16, false);
        }
        let snap = session.snapshot();
        assert_eq!(snap.calls, u64::MAX, "saturated, never wrapped");
        assert_eq!(snap.first_error, Some(NativeGlyphError::InvalidSize));
    }

    /// The i32 baseline cast bound is EXCLUSIVE at 2^31: `i32::MAX` is not
    /// representable as f32 and rounds up to exactly 2^31, so that value
    /// must be refused. The largest f32 below 2^31 (2^31 - 128, the f32
    /// spacing there) is admitted and casts without saturation.
    #[test]
    fn baseline_i32_boundaries_are_exclusive_at_two_to_the_31() {
        assert!(baseline_fits_i32(0.0));
        assert!(baseline_fits_i32(2_147_483_520.0), "largest f32 below 2^31");
        assert_eq!(2_147_483_520.0_f32 as i32, 2_147_483_520);
        assert!(
            !baseline_fits_i32(2_147_483_648.0),
            "2^31 itself, the rounded i32::MAX"
        );
        assert!(!baseline_fits_i32(-0.5));
        assert!(!baseline_fits_i32(f32::NAN));
        assert!(!baseline_fits_i32(f32::INFINITY));
        assert!(!baseline_fits_i32(f32::NEG_INFINITY));
        assert!(!baseline_fits_i32(f32::MAX));
    }

    #[test]
    fn all_four_methods_share_one_call_budget_and_post_terminal_calls_count_without_overflow() {
        let session = limited(|limits| limits.max_calls = 4);
        session.advance('A', 16, false).expect("1");
        session.line_height(16, false).expect("2");
        session.baseline_offset(16, false).expect("3");
        session.raster('A', 16, false).expect("4");
        assert_eq!(
            session.advance('A', 16, false),
            Err(NativeGlyphError::CallBudget)
        );
        // Post-terminal calls do no work; the counter keeps counting
        // without overflowing, the first error is preserved. Real u64
        // boundary saturation is proven directly below with a seeded
        // counter, not extrapolated from this small cap.
        for _ in 0..100 {
            assert_eq!(
                session.raster('A', 16, false),
                Err(NativeGlyphError::CallBudget)
            );
        }
        let snap = session.snapshot();
        assert_eq!(snap.first_error, Some(NativeGlyphError::CallBudget));
        assert_eq!(snap.calls, 105);
        let before = snap.generated_mask_bytes;
        session.raster('B', 16, false).ok();
        assert_eq!(session.snapshot().generated_mask_bytes, before);
    }

    /// Dimension, per-glyph mask and cumulative totals: exact fit passes,
    /// one byte or one pixel over refuses. Boundaries derive from the
    /// fixture's real fontdue metrics, never hand-copied.
    #[test]
    fn exact_limits_fit_and_one_over_refuses() {
        let reference = reference();
        let (metrics, _) = reference.rasterize('W', 16.0);
        let width = u32::try_from(metrics.width).expect("dims");
        let height = u32::try_from(metrics.height).expect("dims");
        let mask = metrics.width * metrics.height;
        assert!(mask > 0, "fixture assumption: W at 16px has ink");

        let fits = limited(|limits| limits.max_glyph_dimension = width.max(height));
        assert!(fits.raster('W', 16, false).is_ok(), "exact dimension fits");
        let over = limited(|limits| limits.max_glyph_dimension = width.max(height) - 1);
        assert_eq!(
            over.raster('W', 16, false),
            Err(NativeGlyphError::DimensionBudget),
            "one pixel over refuses"
        );
        assert_eq!(
            over.snapshot().generated_mask_bytes,
            0,
            "dimension refusal happens before the reservation"
        );

        let fits = limited(|limits| limits.max_glyph_mask_bytes = mask);
        assert!(fits.raster('W', 16, false).is_ok(), "exact mask bytes fit");
        let over = limited(|limits| limits.max_glyph_mask_bytes = mask - 1);
        assert_eq!(
            over.raster('W', 16, false),
            Err(NativeGlyphError::MaskByteBudget),
            "one byte over refuses"
        );
        assert_eq!(
            over.snapshot().generated_mask_bytes,
            0,
            "mask-byte refusal happens before the reservation"
        );

        let fits = limited(|limits| limits.max_total_glyph_bytes = mask * 2);
        fits.raster('W', 16, false).expect("first");
        fits.raster('W', 16, false).expect("exact cumulative fit");
        assert_eq!(
            fits.raster('W', 16, false),
            Err(NativeGlyphError::TotalGlyphByteBudget),
            "one glyph over refuses"
        );
        assert_eq!(
            fits.snapshot().generated_mask_bytes,
            mask * 2,
            "the refused glyph reserved nothing beyond the two fitted rasters"
        );
        let over = limited(|limits| limits.max_total_glyph_bytes = mask - 1);
        assert_eq!(
            over.raster('W', 16, false),
            Err(NativeGlyphError::TotalGlyphByteBudget)
        );
        assert_eq!(
            over.snapshot().generated_mask_bytes,
            0,
            "total-byte refusal happens before the reservation"
        );
    }

    #[test]
    fn construction_and_snapshot_are_outside_the_call_budget() {
        let session = limited(|limits| limits.max_calls = 2);
        assert_eq!(session.snapshot().calls, 0);
        assert_eq!(session.snapshot().calls, 0);
        session.advance('A', 16, false).expect("one call");
        assert_eq!(session.snapshot().calls, 1);
    }
}
