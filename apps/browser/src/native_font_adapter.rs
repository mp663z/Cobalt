//! TEST-ONLY adapter: the single point where the browser's tests meet the
//! kobo-text diagnostic font session and the engine's `FontProvider` trait.
//! The session never names the trait; the engine never names the session;
//! this adapter is the whole seam. Any session error becomes `None` here,
//! and the session's sticky first error then fails the whole preparation.
//! Not the installed typesetter instance; licensed bytes only (OFL 1.1,
//! AtkinsonHyperlegible-Regular, see kobo-text/fonts).

#![cfg(test)]

use kobo_text::diagnostic_font_session::{NativeFontLimits, NativeFontSession, NativeFontSnapshot};
use kobo_web_document::css_text_page::FontProvider;
use kobo_web_document::inline_lines::GlyphBitmap;

pub(crate) struct NativeFontAdapter {
    session: NativeFontSession,
}

impl NativeFontAdapter {
    pub(crate) fn new() -> Option<Self> {
        NativeFontSession::new()
            .ok()
            .map(|session| Self { session })
    }

    fn with_limits(limits: NativeFontLimits) -> Option<Self> {
        NativeFontSession::with_limits(limits)
            .ok()
            .map(|session| Self { session })
    }

    pub(crate) fn snapshot(&self) -> NativeFontSnapshot {
        self.session.snapshot()
    }
}

impl FontProvider for NativeFontAdapter {
    fn advance(&self, character: char, size: u32, bold: bool) -> Option<u32> {
        self.session.advance(character, size, bold).ok()
    }
    fn line_height(&self, size: u32, bold: bool) -> Option<u32> {
        self.session.line_height(size, bold).ok()
    }
    fn baseline_offset(&self, size: u32, bold: bool) -> Option<i32> {
        self.session.baseline_offset(size, bold).ok()
    }
    fn raster(&self, character: char, size: u32, bold: bool) -> Option<GlyphBitmap> {
        self.session
            .raster(character, size, bold)
            .ok()
            .map(|glyph| GlyphBitmap {
                left: glyph.left,
                top: glyph.top,
                width: glyph.width,
                height: glyph.height,
                coverage: glyph.coverage,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::NativeFontAdapter;
    use kobo_text::diagnostic_font_session::{NativeFontLimits, NativeGlyphError};
    use kobo_web_document::box_tree::BoxTree;
    use kobo_web_document::css_text_page::paint_single_text_page;
    use kobo_web_document::display_list::Rgb;
    use kobo_web_document::{parse_style_tree, Limits};

    const PLAIN_PAGE: &[u8] = b"<!doctype html><style>body{margin:0} p{margin:0}</style><html><body style='background-color:#fed'><p style='background-color:#abcdef'>Real native type 0123</p></body></html>";

    fn tree(html: &[u8]) -> BoxTree {
        BoxTree::from_style(&parse_style_tree(html, &[], &Limits::DEFAULT))
    }

    /// Provider failure must fail the WHOLE paint: an error, no partial
    /// `DisplayList`, and the session's sticky reason recorded.
    fn whole_failure(html: &[u8], reason: NativeGlyphError) {
        let adapter = NativeFontAdapter::new().expect("session");
        let boxes = tree(html);
        assert!(
            paint_single_text_page(&boxes, 320, 240, &adapter).is_err(),
            "the page must fail whole"
        );
        assert_eq!(adapter.snapshot().first_error, Some(reason));
    }

    /// The engine's restricted path refuses bold markup and non-ASCII text
    /// on its OWN walk, before the provider is ever consulted: the page
    /// fails whole with no partial artifact, and the session stays
    /// untouched. The session-side gates for the same conditions are
    /// covered adapter-directly below.
    #[test]
    fn bold_markup_and_non_ascii_text_are_refused_whole_by_the_engine_walk() {
        for html in [
            &b"<!doctype html><style>body{margin:0} p{margin:0}</style><html><body><p>plain <b>bold</b></p></body></html>"[..],
            "<!doctype html><style>body{margin:0} p{margin:0}</style><html><body><p>caf\u{00E9}</p></body></html>"
                .as_bytes(),
        ] {
            let adapter = NativeFontAdapter::new().expect("session");
            let boxes = tree(html);
            assert!(
                paint_single_text_page(&boxes, 320, 240, &adapter).is_err(),
                "the page must fail whole"
            );
            assert_eq!(adapter.snapshot().first_error, None);
        }
    }

    /// Adapter-direct: when a caller DOES consult the provider with bold or
    /// shaping-sensitive input, the refusal is a None with the session's
    /// sticky reason recorded - and the session is terminal afterward.
    #[test]
    fn adapter_bold_and_shaping_refuse_with_the_reason_recorded() {
        use kobo_web_document::css_text_page::FontProvider;
        let adapter = NativeFontAdapter::new().expect("session");
        assert_eq!(adapter.advance('b', 16, true), None);
        assert_eq!(
            adapter.snapshot().first_error,
            Some(NativeGlyphError::BoldUnsupported)
        );
        let adapter = NativeFontAdapter::new().expect("session");
        assert_eq!(adapter.advance('\u{00E9}', 16, false), None);
        assert_eq!(
            adapter.snapshot().first_error,
            Some(NativeGlyphError::ShapingRefused)
        );
        assert!(adapter.raster('A', 16, false).is_none());
    }

    #[test]
    fn an_oversize_font_fails_the_whole_paint_with_the_reason_recorded() {
        whole_failure(
            b"<!doctype html><style>body{margin:0} p{margin:0}</style><html><body><p style='font-size:300px'>big</p></body></html>",
            NativeGlyphError::InvalidSize,
        );
    }

    #[test]
    fn an_exhausted_budget_fails_the_whole_paint_with_the_reason_recorded() {
        let limits = NativeFontLimits {
            max_total_glyph_bytes: 128,
            ..NativeFontLimits::default()
        };
        let adapter = NativeFontAdapter::with_limits(limits).expect("session");
        let boxes = tree(PLAIN_PAGE);
        assert!(
            paint_single_text_page(&boxes, 320, 240, &adapter).is_err(),
            "the page must fail whole"
        );
        let reason = adapter.snapshot().first_error;
        assert!(
            matches!(
                reason,
                Some(NativeGlyphError::TotalGlyphByteBudget | NativeGlyphError::MaskByteBudget)
            ),
            "budget refusal recorded, found {reason:?}"
        );
    }

    /// HOST diagnostic proof: a real page through the real engine paint
    /// path, measured and rastered by the native session's actual face, to
    /// an actual `DisplayList` and actual pixels. Not a simulator capture,
    /// not a shipping claim, not the installed instance.
    #[test]
    fn a_real_page_paints_real_glyphs_through_the_native_session() {
        let adapter = NativeFontAdapter::new().expect("session");
        let tree = tree(PLAIN_PAGE);
        let list = paint_single_text_page(&tree, 320, 240, &adapter).expect("paint");
        let pixels = list
            .rasterize(320, 240, Rgb(255, 255, 255))
            .expect("rasterize");
        // Real glyph ink landed on the page.
        let ink = pixels
            .chunks_exact(4)
            .filter(|px| px[0] < 128 && px[1] < 128 && px[2] < 128)
            .count();
        assert!(ink > 50, "expected real glyph ink, found {ink} dark pixels");
        let snap = adapter.snapshot();
        assert_eq!(snap.first_error, None);
        assert!(snap.calls > 0 && snap.generated_mask_bytes > 0);
        // Artifact for human inspection, only when a directory is named.
        if let Ok(dir) = std::env::var("NATIVE_FONT_ARTIFACT_DIR") {
            let mut ppm = b"P6\n320 240\n255\n".to_vec();
            for px in pixels.chunks_exact(4) {
                ppm.extend_from_slice(&px[..3]);
            }
            std::fs::write(format!("{dir}/native-font-proof.ppm"), ppm).expect("artifact");
            std::fs::write(
                format!("{dir}/native-font-proof-SOURCE-LICENSE.txt"),
                "Face: AtkinsonHyperlegible-Regular (OFL 1.1), bytes compiled into kobo-text \
                 (crates/kobo-text/fonts/, LICENSE-AtkinsonHyperlegible.txt). Same native \
                 fontdue implementation as the reader; NOT the installed typesetter instance. \
                 Page: synthetic test markup in native_font_adapter.rs. Host diagnostic proof \
                 only - not a simulator capture, not a shipping claim.\n",
            )
            .expect("label");
        }
    }
}
