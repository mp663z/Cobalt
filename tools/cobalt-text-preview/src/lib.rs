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
    /// Load one explicit TrueType/OpenType face with a bounded source buffer.
    /// # Errors
    /// Rejects empty, oversized or malformed fonts.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.is_empty() || bytes.len() > kobo_sdk::MAX_FONT_BYTES {
            return Err("invalid font byte budget");
        }
        Font::from_bytes(bytes, FontSettings::default())
            .map(Self)
            .map_err(|_| "invalid font")
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct PanelFrame {
    pub width: u32,
    pub height: u32,
    pub format: kobo_sdk::PictureFormat,
    pub pixels: Vec<u8>,
}
impl PanelFrame {
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
            kobo_sdk::PictureFormat::Rgb
        } else {
            kobo_sdk::PictureFormat::Grey
        };
        let bytes = format
            .byte_len(width, height)
            .filter(|&n| n <= kobo_sdk::MAX_PICTURE_BYTES)
            .ok_or("picture budget")?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(bytes).map_err(|_| "allocation")?;
        for pixel in rgba.chunks_exact(4) {
            if pixel[3] != 255 {
                return Err("nonopaque raster");
            }
            if format == kobo_sdk::PictureFormat::Rgb {
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
    pub fn put(
        self,
        context: &mut kobo_sdk::Context,
        handle: kobo_sdk::PictureHandle,
    ) -> Option<kobo_sdk::TilePicture> {
        match self.format {
            kobo_sdk::PictureFormat::Rgb => {
                context.put_colour_picture(handle, self.width, self.height, self.pixels)
            }
            kobo_sdk::PictureFormat::Grey => {
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
    let tree = kobo_web_document::box_tree::BoxTree::from_style(&styled);
    let list =
        kobo_web_document::css_text_page::paint_direct_text_blocks(&tree, width, height, font)
            .map_err(|_| "unsupported complete page")?;
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

#[cfg(test)]
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
                kobo_sdk::PictureFormat::Rgb,
                vec![255, 0, 0, 0, 255, 0, 0, 0, 255],
            ),
            (None, kobo_sdk::PictureFormat::Grey, vec![54, 182, 18]),
            (
                Some(false),
                kobo_sdk::PictureFormat::Grey,
                vec![54, 182, 18],
            ),
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
                format: kobo_sdk::PictureFormat::Grey,
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
