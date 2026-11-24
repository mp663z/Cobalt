//! Opt-in painted pages. The native reader remains available on every screen.
use crate::page_view::{PageFontAdapter, PageView};
use kobo_sdk::keyboard::Keyboard;
use kobo_sdk::{Context, DisplayMetrics, PictureHandle, ScreenBuilder, TilePicture};

pub(crate) const HANDLE: PictureHandle = PictureHandle(0x7fff_0001);

pub(crate) struct PageMode {
    pub(crate) view: PageView,
    pub(crate) page: usize,
    pub(crate) metrics: DisplayMetrics,
    viewport: (u32, u32),
}

fn frame(
    title: &str,
    width: u32,
    height: u32,
    page: usize,
    pages: usize,
    address: &str,
    typing: bool,
) -> ScreenBuilder {
    let builder =
        ScreenBuilder::new("browser-painted-page").top_bar(format!("{}/{pages} {title}", page + 1));
    let builder = if typing {
        builder
    } else {
        builder
            .top_bar_action("native-reader", "Reader")
            .top_bar_action("navigate", "Navigate")
    };
    builder
        .reading(true)
        .page_turns("previous-page", "next-page")
        .page_position((page+1).min(u16::MAX as usize) as u16,pages.max(1).min(u16::MAX as usize) as u16)
        .action_bar([
            ("back", "Back"),
            ("forward", "Forward"),
            ("reload", "Reload"),
        ])
        .field("edit-address", address, "Search or enter address")
        .unframed_picture(TilePicture::new(HANDLE, width, height), 500)
}

impl PageMode {
    pub(crate) fn build(bytes: &[u8], title: &str, metrics: &DisplayMetrics) -> Option<Self> {
        Self::build_zoom(bytes, title, metrics, 100)
    }

    pub(crate) fn build_zoom(
        bytes: &[u8],
        title: &str,
        metrics: &DisplayMetrics,
        zoom: u16,
    ) -> Option<Self> {
        let width = u32::try_from(metrics.width).ok()?;
        let height = u32::try_from(metrics.height).ok()?;
        // Measure actual chrome, never subtract remembered toolbar dimensions.
        let placeholder = crate::browser_chrome::toolbar(
            frame(title, width, height, 0, 1, "", false),
            zoom,
            false,
        )
        .build();
        let (_, room_height) = kobo_web_layout::picture_box(&placeholder, metrics, HANDLE)?;
        let room_width = u32::try_from(metrics.width - 2 * metrics.screen_margin()).ok()?;
        // Refuse before raster allocation when even RGB cannot fit the wire budget.
        if u64::from(room_width) * u64::from(room_height) * 3 > kobo_sdk::MAX_PICTURE_BYTES as u64 {
            return None;
        }
        let face = PageFontAdapter::bundled()?;
        let view = PageView::paint(
            bytes,
            room_width * 100 / u32::from(zoom) / 2,
            room_height * 100 / u32::from(zoom) / 2,
            2,
            &face,
            &face,
            &face,
        )
        .ok()?;
        if view.falls_back_to_reader() {
            return None;
        }
        Some(Self {
            view,
            page: 0,
            metrics: *metrics,
            viewport: (room_width, room_height),
        })
    }

    fn visible_height(&self, keyboard: Option<&Keyboard>) -> u32 {
        let (width, height) = self.viewport;
        let Some(keyboard) = keyboard else {
            return height;
        };
        let preview = crate::browser_chrome::address_keys(crate::browser_chrome::toolbar(
            frame("", width, height, 0, 1, "", true),
            100,
            true,
        ))
        .keyboard(keyboard, "Go")
        .build();
        kobo_web_layout::picture_box(&preview, &self.metrics, HANDLE).map_or(1, |(_, room)| {
            self.view.preview_height(
                self.page,
                room.min(height) * self.view.screen_device_px().1 / height,
            ) * height
                / self.view.screen_device_px().1
        })
    }

    pub(crate) fn screen(
        &self,
        title: &str,
        address: &str,
        keyboard: Option<&Keyboard>,
        resolve:impl Fn(usize)->Option<kobo_sdk::ActionId>,
    ) -> ScreenBuilder {
        let (width, _) = self.viewport;
        let height=self.visible_height(keyboard);
        let links:Vec<_>=self.view.picture_links(self.page,width,height,self.viewport.1).into_iter().filter_map(|(mut region,index)|{region.action=resolve(index)?;Some(region)}).collect();
        let screen=frame(
            title,
            width,
            height,
            self.page,
            self.view.pages(),
            address,
            keyboard.is_some(),
        );
        screen.picture_links(links)
    }

    pub(crate) fn publish(
        &self,
        context: &mut Context,
        colour: bool,
        keyboard: Option<&Keyboard>,
    ) -> bool {
        let (width, _) = self.viewport;
        let height = self.visible_height(keyboard);
        let (source_width, source_height) = self.view.screen_device_px();
        let source = self.view.raster_page_rgb(self.page);
        let mut pixels =
            Vec::with_capacity(width as usize * height as usize * if colour { 3 } else { 1 });
        for y in 0..height {
            for x in 0..width {
                let rgb = sample_rgb(
                    &source,
                    source_width,
                    source_height,
                    x,
                    y,
                    width,
                    self.viewport.1,
                );
                if colour {
                    pixels.extend_from_slice(&rgb);
                } else {
                    pixels.push(
                        ((u32::from(rgb[0]) * 77
                            + u32::from(rgb[1]) * 150
                            + u32::from(rgb[2]) * 29
                            + 128)
                            >> 8) as u8,
                    );
                }
            }
        }
        if colour {
            context
                .put_colour_picture(HANDLE, width, height, pixels)
                .is_some()
        } else {
            context.put_picture(HANDLE, width, height, pixels).is_some()
        }
    }
}

pub(crate) fn retain_source(bytes: &[u8]) -> Option<Vec<u8>> {
    (bytes.len() <= 8 * 1024 * 1024).then(|| bytes.to_vec())
}

/// Pixel-centred bilinear reconstruction. Identity sampling remains byte exact.
fn sample_rgb(source: &[u8], sw: u32, sh: u32, x: u32, y: u32, dw: u32, dh: u32) -> [u8; 3] {
    fn axis(at: u32, source: u32, dest: u32) -> (usize, usize, u64) {
        let position =
            (((u64::from(at) * 2 + 1) * u64::from(source) * 256 / u64::from(dest)) as i64 - 256)
                .max(0) as u64;
        let left = (position / 512).min(u64::from(source - 1));
        (
            left as usize,
            (left + 1).min(u64::from(source - 1)) as usize,
            position % 512,
        )
    }
    let (x0, x1, fx) = axis(x, sw, dw);
    let (y0, y1, fy) = axis(y, sh, dh);
    let pixel = |x: usize, y: usize, c: usize| u64::from(source[(y * sw as usize + x) * 3 + c]);
    let mut result = [0; 3];
    for (c, out) in result.iter_mut().enumerate() {
        let top = pixel(x0, y0, c) * (512 - fx) + pixel(x1, y0, c) * fx;
        let bottom = pixel(x0, y1, c) * (512 - fx) + pixel(x1, y1, c) * fx;
        *out = ((top * (512 - fy) + bottom * fy + 131072) / 262144) as u8;
    }
    result
}

#[cfg(test)]
mod sampling_tests {
    use super::sample_rgb;
    #[test]
    fn zoom_sampling_interpolates_without_changing_identity() {
        let pixels = [0, 0, 0, 255, 255, 255];
        assert_eq!(sample_rgb(&pixels, 2, 1, 0, 0, 2, 1), [0; 3]);
        assert_eq!(sample_rgb(&pixels, 2, 1, 1, 0, 2, 1), [255; 3]);
        assert_eq!(sample_rgb(&pixels, 2, 1, 1, 0, 4, 1), [64; 3]);
        assert_eq!(sample_rgb(&pixels, 2, 1, 2, 0, 4, 1), [191; 3]);
    }
}
