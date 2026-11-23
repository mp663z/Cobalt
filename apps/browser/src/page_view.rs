//! Page view: the basic painter's output as paged, device-scale screens.
//!
//! The semantic reader stays the default presentation. A Page view paints
//! the arriving page through `basic_page`, cuts the painted column into
//! screens at line boundaries, and rasters one screen at a time at the
//! panel's scale, so a 300 ppi panel gets text at reading size instead of
//! 1:1 CSS pixels. The census the painter returns decides whether the page
//! is shown this way at all: past a skip threshold the reader is the
//! honest presentation and the view falls back to it.
//!
//! Memory stays bounded by construction: the raster buffer is one screen,
//! not one page.

use kobo_text::page_font_session::PageFontSession;
use kobo_web_document::basic_page::{
    normalize_for_basic_full, paint_basic_page_families, PartialStats,
};
use kobo_web_document::box_tree::BoxTree;
use kobo_web_document::css_text_page::{FontProvider, NormalLineMetrics};
use kobo_web_document::display_list::{Command, Rect, Rgb};
use kobo_web_document::inline_lines::GlyphBitmap;
use kobo_web_document::style_tree::StyleTree;
use kobo_web_document::{
    parse_style_tree_with_inline_sheets_and_selectors, Limits, MediaContext, SupplyBudget,
};

/// Device pixels per CSS pixel for the first profiles (Clara BW: 536 CSS px
/// across a 1072 px panel).
pub const DEFAULT_SCALE: u32 = 2;

/// How far below a screen's nominal foot to look for a cut no text line
/// crosses, in CSS px. Deeper backtracking costs more short screens than
/// the unsplit line is worth.
const CUT_BACKTRACK_CSS: u32 = 64;

/// Share of blocks skipped (of blocks attempted) at which the painted view
/// stops being the honest presentation and the reader takes over.
const FALLBACK_SKIP_PERCENT: usize = 25;

/// The largest single content height accepted, in CSS px. Matches the
/// harness clamp; a taller column is truncated paint, which the census
/// already records.
const MAX_CONTENT_HEIGHT_CSS: i64 = 100_000;

/// Lane B's page font session wrapped as the engine's `FontProvider`.
/// Best-effort by contract: coverage is checked before drawing and a
/// missing glyph draws the session's visible box, never a terminal error.
pub(crate) struct PageFontAdapter {
    session: PageFontSession,
}

impl PageFontAdapter {
    /// The bundled licensed faces (Atkinson regular and bold). Complex
    /// scripts need a fallback face added on the session first.
    pub(crate) fn bundled() -> Option<Self> {
        PageFontSession::bundled()
            .ok()
            .map(|session| Self { session })
    }

    fn bitmap(&self, character: char, size: u32, bold: bool, italic: bool) -> Option<GlyphBitmap> {
        let glyph = self.session.raster(character, size, bold, italic)?;
        Some(GlyphBitmap {
            left: glyph.left,
            top: glyph.top,
            width: glyph.width,
            height: glyph.height,
            coverage: glyph.coverage,
        })
    }
}

impl FontProvider for PageFontAdapter {
    fn page_advance(&self, character: char, size: u32, bold: bool, italic: bool) -> Option<u32> {
        self.session.advance(character, size, bold, italic)
    }

    fn page_raster(
        &self,
        character: char,
        size: u32,
        bold: bool,
        italic: bool,
    ) -> Option<GlyphBitmap> {
        self.bitmap(character, size, bold, italic)
    }

    fn has_glyph(&self, character: char, bold: bool) -> Option<bool> {
        Some(self.session.has_glyph(character, bold, false))
    }

    fn normal_line_metrics(&self, size: u32, bold: bool) -> Option<NormalLineMetrics> {
        let (height, baseline) = self.session.line_metrics(size, bold, false)?;
        Some(NormalLineMetrics { height, baseline })
    }

    fn advance(&self, character: char, size: u32, bold: bool) -> Option<u32> {
        self.session.advance(character, size, bold, false)
    }

    fn line_height(&self, size: u32, bold: bool) -> Option<u32> {
        self.session.line_metrics(size, bold, false).map(|(h, _)| h)
    }

    fn baseline_offset(&self, size: u32, bold: bool) -> Option<i32> {
        self.session.line_metrics(size, bold, false).map(|(_, b)| b)
    }

    fn raster(&self, character: char, size: u32, bold: bool) -> Option<GlyphBitmap> {
        self.bitmap(character, size, bold, false)
    }
}

/// A painted page, laid out once and rastered per screen.
pub(crate) struct PageView {
    commands: Vec<Command>,
    stats: PartialStats,
    /// Link targets in first-paint order; `TapMap` actions index this.
    links: Vec<String>,
    /// Tap targets resolved at paint time: `(rect, link index)`.
    targets: Vec<(Rect, usize)>,
    /// Whole painted column, CSS px.
    content_height: u32,
    css_width: u32,
    scale: u32,
    /// Screen height in CSS px (panel height / scale, less chrome).
    page_height_css: u32,
    /// Page start offsets in the painted column; page `i` spans
    /// `cuts[i]..cuts.get(i+1).unwrap_or(content_height)`.
    cuts: Vec<u32>,
}

impl PageView {
    /// Paints `bytes` (already fetched; linked sheets the caller already
    /// holds are passed in) at `css_width` and cuts the column into
    /// screens of `page_height_css`.
    pub(crate) fn paint<F: FontProvider>(
        bytes: &[u8],
        css_width: u32,
        page_height_css: u32,
        scale: u32,
        serif: &F,
        sans: &F,
        mono: &F,
    ) -> Result<Self, &'static str> {
        if css_width == 0 || page_height_css == 0 || scale == 0 {
            return Err("viewport");
        }
        let ((bytes, _prefilter), _images, _hoisted, _rows, _chrome) =
            normalize_for_basic_full(bytes);
        let budget = SupplyBudget {
            max_linked_sheets: 48,
            max_sheet_bytes: 2 * 1024 * 1024,
            max_aggregate_bytes: 16 * 1024 * 1024,
        };
        let limits = Limits {
            max_input_bytes: 8 * 1024 * 1024,
            ..Limits::DEFAULT
        };
        let media = MediaContext::new(css_width).ok_or("viewport")?;
        let tree = parse_style_tree_with_inline_sheets_and_selectors(
            &bytes,
            &[],
            &limits,
            Some(media),
            budget,
        );
        let boxes = BoxTree::from_style(&tree);
        // Pass 1 measures the column; pass 2 paints with a canvas that
        // covers it, the same two passes the research harness runs.
        let (_, _, measured) = paint_basic_page_families(&boxes, css_width, 1, serif, sans, mono)?;
        let content_height =
            u32::try_from(measured.clamp(1, MAX_CONTENT_HEIGHT_CSS)).map_err(|_| "viewport")?;
        let (commands, stats, _) =
            paint_basic_page_families(&boxes, css_width, content_height, serif, sans, mono)?;
        let cuts = paginate(&commands, content_height, page_height_css);
        let (links, targets) = link_targets(&tree, &commands);
        Ok(Self {
            commands,
            stats,
            links,
            targets,
            content_height,
            css_width,
            scale,
            page_height_css,
            cuts,
        })
    }

    /// The painter's census, for the fallback decision and for reporting.
    pub(crate) const fn stats(&self) -> &PartialStats {
        &self.stats
    }

    pub(crate) fn pages(&self) -> usize {
        self.cuts.len()
    }

    /// Screen dimensions in device pixels: `(width, height)`.
    pub(crate) fn screen_device_px(&self) -> (u32, u32) {
        (
            self.css_width * self.scale,
            self.page_height_css * self.scale,
        )
    }

    /// Whether the reader is the honest presentation for this page: the
    /// intake itself was cut short, or too much of the page never painted.
    pub(crate) fn falls_back_to_reader(&self) -> bool {
        falls_back(&self.stats)
    }

    /// Rasters one screen as greyscale, one byte per device pixel, white
    /// where nothing painted. `page` is clamped to the last screen.
    pub(crate) fn raster_page_grey(&self, page: usize) -> Vec<u8> {
        let rgb = self.raster_page_rgb(page);
        rgb.chunks_exact(3)
            .map(|px| luma(px[0], px[1], px[2]))
            .collect()
    }

    /// Rasters one screen as RGB, three bytes per device pixel.
    pub(crate) fn raster_page_rgb(&self, page: usize) -> Vec<u8> {
        let (width, height) = self.screen_device_px();
        let mut buffer = vec![255u8; width as usize * height as usize * 3];
        let top = self
            .cuts
            .get(page.min(self.cuts.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0);
        let bottom = top.saturating_add(self.page_height_css);
        let mut clips: Vec<Rect> = Vec::new();
        for command in &self.commands {
            match command {
                Command::Fill { rect, color, .. } => {
                    let Some(rect) = shift(rect, top, bottom) else {
                        continue;
                    };
                    fill_rect(
                        &mut buffer,
                        width,
                        height,
                        &rect,
                        self.scale,
                        *color,
                        &clips,
                    );
                }
                Command::GlyphRun {
                    bounds,
                    coverage,
                    color,
                    ..
                } => {
                    let Some(bounds) = shift(bounds, top, bottom) else {
                        continue;
                    };
                    paint_glyphs(
                        &mut buffer,
                        width,
                        height,
                        &bounds,
                        coverage,
                        self.scale,
                        *color,
                        &clips,
                    );
                }
                Command::PushClip(rect) => {
                    if let Some(rect) = shift(rect, top, bottom) {
                        clips.push(rect);
                    }
                }
                Command::PopClip => {
                    clips.pop();
                }
            }
        }
        buffer
    }

    /// A tap map over the painted column: every command carrying a source
    /// action contributes its rect. Resolution expands each target to a
    /// minimum tap size and refuses ambiguous clusters, where the caller
    /// falls back to a link list. Empty until the painter attaches
    /// actions; the resolution logic is exercised by unit tests with
    /// synthetic commands.
    pub(crate) fn tap_map(&self) -> TapMap {
        TapMap {
            targets: self.targets.clone(),
        }
    }

    /// The page's link targets in first-paint order, for the Links list.
    pub(crate) fn links(&self) -> &[String] {
        &self.links
    }
}

/// Resolves every command's source node to the nearest ancestor `a` href,
/// interning targets in first-paint order. The walk is depth-bounded: the
/// tree comes from an untrusted document.
fn link_targets(tree: &StyleTree, commands: &[Command]) -> (Vec<String>, Vec<(Rect, usize)>) {
    let mut links: Vec<String> = Vec::new();
    let mut targets: Vec<(Rect, usize)> = Vec::new();
    for command in commands {
        let (rect, node) = match command {
            Command::Fill { rect, source, .. } => (rect, source.node),
            Command::GlyphRun { layout, source, .. } => (layout, source.node),
            Command::PushClip(_) | Command::PopClip => continue,
        };
        let Some(node) = node else {
            continue;
        };
        let mut at = Some(node);
        let mut depth = 0;
        let href = loop {
            let Some(i) = at else { break None };
            let Some(styled) = tree.nodes.get(i) else {
                break None;
            };
            if let Some(href) = &styled.href {
                break Some(href.clone());
            }
            depth += 1;
            if depth >= 64 {
                break None;
            }
            at = styled.parent;
        };
        if let Some(href) = href {
            let action = links
                .iter()
                .position(|known| known == &href)
                .unwrap_or_else(|| {
                    links.push(href);
                    links.len() - 1
                });
            targets.push((*rect, action));
        }
    }
    (links, targets)
}

/// The fallback decision on a census alone, so the app can ask before it
/// keeps a painted view around.
pub(crate) fn falls_back(stats: &PartialStats) -> bool {
    if stats.intake_truncated {
        return true;
    }
    let skipped: usize = stats.skips.values().sum();
    let attempted = skipped + stats.blocks_painted;
    attempted > 0 && skipped * 100 >= FALLBACK_SKIP_PERCENT * attempted
}

/// Page start offsets: cut at the nominal screen foot, or just above it
/// where no painted rect crosses. When every candidate in the backtrack
/// window is crossed, the cut lands on the nominal foot and whatever
/// crosses it is split.
fn paginate(commands: &[Command], content_height: u32, page_height: u32) -> Vec<u32> {
    let mut ink: Vec<(i64, i64)> = Vec::new();
    for command in commands {
        let rect = match command {
            Command::GlyphRun { layout, .. } => Some(layout),
            Command::Fill { rect, .. } => Some(rect),
            Command::PushClip(_) | Command::PopClip => None,
        };
        if let Some(rect) = rect {
            ink.push((
                i64::from(rect.y),
                i64::from(rect.y) + i64::from(rect.height),
            ));
        }
    }
    let mut cuts = vec![0];
    let mut foot = u64::from(page_height);
    while foot < u64::from(content_height) {
        let target = i64::try_from(foot).unwrap_or(i64::MAX);
        let floor = target - i64::from(CUT_BACKTRACK_CSS);
        let mut cut = target;
        while cut > floor && crosses(&ink, cut) {
            cut -= 1;
        }
        if crosses(&ink, cut) {
            // The whole backtrack window is crossed (a rect taller than a
            // screen): hard-cut on the nominal foot.
            cut = target;
        }
        let previous = i64::from(*cuts.last().unwrap_or(&0));
        cuts.push(u32::try_from(cut.max(previous + 1)).unwrap_or(content_height));
        foot = u64::from(*cuts.last().unwrap_or(&0)) + u64::from(page_height);
    }
    cuts
}

fn crosses(ink: &[(i64, i64)], y: i64) -> bool {
    ink.iter().any(|&(top, bottom)| top < y && y < bottom)
}

/// Translates a CSS-px rect into screen coordinates for the page spanning
/// `top..bottom`, dropping what the page does not show.
fn shift(rect: &Rect, top: u32, bottom: u32) -> Option<Rect> {
    let y = i64::from(rect.y) - i64::from(top);
    if y + i64::from(rect.height) <= 0 || y >= i64::from(bottom) - i64::from(top) {
        return None;
    }
    Some(Rect {
        x: rect.x,
        y: i32::try_from(y).ok()?,
        width: rect.width,
        height: rect.height,
    })
}

fn luma(r: u8, g: u8, b: u8) -> u8 {
    // ITU-R 601 luma, the weights the reader's picture path already uses.
    u8::try_from((u32::from(r) * 77 + u32::from(g) * 150 + u32::from(b) * 29 + 128) >> 8)
        .unwrap_or(u8::MAX)
}

fn blend(pixel: &mut [u8], color: Rgb, coverage: u8) {
    if coverage == 0 {
        return;
    }
    if coverage == 255 {
        pixel[0] = color.0;
        pixel[1] = color.1;
        pixel[2] = color.2;
        return;
    }
    let a = u32::from(coverage);
    let inv = 255 - a;
    for (channel, ink) in pixel.iter_mut().zip([color.0, color.1, color.2]) {
        *channel = u8::try_from((u32::from(*channel) * inv + u32::from(ink) * a + 127) / 255)
            .unwrap_or(u8::MAX);
    }
}

/// The clip stack intersected with the screen, in device px.
fn clip_bounds(
    clips: &[Rect],
    scale: u32,
    width: u32,
    height: u32,
) -> Option<(i64, i64, i64, i64)> {
    let mut bounds = (0i64, 0i64, i64::from(width), i64::from(height));
    for clip in clips {
        let s = i64::from(scale);
        let (x0, y0) = (i64::from(clip.x) * s, i64::from(clip.y) * s);
        let (x1, y1) = (
            x0 + i64::from(clip.width) * s,
            y0 + i64::from(clip.height) * s,
        );
        bounds.0 = bounds.0.max(x0);
        bounds.1 = bounds.1.max(y0);
        bounds.2 = bounds.2.min(x1);
        bounds.3 = bounds.3.min(y1);
    }
    (bounds.0 < bounds.2 && bounds.1 < bounds.3).then_some(bounds)
}

fn fill_rect(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    rect: &Rect,
    scale: u32,
    color: Rgb,
    clips: &[Rect],
) {
    let Some((clip_left, clip_top, clip_right, clip_bottom)) =
        clip_bounds(clips, scale, width, height)
    else {
        return;
    };
    let s = i64::from(scale);
    let x0 = (i64::from(rect.x) * s).max(clip_left);
    let y0 = (i64::from(rect.y) * s).max(clip_top);
    let x1 = (i64::from(rect.x) * s + i64::from(rect.width) * s).min(clip_right);
    let y1 = (i64::from(rect.y) * s + i64::from(rect.height) * s).min(clip_bottom);
    // clip_bounds keeps these within the screen, so the conversions hold on
    // 32-bit targets too.
    let (x0, x1) = (
        usize::try_from(x0).unwrap_or(0),
        usize::try_from(x1).unwrap_or(0),
    );
    let (y0, y1) = (
        usize::try_from(y0).unwrap_or(0),
        usize::try_from(y1).unwrap_or(0),
    );
    let stride = width as usize;
    for y in y0..y1 {
        for x in x0..x1 {
            let at = (y * stride + x) * 3;
            buffer[at] = color.0;
            buffer[at + 1] = color.1;
            buffer[at + 2] = color.2;
        }
    }
}

/// Paints a coverage mask at `scale`: each mask pixel covers a `scale x
/// scale` device block (nearest neighbour). Text drawn this way is sized
/// for the panel but upscaled from a 1x mask; the text lane's font session
/// replaces the masks with native-size rasters when it lands, and the seam
/// is this one function.
#[allow(clippy::too_many_arguments)]
fn paint_glyphs(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    bounds: &Rect,
    coverage: &[u8],
    scale: u32,
    color: Rgb,
    clips: &[Rect],
) {
    let Some((clip_left, clip_top, clip_right, clip_bottom)) =
        clip_bounds(clips, scale, width, height)
    else {
        return;
    };
    let s = i64::from(scale);
    let mask_w = i64::from(bounds.width);
    let mask_h = i64::from(bounds.height);
    if mask_w <= 0 || mask_h <= 0 || i64::try_from(coverage.len()).ok() != Some(mask_w * mask_h) {
        return;
    }
    let base_x = i64::from(bounds.x) * s;
    let base_y = i64::from(bounds.y) * s;
    let x0 = base_x.max(clip_left);
    let y0 = base_y.max(clip_top);
    let x1 = (base_x + mask_w * s).min(clip_right);
    let y1 = (base_y + mask_h * s).min(clip_bottom);
    for y in y0..y1 {
        let mask_y = (y - base_y) / s;
        let Ok(row) = usize::try_from(y) else {
            continue;
        };
        for x in x0..x1 {
            let mask_x = (x - base_x) / s;
            let (Ok(col), Ok(mask_at)) = (
                usize::try_from(x),
                usize::try_from(mask_y * mask_w + mask_x),
            ) else {
                continue;
            };
            let a = coverage[mask_at];
            let at = (row * width as usize + col) * 3;
            blend(&mut buffer[at..at + 3], color, a);
        }
    }
}

/// Tap targets over a painted column, in CSS px.
pub(crate) struct TapMap {
    /// `(rect, action)` in paint order; the last painted target wins a hit.
    targets: Vec<(Rect, usize)>,
}

/// Half the minimum tap target, in CSS px. A 44 CSS px target at 2x is
/// 88 device px, about 7.5 mm at 300 ppi.
const MIN_TAP_RADIUS_CSS: i32 = 22;

impl TapMap {
    pub(crate) fn from_commands(commands: &[Command]) -> Self {
        let mut targets = Vec::new();
        for command in commands {
            let (rect, source) = match command {
                Command::Fill { rect, source, .. } => (rect, source),
                Command::GlyphRun { layout, source, .. } => (layout, source),
                Command::PushClip(_) | Command::PopClip => continue,
            };
            if let Some(action) = source.action {
                targets.push((*rect, action));
            }
        }
        Self { targets }
    }

    /// The action at `(x, y)` in CSS px, or `None` when nothing is there
    /// or the tap lands in an overlap of two different expanded targets (a
    /// dense cluster, which the caller offers as a link list instead).
    pub(crate) fn resolve(&self, x: i32, y: i32) -> Option<usize> {
        let mut hit: Option<usize> = None;
        for (rect, action) in &self.targets {
            let x0 = rect.x - MIN_TAP_RADIUS_CSS;
            let y0 = rect.y - MIN_TAP_RADIUS_CSS;
            let x1 = rect.x + i32::try_from(rect.width).ok()? + MIN_TAP_RADIUS_CSS;
            let y1 = rect.y + i32::try_from(rect.height).ok()? + MIN_TAP_RADIUS_CSS;
            if x >= x0 && x < x1 && y >= y0 && y < y1 {
                if hit.is_some_and(|prior| prior != *action) {
                    return None;
                }
                hit = Some(*action);
            }
        }
        hit
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kobo_web_document::display_list::Source;

    fn fill(x: i32, y: i32, w: u32, h: u32, action: Option<usize>) -> Command {
        Command::Fill {
            rect: Rect {
                x,
                y,
                width: w,
                height: h,
            },
            color: Rgb(0, 0, 0),
            source: Source { node: None, action },
        }
    }

    fn glyph_line(y: i32, h: u32) -> Command {
        Command::GlyphRun {
            bounds: Rect {
                x: 0,
                y,
                width: 100,
                height: h,
            },
            layout: Rect {
                x: 0,
                y,
                width: 100,
                height: h,
            },
            coverage: vec![255; (100 * h) as usize],
            color: Rgb(0, 0, 0),
            source: Source {
                node: None,
                action: None,
            },
        }
    }

    #[test]
    fn pagination_cuts_between_lines() {
        // Ten 25px lines with 5px gaps, screen 200px: the foot lands inside
        // the line at 180..205, so the cut must move up to its top (y=180).
        let mut commands = Vec::new();
        for line in 0..10 {
            commands.push(glyph_line(line * 30, 25));
        }
        let cuts = paginate(&commands, 300, 200);
        assert_eq!(cuts, vec![0, 180]);
        for &cut in &cuts[1..] {
            for line in 0..10 {
                let (top, bottom) = (line * 30, line * 30 + 25);
                assert!(!(top < i64::from(cut) && i64::from(cut) < bottom));
            }
        }
    }

    #[test]
    fn pagination_hard_cuts_a_wall_of_text() {
        // One line taller than the backtrack window: the cut lands on the
        // nominal foot rather than search forever.
        let cuts = paginate(&[glyph_line(10, 500)], 600, 200);
        assert_eq!(cuts, vec![0, 200, 400]);
    }

    #[test]
    fn raster_scales_fills_and_upsizes_the_screen() {
        let view = PageView {
            commands: vec![fill(5, 5, 10, 10, None)],
            stats: PartialStats::default(),
            links: Vec::new(),
            targets: Vec::new(),
            content_height: 100,
            css_width: 100,
            scale: 2,
            page_height_css: 50,
            cuts: vec![0, 50],
        };
        assert_eq!(view.screen_device_px(), (200, 100));
        assert_eq!(view.pages(), 2);
        let rgb = view.raster_page_rgb(0);
        assert_eq!(rgb.len(), 200 * 100 * 3);
        // The 10x10 CSS fill at (5,5) covers device (10..30, 10..30).
        let at = |x: usize, y: usize| &rgb[(y * 200 + x) * 3..(y * 200 + x) * 3 + 3];
        assert_eq!(at(15, 15), &[0, 0, 0]);
        assert_eq!(at(29, 29), &[0, 0, 0]);
        assert_eq!(at(9, 15), &[255, 255, 255]);
        assert_eq!(at(30, 15), &[255, 255, 255]);
    }

    #[test]
    fn raster_translates_the_second_screen() {
        let view = PageView {
            commands: vec![fill(0, 60, 20, 10, None)],
            stats: PartialStats::default(),
            links: Vec::new(),
            targets: Vec::new(),
            content_height: 100,
            css_width: 50,
            scale: 2,
            page_height_css: 50,
            cuts: vec![0, 50],
        };
        let rgb = view.raster_page_rgb(1);
        let at = |x: usize, y: usize| &rgb[(y * 100 + x) * 3..(y * 100 + x) * 3 + 3];
        // Fill at CSS y 60 lands at screen y 10 -> device 20.
        assert_eq!(at(10, 25), &[0, 0, 0]);
        assert_eq!(at(10, 5), &[255, 255, 255]);
    }

    #[test]
    fn grey_conversion_darkens_ink() {
        assert_eq!(luma(255, 255, 255), 255);
        assert_eq!(luma(0, 0, 0), 0);
        let grey = PageView {
            commands: vec![fill(0, 0, 4, 4, None)],
            stats: PartialStats::default(),
            links: Vec::new(),
            targets: Vec::new(),
            content_height: 10,
            css_width: 10,
            scale: 1,
            page_height_css: 10,
            cuts: vec![0],
        }
        .raster_page_grey(0);
        assert_eq!(grey.len(), 100);
        assert_eq!(grey[0], 0);
    }

    #[test]
    fn fallback_on_truncated_intake() {
        let stats = PartialStats {
            intake_truncated: true,
            ..PartialStats::default()
        };
        assert!(falls_back(&stats));
    }

    #[test]
    fn fallback_on_skip_share() {
        let mut stats = PartialStats {
            blocks_painted: 100,
            ..PartialStats::default()
        };
        assert!(!falls_back(&stats));
        stats.blocks_painted = 75;
        stats.skips.insert("non-ascii-text", 25);
        assert!(falls_back(&stats));
    }

    #[test]
    fn painted_page_collects_link_targets() {
        let face = PageFontAdapter::bundled().expect("bundled faces");
        let html = b"<html><body><p><a href='https://x.invalid/first'>one</a></p>                     <p><a href='https://x.invalid/second'>two</a></p></body></html>";
        let view = PageView::paint(html, 536, 724, 2, &face, &face, &face).expect("paint");
        assert_eq!(
            view.links(),
            &[
                "https://x.invalid/first".to_string(),
                "https://x.invalid/second".to_string()
            ]
        );
        let map = view.tap_map();
        assert!(!map.is_empty());
        let (rect, action) = map.targets[0];
        assert_eq!(view.links()[action], "https://x.invalid/first");
        assert_eq!(map.resolve(rect.x, rect.y), Some(action));
    }

    #[test]
    fn tap_map_resolves_and_refuses_clusters() {
        let map = TapMap::from_commands(&[
            fill(10, 10, 20, 10, Some(0)),
            fill(200, 10, 20, 10, Some(1)),
        ]);
        assert_eq!(map.resolve(15, 15), Some(0));
        assert_eq!(map.resolve(205, 15), Some(1));
        assert_eq!(map.resolve(100, 100), None);
        // Expanded targets overlap between the two links: refuse, so the
        // caller can offer a list rather than guess.
        let dense =
            TapMap::from_commands(&[fill(10, 10, 20, 10, Some(0)), fill(34, 10, 20, 10, Some(1))]);
        assert_eq!(dense.resolve(32, 15), None);
    }

    #[test]
    fn tap_map_ignores_actionless_commands() {
        let map = TapMap::from_commands(&[fill(0, 0, 100, 100, None)]);
        assert!(map.is_empty());
        assert_eq!(map.resolve(5, 5), None);
    }

    #[test]
    fn bundled_page_font_measures_and_rasters() {
        let face = PageFontAdapter::bundled().expect("bundled faces");
        let advance = face.page_advance('A', 16, false, false);
        assert!(advance.is_some_and(|a| a > 0));
        let bitmap = face.page_raster('A', 16, false, false).expect("glyph");
        assert_eq!(
            bitmap.coverage.len(),
            bitmap.width as usize * bitmap.height as usize
        );
        let metrics = face.normal_line_metrics(16, false).expect("metrics");
        assert!(metrics.height > 0 && metrics.baseline > 0);
    }
}
