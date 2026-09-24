use super::*;
use kobo_sdk::{AppRunner, Chrome, DiagnosticSeverity, DisplayMetrics};
use kobo_ui::TextScale;

fn runner(text_scale: TextScale) -> AppRunner<Browser> {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
    let mut runner = AppRunner::with_metrics(
        Browser::default(),
        DisplayMetrics {
            text_scale,
            ..kobo_ui::CLARA_BW_METRICS
        },
    );
    runner.start();
    runner
}

fn title(runner: &AppRunner<Browser>) -> String {
    runner
        .app()
        .loaded
        .as_ref()
        .map(|l| l.title.clone())
        .unwrap_or_default()
}

fn link_to(runner: &AppRunner<Browser>, words: &str) -> ActionId {
    let loaded = runner.app().loaded.as_ref().expect("loaded");
    let index = loaded
        .document
        .links
        .iter()
        .position(|link| link.text.contains(words))
        .expect("link");
    action_id(&link_action(index))
}

#[test]
fn starts_on_the_sample_index() {
    let runner = runner(TextScale::Default);
    assert_eq!(title(&runner), "Browse: sample pages");
    assert_eq!(runner.app().view, View::Page);
}

#[test]
fn follow_a_link_turn_pages_and_come_back() {
    let mut runner = runner(TextScale::Default);
    let article = link_to(&runner, "Notes on reading");
    runner.action(article);
    assert_eq!(title(&runner), "Notes on reading from paper screens");
    let pages = runner
        .app()
        .loaded
        .as_ref()
        .unwrap()
        .paginator
        .pages()
        .len();
    assert!(
        pages > 1,
        "the article should need more than one page, got {pages}"
    );
    runner.page_turn(true);
    assert_eq!(runner.app().loaded.as_ref().unwrap().page, 1);
    runner.action(action_id("back"));
    assert_eq!(title(&runner), "Browse: sample pages");
    runner.action(action_id("forward"));
    assert_eq!(title(&runner), "Notes on reading from paper screens");
    assert_eq!(
        runner.app().loaded.as_ref().unwrap().page,
        1,
        "forward restores the page"
    );
}

#[test]
fn a_fragment_link_opens_on_the_page_holding_its_section() {
    let mut runner = runner(TextScale::ExtraLarge);
    let jump = link_to(&runner, "Jump straight");
    runner.action(jump);
    let loaded = runner.app().loaded.as_ref().unwrap();
    let page = &loaded.paginator.pages()[loaded.page];
    assert!(loaded.page > 0);
    assert!(page.iter().any(
        |piece| matches!(piece, Piece::Heading { text, .. } if text == "Refresh and ghosting")
    ));
}

#[test]
fn the_links_list_follows_links_too() {
    let mut runner = runner(TextScale::Default);
    runner.action(action_id("links"));
    assert_eq!(runner.app().view, View::Links);
    let many = link_to(&runner, "many links");
    runner.action(many);
    assert_eq!(runner.app().view, View::Page);
    assert_eq!(title(&runner), "A page of many links");
}

#[test]
fn a_web_address_says_it_is_not_in_this_build_and_keeps_history() {
    let mut runner = runner(TextScale::Default);
    let web = link_to(&runner, "example.com");
    runner.action(web);
    assert!(matches!(runner.app().view, View::Unavailable(_)));
    assert_eq!(runner.app().history.entries().len(), 1);
    runner.action(action_id("return"));
    assert_eq!(runner.app().view, View::Page);
    assert_eq!(title(&runner), "Browse: sample pages");
}

#[test]
fn every_screen_fits_the_clara_at_every_text_size() {
    for scale in [TextScale::Default, TextScale::Large, TextScale::ExtraLarge] {
        let metrics = DisplayMetrics {
            text_scale: scale,
            ..kobo_ui::CLARA_BW_METRICS
        };
        let mut runner = runner(scale);
        let check = |runner: &AppRunner<Browser>, what: &str| {
            let issues: Vec<_> = runner
                .app()
                .screen()
                .build()
                .diagnostics(&metrics, &Chrome::measuring(true))
                .issues
                .into_iter()
                .filter(|issue| issue.severity == DiagnosticSeverity::Error)
                .collect();
            assert!(issues.is_empty(), "{what} at {scale:?}: {issues:?}");
        };
        check(&runner, "index");
        runner.action(action_id("links"));
        check(&runner, "links");
        runner.action(action_id("return"));
        let article = link_to(&runner, "Notes on reading");
        runner.action(article);
        let pages = runner
            .app()
            .loaded
            .as_ref()
            .unwrap()
            .paginator
            .pages()
            .len();
        for page in 0..pages {
            check(&runner, &format!("article page {page}"));
            runner.page_turn(true);
        }
        let web = runner
            .app()
            .loaded
            .as_ref()
            .unwrap()
            .url
            .join("https://example.com/")
            .unwrap();
        runner.app_mut().view = View::Unavailable(web);
        check(&runner, "unavailable");
    }
}
