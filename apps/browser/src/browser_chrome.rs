//! Shared bottom browser controls. Measurement and display use the same nodes.
use kobo_sdk::ScreenBuilder;

pub(crate) const SLIDER_BASE: kobo_sdk::ActionId = kobo_sdk::ActionId(0x7ffe_0000);

pub(crate) fn toolbar(builder: ScreenBuilder, _zoom: u16, _typing: bool) -> ScreenBuilder {
    builder.compact_browser_bar()
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

/// Fit the final browser frame, including picture-alone size guarantees.
pub(crate) fn page_fits(screen:&kobo_sdk::Screen,pieces:&[kobo_web_layout::Piece],metrics:&kobo_sdk::DisplayMetrics,alone:impl Fn(&kobo_web_layout::Piece)->kobo_sdk::Screen)->bool{
    if !kobo_web_layout::fits(screen,metrics){return false;}
    pieces.iter().all(|piece|{
        let kobo_web_layout::Piece::Picture{image,..}=piece else{return true;};
        let handle=kobo_web_layout::picture_handle(*image);
        let size=|screen:&kobo_sdk::Screen|kobo_web_layout::picture_box(screen,metrics,handle).map(|(_,h)|h);
        match (size(screen),size(&alone(piece))){(Some(drawn),Some(whole))=>drawn>=whole,_=>false}
    })
}
