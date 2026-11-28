use kobo_browser_core::address::{resolve, DEFAULT_SEARCH};
use kobo_web_document::Url;
#[test]
fn fuzzed_encoded_addresses_are_refused_or_round_trip() {
    for bytes in [
        include_bytes!("fixtures/resolve-url/crash-53ba51608a6e45acb1c20be5d1e248e204121dd8").as_slice(),
        include_bytes!("fixtures/resolve-url/crash-89b19ac7f4ef091d07e35fa6c15c310eeb11b7d5").as_slice(),
        include_bytes!("fixtures/resolve-url/crash-7048f3ccdff57a1c60140701b18b34731bca9cd5").as_slice(),
        include_bytes!("fixtures/resolve-url/crash-a71904333c38fd299118efcfe37fd76b97c09aac").as_slice(),
        include_bytes!("fixtures/resolve-url/crash-29b0882bc0d47c40d3cc7d84da03ecd745b49b56").as_slice(),
    ] {
        let typed = std::str::from_utf8(bytes).unwrap();
        if let Some(address) = resolve(typed, DEFAULT_SEARCH) {
            assert_eq!(Url::parse(&address.url().to_string()).as_ref(), Ok(address.url()));
        }
    }
}
#[test]
fn accepted_encoded_addresses_survive_history_reload_and_back() {
    use kobo_browser_core::{Go, History};
    for count in [1, 100, 1000, 1350, 1400] {
        let typed = format!("https://example.test/a{}z", "\0".repeat(count));
        let Some(address) = resolve(&typed, DEFAULT_SEARCH) else { continue };
        let url = address.url().clone();
        let mut history = History::new();
        history.navigate(url.clone());
        let reload = history.current().unwrap().url.to_string();
        assert_eq!(Url::parse(&reload), Ok(url.clone()));
        history.navigate(Url::parse("https://other.test/").unwrap());
        assert_eq!(history.back(), Go::Load(url.clone()));
        assert_eq!(Url::parse(&history.current().unwrap().url.to_string()), Ok(url));
    }
}
