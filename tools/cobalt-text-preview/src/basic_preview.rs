//! Portable host-only entry to the partial page painter.
use crate::LocalFace;
use kobo_web_document::basic_page::{
    normalize_css_for_basic, normalize_for_basic_full, paint_basic_page_families, PartialStats,
};
use kobo_web_document::box_tree::BoxTree;
use kobo_web_document::display_list::{Command, DisplayList, Rect, Rgb};
use kobo_web_document::{
    parse_style_tree_with_inline_sheets_and_selectors, Limits, MediaContext, SupplyBudget,
};
use std::path::Path;

/// Font files selected explicitly, never discovered from the host.
pub struct Faces {
    regular: LocalFace,
    mono: LocalFace,
}
impl Faces {
    /// Load the reader's checked-in regular, bold and monospace faces.
    /// # Errors
    /// Returns file or font decoding errors.
    pub fn load(directory: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let regular = std::fs::read(directory.join("AtkinsonHyperlegible-Regular.ttf"))?;
        let bold = std::fs::read(directory.join("AtkinsonHyperlegible-Bold.ttf"))?;
        let mono = std::fs::read(directory.join("DejaVuSansMono.ttf"))?;
        Ok(Self {
            regular: LocalFace::with_verified_normal_bytes(&regular, Some(&bold))?,
            mono: LocalFace::with_verified_normal_bytes(&mono, Some(&mono))?,
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
    let ((bytes, _), images, _, _, _) = normalize_for_basic_full(html);
    let sheets: Vec<Vec<u8>> = sheets
        .iter()
        .map(|s| normalize_css_for_basic(s).0)
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
    let media = MediaContext::new(width).ok_or("invalid media width")?;
    let style = parse_style_tree_with_inline_sheets_and_selectors(
        &bytes,
        &sheets,
        &limits,
        Some(media),
        budget,
    );
    let tree = BoxTree::from_style(&style);
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
        format!("width={} height={} height_clamped={} blocks={} text_blocks={} glyph_runs={} fills={} output_truncated={} intake_truncated={}\nskips={:?}\napproximations={:?}\nimages={}\n", self.width, self.height, self.height_clamped, self.stats.blocks_painted, self.stats.text_blocks_painted, self.stats.glyph_runs, self.stats.fills, self.stats.output_truncated, self.stats.intake_truncated, self.stats.skips, self.stats.approximations, self.images.len())
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
