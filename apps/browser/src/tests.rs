use super::*;
use kobo_sdk::{AppRunner, Chrome, DiagnosticSeverity};
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
    assert_eq!(runner.app().view, View::Links(0));
    let many = link_to(&runner, "many links");
    runner.action(many);
    assert_eq!(runner.app().view, View::Page);
    assert_eq!(title(&runner), "A page of many links");
}

fn pending_task(runner: &AppRunner<Browser>) -> TaskId {
    runner
        .app()
        .pending
        .as_ref()
        .expect("a fetch in flight")
        .task
}

fn page_title(runner: &AppRunner<Browser>) -> String {
    runner
        .app()
        .loaded
        .as_ref()
        .map(|l| l.url.to_string())
        .unwrap_or_default()
}

#[test]
fn a_web_link_is_fetched_and_recorded_once_it_arrives() {
    let mut runner = runner(TextScale::Default);
    let web = link_to(&runner, "example.com");
    runner.action(web);
    assert!(matches!(runner.app().view, View::Loading(_)));
    assert_eq!(runner.app().history.entries().len(), 1);
    let task = pending_task(&runner);
    runner.task_outcome(
        task,
        TaskOutcome::Completed(
            b"<!doctype html><title>Example Domain</title><h1>Example Domain</h1><p>For use in examples. <a href=\"/more\">More</a>".to_vec(),
        ),
    );
    assert_eq!(runner.app().view, View::Page);
    assert_eq!(title(&runner), "Example Domain");
    assert_eq!(page_title(&runner), "https://example.com/");
    assert_eq!(runner.app().history.entries().len(), 2);
    runner.action(action_id("back"));
    assert_eq!(title(&runner), "Browse: sample pages");
}

#[test]
fn a_failed_fetch_explains_offers_retry_and_leaves_history_alone() {
    let mut runner = runner(TextScale::Default);
    let web = link_to(&runner, "example.com");
    runner.action(web);
    let task = pending_task(&runner);
    runner.task_outcome(task, TaskOutcome::Failed(TaskError::TimedOut));
    assert!(matches!(
        runner.app().view,
        View::Failed(_, Failure::TimedOut)
    ));
    assert_eq!(runner.app().history.entries().len(), 1);
    runner.action(action_id("retry"));
    assert!(matches!(runner.app().view, View::Loading(_)));
    let task = pending_task(&runner);
    runner.task_outcome(task, TaskOutcome::Failed(TaskError::NotFound));
    assert!(matches!(
        runner.app().view,
        View::Failed(_, Failure::NotFound)
    ));
    runner.action(action_id("return"));
    assert_eq!(runner.app().view, View::Page);
    assert_eq!(title(&runner), "Browse: sample pages");
}

#[test]
fn a_page_that_fails_on_back_puts_history_where_it_was() {
    let mut runner = runner(TextScale::Default);
    let web = link_to(&runner, "example.com");
    runner.action(web);
    let task = pending_task(&runner);
    runner.task_outcome(task, TaskOutcome::Completed(b"<title>Web</title><p><a href=\"https://samples.browse.invalid/article.html\">article</a>".to_vec()));
    let article = link_to(&runner, "article");
    runner.action(article);
    assert_eq!(title(&runner), "Notes on reading from paper screens");
    runner.action(action_id("back"));
    assert!(matches!(runner.app().view, View::Loading(_)));
    let task = pending_task(&runner);
    runner.task_outcome(task, TaskOutcome::Failed(TaskError::Offline));
    assert!(matches!(
        runner.app().view,
        View::Failed(_, Failure::Offline)
    ));
    runner.action(action_id("return"));
    assert_eq!(title(&runner), "Notes on reading from paper screens");
    assert_eq!(
        runner
            .app()
            .history
            .current()
            .map(|entry| entry.url.to_string()),
        Some("https://samples.browse.invalid/article.html".into())
    );
    assert!(runner.app().history.can_go_back());
}

#[test]
fn cancelling_a_load_stays_on_the_page_and_ignores_a_late_answer() {
    let mut runner = runner(TextScale::Default);
    let web = link_to(&runner, "example.com");
    runner.action(web);
    let task = pending_task(&runner);
    runner.action(action_id("cancel-load"));
    assert_eq!(runner.app().view, View::Page);
    runner.task_outcome(
        task,
        TaskOutcome::Completed(b"<title>Late</title>".to_vec()),
    );
    assert_eq!(title(&runner), "Browse: sample pages");
    assert_eq!(runner.app().history.entries().len(), 1);
}

#[test]
fn a_file_that_is_not_a_page_is_named_and_text_is_shown_as_lines() {
    let mut runner = runner(TextScale::Default);
    let web = link_to(&runner, "example.com");
    runner.action(web);
    let task = pending_task(&runner);
    runner.task_outcome(task, TaskOutcome::Completed(b"%PDF-1.7 ...".to_vec()));
    assert!(matches!(runner.app().view, View::Unsupported(_, "a PDF")));
    assert_eq!(runner.app().history.entries().len(), 1);
    runner.action(action_id("return"));
    let web = link_to(&runner, "example.com");
    runner.action(web);
    let task = pending_task(&runner);
    runner.task_outcome(
        task,
        TaskOutcome::Completed(b"line one <b>\n    indented\n".to_vec()),
    );
    assert_eq!(runner.app().view, View::Page);
    let loaded = runner.app().loaded.as_ref().unwrap();
    assert!(matches!(
        loaded.document.blocks.first(),
        Some(kobo_web_document::Block::Preformatted(text)) if text.contains("line one <b>\n    indented")
    ));
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
                .screen(&metrics)
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
        runner.app_mut().view = View::Failed(web.clone(), Failure::Unreachable);
        check(&runner, "failed");
        runner.app_mut().view = View::Loading(web);
        check(&runner, "loading");
    }
}

#[test]
fn the_links_list_for_a_page_of_many_links_fits_and_offers_every_link() {
    for scale in [TextScale::Default, TextScale::ExtraLarge] {
        let metrics = DisplayMetrics {
            text_scale: scale,
            ..kobo_ui::CLARA_BW_METRICS
        };
        let mut runner = runner(scale);
        let many = link_to(&runner, "many links");
        runner.action(many);
        runner.action(action_id("links"));
        let loaded = runner.app().loaded.as_ref().unwrap();
        let pages = links_pages(loaded, &runner.app().page_links(), &metrics);
        assert!(
            pages.len() > 1,
            "{scale:?}: expected the list to need paging"
        );
        let mut offered: Vec<usize> = pages.iter().flatten().copied().collect();
        offered.sort_unstable();
        assert_eq!(
            offered,
            (0..loaded.document.links.len()).collect::<Vec<_>>()
        );
        for page in 0..pages.len() {
            assert_eq!(runner.app().view, View::Links(page));
            let issues: Vec<_> = runner
                .app()
                .screen(&metrics)
                .build()
                .diagnostics(&metrics, &Chrome::measuring(true))
                .issues
                .into_iter()
                .filter(|issue| issue.severity == DiagnosticSeverity::Error)
                .collect();
            assert!(issues.is_empty(), "{scale:?} links page {page}: {issues:?}");
            // The page button turns the list, not the document behind it.
            runner.page_turn(true);
        }
    }
}

#[test]
fn the_address_field_searches_for_words() {
    let mut runner = runner(TextScale::Default);
    runner.action(action_id("address"));
    assert!(runner.app().address.is_open());
    for key in [
        "kb.r0c2", "kb.space", "kb.r0c7", "kb.r2c5", "kb.r1c7", "kb.enter",
    ] {
        runner.action(action_id(key));
    }
    assert!(!runner.app().address.is_open());
    assert_eq!(
        runner.app().view,
        View::Loading(Url::parse("https://html.duckduckgo.com/html/?q=e+ink").expect("url"))
    );
}

#[test]
fn cancelling_the_address_field_returns_to_the_page() {
    let mut runner = runner(TextScale::Default);
    runner.action(action_id("address"));
    runner.action(action_id("kb.r0c0"));
    runner.action(action_id("kb.cancel"));
    assert!(!runner.app().address.is_open());
    assert_eq!(title(&runner), "Browse: sample pages");
    assert_eq!(runner.app().view, View::Page);
}

fn long_page() -> Vec<u8> {
    let mut html = String::from("<!doctype html><title>Long</title><h1>Long</h1>");
    for n in 0..120 {
        html.push_str(&format!(
            "<p>Paragraph {n}. Plain words that wrap over a few lines of the panel, \
             so that the whole page runs to a good many screens.</p>"
        ));
    }
    html.into_bytes()
}

fn pages_made(runner: &AppRunner<Browser>) -> usize {
    runner
        .app()
        .loaded
        .as_ref()
        .unwrap()
        .paginator
        .pages()
        .len()
}

#[test]
fn a_long_page_shows_at_once_and_counts_its_pages_in_the_background() {
    let mut runner = runner(TextScale::Default);
    runner.action(link_to(&runner, "example.com"));
    runner.task_outcome(pending_task(&runner), TaskOutcome::Completed(long_page()));
    assert_eq!(title(&runner), "Long");
    assert_eq!(
        pages_made(&runner),
        2,
        "only the first screens are made up front"
    );
    assert!(!runner.app().loaded.as_ref().unwrap().paginator.done());
    let screen = runner.app().screen(&runner.context().metrics()).build();
    assert!(
        format!("{screen:?}").contains("position: None"),
        "no page count until it is known"
    );

    // A turn past the pages made so far still lands.
    runner.page_turn(true);
    runner.page_turn(true);
    assert_eq!(runner.app().loaded.as_ref().unwrap().page, 2);

    let mut steps = 0;
    while let Some(nap) = runner.app().pager {
        runner.task_outcome(nap, TaskOutcome::Completed(Vec::new()));
        steps += 1;
        assert!(steps < 100, "background paging should end");
    }
    let loaded = runner.app().loaded.as_ref().unwrap();
    assert!(loaded.paginator.done());
    assert!(
        loaded.paginator.pages().len() > 4,
        "got {}",
        loaded.paginator.pages().len()
    );
    assert_eq!(loaded.page, 2, "background paging does not move the reader");
    let screen = runner.app().screen(&runner.context().metrics()).build();
    assert!(
        format!("{screen:?}").contains("position: Some"),
        "the count appears once every page exists"
    );
}

#[test]
fn leaving_a_long_page_stops_its_background_paging() {
    let mut runner = runner(TextScale::Default);
    runner.action(link_to(&runner, "example.com"));
    runner.task_outcome(pending_task(&runner), TaskOutcome::Completed(long_page()));
    let nap = runner.app().pager.expect("paging in the background");
    runner.action(action_id("back"));
    assert_eq!(title(&runner), "Browse: sample pages");
    assert_ne!(runner.app().pager, Some(nap));
    // A nap that was already on its way when the page changed does nothing.
    let made = pages_made(&runner);
    runner.task_outcome(nap, TaskOutcome::Completed(Vec::new()));
    assert_eq!(pages_made(&runner), made);
    assert_eq!(title(&runner), "Browse: sample pages");
}

fn naps(commands: &[kobo_sdk::Command]) -> Vec<TaskId> {
    commands
        .iter()
        .filter_map(|command| match command {
            kobo_sdk::Command::Spawn {
                task,
                work: Task::Sleep { .. },
            } => Some(*task),
            _ => None,
        })
        .collect()
}

#[test]
fn the_loading_screen_counts_the_wait_and_stops_when_the_page_arrives() {
    let mut runner = runner(TextScale::Default);
    let commands = runner.action(link_to(&runner, "example.com"));
    let tick = *naps(&commands).first().expect("the wait is being timed");
    let before = format!(
        "{:?}",
        runner.app().screen(&runner.context().metrics()).build()
    );
    assert!(!before.contains("so far"), "nothing to count yet");

    let commands = runner.task_outcome(tick, TaskOutcome::Completed(Vec::new()));
    let screen = format!(
        "{:?}",
        runner.app().screen(&runner.context().metrics()).build()
    );
    assert!(screen.contains("5 seconds so far"), "{screen}");
    let next = *naps(&commands).first().expect("the clock re-arms");

    runner.task_outcome(
        pending_task(&runner),
        TaskOutcome::Completed(b"<title>Here</title><p>Arrived.".to_vec()),
    );
    assert_eq!(title(&runner), "Here");
    assert!(!runner.app().clock.is_running());
    // A tick already on its way when the page landed changes nothing.
    runner.task_outcome(next, TaskOutcome::Completed(Vec::new()));
    assert_eq!(runner.app().view, View::Page);
    assert!(!runner.app().clock.is_running());
}
