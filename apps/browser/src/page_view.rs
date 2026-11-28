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
use kobo_web_document::css_text_page::{FontProvider, NormalLineMetrics, PageCluster};
use kobo_web_document::display_list::{Command, Rect, Rgb};
use kobo_web_document::inline_lines::GlyphBitmap;
use kobo_web_document::style_tree::StyleTree;
use kobo_web_document::{parse_basic_style_tree, Limits, MediaContext};
#[cfg(test)]
use kobo_web_document::SupplyBudget;

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
    session: std::rc::Rc<PageFontSession>,
}

impl PageFontAdapter {
    /// The bundled licensed faces (Atkinson regular and bold). Complex
    /// scripts need a fallback face added on the session first.
    pub(crate) fn bundled() -> Option<Self> {
        PageFontSession::bundled()
            .ok()
            .map(|session| Self { session: std::rc::Rc::new(session) })
    }

    fn load_monospace() -> Option<std::rc::Rc<PageFontSession>> {
        PageFontSession::from_bytes(
            kobo_text::MONO_FONT,
            kobo_text::MONO_BOLD_FONT,
        ).ok().map(std::rc::Rc::new)
    }

    /// One immutable regular/bold pair per app thread. No glyph cache or
    /// document data is retained. Failed construction is not cached.
    pub(crate) fn monospace() -> Option<Self> {
        thread_local! {
            static MONO: std::cell::RefCell<Option<std::rc::Rc<PageFontSession>>> = const { std::cell::RefCell::new(None) };
        }
        #[cfg(debug_assertions)]
        if std::env::var("KOBO_SIM_UNCACHED_MONO").as_deref() == Ok("1") {
            return Self::load_monospace().map(|session| Self { session });
        }
        MONO.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_none() { *slot = Self::load_monospace(); }
            slot.as_ref().map(|session| Self { session: std::rc::Rc::clone(session) })
        })
    }

    #[cfg(test)]
    pub(crate) fn monospace_uncached() -> Option<Self> {
        Self::load_monospace().map(|session| Self { session })
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

    fn page_character_metrics(
        &self,
        character: char,
        size: u32,
        bold: bool,
        italic: bool,
    ) -> Option<NormalLineMetrics> {
        let (height, baseline) = self
            .session
            .character_line_metrics(character, size, bold, italic)?;
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
            self.session
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
    images: std::collections::BTreeMap<usize, (Rgb, Option<kobo_image::Picture>)>,
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
        Self::paint_with_sheets(bytes, &[], css_width, page_height_css, scale, serif, sans, mono)
    }

    /// Like [`Self::paint`], with the page's external stylesheets. `sheets` must be exactly the bodies of
    /// `page_mode::sheet_census(..).links`, in that order (the host supply contract); a count mismatch makes the
    /// engine refuse the style tree, so pass `&[]` only for a page without external sheets.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_with_sheets<F: FontProvider>(
        bytes: &[u8],
        sheets: &[Vec<u8>],
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
        let ((bytes, _prefilter), _rows, _chrome) = normalize_for_basic_full(bytes);
        let budget = crate::page_mode::SHEET_BUDGET;
        let limits = Limits {
            max_input_bytes: 8 * 1024 * 1024,
            ..Limits::DEFAULT
        };
        let media = MediaContext::new(css_width).ok_or("viewport")?.with_light_scheme();
        let sheets: Vec<Vec<u8>> = sheets.iter().map(|sheet| kobo_web_document::basic_page::prefilter_linked_sheet_for_basic(&bytes, sheet).0).collect();
        let tree = parse_basic_style_tree(&bytes, &sheets, &limits, Some(media), budget, None);
        // Incomplete intake cannot become a publishable page. In particular,
        // do not lay out a long unstyled column while its CSS is still pending.
        if tree.diagnostics.count(kobo_web_document::style_diagnostics::RefusalReason::MissingStylesheet)>0 {return Err("pending stylesheet");}
        let mut boxes = BoxTree::from_style(&tree);
        // Pass 1 measures the column; pass 2 paints with a canvas that
        // covers it, the same two passes the research harness runs.
        let (_, _, measured) = paint_basic_page_families(&boxes, css_width, 1, serif, sans, mono)?;
        let mut content_height =
            u32::try_from(measured.clamp(1, MAX_CONTENT_HEIGHT_CSS)).map_err(|_| "viewport")?;
        let (mut commands, mut stats, _) =
            paint_basic_page_families(&boxes, css_width, content_height, serif, sans, mono)?;
        let mut image_stats = PartialStats::default();
        let images = crate::page_images::prepare(&boxes, &mut image_stats);
        if crate::page_images::fit_geometry(&mut boxes, &commands, &images) {
            let (_, _, measured) =
                paint_basic_page_families(&boxes, css_width, 1, serif, sans, mono)?;
            content_height =
                u32::try_from(measured.clamp(1, MAX_CONTENT_HEIGHT_CSS)).map_err(|_| "viewport")?;
            (commands, stats, _) =
                paint_basic_page_families(&boxes, css_width, content_height, serif, sans, mono)?;
        }
        for (key, count) in image_stats.skips {
            *stats.skips.entry(key).or_default() += count;
        }
        for (key, count) in image_stats.approximations {
            *stats.approximations.entry(key).or_default() += count;
        }
        let cuts = paginate(&commands, content_height, page_height_css);
        let (links, targets) = link_targets(&tree, &mut commands);
        Ok(Self {
            commands,
            stats,
            images,
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

    /// End an address-editing preview between text lines without reflowing it.
    pub(crate) fn preview_height(&self, page: usize, room: u32) -> u32 {
        let top = self.cuts.get(page).copied().unwrap_or(0);
        let nominal = top.saturating_add(room / self.scale.max(1));
        let mut cut = nominal;
        while cut > top && nominal - cut < CUT_BACKTRACK_CSS {
            let crosses = self.commands.iter().any(|command| match command {
                Command::GlyphRun { layout, .. } => {
                    i64::from(layout.y) < i64::from(cut)
                        && i64::from(layout.y) + i64::from(layout.height) > i64::from(cut)
                }
                _ => false,
            });
            if !crosses {
                return (cut - top).saturating_mul(self.scale).max(1);
            }
            cut -= 1;
        }
        room.max(1)
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
    pub(crate) fn raster_page_rgb(&self,page:usize)->Vec<u8>{
        self.raster_window_rgb(self.page_top(page),self.page_bottom(page))
    }
    pub(crate) fn page_top(&self,page:usize)->u32{self.cuts.get(page.min(self.pages().saturating_sub(1))).copied().unwrap_or(0)}
    fn page_bottom(&self,page:usize)->u32{self.cuts.get(page+1).copied().unwrap_or(self.content_height)}
    pub(crate) fn viewport_height(&self)->u32{self.page_height_css}
    pub(crate) fn max_scroll(&self)->u32{self.content_height.saturating_sub(self.page_height_css)}
    /// UTF-8 source byte at the first visible painted text fragment.
    pub(crate) fn reading_anchor(&self, top:u32)->Option<(usize,usize,u32)> {
        for c in &self.commands {
            if let Command::GlyphRun{layout,source,..}=c {
                let (Some(node),Some((start,_)))=(source.node,source.text_range) else {continue};
                if i64::from(layout.y)+i64::from(layout.height)>i64::from(top) {
                    let clipped=i64::from(top).saturating_sub(i64::from(layout.y)).max(0) as u32;
                    return Some((node,start,clipped.saturating_mul(1000)/layout.height.max(1)));
                }
            }
        }
        None
    }
    pub(crate) fn anchor_top(&self, anchor:(usize,usize,u32))->Option<u32> {
        self.commands.iter().find_map(|c| {
            let Command::GlyphRun{layout,source,..}=c else{return None};
            let (start,end)=source.text_range?;
            if source.node!=Some(anchor.0)||anchor.1<start||anchor.1>=end{return None;}
            Some((i64::from(layout.y)+i64::from(layout.height)*i64::from(anchor.2)/1000).max(0) as u32)
        })
    }

    pub(crate) fn page_at(&self,top:u32)->usize{self.cuts.partition_point(|cut|*cut<=top).saturating_sub(1)}
    pub(crate) fn raster_window_rgb(&self,top:u32,bottom:u32)->Vec<u8>{
        let (width,height)=self.screen_device_px();
        let mut buffer=vec![255u8;width as usize*height as usize*3];
        let mut clips: Vec<Rect> = vec![Rect {
            x: 0,
            y: 0,
            width: self.css_width,
            height: bottom.saturating_sub(top),
        }];
        for command in &self.commands {
            match command {
                Command::Fill { rect, color, source } => {
                    let image_rect = *rect;
                    let Some(rect) = shift(rect, top, bottom) else {
                        continue;
                    };
                    if let Some((marker,image)) = source.node.and_then(|node| self.images.get(&node)) {
                        if color == marker {
                            fill_rect(&mut buffer,width,height,&rect,self.scale,Rgb(255,255,255),&clips);
                            if let Some(image) = image {
                                paint_image(&mut buffer, width, height, &image_rect, top, self.scale, image, &clips);
                            }
                            continue;
                        }
                        // Alt-text decoration shares the image source too.
                        if image.is_some() { continue; }
                    }
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
                    source,
                    ..
                } => {
                    // The image node's text is its alt fallback. Keep it for
                    // refusals, but do not draw it over a decoded picture.
                    if source.node.and_then(|node| self.images.get(&node))
                        .is_some_and(|(_,image)| image.is_some()) {
                        continue;
                    }
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
                    clips.push(shift(rect, top, bottom).unwrap_or(Rect {
                        x: 0,
                        y: 0,
                        width: 0,
                        height: 0,
                    }));
                }
                Command::PopClip => {
                    if clips.len() > 1 {
                        clips.pop();
                    }
                }
            }
        }
        buffer
    }

    /// A tap map over the painted column: every command carrying a source
    /// action contributes its rect. Resolution expands each target to a
    /// minimum tap size and refuses ambiguous clusters. The app uses the
    /// scoped Links list for ambiguous targets.
    pub(crate) fn tap_map(&self) -> TapMap {
        TapMap {
            targets: self.targets.clone(),
        }
    }

    /// Visible link regions in source pixels of the uploaded picture. Clip
    /// before scaling, matching the raster crop and keyboard preview exactly.
    pub(crate) fn picture_links(&self,page:usize,width:u32,height:u32,full_height:u32)->Vec<(kobo_sdk::PictureLink,usize)>{
        self.window_links(self.page_top(page),self.page_bottom(page),width,height,full_height)
    }
    pub(crate) fn window_links(&self,top:u32,bottom:u32,width:u32,height:u32,full_height:u32)->Vec<(kobo_sdk::PictureLink,usize)>{
        let (sw,sh)=self.screen_device_px();
        let sx=|n:i64| (n.max(0) as u64*u64::from(self.scale)*u64::from(width)/u64::from(sw)) as u32;
        let sy=|n:i64| (n.max(0) as u64*u64::from(self.scale)*u64::from(full_height)/u64::from(sh)) as u32;
        self.targets.iter().filter_map(|(r,index)|{
            let x0=sx(i64::from(r.x).max(0)).min(width);
            let x1=sx((i64::from(r.x)+i64::from(r.width)).min(i64::from(self.css_width))).min(width);
            let y0=sy(i64::from(r.y).max(i64::from(top))-i64::from(top)).min(height);
            let y1=sy((i64::from(r.y)+i64::from(r.height)).min(i64::from(bottom))-i64::from(top)).min(height);
            (x1>x0&&y1>y0).then_some((kobo_sdk::PictureLink{action:kobo_sdk::ActionId(0),x:x0,y:y0,width:x1.saturating_sub(x0),height:y1.saturating_sub(y0)},*index))
        }).take(128).collect()
    }

    /// Link actions with visible geometry on this screen, in paint order.
    pub(crate) fn screen_links(&self,page:usize)->Vec<usize>{self.window_screen_links(self.page_top(page),self.page_bottom(page))}
    pub(crate) fn window_screen_links(&self,top:u32,bottom:u32)->Vec<usize>{
        let mut links = Vec::new();
        for (rect, action) in &self.targets {
            if i64::from(rect.y) < i64::from(bottom)
                && i64::from(rect.y) + i64::from(rect.height) > i64::from(top)
                && !links.contains(action)
            {
                links.push(*action);
            }
        }
        links
    }

    /// The page's link targets in first-paint order, for the Links list.
    pub(crate) fn links(&self) -> &[String] {
        &self.links
    }
}

/// Resolves every command's source node to the nearest ancestor `a` href,
/// interning targets in first-paint order. The walk is depth-bounded: the
/// tree comes from an untrusted document.
fn link_targets(tree: &StyleTree, commands: &mut [Command]) -> (Vec<String>, Vec<(Rect, usize)>) {
    let mut links: Vec<String> = Vec::new();
    let mut targets: Vec<(Rect, usize)> = Vec::new();
    for command in commands {
        let (rect, source) = match command {
            Command::Fill { rect, source, .. } => (rect, source),
            Command::GlyphRun { layout, source, .. } => (layout, source),
            Command::PushClip(_) | Command::PopClip => continue,
        };
        let Some(node) = source.node else {
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
            source.action = Some(action);
            targets.push((*rect, action));
        }
    }
    (links, targets)
}

/// The fallback decision on a census alone, so the app can ask before it
/// keeps a painted view around.
pub(crate) fn falls_back(stats: &PartialStats) -> bool {
    if stats.intake_truncated || stats.output_truncated {
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
    let has_text = commands
        .iter()
        .any(|command| matches!(command, Command::GlyphRun { .. }));
    for command in commands {
        let rect = match command {
            Command::GlyphRun { layout, .. } => Some(layout),
            Command::Fill { rect, .. } if !has_text => Some(rect),
            Command::Fill { .. } | Command::PushClip(_) | Command::PopClip => None,
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

#[allow(clippy::too_many_arguments)]
fn paint_image(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    rect: &Rect,
    top: u32,
    scale: u32,
    image: &kobo_image::Picture,
    clips: &[Rect],
) {
    let Some((left, ct, right, cb)) = clip_bounds(clips, scale, width, height) else {
        return;
    };
    let s = i64::from(scale);
    let ox = i64::from(rect.x) * s;
    let oy = (i64::from(rect.y) - i64::from(top)) * s;
    let available_w = rect.width.min(image.width());
    let available_h = rect.height;
    let (fit_w, fit_h) =
        kobo_image::fitted_size((image.width(), image.height()), available_w, available_h);
    let rw = i64::from(fit_w) * s;
    let rh = i64::from(fit_h) * s;
    if rw == 0 || rh == 0 {
        return;
    }
    let Some(rgb) = image.colour() else {
        return;
    };
    for y in oy.max(ct)..(oy + rh).min(cb) {
        for x in ox.max(left)..(ox + rw).min(right) {
            let sx = usize::try_from((x - ox) * i64::from(image.width()) / rw).unwrap_or(0);
            let sy = usize::try_from((y - oy) * i64::from(image.height()) / rh).unwrap_or(0);
            let source = (sy * image.width() as usize + sx) * 3;
            let dest = (usize::try_from(y).unwrap_or(0) * width as usize
                + usize::try_from(x).unwrap_or(0))
                * 3;
            buffer[dest..dest + 3].copy_from_slice(&rgb[source..source + 3]);
        }
    }
}

/// Reconstructs CSS coverage masks with bilinear filtering, not repeated square
/// pixels. Native-resolution masks remain a separate typography improvement.
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
        let mask_y = (y - base_y) * 256 / s - (256 - 256 / s) / 2;
        let Ok(row) = usize::try_from(y) else {
            continue;
        };
        for x in x0..x1 {
            let mask_x = (x - base_x) * 256 / s - (256 - 256 / s) / 2;
            let Ok(col) = usize::try_from(x) else {
                continue;
            };
            let a = sample_coverage(coverage, mask_w as usize, mask_h as usize, mask_x, mask_y);
            let at = (row * width as usize + col) * 3;
            blend(&mut buffer[at..at + 3], color, a);
        }
    }
}

fn sample_coverage(mask: &[u8], width: usize, height: usize, x: i64, y: i64) -> u8 {
    let x0 = x.div_euclid(256);
    let y0 = y.div_euclid(256);
    let fx = x.rem_euclid(256) as u32;
    let fy = y.rem_euclid(256) as u32;
    let at = |x: i64, y: i64| -> u32 {
        if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
            0
        } else {
            u32::from(mask[y as usize * width + x as usize])
        }
    };
    let top = at(x0, y0) * (256 - fx) + at(x0 + 1, y0) * fx;
    let bottom = at(x0, y0 + 1) * (256 - fx) + at(x0 + 1, y0 + 1) * fx;
    ((top * (256 - fy) + bottom * fy + 32768) / 65536) as u8
}

/// Tap targets over a painted column, in CSS px.
pub(crate) struct TapMap {
    /// `(rect, action)` in paint order; ambiguous actions refuse the hit.
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
            source: Source { node: None, action, text_range: None },
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
                text_range: None,
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
    fn pagination_ignores_backgrounds_and_does_not_repeat_next_line() {
        let commands = vec![fill(0, 0, 100, 200, None), glyph_line(80, 30)];
        assert_eq!(paginate(&commands, 200, 100), vec![0, 80, 180]);
        let view = PageView {
            commands,
            stats: PartialStats::default(),
            images: Default::default(),
            links: vec![],
            targets: vec![],
            content_height: 200,
            css_width: 100,
            scale: 1,
            page_height_css: 100,
            cuts: vec![0, 80, 180],
        };
        let pixels = view.raster_page_grey(0);
        // The first screen ends before the next line, rather than showing it twice.
        assert!(pixels[80 * 100..].iter().all(|pixel| *pixel == 255));
    }

    #[test]
    fn raster_scales_fills_and_upsizes_the_screen() {
        let view = PageView {
            commands: vec![fill(5, 5, 10, 10, None)],
            stats: PartialStats::default(),
            images: Default::default(),
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
            images: Default::default(),
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
            images: Default::default(),
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
    fn fallback_on_truncated_output() {
        assert!(falls_back(&PartialStats {
            output_truncated: true,
            ..PartialStats::default()
        }));
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
    fn page_character_metrics_grow_only_for_used_fallbacks() {
        let face = PageFontAdapter::bundled().expect("faces");
        let latin = face
            .page_character_metrics('A', 24, false, false)
            .expect("Latin");
        let indic = face
            .page_character_metrics('ह', 24, false, false)
            .expect("Indic");
        assert_ne!(latin, indic);
        assert!(indic.height >= latin.height);
    }

    #[test]
    fn shaped_missing_glyph_is_not_lost_by_adapter() {
        let face = PageFontAdapter::bundled().expect("faces");
        let clusters = face.page_shape("日本語", 24, false, false).expect("shaped");
        assert!(clusters.iter().any(|cluster| cluster.missing));
    }

    #[test]
    fn page_shape_clusters_complex_script() {
        let face = PageFontAdapter::bundled().expect("bundled faces");
        let text = "हिन्दी";
        let clusters = face.page_shape(text, 24, false, false).expect("shaped");
        assert_eq!(clusters.first().expect("first").start, 0);
        assert_eq!(clusters.last().expect("last").end, text.len());
        assert!(clusters.len() < text.chars().count());
        assert!(clusters.iter().all(|c| !c.bitmap.coverage.is_empty()));
    }

    #[test]
    fn mono_cache_reuses_one_immutable_pair_and_uncached_path_is_distinct() {
        let a = PageFontAdapter::monospace().unwrap();
        let b = PageFontAdapter::monospace().unwrap();
        let c = PageFontAdapter::monospace_uncached().unwrap();
        assert!(std::rc::Rc::ptr_eq(&a.session, &b.session));
        assert!(!std::rc::Rc::ptr_eq(&a.session, &c.session));
        for bold in [false, true] {
            assert_eq!(a.bitmap('W', 20, bold, false).unwrap().coverage,
                c.bitmap('W', 20, bold, false).unwrap().coverage);
        }
    }

    #[test]
    fn production_mono_adapter_has_fixed_advance_and_real_bold_ink() {
        let face = PageFontAdapter::monospace().expect("mono faces");
        let advance = face.page_advance('i', 20, false, false);
        assert_eq!(advance, face.page_advance('W', 20, false, false));
        assert_eq!(advance, face.page_advance('W', 20, true, false));
        let regular = face.bitmap('W', 20, false, false).expect("regular");
        let bold = face.bitmap('W', 20, true, false).expect("bold");
        assert_ne!(regular.coverage, bold.coverage);
    }

    #[test]
    fn screen_links_follow_source_actions_and_page_boundaries() {
        let face = PageFontAdapter::bundled().expect("faces");
        let view = PageView::paint(
            b"<p><a href='/first'>First</a></p><div style='height:800px'></div><p><a href='/last'>Last</a></p>",
            536, 300, 2, &face, &face, &face,
        ).expect("paint");
        assert_eq!(view.screen_links(0), vec![0]);
        assert!(!view.screen_links(0).contains(&1));
        assert!(view.commands.iter().any(|command| matches!(command,
            Command::GlyphRun { source, .. } if source.action == Some(1))));
        assert!((1..view.pages()).any(|page| view.screen_links(page).contains(&1)));
    }

    #[test]
    fn native_table_links_survive_grid_and_spanning_fallback() {
        let face = PageFontAdapter::bundled().expect("bundled faces");
        let html = br#"<!doctype html><table><tr><td><a href="/grid">Grid link</a></td><td>Second</td></tr></table><table><caption>Caption retained</caption><tr><td rowspan=2><a href="/stack">Stack link</a></td></tr></table><p>After</p>"#;
        let view = PageView::paint(html, 536, 724, 2, &face, &face, &face).expect("paint");
        assert_eq!(view.links(), &["/grid".to_string(), "/stack".to_string()]);
        assert!(view.stats.skips.is_empty(), "{:?}", view.stats.skips);
        assert_eq!(
            view.stats.approximations.get("table-stacked-fallback"),
            Some(&1)
        );
        assert_eq!(view.stats.text_blocks_painted, 5);
        assert_eq!(view.screen_links(0).len(), 2);
    }

    #[test]
    fn authored_colour_survives_regular_bold_italic_code_links_and_backgrounds() {
        let face = PageFontAdapter::bundled().unwrap();
        let html=br#"<!doctype html><style>body{margin:0;font-size:16px}p{margin:0;color:#cc2040}strong{color:#2070c0}em{color:#208040}code{color:#8040c0}a{color:#d07010}div{background-color:#20c0d0;width:80px;height:12px}</style><p>Regular</p><p><strong>Bold</strong></p><p><em>Italic</em></p><p><code>Code</code></p><p><a href='/target'>Link</a></p><div></div>"#;
        let view = PageView::paint(html, 200, 200, 1, &face, &face, &face).unwrap();
        let rgb = view.raster_page_rgb(0);
        for colour in [
            [204, 32, 64],
            [32, 112, 192],
            [32, 128, 64],
            [128, 64, 192],
            [208, 112, 16],
            [32, 192, 208],
        ] {
            assert!(
                rgb.chunks_exact(3).any(|pixel| pixel == colour),
                "missing {colour:?}"
            );
        }
        assert_eq!(view.screen_links(0), vec![0]);
        assert!(view.stats.skips.is_empty(), "{:?}", view.stats.skips);
        if let Ok(path) = std::env::var("COBALT_COLOUR_FONT_PREVIEW_OUT") {
            std::fs::write(path, kobo_image::encode_png_rgb(200, 200, &rgb).unwrap()).unwrap();
        }
    }

    #[test]
    fn native_png_data_image_paints_real_rgb_not_marker_black() {
        let face = PageFontAdapter::bundled().expect("bundled faces");
        let png = kobo_image::encode_png_rgb(2, 1, &[220, 20, 40, 20, 100, 220]).unwrap();
        let uri = format!("data:image/png;base64,{}", test_base64(&png));
        let html = format!("<!doctype html><style>body{{margin:0}}</style><a href='/image'><img src='{uri}' width='2' height='1' alt='ALT'></a>");
        let view = PageView::paint(html.as_bytes(), 100, 80, 1, &face, &face, &face).unwrap();
        let rgb = view.raster_page_rgb(0);
        assert!(rgb.chunks_exact(3).any(|p| p == [220, 20, 40]));
        assert!(rgb.chunks_exact(3).any(|p| p == [20, 100, 220]));
        assert_eq!(view.screen_links(0), vec![0]);
        assert!(view.stats.skips.is_empty(), "{:?}", view.stats.skips);
        // No alt glyphs or link underline remain below the one-pixel image.
        assert!(rgb[100*3..].iter().all(|p| *p == 255));
    }

    #[test]
    fn unsupported_native_image_is_counted_without_black_marker() {
        let face = PageFontAdapter::bundled().unwrap();
        let html = br#"<!doctype html><style>body{margin:0}</style><img src='data:image/svg+xml;base64,PHN2Zy8+' width=20 height=20>"#;
        let view = PageView::paint(html, 100, 80, 1, &face, &face, &face).unwrap();
        assert_eq!(view.stats.skips.get("native-image-unavailable"), Some(&1));
        assert!(view
            .raster_page_rgb(0)
            .chunks_exact(3)
            .all(|p| p == [255, 255, 255]));
    }

    #[test]
    fn native_image_raster_handles_pagination_clips_and_sampling() {
        let image = kobo_image::Picture::from_rgb(
            2,
            2,
            vec![220, 20, 40, 20, 100, 220, 40, 180, 60, 240, 200, 20],
        )
        .unwrap();
        let rect = Rect {
            x: 0,
            y: 49,
            width: 2,
            height: 2,
        };
        let mut buffer = vec![255; 4 * 4 * 3];
        super::paint_image(
            &mut buffer,
            4,
            4,
            &rect,
            50,
            2,
            &image,
            &[Rect {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            }],
        );
        assert_eq!(&buffer[..3], &[40, 180, 60]);
        assert_eq!(&buffer[6..9], &[240, 200, 20]);
        assert!(buffer[24..].iter().all(|p| *p == 255));
    }

    #[test]
    fn native_image_preview_when_requested() {
        let Ok(path) = std::env::var("COBALT_IMAGE_PREVIEW_OUT") else {
            return;
        };
        let face = PageFontAdapter::bundled().unwrap();
        let mut rgb = Vec::new();
        for y in 0..80 {
            for x in 0..240 {
                rgb.extend_from_slice(if x < 80 {
                    &[220, 20, 40]
                } else if x < 160 {
                    &[20, 100, 220]
                } else if y < 40 {
                    &[40, 180, 60]
                } else {
                    &[240, 200, 20]
                });
            }
        }
        let png = kobo_image::encode_png_rgb(240, 80, &rgb).unwrap();
        let html=format!("<!doctype html><style>body{{margin:10px;font-size:18px}}</style><h2>Native data image</h2><a href='/image'><img src='data:image/png;base64,{}' width=240 height=80></a><p>Text after the decoded image.</p>",test_base64(&png));
        let view = PageView::paint(html.as_bytes(), 400, 300, 2, &face, &face, &face).unwrap();
        std::fs::write(
            path,
            kobo_image::encode_png_rgb(800, 600, &view.raster_page_rgb(0)).unwrap(),
        )
        .unwrap();
        eprintln!(
            "image-preview skips={:?} approximations={:?}",
            view.stats.skips, view.stats.approximations
        );
    }

    #[test]
    fn native_wide_image_reserves_fitted_height_inside_its_container() {
        let face = PageFontAdapter::bundled().unwrap();
        let png = kobo_image::encode_png_rgb(400, 200, &vec![160; 400 * 200 * 3]).unwrap();
        let uri = format!("data:image/png;base64,{}", test_base64(&png));
        let html = format!("<!doctype html><style>body{{margin:0}}div{{width:80px}}</style><div><img src='{uri}' alt='ALT'></div>");
        let view = PageView::paint(html.as_bytes(), 200, 100, 1, &face, &face, &face).unwrap();
        assert_eq!(view.content_height, 40);
        assert_eq!(view.pages(), 1);
        assert!(view.stats.skips.is_empty());
        let rgb = view.raster_page_rgb(0);
        assert!(rgb[..80 * 3].chunks_exact(3).all(|p| p == [160, 160, 160]));
        assert!(rgb[40 * 200 * 3..].iter().all(|p| *p == 255));
        if let Ok(path) = std::env::var("COBALT_FITTED_IMAGE_PREVIEW_OUT") {
            std::fs::write(path, kobo_image::encode_png_rgb(200, 100, &rgb).unwrap()).unwrap();
        }
    }

    #[test]
    fn decoded_image_relayout_preserves_links_following_text_and_page_cuts() {
        let face = PageFontAdapter::bundled().unwrap();
        let png = kobo_image::encode_png_rgb(40, 240, &vec![160; 40 * 240 * 3]).unwrap();
        let uri = format!("data:image/png;base64,{}", test_base64(&png));
        let html = format!("<!doctype html><style>body{{margin:0}}p{{margin:0}}</style><a href='/image'><img src='{uri}' alt='ALT'></a><p>After</p>");
        let view = PageView::paint(html.as_bytes(), 100, 80, 1, &face, &face, &face).unwrap();
        assert_eq!(view.pages(), 4);
        assert_eq!(view.screen_links(0), vec![0]);
        assert_eq!(view.screen_links(2), vec![0]);
        for page in 0..3 {
            let rgb = view.raster_page_rgb(page);
            assert_eq!(&rgb[..3], &[160, 160, 160]);
        }
        assert!(view.commands.iter().any(|command| matches!(command,Command::GlyphRun{layout,..} if (240..265).contains(&layout.y))));
    }

    #[test]
    fn decoded_image_fit_tracks_nested_edges_and_exif_orientation() {
        let face = PageFontAdapter::bundled().unwrap();
        let bytes = include_bytes!("../tests/images/rotated-exif.jpg");
        let image = kobo_image::decode_colour(bytes).unwrap();
        assert_eq!((image.width(), image.height()), (240, 320));
        let uri = format!("data:image/jpeg;base64,{}", test_base64(bytes));
        let html = format!("<!doctype html><style>body{{margin:0}}.outer{{width:100px;padding:10px;border:2px solid black}}.inner{{width:60px}}</style><div class='outer'><div class='inner'><img src='{uri}' alt='ALT'></div></div>");
        let view = PageView::paint(html.as_bytes(), 200, 100, 1, &face, &face, &face).unwrap();
        assert_eq!(view.content_height, 104);
        assert_eq!(view.pages(), 2);
        assert!(view.stats.skips.is_empty(), "{:?}", view.stats.skips);
        let rgb = view.raster_page_rgb(0);
        assert_eq!(
            &rgb[(12 * 200 + 12) * 3..][..3],
            &image.colour().unwrap()[..3]
        );
        if let Ok(path) = std::env::var("COBALT_NESTED_IMAGE_PREVIEW_OUT") {
            std::fs::write(path, kobo_image::encode_png_rgb(200,100,&rgb).unwrap()).unwrap();
        }

    }

    #[test]
    fn decoded_image_in_zero_width_container_has_no_reserved_height() {
        let face = PageFontAdapter::bundled().unwrap();
        let png = kobo_image::encode_png_rgb(40, 40, &vec![160; 40 * 40 * 3]).unwrap();
        let uri = format!("data:image/png;base64,{}", test_base64(&png));
        let html = format!("<!doctype html><style>body{{margin:0}}div{{width:0}}</style><div><img src='{uri}' alt='ALT'></div>");
        let view = PageView::paint(html.as_bytes(), 100, 80, 1, &face, &face, &face).unwrap();
        assert_eq!(view.content_height, 1);
        assert!(view.raster_page_rgb(0).iter().all(|p| *p == 255));
    }

    #[test]
    fn native_image_large_attributes_do_not_panic_or_wrap_geometry() {
        let face = PageFontAdapter::bundled().unwrap();
        let png = kobo_image::encode_png_rgb(1, 1, &[220, 20, 40]).unwrap();
        let uri = format!("data:image/png;base64,{}", test_base64(&png));
        for (width, height) in [(801, u32::MAX), (u32::MAX, u32::MAX)] {
            let html =
                format!("<!doctype html><img src='{uri}' width='{width}' height='{height}'>");
            let view = PageView::paint(html.as_bytes(), 100, 80, 1, &face, &face, &face).unwrap();
            assert!(view.content_height <= 4200);
            assert_eq!(
                view.images
                    .values()
                    .filter(|(_, image)| image.is_some())
                    .count(),
                1
            );
        }
    }

    #[test]
    fn native_jpeg_variants_decode_and_malformed_payload_falls_back() {
        let face = PageFontAdapter::bundled().unwrap();
        for bytes in [
            include_bytes!("../tests/images/baseline.jpg").as_slice(),
            include_bytes!("../tests/images/progressive.jpg").as_slice(),
            include_bytes!("../tests/images/cmyk.jpg").as_slice(),
            include_bytes!("../tests/images/grey.jpg").as_slice(),
            include_bytes!("../tests/images/rotated-exif.jpg").as_slice(),
        ] {
            let uri = format!("data:image/jpeg;base64,{}", test_base64(bytes));
            let html = format!("<!doctype html><img src='{uri}' width=80 height=60 alt='JPEG'>");
            let view = PageView::paint(html.as_bytes(), 100, 80, 1, &face, &face, &face).unwrap();
            assert_eq!(
                view.images
                    .values()
                    .filter(|(_, image)| image.is_some())
                    .count(),
                1
            );
            assert!(view.stats.skips.is_empty(), "{:?}", view.stats.skips);
            assert!(view
                .raster_page_rgb(0)
                .chunks_exact(3)
                .any(|p| p != [255, 255, 255]));
        }
        let html = br#"<!doctype html><img src='data:image/jpeg;base64,AQID' width=20 height=20 alt='Fallback'>"#;
        let view = PageView::paint(html, 100, 80, 1, &face, &face, &face).unwrap();
        assert_eq!(view.stats.skips.get("native-image-unavailable"), Some(&1));
        assert!(view.images.values().all(|(_, image)| image.is_none()));
        assert!(view.stats.text_blocks_painted > 0);
    }

    #[test]
    fn native_image_compressed_budget_is_shared_across_images() {
        let png = kobo_image::encode_png_rgb(1, 1, &[220, 20, 40]).unwrap();
        let uri = format!("data:image/png;base64,{}", test_base64(&png));
        let html = format!(
            "<!doctype html><img src='{uri}' width=1 height=1><img src='{uri}' width=1 height=1>"
        );
        let tree = parse_basic_style_tree(
            html.as_bytes(),
            &[],
            &Limits::DEFAULT,
            None,
            SupplyBudget::CANONICAL,
            None,
        );
        let mut boxes = BoxTree::from_style(&tree);
        let mut padded = png;
        padded.resize(2 * 1024 * 1024 + 1, 0);
        let uri = format!("data:image/png;base64,{}", test_base64(&padded));
        // Exercise preparation directly: DOM resource-attribute retention
        // has a separate, lower bound and must not be relaxed for this test.
        for node in &mut boxes.boxes {
            if node.kind == kobo_web_document::box_tree::BoxKind::Image {
                node.image.as_mut().unwrap().src_data_uri = Some(uri.clone());
            }
        }
        let mut stats = PartialStats::default();
        let images = crate::page_images::prepare(&boxes, &mut stats);
        assert_eq!(images.len(), 2);
        assert_eq!(
            images.values().filter(|(_, image)| image.is_some()).count(),
            1
        );
        assert_eq!(stats.skips.get("native-image-unavailable"), Some(&1));
    }

    #[test]
    fn native_image_budget_is_aggregate_and_refuses_large_headers() {
        let face = PageFontAdapter::bundled().unwrap();
        for (w, h, count, good, bad) in [(800, 800, 2, 1, 1), (1025, 1024, 1, 0, 1)] {
            let pixels = vec![160; w * h * 3];
            let png = kobo_image::encode_png_rgb(w as u32, h as u32, &pixels).unwrap();
            let uri = format!("data:image/png;base64,{}", test_base64(&png));
            let image = format!("<img src='{uri}' width=20 height=20>");
            let html = format!(
                "<!doctype html><style>body{{margin:0}}</style>{}",
                image.repeat(count)
            );
            let view = PageView::paint(html.as_bytes(), 100, 80, 1, &face, &face, &face).unwrap();
            assert_eq!(
                view.images
                    .values()
                    .filter(|(_, image)| image.is_some())
                    .count(),
                good
            );
            assert_eq!(view.stats.skips.get("native-image-unavailable"), Some(&bad));
        }
    }

    fn test_base64(bytes: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = (u32::from(chunk[0]) << 16)
                | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
                | u32::from(*chunk.get(2).unwrap_or(&0));
            for shift in [18, 12] {
                out.push(char::from(A[((n >> shift) & 63) as usize]));
            }
            out.push(if chunk.len() > 1 {
                char::from(A[((n >> 6) & 63) as usize])
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                char::from(A[(n & 63) as usize])
            } else {
                '='
            });
        }
        out
    }

    #[test]
    fn native_cell_wrapper_keeps_link_geometry_without_stacking() {
        let face = PageFontAdapter::bundled().expect("bundled faces");
        let html = br#"<!doctype html><table><tr><td><div><a href="/wrapped">Wrapped link</a></div></td><td>Peer</td></tr></table>"#;
        let view = PageView::paint(html, 536, 724, 2, &face, &face, &face).expect("paint");
        assert_eq!(view.links(), &["/wrapped".to_string()]);
        assert!(view.stats.skips.is_empty(), "{:?}", view.stats.skips);
        assert!(!view
            .stats
            .approximations
            .contains_key("table-stacked-fallback"));
        assert_eq!(view.screen_links(0), vec![0]);
    }

    #[test]
    fn native_caption_and_colspan_keep_links_without_stacking() {
        let face = PageFontAdapter::bundled().expect("bundled faces");
        let html = br#"<!doctype html><table><caption><a href="/caption">Caption link</a></caption><tr><td colspan=2><a href="/span">Spanning link</a></td></tr><tr><td>First</td><td>Second</td></tr></table>"#;
        let view = PageView::paint(html, 536, 724, 2, &face, &face, &face).expect("paint");
        assert_eq!(view.links(), &["/caption".to_string(), "/span".to_string()]);
        assert!(view.stats.skips.is_empty(), "{:?}", view.stats.skips);
        assert!(!view
            .stats
            .approximations
            .contains_key("table-stacked-fallback"));
        assert_eq!(view.stats.text_blocks_painted, 4);
        assert_eq!(view.screen_links(0).len(), 2);
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

#[cfg(test)]
mod coverage_tests {
    #[test]
    fn reconstruction_has_subpixel_edges_not_repeated_blocks() {
        assert_eq!(super::sample_coverage(&[0, 255], 2, 1, 0, 0), 0);
        assert_eq!(super::sample_coverage(&[0, 255], 2, 1, 64, 0), 64);
        assert_eq!(super::sample_coverage(&[0, 255], 2, 1, 192, 0), 191);
        assert_eq!(super::sample_coverage(&[255], 1, 1, 0, 0), 255);
    }
}

#[cfg(test)]
mod picture_region_tests {
    use super::*;
    #[test]
    fn page_cuts_scale_and_keyboard_crop_keep_links_on_visible_pixels(){
        let view=PageView{commands:Vec::new(),images:Default::default(),targets:vec![(Rect{x:10,y:90,width:30,height:30},0),(Rect{x:70,y:170,width:20,height:20},1)],links:Vec::new(),css_width:100,content_height:200,page_height_css:100,cuts:vec![0,100],scale:2,stats:Default::default()};
        let regions=view.picture_links(1,400,200,400);
        assert_eq!(regions.len(),1);let (r,index)=regions[0];assert_eq!(index,0);assert_eq!((r.x,r.y,r.width,r.height),(40,0,120,80));
        let regions=view.picture_links(0,400,400,400);assert_eq!(regions.len(),1);assert_eq!((regions[0].0.y,regions[0].0.height),(360,40));
    }
}

#[cfg(test)]
mod light_scheme_regression {
    use super::*;
    #[test]
    fn page_view_uses_light_scheme_for_embedded_media_rules() {
        let face=PageFontAdapter::bundled().unwrap();
        let view=PageView::paint(b"<!doctype html><style>p{color:red}@media(prefers-color-scheme:dark){p{color:blue}}</style><p>Light page</p>",400,600,1,&face,&face,&face).unwrap();
        assert!(view.commands.iter().any(|command| matches!(command,Command::GlyphRun{color:Rgb(255,0,0),..})));
        assert!(!view.commands.iter().any(|command| matches!(command,Command::GlyphRun{color:Rgb(0,0,255),..})));
    }
}
