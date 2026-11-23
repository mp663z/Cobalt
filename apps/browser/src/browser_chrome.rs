//! Shared bottom browser controls. Measurement and display use the same nodes.
use kobo_sdk::ScreenBuilder;

pub(crate) const SLIDER_BASE: kobo_sdk::ActionId = kobo_sdk::ActionId(0x7ffe_0000);

pub(crate) fn toolbar(builder: ScreenBuilder, zoom: u16, _typing: bool) -> ScreenBuilder {
    builder.browser_bar(
        ((zoom.saturating_sub(75)) * 100 / 125).min(100) as u8,
        SLIDER_BASE,
    )
}

pub(crate) fn address_keys(builder: ScreenBuilder) -> ScreenBuilder {
    builder.fill().grid(
        8,
        false,
        [
            ("url-slash", "/"),
            ("url-dot", "."),
            ("url-com", ".com"),
            ("url-colon", ":"),
            ("kb.home", "|<"),
            ("kb.left", "<"),
            ("kb.right", ">"),
            ("kb.end", ">|"),
        ],
    )
}

pub(crate) fn fits(
    builder: ScreenBuilder,
    metrics: &kobo_sdk::DisplayMetrics,
    typing: bool,
) -> bool {
    let screen = builder.build();
    let layout = screen.layout_with(metrics, &kobo_sdk::Chrome::measuring(true));
    let required = if typing { "kb.enter" } else { "browser-more" };
    layout
        .rect_of_action(kobo_sdk::action_id(required))
        .is_some()
        && kobo_web_layout::fits(&screen, metrics)
}
