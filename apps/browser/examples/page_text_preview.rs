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
