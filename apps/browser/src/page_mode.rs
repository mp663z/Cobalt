//! Opt-in painted pages. The native reader remains available on every screen.
use crate::page_view::{PageFontAdapter, PageView};
use kobo_sdk::{Context, DisplayMetrics, PictureHandle, ScreenBuilder, TilePicture};

pub(crate) const HANDLE: PictureHandle = PictureHandle(0x7fff_0001);

pub(crate) struct PageMode {
    pub(crate) view: PageView,
    pub(crate) page: usize,
    pub(crate) metrics: DisplayMetrics,
}

fn frame(
    title: &str,
    width: u32,
    height: u32,
    page: usize,
    pages: usize,
    address: &str,
) -> ScreenBuilder {
    ScreenBuilder::new("browser-painted-page")
        .top_bar(format!("{}/{pages} {title}", page + 1))
        .top_bar_action("native-reader", "Reader")
        .top_bar_action("navigate", "Navigate")
        .reading(true)
        .page_turns("previous-page", "next-page")
        .action_bar([
            ("back", "Back"),
            ("address", "Go to"),
            ("forward", "Forward"),
        ])
        .field("edit-address", address, "Search or enter address")
        .unframed_picture(TilePicture::new(HANDLE, width, height), 500)
}

impl PageMode {
    pub(crate) fn build(bytes: &[u8], title: &str, metrics: &DisplayMetrics) -> Option<Self> {
        let width = u32::try_from(metrics.width).ok()?;
        let height = u32::try_from(metrics.height).ok()?;
        // Measure actual chrome, never subtract remembered toolbar dimensions.
        let placeholder = frame(title, width, height, 0, 1, "").build();
        let (room_width, room_height) =
            kobo_web_layout::picture_box(&placeholder, metrics, HANDLE)?;
        // Refuse before raster allocation when even RGB cannot fit the wire budget.
        if u64::from(room_width) * u64::from(room_height) * 3 > kobo_sdk::MAX_PICTURE_BYTES as u64 {
            return None;
        }
        let face = PageFontAdapter::bundled()?;
        let view = PageView::paint(
            bytes,
            room_width / 2,
            room_height / 2,
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
        })
    }

    pub(crate) fn screen(&self, title: &str, address: &str) -> ScreenBuilder {
        let (width, height) = self.view.screen_device_px();
        frame(title, width, height, self.page, self.view.pages(), address)
    }

    pub(crate) fn publish(&self, context: &mut Context, colour: bool) -> bool {
        let (width, height) = self.view.screen_device_px();
        if colour {
            context
                .put_colour_picture(HANDLE, width, height, self.view.raster_page_rgb(self.page))
                .is_some()
        } else {
            context
                .put_picture(HANDLE, width, height, self.view.raster_page_grey(self.page))
                .is_some()
        }
    }
}

pub(crate) fn retain_source(bytes: &[u8]) -> Option<Vec<u8>> {
    (bytes.len() <= 8 * 1024 * 1024).then(|| bytes.to_vec())
}
