//! Shared bottom browser controls. Measurement and display use the same nodes.
use kobo_sdk::{Glyph, ScreenBuilder};

pub(crate) fn toolbar(builder: ScreenBuilder, zoom: u16, typing: bool) -> ScreenBuilder {
    if typing {
        return builder;
    }
    builder.fill().band(
        kobo_sdk::BandAlign::Middle,
        [
            (
                kobo_sdk::SlotWidth::Fill,
                Box::new(move |slot: ScreenBuilder| {
                    slot.stepper(
                        format!("{zoom}%"),
                        "zoom-less",
                        Glyph::Minus,
                        "zoom-more",
                        Glyph::Plus,
                    )
                    .stepper_ends(zoom > 75, zoom < 200)
                    .stepper_track(((zoom.saturating_sub(75)) * 100 / 125) as u8)
                }) as Box<dyn FnOnce(ScreenBuilder) -> ScreenBuilder>,
            ),
            (
                kobo_sdk::SlotWidth::Fixed(180),
                Box::new(|slot: ScreenBuilder| slot.grid(1, false, [("browser-more", "More")]))
                    as Box<dyn FnOnce(ScreenBuilder) -> ScreenBuilder>,
            ),
        ],
    )
}

pub(crate) fn address_keys(builder: ScreenBuilder) -> ScreenBuilder {
    builder.fill().grid(
        4,
        false,
        [
            ("url-slash", "/"),
            ("url-dot", "."),
            ("url-com", ".com"),
            ("url-colon", ":"),
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
