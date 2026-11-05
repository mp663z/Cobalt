//! PROTOTYPE (isolated branch, not adopted, test-only like the existing
//! adapter): `FontProvider` over the bundled Regular + Bold pair.

#![cfg(test)]

use kobo_text::bundled_face_pair::BundledFacePair;
use kobo_text::diagnostic_font_session::NativeFontSnapshot;
use kobo_web_document::css_text_page::FontProvider;
use kobo_web_document::inline_lines::GlyphBitmap;

pub(crate) struct PairAdapter(BundledFacePair);

impl PairAdapter {
    pub(crate) fn new() -> Option<Self> {
        BundledFacePair::new().ok().map(Self)
    }
    pub(crate) fn snapshot(&self) -> NativeFontSnapshot {
        self.0.snapshot()
    }
}

impl FontProvider for PairAdapter {
    fn advance(&self, c: char, size: u32, bold: bool) -> Option<u32> {
        self.0.advance(c, size, bold).ok()
    }
    fn line_height(&self, size: u32, bold: bool) -> Option<u32> {
        self.0.line_height(size, bold).ok()
    }
    fn baseline_offset(&self, size: u32, bold: bool) -> Option<i32> {
        self.0.baseline_offset(size, bold).ok()
    }
    fn raster(&self, c: char, size: u32, bold: bool) -> Option<GlyphBitmap> {
        self.0.raster(c, size, bold).ok().map(|g| GlyphBitmap {
            left: g.left,
            top: g.top,
            width: g.width,
            height: g.height,
            coverage: g.coverage,
        })
    }
}

/// DIAGNOSTIC PREDICTION ONLY. This scans the box tree's text with the
/// same ASCII rule the engine applies and names what the engine is
/// predicted to refuse. It is NOT the engine's own result: the engine
/// returns the generic `PageError::Unsupported`. The provider records no
/// error for these inputs (the engine may still call it to measure the ASCII
/// prefix; see the counting tests), so no shaping reason may be attributed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PredictedRefusal {
    /// Some text node holds a char outside ASCII graphic/whitespace/NBSP.
    PredictedNonAsciiText,
}

pub(crate) fn predict_refusal(
    tree: &kobo_web_document::box_tree::BoxTree,
) -> Option<PredictedRefusal> {
    use kobo_web_document::box_tree::BoxKind;
    tree.boxes
        .iter()
        .filter(|n| n.kind == BoxKind::Text)
        .filter_map(|n| n.text.as_deref())
        .any(|t| {
            t.chars()
                .any(|c| !(c.is_ascii_graphic() || c.is_ascii_whitespace() || c == '\u{a0}'))
        })
        .then_some(PredictedRefusal::PredictedNonAsciiText)
}

#[cfg(test)]
mod tests {
    use super::PairAdapter;
    use crate::native_font_adapter::NativeFontAdapter;
    use kobo_text::diagnostic_font_session::NativeGlyphError;
    use kobo_web_document::box_tree::BoxTree;
    use kobo_web_document::css_text_page::{paint_single_text_page, FontProvider};
    use kobo_web_document::display_list::Rgb;
    use kobo_web_document::{parse_style_tree, Limits};

    const PAGE: &[u8] = b"<!doctype html><style>body{margin:0} p{margin:0}</style><html><body style='background-color:#fed'><p style='background-color:#abcdef'>Real native type 0123</p></body></html>";

    fn render(html: &[u8], p: &impl FontProvider) -> Option<Vec<u8>> {
        let boxes = BoxTree::from_style(&parse_style_tree(html, &[], &Limits::DEFAULT));
        let list = paint_single_text_page(&boxes, 320, 240, p).ok()?;
        list.rasterize(320, 240, Rgb(255, 255, 255)).ok()
    }

    /// Regression: for a regular-only page the pair adapter paints the same
    /// pixels as the existing regular adapter (byte-identical).
    #[test]
    fn regular_page_is_byte_identical_to_the_existing_adapter() {
        let a = render(PAGE, &NativeFontAdapter::new().unwrap()).expect("old");
        let b = render(PAGE, &PairAdapter::new().unwrap()).expect("pair");
        assert_eq!(a, b);
        assert!(a.chunks_exact(4).any(|p| p[0] < 128), "ink exists");
    }

    /// Provider level: a bold request is answered by the real bold face;
    /// the old adapter refuses it.
    #[test]
    fn provider_answers_bold_with_the_bold_face_and_old_adapter_refuses() {
        let pair = PairAdapter::new().unwrap();
        let reg = pair.raster('A', 32, false).unwrap();
        let bold = pair.raster('A', 32, true).unwrap();
        assert_ne!(reg.coverage, bold.coverage);
        let old = NativeFontAdapter::new().unwrap();
        assert!(old.raster('A', 32, true).is_none());
        assert_eq!(pair.snapshot().first_error, None);
    }

    /// Through the host-style `paint_single_text_page` entry, mixed bold
    /// markup and non-ASCII text still refuse whole (that entry rejects any
    /// node with more than one child; non-ASCII is refused inside the walk).
    /// Both refusals leave no provider error (call counts are asserted in the
    /// counting tests, not here). Corrected: the engine itself
    /// does carry bold via `paint_direct_text_blocks` (see the direct-entry
    /// tests below).
    #[test]
    fn engine_walk_refusals_are_unchanged_with_the_pair() {
        for html in [
            &b"<!doctype html><html><body><p>plain <b>bold</b></p></body></html>"[..],
            "<!doctype html><html><body><p>caf\u{00E9}</p></body></html>".as_bytes(),
        ] {
            let pair = PairAdapter::new().unwrap();
            assert!(render(html, &pair).is_none());
            assert_eq!(pair.snapshot().first_error, None, "no provider error recorded (not a call-count proof)");
        }
    }

    #[test]
    fn non_latin_through_the_provider_is_a_recorded_refusal() {
        let pair = PairAdapter::new().unwrap();
        assert_eq!(pair.advance('日', 16, true), None);
        assert_eq!(
            pair.snapshot().first_error,
            Some(NativeGlyphError::ShapingRefused)
        );
    }

    use super::{predict_refusal, PredictedRefusal};
    use kobo_web_document::css_text_page::paint_direct_text_blocks;
    use kobo_web_document::display_list::Command;

    fn tree_of(html: &str) -> BoxTree {
        BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT))
    }

    /// Expected glyph sequence from the bundled pair itself (`pair.0`, NOT the
    /// adapter under test, so adapter mutations cannot cancel out).
    fn expected(
        pair: &PairAdapter,
        runs: &[(&str, bool)],
        size: u32,
    ) -> Vec<(i32, i32, u32, u32, Vec<u8>)> {
        // (ink x, ink y = baseline + top (top is signed, negative above the baseline), w, h, coverage): pen x advances by the face-specific advance
        // of every char including spaces (margin 0, single line at x = 0).
        let mut out = Vec::new();
        let mut pen = 0_i32;
        for &(text, bold) in runs {
            for ch in text.chars() {
                if !ch.is_whitespace() {
                    let g = pair.0.raster(ch, size, bold).expect("ascii glyph");
                    if g.width > 0 && g.height > 0 {
                        let base = pair.0.baseline_offset(size, bold).expect("baseline");
                        out.push((pen + g.left, base + g.top, g.width, g.height, g.coverage));
                    }
                }
                pen += i32::try_from(pair.0.advance(ch, size, bold).expect("adv")).unwrap();
            }
        }
        out
    }

    fn painted(
        list: &kobo_web_document::display_list::DisplayList,
    ) -> Vec<(i32, i32, u32, u32, Vec<u8>)> {
        list.commands()
            .iter()
            .filter_map(|c| match c {
                Command::GlyphRun {
                    bounds, coverage, ..
                } => Some((
                    bounds.x,
                    bounds.y,
                    bounds.width,
                    bounds.height,
                    coverage.clone(),
                )),
                _ => None,
            })
            .collect()
    }

    const MIXED: &str = "<!doctype html><style>body{margin:0} p{margin:0}</style><html><body><p>plain <b>bold</b> end</p></body></html>";

    /// Pixel check: every glyph the engine paints through the pair has the
    /// exact coverage bytes the bundled Regular or Bold face produces for
    /// that char (face per run), in order; a wrong-face expectation fails.
    #[test]
    fn mixed_regular_bold_via_direct_entry_matches_bundled_rasters() {
        let pair = PairAdapter::new().unwrap();
        let tree = tree_of(MIXED);
        let list = paint_direct_text_blocks(&tree, 320, 240, &pair).expect("direct paints");
        let got = painted(&list);
        let size = 16;
        let want = expected(
            &pair,
            &[("plain ", false), ("bold", true), (" end", false)],
            size,
        );
        assert!(!got.is_empty());
        assert_eq!(
            got, want,
            "engine glyph x/size/coverage == bundled face output"
        );
        // Negative control: expecting regular for the bold word must differ.
        let wrong = expected(
            &pair,
            &[("plain ", false), ("bold", false), (" end", false)],
            size,
        );
        assert_ne!(got, wrong, "bold glyphs are not regular glyphs");
        // Rasterized page has ink; no provider error.
        let px = list.rasterize(320, 240, Rgb(255, 255, 255)).unwrap();
        assert!(px.chunks_exact(4).any(|p| p[0] < 128));
        assert_eq!(pair.snapshot().first_error, None);
    }

    /// Regular-only control: the same text without bold paints the
    /// regular-face sequence, and differs from the mixed page.
    #[test]
    fn regular_control_differs_from_mixed() {
        let pair = PairAdapter::new().unwrap();
        let reg = tree_of("<!doctype html><style>body{margin:0} p{margin:0}</style><html><body><p>plain bold end</p></body></html>");
        let a = painted(&paint_direct_text_blocks(&reg, 320, 240, &pair).unwrap());
        let b = painted(&paint_direct_text_blocks(&tree_of(MIXED), 320, 240, &pair).unwrap());
        assert_eq!(a, expected(&pair, &[("plain bold end", false)], 16));
        assert_ne!(a, b);
    }

    /// The host-style single entry still refuses the mixed page; this
    /// prototype does not change it.
    #[test]
    fn single_entry_still_refuses_mixed_structure() {
        let pair = PairAdapter::new().unwrap();
        assert!(paint_single_text_page(&tree_of(MIXED), 320, 240, &pair).is_err());
    }

    /// Non-ASCII: engine refusal is the generic `Unsupported` and leaves
    /// no provider error (call counts are not asserted here; the counting
    /// test shows the ASCII prefix is still measured). The PREDICTION is a separate
    /// diagnostic label; it is never an engine reason. Provider-level
    /// `ShapingRefused` is shown separately and only when the provider is
    /// actually called.
    #[test]
    fn non_ascii_prediction_is_separate_from_engine_and_provider_results() {
        use kobo_web_document::css_text_page::PageError;
        let pair = PairAdapter::new().unwrap();
        let tree = tree_of("<!doctype html><html><body><p>caf\u{00E9} <b>b</b></p></body></html>");
        assert_eq!(
            predict_refusal(&tree),
            Some(PredictedRefusal::PredictedNonAsciiText)
        );
        assert!(matches!(
            paint_direct_text_blocks(&tree, 320, 240, &pair),
            Err(PageError::Unsupported)
        ));
        assert_eq!(pair.snapshot().first_error, None, "no provider error recorded (not a call-count proof)");
        assert_eq!(predict_refusal(&tree_of(MIXED)), None);
        // NBSP is inside the engine's accepted set: not predicted as refused.
        assert_eq!(
            predict_refusal(&tree_of(
                "<!doctype html><html><body><p>a&nbsp;b</p></body></html>"
            )),
            None
        );
        // Provider-level, separately: direct call records ShapingRefused.
        assert_eq!(pair.advance('\u{e9}', 16, false), None);
        assert!(pair.snapshot().first_error.is_some());
    }
}

#[cfg(test)]
mod wrap_diag {
    use super::PairAdapter;
    use kobo_web_document::box_tree::BoxTree;
    use kobo_web_document::css_text_page::{paint_direct_text_blocks, FontProvider};
    use kobo_web_document::display_list::Command;
    use kobo_web_document::{parse_style_tree, Limits};

    type Ink = (i32, i32, u32, u32, Vec<u8>);

    /// Independent greedy-wrap oracle. Break only at single ASCII spaces; a
    /// word that does not fit moves whole to the next line; the space at a
    /// break is dropped; zero margin; pen x restarts at 0; line pitch is the
    /// provider line height. Glyph ink = (pen+left, line*pitch + baseline + top).
    fn wrapped_expected(pair: &PairAdapter, text: &str, size: u32, width: i32) -> Vec<Ink> {
        let adv = |c: char| i32::try_from(pair.0.advance(c, size, false).expect("adv")).unwrap();
        let base = pair.0.baseline_offset(size, false).expect("baseline");
        let pitch = i32::try_from(pair.line_height(size, false).expect("lh")).unwrap();
        let mut lines: Vec<Vec<&str>> = vec![vec![]];
        let mut used = 0_i32;
        for word in text.split(' ') {
            let w: i32 = word.chars().map(adv).sum();
            let space = adv(' ');
            let cur = lines.last_mut().unwrap();
            if cur.is_empty() {
                cur.push(word);
                used = w;
            } else if used + space + w <= width {
                cur.push(word);
                used += space + w;
            } else {
                lines.push(vec![word]);
                used = w;
            }
        }
        let mut out = Vec::new();
        for (n, line) in lines.iter().enumerate() {
            let mut pen = 0_i32;
            let y0 = i32::try_from(n).unwrap() * pitch;
            for (k, word) in line.iter().enumerate() {
                if k > 0 {
                    pen += adv(' ');
                }
                for ch in word.chars() {
                    let g = pair.0.raster(ch, size, false).expect("glyph");
                    if g.width > 0 && g.height > 0 {
                        out.push((pen + g.left, y0 + base + g.top, g.width, g.height, g.coverage));
                    }
                    pen += adv(ch);
                }
            }
        }
        out
    }

    fn painted(list: &kobo_web_document::display_list::DisplayList) -> Vec<Ink> {
        list.commands()
            .iter()
            .filter_map(|c| match c {
                Command::GlyphRun { bounds, coverage, .. } => {
                    Some((bounds.x, bounds.y, bounds.width, bounds.height, coverage.clone()))
                }
                _ => None,
            })
            .collect()
    }

    const TEXT: &str = "alpha beta gamma delta epsilon zeta eta theta";

    fn page(text: &str) -> BoxTree {
        let html = format!("<!doctype html><style>body{{margin:0}} p{{margin:0}}</style><html><body><p>{text}</p></body></html>");
        BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT))
    }

    /// Multi-line ASCII text at 16 px, zero margin, regular face only: every
    /// painted glyph equals the bundled raster at the independently derived
    /// wrapped position, for several widths (so 2, 3 and 4+ lines occur).
    #[test]
    fn wrapped_ascii_lines_match_independent_oracle() {
        let pair = PairAdapter::new().unwrap();
        let mut line_counts = std::collections::BTreeSet::new();
        for width in [60_u32, 90, 120, 160, 200] {
            let got = painted(&paint_direct_text_blocks(&page(TEXT), width, 240, &pair).unwrap());
            let want = wrapped_expected(&pair, TEXT, 16, i32::try_from(width).unwrap());
            line_counts.insert(want.iter().map(|g| g.1).max().unwrap() / 20);
            assert_eq!(got, want, "width {width}");
        }
        assert!(line_counts.iter().any(|&n| n >= 2), "wrapping exercised: {line_counts:?}");
        assert_eq!(pair.snapshot().first_error, None);
    }

    /// Negative control: an oracle with the wrong line pitch must differ.
    #[test]
    fn wrong_pitch_oracle_differs() {
        let pair = PairAdapter::new().unwrap();
        let got = painted(&paint_direct_text_blocks(&page(TEXT), 90, 240, &pair).unwrap());
        let mut bad = wrapped_expected(&pair, TEXT, 16, 90);
        for g in &mut bad {
            if g.1 > 20 {
                g.1 += 1;
            }
        }
        assert_ne!(got, bad);
    }

    /// Exact-fit boundary: the first two words fit at width == their advance
    /// sum and must wrap at width - 1. Kills `<=` vs `<` and off-by-one wraps.
    #[test]
    fn exact_fit_boundary_wraps_at_one_pixel_less() {
        let pair = PairAdapter::new().unwrap();
        let w: u32 = "alpha beta".chars().map(|c| pair.0.advance(c, 16, false).unwrap()).sum();
        for width in [w, w - 1, w + 1] {
            let got = painted(&paint_direct_text_blocks(&page("alpha beta gamma"), width, 240, &pair).unwrap());
            let want = wrapped_expected(&pair, "alpha beta gamma", 16, i32::try_from(width).unwrap());
            assert_eq!(got, want, "width {width}");
        }
        let at = painted(&paint_direct_text_blocks(&page("alpha beta"), w, 240, &pair).unwrap());
        let under = painted(&paint_direct_text_blocks(&page("alpha beta"), w - 1, 240, &pair).unwrap());
        assert_ne!(at, under, "one pixel narrower changes the line break");
        assert_eq!(under, wrapped_expected(&pair, "alpha beta", 16, i32::try_from(w - 1).unwrap()));
    }

    /// Oracle with a content-box origin: x0/y0 offset every glyph and `width`
    /// is the content width. Same wrap rule as `wrapped_expected`.
    fn wrapped_at(pair: &PairAdapter, text: &str, width: i32, x0: i32, y0: i32) -> Vec<Ink> {
        wrapped_expected(pair, text, 16, width)
            .into_iter()
            .map(|(x, y, w, h, c)| (x + x0, y + y0, w, h, c))
            .collect()
    }

    fn styled_page(style: &str, text: &str) -> BoxTree {
        let html = format!("<!doctype html><style>body{{margin:0}} p{{{style}}}</style><html><body><p>{text}</p></body></html>");
        BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT))
    }

    /// Nonzero margin and padding: the content box shrinks by margin+padding
    /// on both sides, so wrapping uses viewport - 2*(10+6) = content width,
    /// glyphs start at x = 10+6 and the first line at y = 6 (padding-top).
    #[test]
    fn margin_and_padding_shift_origin_and_narrow_wrap_width() {
        let pair = PairAdapter::new().unwrap();
        for vw in [120_u32, 160, 200] {
            let tree = styled_page("margin:0 10px;padding:6px", TEXT);
            let got = painted(&paint_direct_text_blocks(&tree, vw, 240, &pair).unwrap());
            let content = i32::try_from(vw).unwrap() - 32;
            assert_eq!(got, wrapped_at(&pair, TEXT, content, 16, 6), "vw {vw}");
            // Negative control: ignoring the shrink must differ at 120.
            if vw == 120 {
                assert_ne!(got, wrapped_at(&pair, TEXT, 120, 16, 6));
            }
        }
    }

    /// Vertical margin only moves the block down; it does not change wrap.
    #[test]
    fn top_margin_moves_lines_down_only() {
        let pair = PairAdapter::new().unwrap();
        let tree = styled_page("margin:12px 0 0 0", TEXT);
        let got = painted(&paint_direct_text_blocks(&tree, 120, 240, &pair).unwrap());
        assert_eq!(got, wrapped_at(&pair, TEXT, 120, 0, 12));
    }

    /// A single word wider than the content width is refused whole by the
    /// engine (generic `Unsupported`), never painted overflowing or split.
    /// Boundary: a word exactly as wide as the line paints and matches the
    /// oracle; one pixel narrower refuses. The provider records no error
    /// (the refusal is not a provider refusal).
    #[test]
    fn word_wider_than_line_is_refused_exact_fit_paints() {
        use kobo_web_document::css_text_page::PageError;
        let pair = PairAdapter::new().unwrap();
        let word = "supercalifragilisticexpialidocious";
        let w: u32 = word.chars().map(|c| pair.0.advance(c, 16, false).unwrap()).sum();
        let got = painted(&paint_direct_text_blocks(&page(word), w, 240, &pair).unwrap());
        assert_eq!(got, wrapped_expected(&pair, word, 16, i32::try_from(w).unwrap()));
        assert!(matches!(
            paint_direct_text_blocks(&page(word), w - 1, 240, &pair),
            Err(PageError::Unsupported)
        ));
        let mixed = "hi supercalifragilisticexpialidocious beta";
        assert!(matches!(
            paint_direct_text_blocks(&page(mixed), 100, 240, &pair),
            Err(PageError::Unsupported)
        ));
        assert_eq!(pair.snapshot().first_error, None);
    }

    /// Independent mixed-face wrap oracle. `words` are (word, bold, space_bold):
    /// `space_bold` is the face of the space BEFORE the word. A space inside a
    /// `<b>` run is bold; a space outside it (including the ones touching the
    /// tags) is regular. (My first oracle used a regular space everywhere and
    /// differed by 1 px after the in-run space; the engine was right.)
    fn mixed_wrapped(
        pair: &PairAdapter,
        words: &[(&str, bool, bool)],
        width: i32,
        pitch: i32,
    ) -> Vec<Ink> {
        let size = 16;
        let adv = |c: char, b: bool| i32::try_from(pair.0.advance(c, size, b).expect("adv")).unwrap();
        let ww = |w: &str, b: bool| -> i32 { w.chars().map(|c| adv(c, b)).sum() };
        let mut lines: Vec<Vec<(&str, bool, bool)>> = vec![vec![]];
        let mut used = 0_i32;
        for &(w, b, sb) in words {
            let x = ww(w, b);
            let sp = adv(' ', sb);
            let cur = lines.last_mut().unwrap();
            if cur.is_empty() {
                cur.push((w, b, sb));
                used = x;
            } else if used + sp + x <= width {
                cur.push((w, b, sb));
                used += sp + x;
            } else {
                lines.push(vec![(w, b, sb)]);
                used = x;
            }
        }
        let mut out = Vec::new();
        for (n, line) in lines.iter().enumerate() {
            let y0 = i32::try_from(n).unwrap() * pitch;
            let mut pen = 0_i32;
            for (k, &(w, b, sb)) in line.iter().enumerate() {
                if k > 0 {
                    pen += adv(' ', sb);
                }
                let base = pair.0.baseline_offset(size, b).expect("baseline");
                for ch in w.chars() {
                    let g = pair.0.raster(ch, size, b).expect("glyph");
                    if g.width > 0 && g.height > 0 {
                        out.push((pen + g.left, y0 + base + g.top, g.width, g.height, g.coverage));
                    }
                    pen += adv(ch, b);
                }
            }
        }
        out
    }

    /// Mixed regular/bold text wrapping across line breaks, several widths.
    /// Asserts glyph-exact equality with the independent oracle (bold word
    /// widths use bold advances, in-run space is bold, regular line pitch).
    #[test]
    fn mixed_bold_wrap_across_line_break_matches_oracle() {
        let pair = PairAdapter::new().unwrap();
        let html = "alpha beta <b>gamma delta</b> epsilon zeta";
        let words = [
            ("alpha", false, false),
            ("beta", false, false),
            ("gamma", true, false),
            ("delta", true, true),
            ("epsilon", false, false),
            ("zeta", false, false),
        ];
        // Bold and regular line heights are equal here (20), so the pitch
        // rule is not distinguished by this page.
        assert_eq!(pair.line_height(16, true), pair.line_height(16, false));
        let pitch = i32::try_from(pair.line_height(16, false).unwrap()).unwrap();
        let mut line_counts = std::collections::BTreeSet::new();
        let mut break_between_bold_words = false;
        for width in [60_u32, 90, 120, 160, 200, 320] {
            let w = i32::try_from(width).unwrap();
            let want = mixed_wrapped(&pair, &words, w, pitch);
            let got = painted(&paint_direct_text_blocks(&page(html), width, 240, &pair).unwrap());
            assert_eq!(got, want, "width {width}");
            // Wrong-space-face oracle (regular space in the bold run) must differ at width 120.
            if width == 120 {
                let mut wrong = words;
                wrong[3].2 = false;
                assert_ne!(got, mixed_wrapped(&pair, &wrong, w, pitch), "bold in-run space is bold");
            }
            let ys: std::collections::BTreeSet<i32> = want.iter().map(|g| g.1 / pitch).collect();
            line_counts.insert(ys.len());
            // Glyph index 13 is the last 'a' of bold "gamma", 14 the first 'd' of bold
            // "delta" (alpha 5 + beta 4 + gamma 5). Line = ink y / pitch (ink y stays
            // within its 20 px line at 16 px).
            let line = |i: usize| want[i].1 / pitch;
            break_between_bold_words |= line(14) > line(13);
        }
        assert!(break_between_bold_words, "some width breaks between the two bold words");
        assert!(line_counts.iter().any(|&n| n >= 2), "wrapping exercised: {line_counts:?}");
        assert_eq!(pair.snapshot().first_error, None);
    }

    use super::GlyphBitmap;
    use std::cell::Cell;

    /// Delegating provider that counts every call so a refusal can be
    /// located relative to the provider instead of inferred from
    /// `first_error` alone.
    struct Counting<'a> {
        inner: &'a PairAdapter,
        advance: Cell<u32>,
        chars: std::cell::RefCell<String>,
        metrics: Cell<u32>,
        raster: Cell<u32>,
    }
    impl FontProvider for Counting<'_> {
        fn advance(&self, c: char, size: u32, bold: bool) -> Option<u32> {
            self.advance.set(self.advance.get() + 1);
            self.chars.borrow_mut().push(c);
            self.inner.advance(c, size, bold)
        }
        fn line_height(&self, size: u32, bold: bool) -> Option<u32> {
            self.metrics.set(self.metrics.get() + 1);
            self.inner.line_height(size, bold)
        }
        fn baseline_offset(&self, size: u32, bold: bool) -> Option<i32> {
            self.metrics.set(self.metrics.get() + 1);
            self.inner.baseline_offset(size, bold)
        }
        fn raster(&self, c: char, size: u32, bold: bool) -> Option<GlyphBitmap> {
            self.raster.set(self.raster.get() + 1);
            self.inner.raster(c, size, bold)
        }
    }

    /// Where the overlong-word refusal happens: the engine MEASURES (advance
    /// and line-height calls > 0) but never rasters (raster calls == 0), and
    /// the provider records no error. Non-ASCII "caf\u{e9}": the ASCII prefix is
    /// measured (advance calls on c,a,f only) and the non-ASCII char itself is
    /// never passed to the provider.
    #[test]
    fn overlong_word_refusal_is_after_measurement_before_raster() {
        use kobo_web_document::css_text_page::PageError;
        let pair = PairAdapter::new().unwrap();
        let c = Counting { inner: &pair, advance: Cell::new(0), chars: Default::default(), metrics: Cell::new(0), raster: Cell::new(0) };
        let r = paint_direct_text_blocks(&page("supercalifragilisticexpialidocious"), 100, 240, &c);
        assert!(matches!(r, Err(PageError::Unsupported)));
        assert!(c.advance.get() > 0, "measured before refusing");
        assert_eq!(c.raster.get(), 0, "no glyph rastered");
        assert_eq!(pair.snapshot().first_error, None);
        let n = Counting { inner: &pair, advance: Cell::new(0), chars: Default::default(), metrics: Cell::new(0), raster: Cell::new(0) };
        let r = paint_direct_text_blocks(&page("caf\u{e9}"), 100, 240, &n);
        assert!(matches!(r, Err(PageError::Unsupported)));
        // Observed, not assumed: the engine measures the ASCII prefix "caf"
        // (two passes) and refuses at the first non-ASCII char without ever
        // passing it to the provider; no metrics and no raster calls.
        assert_eq!(n.chars.borrow().as_str(), "cafcaf");
        assert_eq!((n.metrics.get(), n.raster.get()), (0, 0));
    }
}
