//! Font-session Page view fixture preview, no installed fonts required.
use kobo_text::page_font_session::PageFontSession;
use kobo_web_document::{
    basic_page::paint_basic_page,
    box_tree::BoxTree,
    css_text_page::FontProvider,
    display_list::{Command, DisplayList, Rgb},
    inline_lines::GlyphBitmap,
    Limits,
};
use std::io::Write;
struct Face(PageFontSession);
impl FontProvider for Face {
    fn page_character_metrics(
        &self,
        ch: char,
        size: u32,
        bold: bool,
        italic: bool,
    ) -> Option<kobo_web_document::css_text_page::NormalLineMetrics> {
        let (height, baseline) = self.0.character_line_metrics(ch, size, bold, italic)?;
        Some(kobo_web_document::css_text_page::NormalLineMetrics { height, baseline })
    }
    fn page_shape(
        &self,
        text: &str,
        size: u32,
        bold: bool,
        italic: bool,
    ) -> Option<Vec<kobo_web_document::css_text_page::PageCluster>> {
        Some(
            self.0
                .shape(text, size, bold, italic)?
                .into_iter()
                .map(|c| kobo_web_document::css_text_page::PageCluster {
                    start: c.start,
                    end: c.end,
                    advance: c.glyph.advance,
                    bitmap: GlyphBitmap {
                        left: c.glyph.left,
                        top: c.glyph.top,
                        width: c.glyph.width,
                        height: c.glyph.height,
                        coverage: c.glyph.coverage,
                    },
                })
                .collect(),
        )
    }
    fn has_glyph(&self, c: char, b: bool) -> Option<bool> {
        Some(self.0.has_glyph(c, b, false))
    }
    fn advance(&self, c: char, s: u32, b: bool) -> Option<u32> {
        self.0.advance(c, s, b, false)
    }
    fn line_height(&self, s: u32, b: bool) -> Option<u32> {
        Some(self.0.line_metrics(s, b, false)?.0)
    }
    fn baseline_offset(&self, s: u32, b: bool) -> Option<i32> {
        Some(self.0.line_metrics(s, b, false)?.1)
    }
    fn raster(&self, c: char, s: u32, b: bool) -> Option<GlyphBitmap> {
        self.page_raster(c, s, b, false)
    }
    fn page_advance(&self, c: char, s: u32, b: bool, i: bool) -> Option<u32> {
        self.0.advance(c, s, b, i)
    }
    fn page_raster(&self, c: char, s: u32, b: bool, i: bool) -> Option<GlyphBitmap> {
        let g = self.0.raster(c, s, b, i)?;
        Some(GlyphBitmap {
            left: g.left,
            top: g.top,
            width: g.width,
            height: g.height,
            coverage: g.coverage,
        })
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let html = std::fs::read(args.get(1).ok_or("input HTML required")?)?;
    let mut session = PageFontSession::bundled()?;
    let mut fonts = args.iter().skip(3);
    while let Some(font) = fonts.next() {
        if font == "--italic" {
            let regular = std::fs::read(fonts.next().ok_or("italic font required")?)?;
            let bold = std::fs::read(fonts.next().ok_or("bold italic font required")?)?;
            session.set_italic(&regular, &bold)?;
        } else {
            session.add_fallback(&std::fs::read(font)?)?;
        }
    }
    let styled = kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(
        &html,
        &[],
        &Limits::DEFAULT,
        Some(kobo_web_document::MediaContext::new(760).ok_or("viewport")?),
        kobo_web_document::SupplyBudget {
            max_linked_sheets: 16,
            max_sheet_bytes: 1048576,
            max_aggregate_bytes: 4194304,
        },
    );
    let tree = BoxTree::from_style(&styled);
    let (commands, stats, height) = paint_basic_page(&tree, 760, 1200, &Face(session))?;
    let mut list = DisplayList::default();
    for c in commands {
        match c {
            Command::Fill {
                rect,
                color,
                source,
            } => list
                .fill(rect, color, source)
                .map_err(|e| format!("{e:?}"))?,
            Command::GlyphRun {
                bounds,
                layout,
                coverage,
                color,
                source,
            } => list
                .glyph_run(bounds, layout, coverage, color, source)
                .map_err(|e| format!("{e:?}"))?,
            Command::PushClip(r) => list.push_clip(r).map_err(|e| format!("{e:?}"))?,
            Command::PopClip => list.pop_clip().map_err(|e| format!("{e:?}"))?,
        }
    }
    let height = u32::try_from(height.clamp(1, 1200))?;
    let rgba = list
        .rasterize(760, height, Rgb(255, 255, 255))
        .map_err(|e| format!("{e:?}"))?;
    let mut out = std::fs::File::create(args.get(2).ok_or("output PPM required")?)?;
    write!(out, "P6\n760 {height}\n255\n")?;
    for p in rgba.chunks_exact(4) {
        out.write_all(&p[..3])?;
    }
    println!("height={height} stats={stats:?}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_hindi_shaper_reorders_and_clusters_vowel_marks() {
        let face = Face(PageFontSession::bundled().unwrap());
        let text = "हिन्दी";
        let clusters = face.page_shape(text, 24, false, false).unwrap();
        assert_eq!(clusters.first().unwrap().start, 0);
        assert_eq!(clusters.last().unwrap().end, text.len());
        assert!(clusters.len() < text.chars().count());
        assert!(clusters.iter().all(|c| !c.bitmap.coverage.is_empty()));
    }

    #[test]
    fn latin_lines_ignore_unused_fallback_and_mixed_line_grows_once() {
        fn heights(face: &Face, html: &str) -> Vec<i32> {
            let styled =
                kobo_web_document::parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
            let (commands, _, _) =
                paint_basic_page(&BoxTree::from_style(&styled), 700, 500, face).unwrap();
            commands
                .into_iter()
                .filter_map(|c| {
                    if let Command::GlyphRun { bounds, .. } = c {
                        Some(bounds.y)
                    } else {
                        None
                    }
                })
                .collect()
        }
        let plain = Face(
            PageFontSession::from_bytes(kobo_text::TEXT_FONT, kobo_text::DISPLAY_FONT).unwrap(),
        );
        let bundled = Face(PageFontSession::bundled().unwrap());
        let latin="<!doctype html><style>p{margin:0;font-size:24px}</style><p>Latin first</p><p>Latin second</p><p>Latin third</p>";
        assert_eq!(heights(&plain, latin), heights(&bundled, latin));
        let mixed="<!doctype html><style>p{margin:0;font-size:24px}</style><p>Latin first</p><p>हिन्दी</p><p>Latin third</p><p>Latin fourth</p>";
        let ys = heights(&bundled, mixed);
        let base = heights(&bundled, latin);
        assert_eq!(
            ys[3] - ys[2],
            base[2] - base[1],
            "only the used-script line changes the strut"
        );
    }

    #[test]
    fn indic_cluster_shape_survives_a_color_span_boundary() {
        fn ink(html: &str) -> Vec<u8> {
            let styled = kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(
                html.as_bytes(),
                &[],
                &Limits::DEFAULT,
                None,
                kobo_web_document::SupplyBudget::CANONICAL,
            );
            let (commands, _, _) = paint_basic_page(
                &BoxTree::from_style(&styled),
                300,
                200,
                &Face(PageFontSession::bundled().unwrap()),
            )
            .unwrap();
            let mut list = DisplayList::default();
            for c in commands {
                if let Command::GlyphRun {
                    bounds,
                    layout,
                    coverage,
                    source,
                    ..
                } = c
                {
                    list.glyph_run(bounds, layout, coverage, Rgb(0, 0, 0), source)
                        .unwrap();
                }
            }
            list.rasterize(300, 200, Rgb(255, 255, 255)).unwrap()
        }
        let base = "<!doctype html><style>p{margin:0;font-size:28px}</style>";
        let a = ink(&format!("{base}<p>हिन्दी</p>"));
        let b = ink(&format!(
            "{base}<p>ह<span style='color:red'>िन्दी</span></p>"
        ));
        assert_eq!(
            a.chunks_exact(4)
                .zip(b.chunks_exact(4))
                .filter(
                    |(a, b)| (a[..3].iter().any(|&v| v < 255)) != (b[..3].iter().any(|&v| v < 255))
                )
                .count(),
            0
        );
    }

    #[test]
    fn shaped_hindi_block_keeps_following_text_and_has_no_skips() {
        let html = "<!doctype html><p>हिन्दी पाठ</p><p>Русский текст</p><p>Latin after</p>";
        let styled = kobo_web_document::parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
        let (_, stats, height) = paint_basic_page(
            &BoxTree::from_style(&styled),
            300,
            500,
            &Face(PageFontSession::bundled().unwrap()),
        )
        .unwrap();
        assert_eq!(stats.text_blocks_painted, 3);
        assert!(stats.skips.is_empty());
        assert!(height > 0);
    }
}
