//! Experimental font and panel-picture seam. Not linked into any shipping app.
use fontdue::{Font, FontSettings};
use kobo_web_document::{css_text_page::FontProvider, inline_lines::GlyphBitmap};
pub struct LocalFace(Font);
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
impl FontProvider for LocalFace {
    fn advance(&self, character: char, size: u32) -> Option<u32> {
        if self.0.lookup_glyph_index(character) == 0 {
            return None;
        }
        let width = self.0.metrics(character, Self::pixels(size)?).advance_width;
        Self::bounded_u32(width.round().max(1.0))
    }
    fn line_height(&self, size: u32) -> Option<u32> {
        let metrics = self.0.horizontal_line_metrics(Self::pixels(size)?)?;
        Self::bounded_u32((metrics.ascent - metrics.descent + metrics.line_gap).ceil())
    }
    fn baseline_offset(&self, size: u32) -> Option<i32> {
        let ascent = self
            .0
            .horizontal_line_metrics(Self::pixels(size)?)?
            .ascent
            .ceil();
        i32::try_from(Self::bounded_u32(ascent)?).ok()
    }
    fn raster(&self, character: char, size: u32) -> Option<GlyphBitmap> {
        if self.0.lookup_glyph_index(character) == 0 {
            return None;
        }
        let (metrics, coverage) = self.0.rasterize(character, Self::pixels(size)?);
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

    /// Load one explicit TrueType/OpenType face with a bounded source buffer.
    /// # Errors
    /// Rejects empty, oversized or malformed fonts.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.is_empty() || bytes.len() > kobo_web_document::Limits::DEFAULT.max_input_bytes {
            return Err("invalid font byte budget");
        }
        Font::from_bytes(bytes, FontSettings::default())
            .map(Self)
            .map_err(|_| "invalid font")
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
    let styled = kobo_web_document::parse_style_tree(html, sheets, &limits);
    // A picture has no semantic hit targets. Refuse controls/links until
    // a shipping caller can preserve their interactions, even if styled block.
    if styled.nodes.iter().any(|node| {
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
    }) {
        return Err("interactive page needs semantic fallback");
    }
    let tree = kobo_web_document::box_tree::BoxTree::from_style(&styled);
    let list =
        kobo_web_document::css_text_page::paint_direct_text_blocks(&tree, width, height, font)
            .map_err(|_| "unsupported complete page")?;
    // This seam has no scrolling or pagination. Viewport clipping is useful
    // in the paint primitive, but cannot silently drop content on handoff.
    if list.commands().iter().any(|command| {
        use kobo_web_document::display_list::Command;
        let rect = match command {
            Command::Fill { rect, .. } | Command::PushClip(rect) => rect,
            Command::GlyphRun { bounds, .. } => bounds,
            Command::PopClip => return false,
        };
        rect.x < 0
            || rect.y < 0
            || i64::from(rect.x) + i64::from(rect.width) > i64::from(width)
            || i64::from(rect.y) + i64::from(rect.height) > i64::from(height)
    }) {
        return Err("page needs scrolling or pagination");
    }
    let rgba = list
        .rasterize(
            width,
            height,
            kobo_web_document::display_list::Rgb(255, 255, 255),
        )
        .map_err(|_| "invalid complete raster")?;
    PanelFrame::from_rgba(width, height, &rgba, colour)
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
        assert!(face.advance('A', 16).is_some());
        assert!(face.raster('A', 16).is_some());
        assert_eq!(face.advance('A', 0), None);
        assert_eq!(face.advance('A', 257), None);
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
            let natural = face.line_height(size).unwrap();
            let baseline = face.baseline_offset(size).unwrap();
            assert!(baseline >= 0 && u32::try_from(baseline).unwrap() < natural);
            for ch in ['A', 'T', 'g', ' ', '\u{a0}'] {
                assert!(face.advance(ch, size).unwrap() > 0);
                let glyph = face.raster(ch, size).unwrap();
                assert_eq!(
                    glyph.coverage.len(),
                    usize::try_from(glyph.width * glyph.height).unwrap()
                );
            }
        }
        assert_eq!(face.advance('A', 0), None);
        assert_eq!(face.advance('A', 257), None);
        assert_eq!(face.advance('\u{10ffff}', 16), None);
        assert!(face.raster('\u{10ffff}', 16).is_none());
    }
    struct UnusedFont;
    impl FontProvider for UnusedFont {
        fn advance(&self, _: char, _: u32) -> Option<u32> {
            panic!("invalid room asked font")
        }
        fn line_height(&self, _: u32) -> Option<u32> {
            panic!("invalid room asked font")
        }
        fn baseline_offset(&self, _: u32) -> Option<i32> {
            panic!("invalid room asked font")
        }
        fn raster(&self, _: char, _: u32) -> Option<GlyphBitmap> {
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
}
