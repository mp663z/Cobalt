//! Experimental font and panel-picture seam. Not linked into any shipping app.
mod normal_metrics;
use fontdue::{Font, FontSettings};
use kobo_web_document::{css_text_page::FontProvider, inline_lines::GlyphBitmap};
pub struct LocalFace {
    regular: Font,
    sha256: String,
    normal: Option<normal_metrics::Pair>,
    /// The bold face is loaded lazily on the first bold glyph: pages without
    /// bold runs never consult it, so a missing or broken bold file cannot
    /// fail a page that does not need it. A page WITH a bold run and no
    /// working bold face refuses through the provider returning `None`.
    bold_loader: Option<Box<dyn Fn() -> Option<Font>>>,
    bold: std::cell::OnceCell<Option<Font>>,
}
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
impl LocalFace {
    fn pixels(size: u32) -> Option<f32> {
        (1..=256).contains(&size).then_some(size as f32) // bounded exact integer -> f32
    }
    fn bounded_u32(value: f32) -> Option<u32> {
        if !value.is_finite() || value < 1.0 || value > f32::from(u16::MAX) {
            return None;
        }
        Some(value as u32) // checked finite positive range above
    }
}
impl LocalFace {
    fn face(&self, bold: bool) -> Option<&Font> {
        if !bold {
            return Some(&self.regular);
        }
        self.bold
            .get_or_init(|| self.bold_loader.as_ref().and_then(|load| load()))
            .as_ref()
    }
}

impl FontProvider for LocalFace {
    fn face_sha256_hex(&self) -> Option<String> {
        Some(self.sha256.clone())
    }

    fn normal_line_metrics(
        &self,
        size: u32,
        bold: bool,
    ) -> Option<kobo_web_document::css_text_page::NormalLineMetrics> {
        self.normal.as_ref()?.at(size, bold)
    }

    fn advance(&self, character: char, size: u32, bold: bool) -> Option<u32> {
        let face = self.face(bold)?;
        if face.lookup_glyph_index(character) == 0 {
            return None;
        }
        let width = face.metrics(character, Self::pixels(size)?).advance_width;
        Self::bounded_u32(width.round().max(1.0))
    }
    fn line_height(&self, size: u32, bold: bool) -> Option<u32> {
        let metrics = self
            .face(bold)?
            .horizontal_line_metrics(Self::pixels(size)?)?;
        Self::bounded_u32(metrics.ascent.round() + (-metrics.descent).round() + metrics.line_gap.round())
    }
    fn baseline_offset(&self, size: u32, bold: bool) -> Option<i32> {
        let ascent = self
            .face(bold)?
            .horizontal_line_metrics(Self::pixels(size)?)?
            .ascent
            .round();
        i32::try_from(Self::bounded_u32(ascent)?).ok()
    }
    fn raster(&self, character: char, size: u32, bold: bool) -> Option<GlyphBitmap> {
        let face = self.face(bold)?;
        if face.lookup_glyph_index(character) == 0 {
            return None;
        }
        let (metrics, coverage) = face.rasterize(character, Self::pixels(size)?);
        Some(GlyphBitmap {
            left: metrics.xmin,
            top: -(metrics.ymin + i32::try_from(metrics.height).ok()?),
            width: u32::try_from(metrics.width).ok()?,
            height: u32::try_from(metrics.height).ok()?,
            coverage,
        })
    }
}

impl LocalFace {
    /// Explicit experimental fallback using the same unmodified bundled
    /// Atkinson regular face as Cobalt's reader. This does not discover or
    /// substitute the user's device font and makes no CSS font-family claim.
    /// # Errors
    /// Returns font validation failure rather than a fabricated metric.
    pub fn bundled_reader_face() -> Result<Self, &'static str> {
        Self::from_bytes(include_bytes!(
            "../../../crates/kobo-text/fonts/AtkinsonHyperlegible-Regular.ttf"
        ))
    }

    /// License notice for the unmodified bundled fallback face.
    #[must_use]
    pub const fn bundled_license() -> &'static str {
        include_str!("../../../crates/kobo-text/fonts/LICENSE-AtkinsonHyperlegible.txt")
    }

    /// Byte-aware normal-strut proof path. No CLI or canonical profile uses it.
    /// Activates only at integer px sizes 8,10,12,13,14,15,16,17,18,20,24,32,48.
    /// All other sizes return None for both weights and uniformly use legacy.
    /// Existing constructors keep their legacy metric contract unchanged.
    /// Unsupported metadata or per-size pair disagreement returns no new normal
    /// metrics for either face, preserving documented legacy behavior, not parity.
    /// # Errors
    /// Invalid/oversized font sources refuse as with the existing constructor.
    pub fn with_verified_normal_bytes(
        bytes: &[u8],
        bold_bytes: Option<&[u8]>,
    ) -> Result<Self, &'static str> {
        let mut face = Self::from_bytes(bytes)?;
        let bold = bold_bytes.map(Self::from_bytes).transpose()?;
        let regular_metadata = normal_metrics::Metrics::parse(bytes);
        let bold_metadata = bold_bytes.and_then(normal_metrics::Metrics::parse);
        face.normal = Some(normal_metrics::Pair {
            regular: regular_metadata,
            bold: bold_metadata,
            has_bold: bold_bytes.is_some(),
        });
        if let Some(bold) = bold {
            let _ = face.bold.set(Some(bold.regular));
        }
        Ok(face)
    }

    /// Load one explicit TrueType/OpenType face with a bounded source buffer.
    /// # Errors
    /// Rejects empty, oversized or malformed fonts.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.is_empty() || bytes.len() > kobo_web_document::Limits::DEFAULT.max_input_bytes {
            return Err("invalid font byte budget");
        }
        Self::from_bytes_inner(bytes, None)
    }

    /// The same explicit regular face plus a lazily loaded bold face for
    /// weight-700 runs. The loader runs at most once, on the first bold
    /// glyph; its failure refuses only pages that actually contain bold.
    /// # Errors
    /// Rejects an invalid regular face buffer.
    pub fn with_bold_loader(
        bytes: &[u8],
        bold_loader: Box<dyn Fn() -> Option<Font>>,
    ) -> Result<Self, &'static str> {
        Self::from_bytes_inner(bytes, Some(bold_loader))
    }

    fn from_bytes_inner(
        bytes: &[u8],
        bold_loader: Option<Box<dyn Fn() -> Option<Font>>>,
    ) -> Result<Self, &'static str> {
        if bytes.is_empty() || bytes.len() > kobo_web_document::Limits::DEFAULT.max_input_bytes {
            return Err("invalid font byte budget");
        }
        let regular =
            Font::from_bytes(bytes, FontSettings::default()).map_err(|_| "invalid font")?;
        Ok(Self {
            regular,
            sha256: kobo_net::sha256::hex_digest(bytes),
            normal: None,
            bold_loader,
            bold: std::cell::OnceCell::new(),
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct PanelFrame {
    width: u32,
    height: u32,
    format: kobo_ui::PictureFormat,
    pixels: Vec<u8>,
}
impl PanelFrame {
    #[must_use]
    pub const fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    #[must_use]
    pub const fn format(&self) -> kobo_ui::PictureFormat {
        self.format
    }
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Convert an opaque display-list raster for an explicitly confirmed panel.
    /// Unknown identity uses grey; RGB requires `Some(true)`.
    /// # Errors
    /// Refuses invalid surfaces, transparency, allocation or picture budgets.
    pub fn from_rgba(
        width: u32,
        height: u32,
        rgba: &[u8],
        colour: Option<bool>,
    ) -> Result<Self, &'static str> {
        let count = usize::try_from(width)
            .ok()
            .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
            .ok_or("invalid surface")?;
        if count == 0
            || count > kobo_web_document::display_list::MAX_PIXELS
            || count.checked_mul(4) != Some(rgba.len())
        {
            return Err("invalid surface");
        }
        let format = if colour == Some(true) {
            kobo_ui::PictureFormat::Rgb
        } else {
            kobo_ui::PictureFormat::Grey
        };
        let bytes = format
            .byte_len(width, height)
            .filter(|&n| n <= 3 * kobo_web_document::display_list::MAX_PIXELS)
            .ok_or("picture budget")?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(bytes).map_err(|_| "allocation")?;
        for pixel in rgba.chunks_exact(4) {
            if pixel[3] != 255 {
                return Err("nonopaque raster");
            }
            if format == kobo_ui::PictureFormat::Rgb {
                pixels.extend_from_slice(&pixel[..3]);
            } else {
                // Same integer Rec.709 luminance weights as image's luma conversion.
                let value = (2126 * u32::from(pixel[0])
                    + 7152 * u32::from(pixel[1])
                    + 722 * u32::from(pixel[2]))
                    / 10_000;
                pixels.push(u8::try_from(value).map_err(|_| "invalid luminance")?);
            }
        }
        Ok(Self {
            width,
            height,
            format,
            pixels,
        })
    }

    /// Convert an opaque display-list raster in place, reusing its allocation
    /// so no second full-size pixel buffer exists at peak. Byte-for-byte the
    /// same acceptance rules and output as [`PanelFrame::from_rgba`]; the
    /// caller gives up ownership of the raster. Compacted bytes keep the
    /// raster allocation's capacity rather than re-allocating.
    /// # Errors
    /// Refuses invalid surfaces, transparency, allocation or picture budgets.
    pub fn from_rgba_in_place(
        width: u32,
        height: u32,
        mut rgba: Vec<u8>,
        colour: Option<bool>,
    ) -> Result<Self, &'static str> {
        let count = usize::try_from(width)
            .ok()
            .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
            .ok_or("invalid surface")?;
        if count == 0
            || count > kobo_web_document::display_list::MAX_PIXELS
            || count.checked_mul(4) != Some(rgba.len())
        {
            return Err("invalid surface");
        }
        let format = if colour == Some(true) {
            kobo_ui::PictureFormat::Rgb
        } else {
            kobo_ui::PictureFormat::Grey
        };
        let bytes = format
            .byte_len(width, height)
            .filter(|&n| n <= 3 * kobo_web_document::display_list::MAX_PIXELS)
            .ok_or("picture budget")?;
        // Transparency is checked before any byte moves, so a refusal never
        // leaves a half-compacted raster observable.
        if rgba.chunks_exact(4).any(|pixel| pixel[3] != 255) {
            return Err("nonopaque raster");
        }
        if format == kobo_ui::PictureFormat::Rgb {
            for index in 0..count {
                rgba.copy_within(index * 4..index * 4 + 3, index * 3);
            }
        } else {
            for index in 0..count {
                // Same integer Rec.709 luminance weights as image's luma conversion.
                let value = (2126 * u32::from(rgba[index * 4])
                    + 7152 * u32::from(rgba[index * 4 + 1])
                    + 722 * u32::from(rgba[index * 4 + 2]))
                    / 10_000;
                rgba[index] = u8::try_from(value).map_err(|_| "invalid luminance")?;
            }
        }
        rgba.truncate(bytes);
        Ok(Self {
            width,
            height,
            format,
            pixels: rgba,
        })
    }
    /// Queue on the existing SDK picture path. No screen is changed or shown.
    #[cfg(feature = "sdk-handoff")]
    pub fn put(
        self,
        context: &mut kobo_sdk::Context,
        handle: kobo_sdk::PictureHandle,
    ) -> Option<kobo_sdk::TilePicture> {
        match self.format {
            kobo_ui::PictureFormat::Rgb => {
                context.put_colour_picture(handle, self.width, self.height, self.pixels)
            }
            kobo_ui::PictureFormat::Grey => {
                context.put_picture(handle, self.width, self.height, self.pixels)
            }
        }
    }
}

fn build_page_list(
    html: &[u8],
    sheets: &[Vec<u8>],
    width: u32,
    height: u32,
    font: &impl FontProvider,
    allow_interactive: bool,
    underline: bool,
) -> Result<kobo_web_document::display_list::DisplayList, &'static str> {
    // Reject invalid room before parsing markup or asking a font provider.
    let pixels = usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)));
    if width == 0
        || height == 0
        || pixels.is_none_or(|n| n > kobo_web_document::display_list::MAX_PIXELS)
    {
        return Err("invalid room budget");
    }
    let limits = kobo_web_document::Limits::DEFAULT;
    if html.len() > limits.max_input_bytes || sheets.len() > 2 {
        return Err("input budget");
    }
    let sheet_bytes = sheets
        .iter()
        .try_fold(0_usize, |total, sheet| total.checked_add(sheet.len()));
    if sheet_bytes.is_none_or(|n| n > limits.max_input_bytes) {
        return Err("sheet budget");
    }
    // Media queries see the same width the page is laid out with; a different
    // width means a new parse. Width is non-zero (checked above).
    let media = kobo_web_document::MediaContext::new(width).ok_or("invalid room budget")?;
    let styled = kobo_web_document::parse_style_tree_with_media(html, sheets, &limits, media);
    // A picture has no semantic hit targets. Refuse controls/links until
    // a shipping caller can preserve their interactions, even if styled block.
    // The measurement-only static capture renders them inert instead; the
    // shipping prepare paths below never set that flag.
    if !allow_interactive
        && styled.nodes.iter().any(|node| {
            matches!(
                node.tag.as_str(),
                "a" | "area"
                    | "form"
                    | "input"
                    | "button"
                    | "select"
                    | "textarea"
                    | "option"
                    | "label"
                    | "details"
                    | "summary"
                    | "iframe"
                    | "audio"
                    | "video"
            )
        })
    {
        return Err("interactive page needs semantic fallback");
    }
    let list = if underline {
        // EXPERIMENTAL measurement path: Chrome 156 / Liberation Serif Regular underline profile.
        let tree = kobo_web_document::box_tree::BoxTree::from_style_with_text_decoration(&styled);
        kobo_web_document::css_text_page::paint_direct_text_blocks_underline(
            &tree,
            width,
            height,
            font,
            kobo_web_document::css_text_page::UnderlineProfile::ChromeLiberationSerif156,
        )
        .map_err(|_| "unsupported complete page")?
    } else {
        let tree = kobo_web_document::box_tree::BoxTree::from_style(&styled);
        kobo_web_document::css_text_page::paint_direct_text_blocks(&tree, width, height, font)
            .map_err(|_| "unsupported complete page")?
    };
    // This seam has no scrolling or pagination. Viewport clipping is useful
    // in the paint primitive, but cannot silently drop content on handoff.
    // Glyph ink may overhang its laid-out cell by a font bearing and is
    // clipped by the rasterizer like any browser clips it; the check reads
    // the emitter-proven layout extent instead, so a genuinely off-page
    // origin or a one-pixel layout overflow still refuses.
    if list.commands().iter().any(|command| {
        use kobo_web_document::display_list::Command;
        let rect = match command {
            Command::Fill { rect, .. } | Command::PushClip(rect) => rect,
            Command::GlyphRun { layout, .. } => layout,
            Command::PopClip => return false,
        };
        rect.x < 0
            || rect.y < 0
            || i64::from(rect.x) + i64::from(rect.width) > i64::from(width)
            || i64::from(rect.y) + i64::from(rect.height) > i64::from(height)
    }) {
        return Err("page needs scrolling or pagination");
    }
    Ok(list)
}

/// Prepare a complete restricted HTML page without queueing any SDK commands.
/// External sheets must already be fetched and scoped by the caller. This seam
/// does not fetch, paginate, register hit targets or change a shipping screen.
/// # Errors
/// Refuses oversized input/sheets, unsupported page geometry, or raster limits.
pub fn prepare_page(
    html: &[u8],
    sheets: &[Vec<u8>],
    width: u32,
    height: u32,
    font: &impl FontProvider,
    colour: Option<bool>,
) -> Result<PanelFrame, &'static str> {
    let list = build_page_list(html, sheets, width, height, font, false, false)?;
    let rgba = list
        .rasterize(
            width,
            height,
            kobo_web_document::display_list::Rgb(255, 255, 255),
        )
        .map_err(|_| "invalid complete raster")?;
    PanelFrame::from_rgba_in_place(width, height, rgba, colour)
}

/// The same restricted render, plus every glyph-run bounding box in paint
/// order. The bounds mark where text ink may land so a measurement harness
/// can mask text bands in both compared images instead of treating one
/// font's rasterization as page geometry. This adds no rendering capability:
/// every refusal in `prepare_page` applies unchanged.
/// # Errors
/// Refuses exactly as `prepare_page` does.
fn prepare_page_impl(
    html: &[u8],
    sheets: &[Vec<u8>],
    width: u32,
    height: u32,
    font: &impl FontProvider,
    colour: Option<bool>,
    allow_interactive: bool,
    underline: bool,
) -> Result<(PanelFrame, Vec<kobo_web_document::display_list::Rect>), &'static str> {
    let list = build_page_list(html, sheets, width, height, font, allow_interactive, underline)?;
    let bounds = list
        .commands()
        .iter()
        .filter_map(|command| match command {
            kobo_web_document::display_list::Command::GlyphRun { bounds, .. } => Some(*bounds),
            _ => None,
        })
        .collect();
    let rgba = list
        .rasterize(
            width,
            height,
            kobo_web_document::display_list::Rgb(255, 255, 255),
        )
        .map_err(|_| "invalid complete raster")?;
    Ok((
        PanelFrame::from_rgba_in_place(width, height, rgba, colour)?,
        bounds,
    ))
}

/// The same restricted render, plus every glyph-run bounding box in paint
/// order, through the shipping (non-interactive) path.
/// # Errors
/// Refuses exactly as `prepare_page` does.
pub fn prepare_page_masked(
    html: &[u8],
    sheets: &[Vec<u8>],
    width: u32,
    height: u32,
    font: &impl FontProvider,
    colour: Option<bool>,
) -> Result<(PanelFrame, Vec<kobo_web_document::display_list::Rect>), &'static str> {
    prepare_page_impl(html, sheets, width, height, font, colour, false, false)
}

/// Measurement-only static capture: interactive elements (links, controls)
/// render as inert styled content with no hit targets. This is NOT a
/// shipping surface - `prepare_page` still refuses such pages, and a static
/// capture is never evidence of navigation or interaction support.
/// # Errors
/// Same refusals as `prepare_page_masked` except the interactive guard.
pub fn prepare_page_static(
    html: &[u8],
    sheets: &[Vec<u8>],
    width: u32,
    height: u32,
    font: &impl FontProvider,
    colour: Option<bool>,
) -> Result<(PanelFrame, Vec<kobo_web_document::display_list::Rect>), &'static str> {
    prepare_page_impl(html, sheets, width, height, font, colour, true, false)
}

/// EXPERIMENTAL measurement entry: `prepare_page_masked` plus the restricted
/// `text-decoration: underline` subset bound to the Chrome 156 / Liberation Serif Regular
/// profile. The caller must pass that face; refusals of `prepare_page_masked` apply unchanged.
/// # Errors
/// Refuses exactly as `prepare_page_masked`, and anything outside the underline profile.
pub fn prepare_page_underline_masked(
    html: &[u8],
    sheets: &[Vec<u8>],
    width: u32,
    height: u32,
    font: &impl FontProvider,
    colour: Option<bool>,
) -> Result<(PanelFrame, Vec<kobo_web_document::display_list::Rect>), &'static str> {
    prepare_page_impl(html, sheets, width, height, font, colour, false, true)
}

/// All-or-nothing prepare and queue, using one caller-reserved handle.
/// On refusal there is no partial picture command; the caller retains its
/// semantic reader fallback. Successful queueing does not show a screen.
/// # Errors
/// Propagates preparation refusal or SDK picture rejection.
#[allow(clippy::too_many_arguments)] // Explicit experimental boundary; no app state hidden here.
#[cfg(feature = "sdk-handoff")]
pub fn queue_page(
    context: &mut kobo_sdk::Context,
    handle: kobo_sdk::PictureHandle,
    html: &[u8],
    sheets: &[Vec<u8>],
    width: u32,
    height: u32,
    font: &impl FontProvider,
    colour: Option<bool>,
) -> Result<kobo_sdk::TilePicture, &'static str> {
    prepare_page(html, sheets, width, height, font, colour)?
        .put(context, handle)
        .ok_or("SDK picture rejected")
}

/// Conservative, native-pixel room for one experimental picture screen.
/// This is not a shipping page mode. Non-default reader text sizes refuse
/// rather than ignoring the user's accessibility preference or scaling CSS.
#[cfg(feature = "sdk-handoff")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExperimentalRoom {
    metrics: kobo_ui::DisplayMetrics,
    width: u32,
    height: u32,
}
#[cfg(feature = "sdk-handoff")]
fn experimental_shell(title: &str) -> kobo_sdk::ScreenBuilder {
    kobo_sdk::ScreenBuilder::new("browser-css-experiment")
        .top_bar(title)
        .top_bar_action("reader", "Reader")
        .top_bar_action("navigate", "Navigate")
        .reading(true)
        .action_bar([
            ("back", "Back"),
            ("address", "Go to"),
            ("forward", "Forward"),
        ])
}
#[cfg(feature = "sdk-handoff")]
impl ExperimentalRoom {
    /// Measure real shell chrome and cap native pixels without enlarging the
    /// painter budget. Extra room on a large panel stays unused.
    /// # Errors
    /// Refuses invalid metrics, non-default text scale or an empty room.
    pub fn measure(metrics: kobo_ui::DisplayMetrics) -> Result<Self, &'static str> {
        if metrics.text_scale != kobo_ui::TextScale::Default {
            return Err("reader text scale needs semantic fallback");
        }
        // Bound arithmetic before calling the UI's physical-unit helpers.
        if !(1..=4096).contains(&metrics.width)
            || !(1..=4096).contains(&metrics.height)
            || !(1..=1200).contains(&metrics.pixels_per_inch)
        {
            return Err("invalid display metrics");
        }
        let layout = experimental_shell("")
            .build()
            .layout_with(&metrics, &kobo_ui::Chrome::measuring(true));
        let width = u32::try_from(metrics.content_width()).map_err(|_| "invalid room")?;
        if width == 0 {
            return Err("invalid room");
        }
        let ceiling = u32::try_from(kobo_web_document::display_list::MAX_PIXELS)
            .map_err(|_| "invalid pixel budget")?
            / width;
        let height = u32::try_from(layout.content.height)
            .map_err(|_| "invalid room")?
            .min(ceiling);
        if height == 0 {
            return Err("invalid room");
        }
        Ok(Self {
            metrics,
            width,
            height,
        })
    }
    #[must_use]
    pub const fn dimensions(self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Make a screen only for the exact measured viewport and picture. No
    /// page turns or content hit targets are invented. Validate before showing.
    /// # Errors
    /// Refuses stale metrics, mismatched pictures, shrinking or layout errors.
    pub fn screen(
        self,
        title: &str,
        picture: kobo_sdk::TilePicture,
        metrics: kobo_ui::DisplayMetrics,
    ) -> Result<kobo_ui::Screen, &'static str> {
        if metrics != self.metrics || picture.source != self.dimensions() {
            return Err("stale room or mismatched picture");
        }
        let screen = experimental_shell(title)
            .unframed_picture(picture, 1200)
            .build();
        let diagnostics = screen.diagnostics(&metrics, &kobo_ui::Chrome::measuring(true));
        let rect = diagnostics.layout.nodes.iter().find_map(|node| {
            matches!(node.kind, kobo_ui::LayoutKind::Picture(handle) if handle == picture.handle)
                .then_some(node.rect)
        }).ok_or("missing picture layout")?;
        let content = diagnostics.layout.content;
        if diagnostics.has_errors()
            || u32::try_from(rect.width).ok() != Some(self.width)
            || u32::try_from(rect.height).ok() != Some(self.height)
            || rect.x < 0
            || rect.x + rect.width > metrics.width
            || rect.y < content.y
            || rect.y + rect.height > content.y + content.height
        {
            return Err("picture room does not fit");
        }
        Ok(screen)
    }
}

/// Experimental owner of two caller-reserved picture handles. Preparing a
/// replacement happens separately; queue the new frame before releasing the
/// old one. The future app caller must clear on semantic fallback/navigation.
#[cfg(feature = "sdk-handoff")]
pub struct PictureSlot {
    handles: [kobo_sdk::PictureHandle; 2],
    active: Option<usize>,
}
#[cfg(feature = "sdk-handoff")]
impl PictureSlot {
    /// # Errors
    /// Handles must differ so replacing a page cannot release its new image.
    pub fn new(handles: [kobo_sdk::PictureHandle; 2]) -> Result<Self, &'static str> {
        if handles[0] == handles[1] {
            return Err("distinct picture handles required");
        }
        Ok(Self {
            handles,
            active: None,
        })
    }
    /// Queue one prepared frame, then release the previous page. On SDK
    /// rejection the old slot remains owned and no drop command is issued.
    /// # Errors
    /// Returns SDK picture rejection without changing slot ownership.
    pub fn replace(
        &mut self,
        context: &mut kobo_sdk::Context,
        frame: PanelFrame,
    ) -> Result<kobo_sdk::TilePicture, &'static str> {
        let next = self.active.map_or(0, |old| 1 - old);
        let picture = frame
            .put(context, self.handles[next])
            .ok_or("SDK picture rejected")?;
        if let Some(old) = self.active.replace(next) {
            context.drop_picture(self.handles[old]);
        }
        Ok(picture)
    }
    /// Idempotent release on navigation, fallback or exit. Does not show UI.
    pub fn clear(&mut self, context: &mut kobo_sdk::Context) {
        if let Some(old) = self.active.take() {
            context.drop_picture(self.handles[old]);
        }
    }
}

#[cfg(all(test, feature = "sdk-handoff"))]
mod tests {
    use super::*;
    struct EmptyApp;
    impl kobo_sdk::KoboApp for EmptyApp {
        fn on_start(&mut self, _: &mut kobo_sdk::Context) {}
        fn on_action(&mut self, _: &mut kobo_sdk::Context, _: kobo_sdk::ActionId) {}
    }
    #[test]
    fn native_room_preserves_controls_across_panels_and_refuses_accessibility_mismatch() {
        for (width, height, pixels_per_inch) in [
            (1072, 1448, 300),
            (758, 1024, 212),
            (1264, 1680, 300),
            (1440, 1920, 300),
            (1404, 1872, 227),
        ] {
            for text_scale in kobo_ui::TextScale::STEPS {
                let metrics = kobo_ui::DisplayMetrics {
                    width,
                    height,
                    pixels_per_inch,
                    text_scale,
                };
                let room = ExperimentalRoom::measure(metrics);
                if text_scale != kobo_ui::TextScale::Default {
                    assert_eq!(room, Err("reader text scale needs semantic fallback"));
                    continue;
                }
                let room = room.unwrap();
                let (w, h) = room.dimensions();
                assert!(
                    usize::try_from(w * h).unwrap() <= kobo_web_document::display_list::MAX_PIXELS
                );
                let picture = kobo_sdk::TilePicture::new(kobo_sdk::PictureHandle(700), w, h);
                let screen = room
                    .screen("Restricted CSS preview", picture, metrics)
                    .unwrap();
                assert!(screen.page_turns.is_none());
                for chrome in [
                    kobo_ui::Chrome::with_back(true),
                    kobo_ui::Chrome::measuring(true),
                ] {
                    let diagnostic = screen.diagnostics(&metrics, &chrome);
                    assert!(!diagnostic.has_errors(), "{:?}", diagnostic.issues);
                    let rect = diagnostic
                        .layout
                        .nodes
                        .iter()
                        .find(|node| matches!(node.kind, kobo_ui::LayoutKind::Picture(_)))
                        .unwrap()
                        .rect;
                    assert_eq!(
                        (
                            u32::try_from(rect.width).unwrap(),
                            u32::try_from(rect.height).unwrap()
                        ),
                        (w, h)
                    );
                    assert!(rect.y >= diagnostic.layout.content.y);
                    assert!(
                        rect.y + rect.height
                            <= diagnostic.layout.content.y + diagnostic.layout.content.height
                    );
                }
                assert!(room
                    .screen("Long title ".repeat(200).as_str(), picture, metrics)
                    .is_ok());
                let wrong = kobo_sdk::TilePicture::new(picture.handle, w, h - 1);
                assert!(room.screen("Preview", wrong, metrics).is_err());
                let stale = kobo_ui::DisplayMetrics {
                    height: height + 1,
                    ..metrics
                };
                assert!(room.screen("Preview", picture, stale).is_err());
            }
        }
    }
    #[test]
    fn invalid_panel_metrics_refuse_before_layout_arithmetic() {
        for (width, height, pixels_per_inch) in [
            (0, 1448, 300),
            (1072, 0, 300),
            (i32::MAX, 1448, 300),
            (1072, 1448, i32::MAX),
            (1, 1, 300),
            (1072, 1448, 0),
        ] {
            assert!(ExperimentalRoom::measure(kobo_ui::DisplayMetrics {
                width,
                height,
                pixels_per_inch,
                text_scale: kobo_ui::TextScale::Default,
            })
            .is_err());
        }
    }
    #[test]
    fn rgb_and_unknown_identity_use_existing_sdk_picture_commands() {
        let rgba = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255];
        for (identity, format, expected) in [
            (
                Some(true),
                kobo_ui::PictureFormat::Rgb,
                vec![255, 0, 0, 0, 255, 0, 0, 0, 255],
            ),
            (None, kobo_ui::PictureFormat::Grey, vec![54, 182, 18]),
            (Some(false), kobo_ui::PictureFormat::Grey, vec![54, 182, 18]),
        ] {
            let frame = PanelFrame::from_rgba(3, 1, &rgba, identity).unwrap();
            let mut context = kobo_sdk::AppRunner::new(EmptyApp).context();
            assert!(frame
                .put(&mut context, kobo_sdk::PictureHandle(700))
                .is_some());
            assert!(
                matches!(&context.commands()[0], kobo_sdk::Command::PutPicture { handle, width:3, height:1, format:f, pixels } if handle.0==700 && *f==format && *pixels==expected)
            );
        }
    }
    #[test]
    fn bundled_device_face_paints_into_both_panel_commands() {
        let bytes =
            include_bytes!("../../../crates/kobo-text/fonts/AtkinsonHyperlegible-Regular.ttf");
        let face = LocalFace::from_bytes(bytes).unwrap();
        assert!(face.advance('A', 16, false).is_some());
        assert!(face.raster('A', 16, false).is_some());
        assert_eq!(face.advance('A', 0, false), None);
        assert_eq!(face.advance('A', 257, false), None);
        let styled = kobo_web_document::parse_style_tree(b"<!doctype html><html style='background-color:#f08020'><body><p style='font-size:16px'>Hello world</p></body></html>", &[], &kobo_web_document::Limits::DEFAULT);
        let tree = kobo_web_document::box_tree::BoxTree::from_style(&styled);
        let list =
            kobo_web_document::css_text_page::paint_direct_text_blocks(&tree, 200, 80, &face)
                .unwrap();
        let rgba = list
            .rasterize(200, 80, kobo_web_document::display_list::Rgb(255, 255, 255))
            .unwrap();
        let rgb = PanelFrame::from_rgba(200, 80, &rgba, Some(true)).unwrap();
        let grey = PanelFrame::from_rgba(200, 80, &rgba, None).unwrap();
        assert_eq!(&rgb.pixels[0..3], &[240, 128, 32]);
        assert_eq!(grey.pixels[0], 144);
        for frame in [rgb, grey] {
            let mut context = kobo_sdk::AppRunner::new(EmptyApp).context();
            assert!(frame
                .put(&mut context, kobo_sdk::PictureHandle(700))
                .is_some());
            assert_eq!(context.commands().len(), 1);
        }
    }
    #[test]
    fn complete_page_queue_refuses_without_partial_commands() {
        let face = LocalFace::from_bytes(include_bytes!(
            "../../../crates/kobo-text/fonts/AtkinsonHyperlegible-Regular.ttf"
        ))
        .unwrap();
        let mut context = kobo_sdk::AppRunner::new(EmptyApp).context();
        for html in [
            b"<!doctype html><p style='display:flex'>ab</p>".as_slice(),
            b"<!doctype html><p>ab <em>cd</em></p>".as_slice(),
        ] {
            assert!(queue_page(
                &mut context,
                kobo_sdk::PictureHandle(700),
                html,
                &[],
                200,
                80,
                &face,
                None
            )
            .is_err());
            assert!(context.commands().is_empty());
        }
        assert!(queue_page(
            &mut context,
            kobo_sdk::PictureHandle(700),
            b"<!doctype html><p>Hello</p>",
            &[],
            200,
            80,
            &face,
            None
        )
        .is_ok());
        assert!(matches!(
            &context.commands()[0],
            kobo_sdk::Command::PutPicture {
                format: kobo_ui::PictureFormat::Grey,
                ..
            }
        ));
        context.drop_picture(kobo_sdk::PictureHandle(700));
        assert!(
            matches!(&context.commands()[1], kobo_sdk::Command::DropPicture(handle) if handle.0==700)
        );
    }
    #[test]
    fn complete_page_preserves_scoped_styles_and_rejects_input_truncation() {
        let face = LocalFace::from_bytes(include_bytes!(
            "../../../crates/kobo-text/fonts/AtkinsonHyperlegible-Regular.ttf"
        ))
        .unwrap();
        let html = b"<!doctype html><html><head><link rel=stylesheet href=page.css></head><body><p>Hello</p></body></html>";
        let frame = prepare_page(
            html,
            &[b"html { background-color: #123456 }".to_vec()],
            200,
            80,
            &face,
            Some(true),
        )
        .unwrap();
        assert_eq!(&frame.pixels[..3], &[18, 52, 86]);
        assert!(prepare_page(
            &vec![b' '; kobo_web_document::Limits::DEFAULT.max_input_bytes + 1],
            &[],
            200,
            80,
            &face,
            None
        )
        .is_err());
        assert!(prepare_page(html, &vec![vec![]; 3], 200, 80, &face, None).is_err());
        assert!(prepare_page(
            html,
            &[vec![
                b' ';
                kobo_web_document::Limits::DEFAULT.max_input_bytes + 1
            ]],
            200,
            80,
            &face,
            None
        )
        .is_err());
    }
    #[test]
    fn interaction_and_overflow_refuse_without_partial_queue() {
        let face = LocalFace::from_bytes(include_bytes!(
            "../../../crates/kobo-text/fonts/AtkinsonHyperlegible-Regular.ttf"
        ))
        .unwrap();
        let mut context = kobo_sdk::AppRunner::new(EmptyApp).context();
        for html in [
            "<!doctype html><a style='display:block' href='/next'>Next</a>",
            "<!doctype html><form style='display:block'>Text</form>",
            "<!doctype html><button style='display:block'>Go</button>",
            "<!doctype html><p style='height:100px;background-color:red'>Text</p>",
            "<!doctype html><p style='width:250px;background-color:red'>Text</p>",
            "<!doctype html><p style='width:50px'>One two three four five six seven eight nine ten</p>",
        ] {
            assert!(queue_page(&mut context,kobo_sdk::PictureHandle(700),html.as_bytes(),&[],200,80,&face,None).is_err(), "{html}");
            assert!(context.commands().is_empty());
        }
    }
    #[test]
    fn slot_replacement_queues_new_before_dropping_old_and_clear_is_idempotent() {
        let mut slot =
            PictureSlot::new([kobo_sdk::PictureHandle(700), kobo_sdk::PictureHandle(701)]).unwrap();
        let mut context = kobo_sdk::AppRunner::new(EmptyApp).context();
        let frame = || PanelFrame::from_rgba(1, 1, &[1, 2, 3, 255], None).unwrap();
        slot.replace(&mut context, frame()).unwrap();
        slot.replace(&mut context, frame()).unwrap();
        slot.replace(&mut context, frame()).unwrap();
        slot.clear(&mut context);
        slot.clear(&mut context);
        let commands = context.commands();
        assert_eq!(commands.len(), 6);
        for (i, h) in [(0, 700), (1, 701), (3, 700)] {
            assert!(
                matches!(&commands[i], kobo_sdk::Command::PutPicture { handle, .. } if handle.0==h)
            );
        }
        for (i, h) in [(2, 700), (4, 701), (5, 700)] {
            assert!(matches!(&commands[i], kobo_sdk::Command::DropPicture(handle) if handle.0==h));
        }
    }
    #[test]
    fn slot_rejection_keeps_old_ownership_until_explicit_fallback_clear() {
        assert!(PictureSlot::new([kobo_sdk::PictureHandle(700); 2]).is_err());
        let mut slot =
            PictureSlot::new([kobo_sdk::PictureHandle(700), kobo_sdk::PictureHandle(701)]).unwrap();
        let mut context = kobo_sdk::AppRunner::new(EmptyApp).context();
        slot.replace(
            &mut context,
            PanelFrame::from_rgba(1, 1, &[1, 2, 3, 255], None).unwrap(),
        )
        .unwrap();
        let _ = context.take_commands();
        let invalid = PanelFrame {
            width: 1,
            height: 1,
            format: kobo_ui::PictureFormat::Rgb,
            pixels: vec![0],
        };
        assert!(slot.replace(&mut context, invalid).is_err());
        assert!(context.commands().is_empty());
        slot.clear(&mut context);
        assert!(matches!(&context.commands()[0], kobo_sdk::Command::DropPicture(h) if h.0==700));
    }
    #[test]
    fn invalid_surfaces_and_fonts_refuse_without_a_picture() {
        assert!(LocalFace::from_bytes(&[]).is_err());
        assert!(LocalFace::from_bytes(b"not a font").is_err());
        for (w, h, bytes) in [
            (0, 1, vec![]),
            (1, 1, vec![0; 3]),
            (1, 1, vec![0, 0, 0, 0]),
            (u32::MAX, u32::MAX, vec![]),
        ] {
            assert!(PanelFrame::from_rgba(w, h, &bytes, Some(true)).is_err());
        }
    }
}

#[cfg(test)]
mod core_tests {
    use super::*;
    #[test]
    fn explicit_bundled_source_has_notice_and_consistent_size_metrics() {
        let face = LocalFace::bundled_reader_face().unwrap();
        assert!(LocalFace::bundled_license().contains("SIL OPEN FONT LICENSE"));
        for size in [12, 16, 20, 24, 32, 48] {
            let natural = face.line_height(size, false).unwrap();
            let baseline = face.baseline_offset(size, false).unwrap();
            assert!(baseline >= 0 && u32::try_from(baseline).unwrap() < natural);
            for ch in ['A', 'T', 'g', ' ', '\u{a0}'] {
                assert!(face.advance(ch, size, false).unwrap() > 0);
                let glyph = face.raster(ch, size, false).unwrap();
                assert_eq!(
                    glyph.coverage.len(),
                    usize::try_from(glyph.width * glyph.height).unwrap()
                );
            }
        }
        assert_eq!(face.advance('A', 0, false), None);
        assert_eq!(face.advance('A', 257, false), None);
        assert_eq!(face.advance('\u{10ffff}', 16, false), None);
        assert!(face.raster('\u{10ffff}', 16, false).is_none());
    }
    struct UnusedFont;
    impl FontProvider for UnusedFont {
        fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> {
            panic!("invalid room asked font")
        }
        fn line_height(&self, _: u32, _: bool) -> Option<u32> {
            panic!("invalid room asked font")
        }
        fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> {
            panic!("invalid room asked font")
        }
        fn raster(&self, _: char, _: u32, _: bool) -> Option<GlyphBitmap> {
            panic!("invalid room asked font")
        }
    }
    #[test]
    fn room_budget_refuses_before_font_and_frame_accessors_preserve_validation() {
        for (w, h) in [(0, 80), (200, 0), (u32::MAX, u32::MAX), (1025, 1024)] {
            assert_eq!(
                prepare_page(b"<!doctype html><p>Hello</p>", &[], w, h, &UnusedFont, None),
                Err("invalid room budget")
            );
        }
        let frame = PanelFrame::from_rgba(1, 1, &[1, 2, 3, 255], Some(true)).unwrap();
        assert_eq!(frame.dimensions(), (1, 1));
        assert_eq!(frame.format(), kobo_ui::PictureFormat::Rgb);
        assert_eq!(frame.pixels(), &[1, 2, 3]);
    }
    #[test]
    fn pure_core_prepares_device_face_without_sdk_dependencies() {
        let face = LocalFace::from_bytes(include_bytes!(
            "../../../crates/kobo-text/fonts/AtkinsonHyperlegible-Regular.ttf"
        ))
        .unwrap();
        let html = b"<!doctype html><html style='background-color:#123456'><body><p>Hello</p></body></html>";
        let rgb = prepare_page(html, &[], 200, 80, &face, Some(true)).unwrap();
        let grey = prepare_page(html, &[], 200, 80, &face, None).unwrap();
        assert_eq!(&rgb.pixels[..3], &[18, 52, 86]);
        assert_eq!(grey.format, kobo_ui::PictureFormat::Grey);
        assert_eq!(grey.pixels[0], 47);
    }

    /// Test face with configurable ink bearings: advances are a constant 4px,
    /// the line box is 10px with baseline 7, and each non-space glyph inks a
    /// width x height mask offset by (left, top) from the pen/baseline.
    struct BearingFace {
        left: i32,
        top: i32,
        width: u32,
        height: u32,
    }
    impl FontProvider for BearingFace {
        fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> {
            Some(4)
        }
        fn line_height(&self, _: u32, _: bool) -> Option<u32> {
            Some(10)
        }
        fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> {
            Some(7)
        }
        fn raster(&self, character: char, _: u32, _: bool) -> Option<GlyphBitmap> {
            if character == ' ' {
                return Some(GlyphBitmap {
                    left: 0,
                    top: 0,
                    width: 0,
                    height: 0,
                    coverage: Vec::new(),
                });
            }
            Some(GlyphBitmap {
                left: self.left,
                top: self.top,
                width: self.width,
                height: self.height,
                coverage: vec![255; usize::try_from(self.width * self.height).unwrap()],
            })
        }
    }

    fn rgb_pixel(frame: &PanelFrame, x: usize, y: usize) -> [u8; 3] {
        let at = (y * usize::try_from(frame.dimensions().0).unwrap() + x) * 3;
        frame.pixels()[at..at + 3].try_into().unwrap()
    }

    #[test]
    fn bearing_overhang_at_page_origin_renders_without_dropping_ink() {
        // DejaVu 'T' has a negative left side bearing: ink starts at x=-1
        // when the pen sits at x=0. The layout cell is fully on-page, so the
        // seam accepts and the rasterizer clips the overhang like a browser.
        let face = BearingFace {
            left: -1,
            top: -8,
            width: 3,
            height: 9,
        };
        let frame = prepare_page(
            b"<!doctype html><style>body{margin:0} p{margin:0}</style><p>ab</p>",
            &[],
            200,
            80,
            &face,
            Some(true),
        )
        .unwrap();
        // Ink rows: line top 0 + baseline 7 + top -8 = -1, clipped to row 0.
        assert_eq!(rgb_pixel(&frame, 0, 0), [0, 0, 0]);
        assert_eq!(rgb_pixel(&frame, 1, 0), [0, 0, 0]);
        assert_eq!(rgb_pixel(&frame, 2, 0), [255, 255, 255]);
    }

    #[test]
    fn right_edge_bearing_overhang_clips_at_the_last_column() {
        // Fifty 4px cells end exactly at the 200px page edge; only the last
        // glyph's ink overhangs, and it clips at the final column.
        let face = BearingFace {
            left: 3,
            top: -1,
            width: 3,
            height: 3,
        };
        let text = "a".repeat(50);
        let html =
            format!("<!doctype html><style>body{{margin:0}} p{{margin:0}}</style><p>{text}</p>");
        let frame = prepare_page(html.as_bytes(), &[], 200, 80, &face, Some(true)).unwrap();
        assert_eq!(rgb_pixel(&frame, 199, 6), [0, 0, 0]);
        assert_eq!(rgb_pixel(&frame, 198, 6), [255, 255, 255]);
    }

    #[test]
    fn static_capture_renders_links_inert_while_prepare_still_refuses() {
        let face = BearingFace {
            left: 0,
            top: -1,
            width: 2,
            height: 2,
        };
        let html = b"<!doctype html><style>body{margin:0} p{margin:0}</style><p><a href='/next'>ab</a></p>";
        // Shipping prepare refuses the interactive page, flag or no flag.
        assert_eq!(
            prepare_page(html, &[], 200, 80, &face, Some(true)).unwrap_err(),
            "interactive page needs semantic fallback"
        );
        assert_eq!(
            prepare_page_masked(html, &[], 200, 80, &face, Some(true)).unwrap_err(),
            "interactive page needs semantic fallback"
        );
        // The measurement-only capture renders the link text inert instead.
        let (frame, bounds) = prepare_page_static(html, &[], 200, 80, &face, Some(true)).unwrap();
        assert_eq!(rgb_pixel(&frame, 0, 6), [0, 0, 0]);
        assert_eq!(rgb_pixel(&frame, 1, 6), [0, 0, 0]);
        assert_eq!(rgb_pixel(&frame, 2, 6), [255, 255, 255]);
        assert!(!bounds.is_empty());
    }
    #[test]
    fn bottom_edge_bearing_overhang_clips_at_the_last_row() {
        // Eight 10px line boxes exactly fill the 80px page; the last line's
        // descender ink reaches five rows below the page bottom and clips.
        let face = BearingFace {
            left: 0,
            top: 2,
            width: 2,
            height: 6,
        };
        let words: Vec<String> = (0..16).map(|_| "x".repeat(24)).collect();
        let html = format!(
            "<!doctype html><style>body{{margin:0}} p{{margin:0}}</style><p>{}</p>",
            words.join(" ")
        );
        let frame = prepare_page(html.as_bytes(), &[], 200, 80, &face, Some(true)).unwrap();
        // Last line box 70..80 is fully on-page; ink rows 79..84 clip at 79,
        // and row 78 sits between the seventh and eighth lines' ink.
        assert_eq!(rgb_pixel(&frame, 0, 79), [0, 0, 0]);
        assert_eq!(rgb_pixel(&frame, 0, 78), [255, 255, 255]);
    }

    #[test]
    fn negative_text_origin_still_refuses_handoff() {
        // An escaped negative top margin puts the first line box at y=-5:
        // genuine off-page layout, not ink bearing, so the seam refuses.
        let face = BearingFace {
            left: -1,
            top: -1,
            width: 3,
            height: 3,
        };
        assert_eq!(
            prepare_page(
                b"<!doctype html><style>body{margin:0} p{margin:0}</style><p style='margin-top:-5px'>ab</p>",
                &[],
                200,
                80,
                &face,
                Some(true),
            )
            .unwrap_err(),
            "page needs scrolling or pagination"
        );
    }

    #[test]
    fn one_pixel_layout_overflow_still_refuses_handoff() {
        let face = BearingFace {
            left: 0,
            top: -1,
            width: 2,
            height: 2,
        };
        let words: Vec<String> = (0..16).map(|_| "x".repeat(24)).collect();
        let page = |margin: &str| {
            format!(
                "<!doctype html><style>body{{margin:0}} p{{margin:0;margin-top:{margin}}}</style><p>{}</p>",
                words.join(" ")
            )
        };
        // Eight 10px line boxes exactly fill the 80px page: accepted.
        prepare_page(page("0px").as_bytes(), &[], 200, 80, &face, Some(true)).unwrap();
        // One escaped pixel of margin moves the last line box to y=71, one
        // pixel past the page bottom: refuse, never silently clip text.
        assert_eq!(
            prepare_page(page("1px").as_bytes(), &[], 200, 80, &face, Some(true)).unwrap_err(),
            "page needs scrolling or pagination"
        );
    }

    #[test]
    fn multiline_overflow_still_refuses_handoff() {
        let face = BearingFace {
            left: 0,
            top: -1,
            width: 2,
            height: 2,
        };
        let words: Vec<String> = (0..18).map(|_| "x".repeat(24)).collect();
        let html = format!(
            "<!doctype html><style>body{{margin:0}} p{{margin:0}}</style><p>{}</p>",
            words.join(" ")
        );
        assert_eq!(
            prepare_page(html.as_bytes(), &[], 200, 80, &face, Some(true)).unwrap_err(),
            "page needs scrolling or pagination"
        );
    }
}

#[cfg(test)]
mod lazy_bold_tests {
    use super::*;

    #[test]
    fn in_place_compaction_is_byte_equivalent_for_all_channels_and_identities() {
        // Deterministic xorshift pattern so R, G and B each see many distinct
        // byte values, with every pixel opaque.
        let (width, height) = (251u32, 131u32);
        let mut state = 0x243F_6A88_u32;
        let mut rgba = Vec::new();
        for _ in 0..width * height {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let word = state.to_be_bytes();
            rgba.extend_from_slice(&[word[1], word[2], word[3], 255]);
        }
        for colour in [Some(true), Some(false), None] {
            let reference = PanelFrame::from_rgba(width, height, &rgba, colour).unwrap();
            let compacted =
                PanelFrame::from_rgba_in_place(width, height, rgba.clone(), colour).unwrap();
            assert_eq!(reference, compacted);
        }
        // Spot-check exact grey luminance values against the integer Rec.709 rule.
        let rgba = [
            255, 255, 255, 255, 0, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255,
        ];
        let grey = PanelFrame::from_rgba_in_place(5, 1, rgba.to_vec(), None).unwrap();
        assert_eq!(grey.pixels(), &[255, 0, 54, 182, 18]);
        let rgb = PanelFrame::from_rgba_in_place(5, 1, rgba.to_vec(), Some(true)).unwrap();
        assert_eq!(
            rgb.pixels(),
            &[255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]
        );
    }

    #[test]
    fn in_place_compaction_rejects_exactly_like_the_borrowing_path() {
        // Transparency anywhere refuses, including after valid pixels.
        let mut rgba = vec![7u8, 8, 9, 255];
        rgba.extend_from_slice(&[1, 2, 3, 254]);
        assert_eq!(
            PanelFrame::from_rgba(2, 1, &rgba, Some(true)).unwrap_err(),
            "nonopaque raster"
        );
        assert_eq!(
            PanelFrame::from_rgba_in_place(2, 1, rgba, Some(true)).unwrap_err(),
            "nonopaque raster"
        );
        assert_eq!(
            PanelFrame::from_rgba_in_place(1, 1, vec![1, 2, 3, 0], None).unwrap_err(),
            "nonopaque raster"
        );
        // Checked length: wrong byte count, empty, zero-sized and overflow
        // surfaces all refuse identically on both paths.
        for (w, h, len) in [
            (2u32, 2u32, 15usize),
            (2, 2, 17),
            (1, 1, 0),
            (0, 4, 0),
            (4, 0, 0),
            (u32::MAX, u32::MAX, 0),
            (3, 3, 4),
        ] {
            let bytes = vec![255u8; len];
            let reference = PanelFrame::from_rgba(w, h, &bytes, Some(true));
            let compacted = PanelFrame::from_rgba_in_place(w, h, bytes, Some(true));
            assert_eq!(reference, compacted);
        }
    }
    #[test]
    fn unused_bold_loader_is_never_called_and_failure_is_cached() {
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let count = calls.clone();
        let face = LocalFace::with_bold_loader(
            include_bytes!("../../../crates/kobo-text/fonts/AtkinsonHyperlegible-Regular.ttf"),
            Box::new(move || {
                count.set(count.get() + 1);
                None
            }),
        )
        .unwrap();
        let plain = b"<!doctype html><p>ab</p>";
        assert!(prepare_page(plain, &[], 200, 100, &face, Some(true)).is_ok());
        assert_eq!(calls.get(), 0);
        let bold = b"<!doctype html><p>a <strong>b</strong></p>";
        assert!(prepare_page(bold, &[], 200, 100, &face, Some(true)).is_err());
        assert!(prepare_page(bold, &[], 200, 100, &face, Some(true)).is_err());
        assert_eq!(calls.get(), 1);
        assert!(prepare_page(plain, &[], 200, 100, &face, Some(true)).is_ok());
    }
}

#[cfg(test)]
mod normal_constructor_tests {
    use super::*;
    fn table_offset(bytes: &[u8], tag: [u8; 4]) -> usize {
        let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap());
        for r in bytes[12..12 + usize::from(count) * 16].chunks_exact(16) {
            if r[..4] == tag {
                return usize::try_from(u32::from_be_bytes(r[8..12].try_into().unwrap())).unwrap();
            }
        }
        panic!("missing test font table");
    }
    #[test]
    fn byte_constructor_activates_only_sampled_sizes_and_keeps_explicit_css_legacy() {
        // Mutated fixture tests plumbing only, never Chrome parity or real-face eligibility.
        let mut bytes =
            include_bytes!("../../../crates/kobo-text/fonts/AtkinsonHyperlegible-Regular.ttf")
                .to_vec();
        let os = table_offset(&bytes, *b"OS/2");
        bytes[os + 62] &= 0xff;
        bytes[os + 63] &= !128;
        let legacy = LocalFace::from_bytes(&bytes).unwrap();
        let face = LocalFace::with_verified_normal_bytes(&bytes, Some(&bytes)).unwrap();
        for size in [8, 10, 12, 13, 14, 15, 16, 17, 18, 20, 24, 32, 48] {
            assert!(face.normal_line_metrics(size, false).is_some());
            assert_eq!(
                face.normal_line_metrics(size, false),
                face.normal_line_metrics(size, true)
            );
        }
        for size in [9, 11, 19, 21, 23, 25, 31, 33, 47, 49] {
            assert_eq!(face.normal_line_metrics(size, false), None);
            assert_eq!(face.normal_line_metrics(size, true), None);
        }
        for style in [
            "line-height:24px",
            "line-height:150%",
            "line-height:1.5",
            "font-size:19px",
        ] {
            let html = format!("<!doctype html><p style='{style}'>ab</p>");
            assert_eq!(
                prepare_page(html.as_bytes(), &[], 200, 100, &face, Some(true)),
                prepare_page(html.as_bytes(), &[], 200, 100, &legacy, Some(true)),
                "{style}"
            );
        }
        let mut bold = bytes.clone();
        let h = table_offset(&bold, *b"hhea");
        bold[h + 4..h + 6].copy_from_slice(&3000_i16.to_be_bytes());
        let mismatched = LocalFace::with_verified_normal_bytes(&bytes, Some(&bold)).unwrap();
        assert_eq!(mismatched.normal_line_metrics(16, false), None);
        assert_eq!(mismatched.normal_line_metrics(16, true), None);
        bold[os + 63] |= 128;
        let ineligible = LocalFace::with_verified_normal_bytes(&bytes, Some(&bold)).unwrap();
        assert_eq!(ineligible.normal_line_metrics(16, false), None);
        assert_eq!(ineligible.normal_line_metrics(16, true), None);
    }
    #[test]
    fn typo_face_keeps_legacy_contract_and_explicit_css_byte_identity() {
        let bytes =
            include_bytes!("../../../crates/kobo-text/fonts/AtkinsonHyperlegible-Regular.ttf");
        let legacy = LocalFace::from_bytes(bytes).unwrap();
        let verified = LocalFace::with_verified_normal_bytes(bytes, Some(bytes)).unwrap();
        for size in [8, 12, 16, 24, 48] {
            assert_eq!(verified.normal_line_metrics(size, false), None);
            assert_eq!(verified.normal_line_metrics(size, true), None);
            assert_eq!(
                verified.line_height(size, false),
                legacy.line_height(size, false)
            );
            assert_eq!(
                verified.baseline_offset(size, false),
                legacy.baseline_offset(size, false)
            );
        }
        for style in [
            "",
            "line-height:normal",
            "line-height:24px",
            "line-height:150%",
            "line-height:1.5",
        ] {
            let html = format!("<!doctype html><p style='{style}'>ab</p>");
            assert_eq!(
                prepare_page(html.as_bytes(), &[], 200, 100, &verified, Some(true)),
                prepare_page(html.as_bytes(), &[], 200, 100, &legacy, Some(true)),
                "{style}"
            );
        }
    }
}

#[cfg(test)]
mod nested_interaction_guard_tests {
    use super::*;
    #[test]
    fn nested_link_never_bypasses_shipping_interaction_guard() {
        let face = LocalFace::bundled_reader_face().unwrap();
        let html = b"<!doctype html><p>aa<span><a href='https://example.org'>bb</a></span>cc</p>";
        assert_eq!(
            prepare_page(html, &[], 200, 100, &face, Some(true)),
            Err("interactive page needs semantic fallback")
        );
    }
}

#[cfg(test)]
mod media_width_wiring {
    use super::*;
    #[test]
    fn page_media_queries_see_the_same_width_the_page_is_prepared_with() {
        let face = LocalFace::from_bytes(include_bytes!(
            "../../../crates/kobo-text/fonts/AtkinsonHyperlegible-Regular.ttf"
        ))
        .unwrap();
        let html = b"<!doctype html><style>html{background:#0000fa}@media (min-width:300px){html{background:#fa0000}}</style><body><p>Hi</p></body>";
        let narrow = prepare_page(html, &[], 299, 80, &face, Some(true)).unwrap();
        let wide = prepare_page(html, &[], 300, 80, &face, Some(true)).unwrap();
        assert_eq!(&narrow.pixels()[0..3], &[0, 0, 250]);
        assert_eq!(&wide.pixels()[0..3], &[250, 0, 0]);
        // An unknown media feature still refuses the whole page.
        let unknown = b"<!doctype html><style>@media (hover:hover){html{background:#fa0000}}</style><body><p>Hi</p></body>";
        assert!(prepare_page(unknown, &[], 300, 80, &face, Some(true)).is_err());
    }
}
