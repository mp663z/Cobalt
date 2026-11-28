//! Opt-in painted pages. The native reader remains available on every screen.
use crate::page_view::{PageFontAdapter, PageView};
use kobo_sdk::keyboard::Keyboard;
use kobo_sdk::{Context, DisplayMetrics, PictureHandle, ScreenBuilder, TilePicture};

/// Bounded app profile: enough slots for common documentation themes, with an independent total cap.
pub(crate) const SHEET_BUDGET:kobo_web_document::SupplyBudget=kobo_web_document::SupplyBudget{max_linked_sheets:16,max_sheet_bytes:512*1024,max_aggregate_bytes:4*1024*1024};
pub(crate) const SCROLL_BASE:kobo_sdk::ActionId=kobo_sdk::ActionId(0x7ffd_0000);
pub(crate) const HANDLE: PictureHandle = PictureHandle(0x7fff_0001);

pub(crate) struct PageMode {
    pub(crate) view: PageView,
    pub(crate) page: usize,
    pub(crate) metrics: DisplayMetrics,
    viewport: (u32, u32),
    scroll_top:Option<u32>,
    reading_anchor:Option<(usize,usize,u32)>,
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
        Self::build_zoom_with_sheets(bytes, &[], title, metrics, zoom)
    }

    /// CSS pixel width the painter uses at `zoom`; the sheet census must be evaluated at exactly this width.
    pub(crate) fn css_width(metrics: &DisplayMetrics, zoom: u16) -> Option<u32> {
        let room_width = u32::try_from(metrics.width - 2 * metrics.screen_margin()).ok()?;
        (zoom != 0).then(|| room_width * 100 / u32::from(zoom) / 2)
    }

    /// The external stylesheet links the painter will index for `bytes` at `zoom`: same normalised document, same
    /// light-scheme profile and same width as [`Self::build_zoom_with_sheets`]. Feed this to `SheetSupply::begin`
    /// and pass the Ready bodies back, unchanged and in order.
    pub(crate) fn sheet_census(
        bytes: &[u8],
        url: &kobo_web_document::Url,
        metrics: &DisplayMetrics,
        zoom: u16,
    ) -> Option<kobo_web_document::style_tree::HostSheetLinks> {
        let width = Self::css_width(metrics, zoom)?;
        let media = kobo_web_document::MediaContext::new(width).map(kobo_web_document::MediaContext::with_light_scheme);
        let ((normalised, _), _, _) = kobo_web_document::basic_page::normalize_for_basic_full(bytes);
        let limits = kobo_web_document::Limits {
            max_input_bytes: 8 * 1024 * 1024,
            ..kobo_web_document::Limits::DEFAULT
        };
        Some(kobo_web_document::host_stylesheet_links(&normalised, url, &limits, media))
    }

    pub(crate) fn build_zoom_with_sheets(
        bytes: &[u8],
        sheets: &[Vec<u8>],
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
        let view = PageView::paint_with_sheets(
            bytes,
            sheets,
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
            scroll_top:None,
            reading_anchor:None,
        })
    }

    #[cfg(test)]
    pub(crate) fn top_for_test(&self)->u32{self.top()}
    #[cfg(test)]
    pub(crate) fn is_window_for_test(&self)->bool{self.scroll_top.is_some()}
    pub(crate) fn scroll_position(&self)->u32{self.top()}
    pub(crate) fn reading_anchor(&self)->Option<(usize,usize,u32)>{
        self.reading_anchor.or_else(||self.view.reading_anchor(self.top()))
    }
    pub(crate) fn restore_anchor(&mut self,anchor:(usize,usize,u32)){
        if let Some(top)=self.view.anchor_top(anchor){self.restore_scroll(top);self.reading_anchor=Some(anchor);}
    }
    pub(crate) fn restore_scroll(&mut self,top:u32){
        self.reading_anchor=None;
        let top=top.min(self.view.max_scroll());self.page=self.view.page_at(top);
        // A page-button boundary must use its complete-line pagination again,
        // rather than a continuous window exposing a clipped next line.
        self.scroll_top=(top!=self.view.page_top(self.page)).then_some(top);
    }
    fn top(&self)->u32{self.scroll_top.unwrap_or_else(||self.view.page_top(self.page))}
    fn bottom(&self)->u32{self.top().saturating_add(self.view.viewport_height())}
    pub(crate) fn visible_links(&self)->Vec<usize>{if self.scroll_top.is_some(){self.view.window_screen_links(self.top(),self.bottom())}else{self.view.screen_links(self.page)}}
    pub(crate) fn turn_to(&mut self,page:usize){self.page=page.min(self.view.pages().saturating_sub(1));self.scroll_top=None;self.reading_anchor=None;}
    pub(crate) fn scroll(&mut self,amount:i32){
        self.reading_anchor=None;
        let delta=i64::from(amount)*i64::from(self.view.viewport_height())/100;
        let top=(i64::from(self.top())+delta).clamp(0,i64::from(self.view.max_scroll())) as u32;
        self.scroll_top=Some(top);self.page=self.view.page_at(top);
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
            if self.scroll_top.is_some(){return room.min(height).max(1);}
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
        let regions=if self.scroll_top.is_some(){self.view.window_links(self.top(),self.bottom(),width,height,self.viewport.1)}else{self.view.picture_links(self.page,width,height,self.viewport.1)};
        let links:Vec<_>=regions.into_iter().filter_map(|(mut region,index)|{region.action=resolve(index)?;Some(region)}).collect();
        let screen=frame(
            title,
            width,
            height,
            self.page,
            self.view.pages(),
            address,
            keyboard.is_some(),
        );
        screen.picture_links(links).picture_scroll(keyboard.is_none().then_some(SCROLL_BASE))
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
        let source = if self.scroll_top.is_some(){self.view.raster_window_rgb(self.top(),self.bottom())}else{self.view.raster_page_rgb(self.page)};
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

#[cfg(test)]
mod sheet_supply_plumbing_tests {
    use super::*;
    use kobo_web_document::Url;

    const PAGE: &[u8] = b"<!DOCTYPE html><html><head><title>t</title>\
<link rel=\"stylesheet\" href=\"https://example.test/a.css\">\
<link rel=\"stylesheet\" media=\"(prefers-color-scheme: dark)\" href=\"https://example.test/dark.css\">\
<link rel=\"stylesheet\" href=\"https://example.test/b.css\"></head>\
<body><h1>Head</h1><p>one two three</p></body></html>";

    fn metrics() -> DisplayMetrics {
        kobo_ui::CLARA_BW_METRICS
    }

    #[test]
    fn census_matches_the_painter_width_and_skips_dark_scheme_links() {
        let url = Url::parse("https://example.test/p.html").unwrap();
        let census = PageMode::sheet_census(PAGE, &url, &metrics(), 100).expect("census");
        assert!(census.complete);
        assert_eq!(census.unknown_media, 0);
        let hosts: Vec<String> = census.links.iter().map(|l| l.as_ref().unwrap().to_string()).collect();
        assert_eq!(hosts, ["https://example.test/a.css", "https://example.test/b.css"]);
        assert_eq!(census.link_takes_host_slot, [true, false, true]);
        assert!(PageMode::css_width(&metrics(), 0).is_none());
        let (half, full) = (
            PageMode::css_width(&metrics(), 200).unwrap() * 2,
            PageMode::css_width(&metrics(), 100).unwrap(),
        );
        assert!(half.abs_diff(full) <= 1, "{half} vs {full}");
    }

    #[test]
    fn supplied_sheets_change_the_painted_page_and_mismatched_counts_do_not_panic() {
        kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
        let face = PageFontAdapter::bundled().expect("bundled font");
        let plain = PageView::paint(PAGE, 400, 100, 2, &face, &face, &face).expect("plain");
        let tall = b"h1{margin-top:200px;font-size:40px} p{margin:80px 0}".to_vec();
        let styled = PageView::paint_with_sheets(
            PAGE,
            &[tall.clone(), b"p{color:red}".to_vec()],
            400,
            100,
            2,
            &face,
            &face,
            &face,
        )
        .expect("styled");
        assert!(styled.max_scroll() > plain.max_scroll(), "{} vs {}", styled.max_scroll(), plain.max_scroll());
        // The host owes exactly census.links.len() bodies; a wrong count is refused by the engine, never a panic.
        let _ = PageView::paint_with_sheets(PAGE, &[tall], 400, 100, 2, &face, &face, &face);
    }
}
