//! Portable host-only entry to the partial page painter.
use kobo_text::page_font_session::PageFontSession;
use kobo_web_document::basic_page::{
    normalize_for_basic_full, paint_basic_page_families, PartialStats,
};
use kobo_web_document::box_tree::BoxTree;
use kobo_web_document::css_text_page::{FontProvider, NormalLineMetrics, PageCluster};
use kobo_web_document::display_list::{Command, DisplayList, Rect, Rgb};
use kobo_web_document::inline_lines::GlyphBitmap;
use kobo_web_document::{
    parse_basic_style_tree, HistoryState, Limits, MediaContext, SnapshotState, SupplyBudget,
};
use std::path::Path;

/// Font files selected explicitly, never discovered from the host.
pub struct Faces {
    regular: PageFace,
    mono: PageFace,
}
impl Faces {
    /// Load the reader's checked-in regular, bold and monospace faces.
    /// # Errors
    /// Returns file or font decoding errors.
    pub fn load(directory: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let regular = std::fs::read(directory.join("AtkinsonHyperlegible-Regular.ttf"))?;
        let bold = std::fs::read(directory.join("AtkinsonHyperlegible-Bold.ttf"))?;
        let mono = std::fs::read(directory.join("DejaVuSansMono.ttf"))?;
        let mono_bold = std::fs::read(directory.join("DejaVuSansMono-Bold.ttf"))?;
        let mut regular_session = PageFontSession::from_bytes(&regular, &bold)?;
        // The bundled mono face covers additional scripts, including Cyrillic.
        // Missing glyphs in every face remain visible boxes, never dropped text.
        regular_session.add_fallback(&std::fs::read(directory.join("NotoSans-Cyrillic.ttf"))?)?;
        regular_session.add_fallback(&std::fs::read(directory.join("NotoSansDevanagari.ttf"))?)?;
        for (name, bold, italic) in [
            ("KoboPage-cyrillic-bold.ttf", true, false),
            ("KoboPage-cyrillic-italic.ttf", false, true),
            ("KoboPage-cyrillic-bolditalic.ttf", true, true),
            ("KoboPage-devanagari-bold.ttf", true, false),
        ] {
            regular_session.add_styled_fallback(
                &std::fs::read(directory.join(name))?,
                bold,
                italic,
            )?;
        }
        regular_session.add_fallback(&mono)?;
        Ok(Self {
            regular: PageFace(regular_session),
            mono: PageFace(PageFontSession::from_bytes(&mono, &mono_bold)?),
        })
    }
}

/// Layout and census. Coverage masks are host research output, not device memory bounds.
pub struct Page {
    commands: Vec<Command>,
    /// Actual selected viewport.
    pub width: u32,
    /// Content height, bounded by the harness to 100,000 pixels.
    pub height: u32,
    /// The unmodified partial-painter census.
    pub stats: PartialStats,
    /// Image data URIs reserved by the temporary intake path.
    pub images: Vec<Vec<u8>>,
    /// Whether the host harness clipped content height.
    pub height_clamped: bool,
}

/// Normalize supplied bytes and lay out once using deterministic bundled fonts.
/// # Errors
/// Refuses invalid viewports and failed paint operations.
pub fn prepare(
    html: &[u8],
    sheets: &[Vec<u8>],
    width: u32,
    faces: &Faces,
) -> Result<Page, &'static str> {
    if !(1..=4096).contains(&width) {
        return Err("width must be in 1..=4096");
    }
    let ((bytes, _), _, _) = normalize_for_basic_full(html);
    // Sheets pass through RAW: `:link`/`:visited` are resolved by the
    // state-aware entry below and `pt` by the length parser.
    let media = MediaContext::new(width)
        .ok_or("invalid media width")?
        // Static capture is a light-scheme profile, so `prefers-color-scheme: dark` links and rules are skipped
        // instead of refusing the page.
        .with_light_scheme();
    // The caller keeps one file per `rel=stylesheet` link, in link order. Links the engine skips by media
    // (for example the dark-scheme sheets) take no host slot, so their files must not be supplied.
    let sheets: Vec<Vec<u8>> = {
        let census_limits = Limits {
            max_input_bytes: 8 * 1024 * 1024,
            ..Limits::DEFAULT
        };
        let page_url = kobo_web_document::url::Url::parse("https://example.invalid/")
            .map_err(|_| "page url")?;
        let census = kobo_web_document::host_stylesheet_links(&bytes, &page_url, &census_limits, Some(media));
        if census.complete && census.link_takes_host_slot.len() == sheets.len() {
            sheets
                .iter()
                .zip(&census.link_takes_host_slot)
                .filter(|(_, keep)| **keep)
                .map(|(sheet, _)| sheet.clone())
                .collect()
        } else {
            sheets.to_vec()
        }
    };
    // Static-state prefilter: off-canvas drawers, screen-reader-only text and false @media blocks are folded
    // before the cascade, because the engine has no transform or clipping support.
    let sheets: Vec<Vec<u8>> = sheets
        .iter()
        .map(|sheet| kobo_web_document::basic_page::prefilter_linked_sheet_for_basic(&bytes, sheet).0)
        .collect();
    let budget = SupplyBudget {
        max_linked_sheets: 48,
        max_sheet_bytes: 2 * 1024 * 1024,
        max_aggregate_bytes: 16 * 1024 * 1024,
    };
    let limits = Limits {
        max_input_bytes: 8 * 1024 * 1024,
        ..Limits::DEFAULT
    };
    // Static capture contract: a fresh profile with no prior navigation, so
    // every href-present hyperlink is unvisited (`:link` matches) and none is
    // visited (`:visited` never matches). No hover/active/focus/target
    // guarantee is asserted; those families keep failing closed.
    let snapshot = SnapshotState {
        history: Some(HistoryState::FreshNoHistory),
        input: None,
        focus: None,
        target: None,
    };
    let style = parse_basic_style_tree(
        &bytes,
        &sheets,
        &limits,
        Some(media),
        budget,
        Some(&snapshot),
    );
    let tree = BoxTree::from_style(&style);
    // Image payloads come from the box tree now that img is a real box
    // kind; the sentinel rectangles in the raster pair with these.
    let images = tree.images_in_order();
    let (mut commands, stats, height) =
        paint_basic_page_families(&tree, width, 1, &faces.regular, &faces.regular, &faces.mono)
            .map_err(|_| "basic paint failed")?;
    let height_px = u32::try_from(height.clamp(1, 100_000)).map_err(|_| "height overflow")?;
    // The measurement viewport is one pixel high. Stretch only the engine's
    // leading canvas fill, not an arbitrary content background, after layout.
    let root = tree.roots[0];
    let body = tree.boxes[root].children.first().copied();
    let has_canvas = tree.boxes[root]
        .style
        .used_background_color()
        .or_else(|| body.and_then(|b| tree.boxes[b].style.used_background_color()))
        .is_some();
    if has_canvas {
        if let Some(Command::Fill { rect, .. }) = commands.first_mut() {
            rect.height = height_px;
        }
    }
    Ok(Page {
        commands,
        width,
        height: height_px,
        stats,
        images,
        height_clamped: height > 100_000,
    })
}

fn translated(rect: Rect, top: i64) -> Option<Rect> {
    Some(Rect {
        y: i32::try_from(i64::from(rect.y) - top).ok()?,
        ..rect
    })
}
fn visible(rect: Rect, top: i64, height: u32) -> bool {
    i64::from(rect.y) + i64::from(rect.height) > top && i64::from(rect.y) < top + i64::from(height)
}
impl Page {
    /// Stable, sorted census text suitable for reviewable regression snapshots.
    #[must_use]
    pub fn census(&self) -> String {
        format!(
            "width={} height={} height_clamped={} blocks={} text_blocks={} glyph_runs={} fills={} output_truncated={} intake_truncated={}\nskips={:?}\napproximations={:?}\nimages={}\n",
            self.width,
            self.height,
            self.height_clamped,
            self.stats.blocks_painted,
            self.stats.text_blocks_painted,
            self.stats.glyph_runs,
            self.stats.fills,
            self.stats.output_truncated,
            self.stats.intake_truncated,
            self.stats.skips,
            self.stats.approximations,
            self.images.len()
        )
    }

    /// Stream RGBA strips without allocating a second full-page framebuffer.
    /// # Errors
    /// Returns raster, display-list budget or output errors. Never silently drops a strip.
    pub fn write_rgba(
        &self,
        output: &mut impl std::io::Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut top = 0_u32;
        while top < self.height {
            let height = 512.min(self.height - top);
            let mut list = DisplayList::default();
            for command in &self.commands {
                let result = match command {
                    Command::Fill {
                        rect,
                        color,
                        source,
                    } if visible(*rect, i64::from(top), height) => list.fill(
                        translated(*rect, i64::from(top)).ok_or("coordinate overflow")?,
                        *color,
                        *source,
                    ),
                    Command::GlyphRun {
                        bounds,
                        layout,
                        coverage,
                        color,
                        source,
                    } if visible(*bounds, i64::from(top), height) => list.glyph_run(
                        translated(*bounds, i64::from(top)).ok_or("coordinate overflow")?,
                        translated(*layout, i64::from(top)).ok_or("coordinate overflow")?,
                        coverage.clone(),
                        *color,
                        *source,
                    ),
                    Command::PushClip(rect) => list
                        .push_clip(translated(*rect, i64::from(top)).ok_or("coordinate overflow")?),
                    Command::PopClip => list.pop_clip(),
                    _ => continue,
                };
                result.map_err(|_| "strip display-list budget exceeded")?;
            }
            let image = list
                .rasterize(self.width, height, Rgb(255, 255, 255))
                .map_err(|_| "strip raster failed")?;
            output.write_all(&image)?;
            top += height;
        }
        Ok(())
    }
}

/// Best-effort Page view adapter. This is intentionally not an exact-paint provider.
struct PageFace(PageFontSession);
impl FontProvider for PageFace {
    fn page_character_metrics(
        &self,
        ch: char,
        size: u32,
        bold: bool,
        italic: bool,
    ) -> Option<NormalLineMetrics> {
        let (height, baseline) = self.0.character_line_metrics(ch, size, bold, italic)?;
        Some(NormalLineMetrics { height, baseline })
    }
    fn page_shape(
        &self,
        text: &str,
        size: u32,
        bold: bool,
        italic: bool,
    ) -> Option<Vec<PageCluster>> {
        Some(
            self.0
                .shape(text, size, bold, italic)?
                .into_iter()
                .map(|cluster| PageCluster {
                    start: cluster.start,
                    end: cluster.end,
                    advance: cluster.glyph.advance,
                    missing: cluster.glyph.missing,
                    bitmap: GlyphBitmap {
                        left: cluster.glyph.left,
                        top: cluster.glyph.top,
                        width: cluster.glyph.width,
                        height: cluster.glyph.height,
                        coverage: cluster.glyph.coverage,
                    },
                })
                .collect(),
        )
    }
    fn has_glyph(&self, ch: char, bold: bool) -> Option<bool> {
        Some(self.0.has_glyph(ch, bold, false))
    }
    fn advance(&self, ch: char, size: u32, bold: bool) -> Option<u32> {
        self.0.advance(ch, size, bold, false)
    }
    fn line_height(&self, size: u32, bold: bool) -> Option<u32> {
        self.0
            .line_metrics(size, bold, false)
            .map(|metrics| metrics.0)
    }
    fn baseline_offset(&self, size: u32, bold: bool) -> Option<i32> {
        self.0
            .line_metrics(size, bold, false)
            .map(|metrics| metrics.1)
    }
    fn raster(&self, ch: char, size: u32, bold: bool) -> Option<GlyphBitmap> {
        self.page_raster(ch, size, bold, false)
    }
    fn page_advance(&self, ch: char, size: u32, bold: bool, italic: bool) -> Option<u32> {
        self.0.advance(ch, size, bold, italic)
    }
    fn page_raster(&self, ch: char, size: u32, bold: bool, italic: bool) -> Option<GlyphBitmap> {
        let glyph = self.0.raster(ch, size, bold, italic)?;
        Some(GlyphBitmap {
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
    use super::*;
    #[test]
    fn page_provider_keeps_bold_and_uses_coverage_fallback() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/kobo-text/fonts");
        let faces = Faces::load(&directory).unwrap();
        assert_ne!(
            faces.regular.raster('A', 24, false).unwrap().coverage,
            faces.regular.raster('A', 24, true).unwrap().coverage
        );
        assert_eq!(faces.regular.has_glyph('Ж', false), Some(true));
        assert_eq!(faces.regular.has_glyph('日', false), Some(false));
        let missing = faces.regular.raster('日', 24, false).unwrap();
        assert!(missing.coverage.contains(&255));
        assert!(faces.regular.advance('日', 24, false).is_some());
    }
}
