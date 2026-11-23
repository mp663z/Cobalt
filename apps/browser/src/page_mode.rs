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
    ) -> ScreenBuilder {
        let (width, _) = self.viewport;
        frame(
            title,
            width,
            self.visible_height(keyboard),
            self.page,
            self.view.pages(),
            address,
            keyboard.is_some(),
        )
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
            let sy =
                (u64::from(y) * u64::from(source_height) / u64::from(self.viewport.1)) as usize;
            for x in 0..width {
                let sx = (u64::from(x) * u64::from(source_width) / u64::from(width)) as usize;
                let at = (sy * source_width as usize + sx) * 3;
                let rgb = &source[at..at + 3];
                if colour {
                    pixels.extend_from_slice(rgb);
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
