use super::*;
use kobo_sdk::{AppRunner, Chrome, DiagnosticSeverity};
use kobo_ui::TextScale;
use std::fmt::Write;

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
    runner.action(action_id("samples"));
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

fn form_runner(scale: TextScale) -> AppRunner<Browser> {
    let mut runner = runner(scale);
    runner.app_mut().retry = Some(Url::parse("https://example.com/").unwrap());
    runner.action(action_id("retry"));
    runner.task_outcome(pending_task(&runner), TaskOutcome::Completed(br#"
        <title>Form test</title><main><h1>Form test</h1>
        <form action="/search" method="get"><label for=q>Search</label><input id=q type=search name=q value=ink>
        <select name=size><option value=s>Small</option><option value=l selected>Large</option></select>
        <label><input type=checkbox name=stock checked>In stock</label>
        <input type=hidden name=src value=reader><input type=submit name=go value=Find>
        </form></main>"#.to_vec()));
    runner
}

#[test]
fn form_controls_edit_and_keep_their_values_across_the_view() {
    let mut runner = form_runner(TextScale::Default);
    assert!(runner
        .app()
        .current_pieces()
        .iter()
        .any(|p| matches!(p, Piece::Form { index: 0, .. })));
    runner.action(action_id("form-0"));
    assert_eq!(runner.app().view, View::Form(0, 0));
    runner.action(action_id("field-0"));
    assert!(runner.app().form_entry.is_open());
    runner.action(action_id("kb.enter"));
    assert!(!runner.app().form_entry.is_open());
    runner.action(action_id("field-1"));
    assert_eq!(runner.app().view, View::Options(0, 1, 0));
    runner.action(action_id("option-0"));
    assert_eq!(runner.app().view, View::Form(0, 0));
    runner.action(action_id("field-2"));
    let loaded = runner.app().loaded.as_ref().unwrap();
    let form = forms::forms(&loaded.document.blocks)[0];
    assert!(matches!(&form.fields[0], Field::Text { value, .. } if value == "ink"));
    assert!(matches!(
        &form.fields[1],
        Field::Select {
            chosen: Some(0),
            ..
        }
    ));
    assert!(matches!(
        &form.fields[2],
        Field::Checkbox { checked: false, .. }
    ));
    runner.action(action_id("return"));
    assert_eq!(runner.app().view, View::Page);
}

#[test]
fn get_form_submits_current_values_and_waits_for_the_result() {
    let mut runner = form_runner(TextScale::Default);
    runner.action(action_id("form-0"));
    runner.action(action_id("field-1"));
    runner.action(action_id("option-0"));
    runner.action(action_id("field-2"));
    runner.action(action_id("submit-form"));
    assert_eq!(
        runner.app().view,
        View::Loading(
            Url::parse("https://example.com/search?q=ink&size=s&src=reader&go=Find").unwrap()
        )
    );
    assert_eq!(
        runner.app().history.entries().len(),
        2,
        "not recorded until a response arrives"
    );
}

#[test]
fn post_review_does_not_send_and_cancel_returns_to_form() {
    let mut runner = runner(TextScale::Default);
    runner.app_mut().retry = Some(Url::parse("https://example.com/").unwrap());
    runner.action(action_id("retry"));
    runner.task_outcome(pending_task(&runner), TaskOutcome::Completed(
        br#"<title>POST</title><form method=post action="/order"><input name=note value=hello><input type=submit name=go value=Send></form>"#.to_vec()));
    runner.action(action_id("form-0"));
    let history = runner.app().history.entries().len();
    runner.action(action_id("submit-form"));
    assert_eq!(runner.app().view, View::PostConfirm(0));
    assert!(runner.app().pending.is_none());
    assert_eq!(runner.app().history.entries().len(), history);
    runner.action(action_id("cancel-post"));
    assert_eq!(runner.app().view, View::Form(0, 0));
}

#[test]
fn form_control_screen_fits_on_three_profiles_and_sizes() {
    for profile in [
        kobo_ui::CLARA_BW_METRICS,
        DisplayMetrics {
            width: 1264,
            height: 1680,
            pixels_per_inch: 300,
            ..kobo_ui::CLARA_BW_METRICS
        },
        DisplayMetrics {
            width: 1404,
            height: 1872,
            pixels_per_inch: 227,
            ..kobo_ui::CLARA_BW_METRICS
        },
    ] {
        for scale in [TextScale::Default, TextScale::Large, TextScale::ExtraLarge] {
            let runner = form_runner(scale);
            let metrics = DisplayMetrics {
                text_scale: scale,
                ..profile
            };
            let loaded = runner.app().loaded.as_ref().unwrap();
            let screen = forms::form_screen(loaded, 0, 0, &metrics).build();
            let issues: Vec<_> = screen
                .diagnostics(&metrics, &Chrome::measuring(true))
                .issues
                .into_iter()
                .filter(|issue| issue.severity == DiagnosticSeverity::Error)
                .collect();
            assert!(issues.is_empty(), "{} {scale:?}: {issues:?}", metrics.width);
        }
    }
}

#[test]
fn samples_action_opens_the_sample_index() {
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
    while !runner.app().page_links().contains(&1)
        && runner.app().loaded.as_ref().unwrap().page + 1
            < runner
                .app()
                .loaded
                .as_ref()
                .unwrap()
                .paginator
                .pages()
                .len()
    {
        runner.page_turn(true);
    }
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

#[test]
fn sections_jump_to_headings_and_return_to_page() {
    for scale in [TextScale::Default, TextScale::ExtraLarge] {
        let mut runner = runner(scale);
        let article = link_to(&runner, "Notes on reading");
        runner.action(article);
        let headings = headings(runner.app().loaded.as_ref().unwrap());
        assert!(headings.len() >= 2);
        runner.action(action_id("sections"));
        assert_eq!(runner.app().view, View::Sections(0));
        let (place, _, label) = headings.last().unwrap();
        runner.action(action_id(&section_action(*place)));
        assert_eq!(runner.app().view, View::Page);
        let loaded = runner.app().loaded.as_ref().unwrap();
        assert!(loaded.paginator.pages()[loaded.page]
            .iter()
            .any(|piece| matches!(piece, Piece::Heading { text, .. } if text == label)));
        runner.action(action_id("sections"));
        runner.action(action_id("back"));
        assert_eq!(runner.app().view, View::Page);
        assert_eq!(title(&runner), "Notes on reading from paper screens");
    }
}

#[test]
fn a_long_sections_list_fits_and_pages_on_a_small_screen() {
    let mut runner = runner(TextScale::ExtraLarge);
    runner.app_mut().retry = Some(Url::parse("https://example.com/").unwrap());
    runner.action(action_id("retry"));
    let mut html = "<title>Many headings</title>".to_owned();
    for i in 0..80 {
        write!(html, "<h2>Part {i}</h2><p>A sentence for part {i}.</p>").unwrap();
    }
    runner.task_outcome(
        pending_task(&runner),
        TaskOutcome::Completed(html.into_bytes()),
    );
    let headings = headings(runner.app().loaded.as_ref().unwrap());
    let pages = sections_pages(&headings, &runner.context().metrics());
    assert!(pages.len() > 1);
    assert_eq!(
        pages.iter().flatten().cloned().collect::<Vec<_>>(),
        headings
    );
    runner.action(action_id("sections"));
    for page in 0..pages.len() {
        assert_eq!(runner.app().view, View::Sections(page));
        let issues: Vec<_> = runner
            .app()
            .screen(&runner.context().metrics())
            .build()
            .diagnostics(&runner.context().metrics(), &Chrome::measuring(true))
            .issues
            .into_iter()
            .filter(|issue| issue.severity == DiagnosticSeverity::Error)
            .collect();
        assert!(issues.is_empty(), "sections page {page}: {issues:?}");
        runner.page_turn(true);
    }
}

#[test]
fn links_list_does_not_offer_links_from_other_pages() {
    let mut runner = runner(TextScale::ExtraLarge);
    runner.app_mut().retry = Some(Url::parse("https://example.com/").unwrap());
    runner.action(action_id("retry"));
    let mut html = "<title>Long links</title>".to_owned();
    for i in 0..120 {
        write!(html, r#"<p><a href="/item/{i}">Item {i}</a> A sentence around the link, with plenty of room for words.</p>"#).unwrap();
    }
    runner.task_outcome(
        pending_task(&runner),
        TaskOutcome::Completed(html.into_bytes()),
    );
    let total = runner.app().loaded.as_ref().unwrap().document.links.len();
    let here = runner.app().page_links();
    assert!(!here.is_empty());
    assert!(here.len() < total);
    runner.action(action_id("links"));
    let loaded = runner.app().loaded.as_ref().unwrap();
    let pages = links_pages(loaded, &here, &runner.context().metrics());
    assert_eq!(pages.iter().flatten().copied().collect::<Vec<_>>(), here);
    let absent = (0..total).find(|index| !here.contains(index)).unwrap();
    assert!(runner
        .app()
        .link_named(action_id(&link_action(absent)))
        .is_none());
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
fn the_links_list_for_a_page_of_many_links_fits_and_offers_this_page() {
    for scale in [TextScale::Default, TextScale::ExtraLarge] {
        let metrics = DisplayMetrics {
            text_scale: scale,
            ..kobo_ui::CLARA_BW_METRICS
        };
        let mut runner = runner(scale);
        let many = link_to(&runner, "many links");
        while !runner.app().page_links().contains(&2)
            && runner.app().loaded.as_ref().unwrap().page + 1
                < runner
                    .app()
                    .loaded
                    .as_ref()
                    .unwrap()
                    .paginator
                    .pages()
                    .len()
        {
            runner.page_turn(true);
        }
        runner.action(many);
        runner.action(action_id("links"));
        let loaded = runner.app().loaded.as_ref().unwrap();
        let pages = links_pages(loaded, &runner.app().page_links(), &metrics);
        assert!(
            pages.len() > 1,
            "{scale:?}: expected the page links to need paging"
        );
        let offered: Vec<usize> = pages.iter().flatten().copied().collect();
        assert_eq!(offered, runner.app().page_links());
        assert!(offered
            .iter()
            .all(|&index| index < loaded.document.links.len()));
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
    runner.app_mut().retry = Some(Url::parse("https://example.com/").unwrap());
    runner.action(action_id("retry"));
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
    runner.app_mut().retry = Some(Url::parse("https://example.com/").unwrap());
    runner.action(action_id("retry"));
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

fn fetches(commands: &[kobo_sdk::Command]) -> Vec<(TaskId, String, Vec<Header>)> {
    commands
        .iter()
        .filter_map(|command| match command {
            kobo_sdk::Command::Spawn {
                task,
                work: Task::Fetch { url, headers, .. },
            } => Some((*task, url.clone(), headers.clone())),
            _ => None,
        })
        .collect()
}

fn put_pictures(commands: &[kobo_sdk::Command]) -> Vec<(u32, u32, u32)> {
    commands
        .iter()
        .filter_map(|command| match command {
            kobo_sdk::Command::PutPicture {
                handle,
                width,
                height,
                ..
            } => Some((handle.0, *width, *height)),
            _ => None,
        })
        .collect()
}

fn a_png(width: u32, height: u32) -> Vec<u8> {
    let grey: Vec<u8> = (0..width * height).map(|n| (n % 251) as u8).collect();
    kobo_image::encode_png_grey(width, height, &grey).expect("png")
}

/// Opens a web page with `body` and returns the commands its arrival made.
fn open_web_page(runner: &mut AppRunner<Browser>, body: &str) -> Vec<kobo_sdk::Command> {
    runner.app_mut().retry = Some(Url::parse("https://example.com/").unwrap());
    runner.action(action_id("retry"));
    runner.task_outcome(
        pending_task(runner),
        TaskOutcome::Completed(body.as_bytes().to_vec()),
    )
}

#[test]
fn a_sized_picture_on_the_screen_is_fetched_fitted_and_shown() {
    let mut runner = runner(TextScale::Default);
    let commands = open_web_page(
        &mut runner,
        "<title>Plate</title><p>Above.</p><img src=/plate.png width=400 height=300 alt=\"A plate\"><p>Below.</p>",
    );
    let asked = fetches(&commands);
    assert_eq!(asked.len(), 1, "{asked:?}");
    let (task, url, headers) = &asked[0];
    assert_eq!(url, "https://example.com/plate.png");
    assert!(headers.iter().any(|h| h.value.starts_with("image/jpeg")));

    let commands = runner.task_outcome(*task, TaskOutcome::Completed(a_png(200, 150)));
    let put = put_pictures(&commands);
    assert_eq!(put.len(), 1, "one picture sent");
    let (handle, width, height) = put[0];
    assert_eq!(handle, 1);
    // Fitted to its room, which keeps the declared 4:3 shape.
    assert!(
        width > 200 && (width * 3).abs_diff(height * 4) <= 4,
        "{width}x{height}"
    );
    assert_eq!(runner.app().pictures.state(0), Some(pictures::State::Shown));
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, kobo_sdk::Command::SetScreen(_))),
        "repainted with the picture"
    );
}

#[test]
fn a_picture_that_will_not_decode_leaves_its_description() {
    let mut runner = runner(TextScale::Default);
    let commands = open_web_page(
        &mut runner,
        "<title>Plate</title><img src=/plate.png width=400 height=300 alt=\"A plate\">",
    );
    let (task, ..) = fetches(&commands)[0].clone();
    let commands = runner.task_outcome(task, TaskOutcome::Completed(b"not a picture".to_vec()));
    assert!(put_pictures(&commands).is_empty());
    assert_eq!(
        runner.app().pictures.state(0),
        Some(pictures::State::Failed)
    );
    let screen = format!(
        "{:?}",
        runner.app().screen(&runner.context().metrics()).build()
    );
    assert!(screen.contains("Image: A plate"));
}

#[test]
fn pictures_further_on_wait_until_their_screen_is_read() {
    let mut runner = runner(TextScale::Default);
    let mut body = String::from("<title>Plates</title>");
    for n in 0..8 {
        body.push_str(&format!(
            "<p>Plate {n} follows.</p><img src=/p{n}.jpg width=800 height=600 alt=\"Plate {n}\">"
        ));
    }
    let commands = open_web_page(&mut runner, &body);
    let first = fetches(&commands).len();
    assert!(
        (1..8).contains(&first),
        "only this screen's pictures: {first}"
    );
    let commands = runner.page_turn(true);
    assert!(!fetches(&commands).is_empty(), "the next screen's pictures");
}

#[test]
fn leaving_a_page_cancels_and_releases_its_pictures() {
    let mut runner = runner(TextScale::Default);
    let commands = open_web_page(
        &mut runner,
        "<title>Two</title><img src=/a.png width=400 height=300><img src=/b.svg width=400 height=300>",
    );
    let asked = fetches(&commands);
    assert_eq!(asked.len(), 1, "the SVG is not fetched: {asked:?}");
    runner.task_outcome(asked[0].0, TaskOutcome::Completed(a_png(100, 75)));
    runner.action(action_id("back"));
    assert_eq!(runner.app().pictures.state(0), None);
}

fn len32(blob: &[u8]) -> u32 {
    u32::try_from(blob.len()).expect("a test blob is small")
}

/// The reader's store and shelf, kept in memory and answered in order.
#[derive(Default)]
struct Store {
    keys: std::collections::BTreeMap<String, Vec<u8>>,
    shelf: std::collections::BTreeMap<String, Vec<u8>>,
    /// Refuse shelf writes, as a full reader does.
    full: bool,
}

impl Store {
    fn answer(&mut self, request: kobo_sdk::StoreRequest) -> StoreResult {
        use kobo_sdk::{StoreError, StoreRequest as R};
        match request {
            R::Save { key, value } => {
                self.keys.insert(key.clone(), value);
                StoreResult::Saved { key }
            }
            R::Load { key } => StoreResult::Loaded {
                value: self.keys.get(&key).cloned(),
                key,
            },
            R::Forget { key } => {
                self.keys.remove(&key);
                StoreResult::Forgotten { key }
            }
            R::List => StoreResult::Keys(self.keys.keys().cloned().collect()),
            R::ShelfWrite { .. } if self.full => StoreResult::Denied(StoreError::NoRoom),
            R::ShelfWrite {
                name,
                offset,
                bytes,
                ..
            } => {
                let blob = self.shelf.entry(name.clone()).or_default();
                blob.truncate(offset as usize);
                blob.extend_from_slice(&bytes);
                StoreResult::ShelfWritten {
                    size: len32(blob),
                    name,
                }
            }
            R::ShelfRead {
                name,
                offset,
                length,
            } => match self.shelf.get(&name) {
                Some(blob) => {
                    let from = (offset as usize).min(blob.len());
                    let to = (from + length as usize).min(blob.len());
                    StoreResult::ShelfRead {
                        offset,
                        bytes: blob[from..to].to_vec(),
                        size: len32(blob),
                        name,
                    }
                }
                None => StoreResult::Denied(StoreError::Missing),
            },
            R::ShelfRemove { name } => {
                self.shelf.remove(&name);
                StoreResult::ShelfRemoved { name }
            }
            R::ShelfList => StoreResult::Shelf(
                self.shelf
                    .iter()
                    .map(|(name, blob)| (name.clone(), len32(blob)))
                    .collect(),
            ),
        }
    }

    /// Answers every store request in `commands`, and those the answers
    /// lead to, and returns the other commands.
    fn pump(
        &mut self,
        runner: &mut AppRunner<Browser>,
        commands: Vec<kobo_sdk::Command>,
    ) -> Vec<kobo_sdk::Command> {
        let mut queue: std::collections::VecDeque<_> = commands.into();
        let mut rest = Vec::new();
        while let Some(command) = queue.pop_front() {
            match command {
                kobo_sdk::Command::Store(request) => {
                    let answer = self.answer(request);
                    queue.extend(runner.store_result(answer));
                }
                other => rest.push(other),
            }
        }
        rest
    }
}

fn runner_with(store: &mut Store) -> AppRunner<Browser> {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
    let mut runner = AppRunner::with_metrics(Browser::default(), kobo_ui::CLARA_BW_METRICS);
    let started = runner.start();
    store.pump(&mut runner, started);
    let samples = runner.action(action_id("samples"));
    store.pump(&mut runner, samples);
    runner
}

const KEPT: &str =
    "<!doctype html><title>Kept page</title><h1>Kept page</h1><p>Words worth reading twice.";

fn first_piece(runner: &AppRunner<Browser>) -> Piece {
    runner.app().current_pieces()[0].clone()
}

#[test]
fn a_page_that_arrives_is_kept_and_read_back_when_offline() {
    let mut store = Store::default();
    let mut runner = runner_with(&mut store);
    let arrived = open_web_page(&mut runner, KEPT);
    store.pump(&mut runner, arrived);
    let index = runner.app().saved.index().expect("index read").clone();
    let entry = index.find("https://example.com/").expect("kept");
    assert_eq!(
        store.shelf.get(&entry.name).map(Vec::as_slice),
        Some(KEPT.as_bytes())
    );
    assert!(store.keys.contains_key(saved::INDEX_KEY));

    runner.action(action_id("back"));
    assert_eq!(title(&runner), "Browse: sample pages");
    runner.app_mut().retry = Some(Url::parse("https://example.com/").unwrap());
    runner.action(action_id("retry"));
    let failed = runner.task_outcome(
        pending_task(&runner),
        TaskOutcome::Failed(TaskError::Offline),
    );
    // The copy is on its way off the shelf: still loading, not an error.
    assert!(matches!(runner.app().view, View::Loading(_)));
    store.pump(&mut runner, failed);
    assert_eq!(runner.app().view, View::Page);
    assert_eq!(title(&runner), "Saved: Kept page");
    let Piece::Note(note) = first_piece(&runner) else {
        panic!("the page opens with a note");
    };
    assert!(note.starts_with("Saved copy from today at "), "{note}");
    assert!(
        note.contains(": no Wi-Fi, so it may not be the latest."),
        "{note}"
    );
    assert_eq!(runner.app().history.entries().len(), 2);
    runner.action(action_id("back"));
    assert_eq!(title(&runner), "Browse: sample pages");
}

#[test]
fn a_page_never_kept_or_a_slow_site_still_shows_the_error() {
    let mut store = Store::default();
    let mut runner = runner_with(&mut store);
    runner.app_mut().retry = Some(Url::parse("https://example.com/").unwrap());
    runner.action(action_id("retry"));
    let failed = runner.task_outcome(
        pending_task(&runner),
        TaskOutcome::Failed(TaskError::Offline),
    );
    store.pump(&mut runner, failed);
    assert!(matches!(
        runner.app().view,
        View::Failed(_, Failure::Offline)
    ));

    runner.action(action_id("return"));
    let arrived = open_web_page(&mut runner, KEPT);
    store.pump(&mut runner, arrived);
    runner.action(action_id("back"));
    runner.app_mut().retry = Some(Url::parse("https://example.com/").unwrap());
    runner.action(action_id("retry"));
    // A site that answers slowly is there; an old copy would hide that.
    let failed = runner.task_outcome(
        pending_task(&runner),
        TaskOutcome::Failed(TaskError::TimedOut),
    );
    store.pump(&mut runner, failed);
    assert!(matches!(
        runner.app().view,
        View::Failed(_, Failure::TimedOut)
    ));
}

#[test]
fn a_kept_copy_that_cannot_be_read_is_forgotten_and_the_error_shown() {
    let mut store = Store::default();
    let mut runner = runner_with(&mut store);
    let arrived = open_web_page(&mut runner, KEPT);
    store.pump(&mut runner, arrived);
    store.shelf.clear();
    let back = runner.action(action_id("back"));
    store.pump(&mut runner, back);
    let link = runner.action(link_to(&runner, "example.com"));
    store.pump(&mut runner, link);
    let failed = runner.task_outcome(
        pending_task(&runner),
        TaskOutcome::Failed(TaskError::Offline),
    );
    store.pump(&mut runner, failed);
    assert!(matches!(
        runner.app().view,
        View::Failed(_, Failure::Offline)
    ));
    let index = runner.app().saved.index().expect("index");
    assert!(index.find("https://example.com/").is_none());
}

#[test]
fn a_full_reader_keeps_nothing_and_says_nothing() {
    let mut store = Store {
        full: true,
        ..Store::default()
    };
    let mut runner = runner_with(&mut store);
    let arrived = open_web_page(&mut runner, KEPT);
    store.pump(&mut runner, arrived);
    assert_eq!(runner.app().view, View::Page);
    assert_eq!(title(&runner), "Kept page");
    assert!(store.shelf.is_empty());
    let index = runner.app().saved.index().expect("index");
    assert!(index.find("https://example.com/").is_none());
}

#[test]
fn kept_pages_outlive_the_app_and_strays_are_cleared() {
    let mut store = Store::default();
    let mut runner = runner_with(&mut store);
    let arrived = open_web_page(&mut runner, KEPT);
    store.pump(&mut runner, arrived);
    store
        .shelf
        .insert("page-stray".into(), b"left behind".to_vec());
    store
        .shelf
        .insert("other-app-blob".into(), b"not ours".to_vec());

    let mut runner = runner_with(&mut store);
    assert!(!store.shelf.contains_key("page-stray"));
    assert!(store.shelf.contains_key("other-app-blob"));
    runner.app_mut().retry = Some(Url::parse("https://example.com/").unwrap());
    runner.action(action_id("retry"));
    let failed = runner.task_outcome(
        pending_task(&runner),
        TaskOutcome::Failed(TaskError::Unreachable),
    );
    store.pump(&mut runner, failed);
    assert_eq!(title(&runner), "Saved: Kept page");
}

const PLATE: &str = "<title>Plate</title><p>Above.</p><img src=/plate.png width=400 height=300 alt=\"A plate\"><p>Below.</p>";

#[test]
fn a_shown_picture_is_kept_and_comes_back_with_its_page_offline() {
    let mut store = Store::default();
    let mut runner = runner_with(&mut store);
    let arrived = open_web_page(&mut runner, PLATE);
    let rest = store.pump(&mut runner, arrived);
    let asked = fetches(&rest);
    assert_eq!(asked.len(), 1);
    let shown = runner.task_outcome(asked[0].0, TaskOutcome::Completed(a_png(200, 150)));
    let rest = store.pump(&mut runner, shown);
    let first = put_pictures(&rest);
    assert_eq!(first.len(), 1);
    let kept = runner.app().saved.index().expect("index").clone();
    let key = kept
        .entries()
        .iter()
        .map(|entry| entry.url.clone())
        .find(|url| {
            url.starts_with("picture:grey:") && url.ends_with("https://example.com/plate.png")
        })
        .expect("the fitted picture is kept");
    assert!(
        store
            .shelf
            .values()
            .any(|blob| pictures::unpack(blob).is_some()),
        "{key}"
    );

    runner.action(action_id("back"));
    runner.app_mut().retry = Some(Url::parse("https://example.com/").unwrap());
    runner.action(action_id("retry"));
    let failed = runner.task_outcome(
        pending_task(&runner),
        TaskOutcome::Failed(TaskError::Offline),
    );
    let rest = store.pump(&mut runner, failed);
    assert_eq!(title(&runner), "Saved: Plate");
    assert!(
        fetches(&rest).is_empty(),
        "nothing asked of the network: {:?} {:?}",
        fetches(&rest),
        runner.app().saved.index().map(|i| i
            .entries()
            .iter()
            .map(|e| e.url.clone())
            .collect::<Vec<_>>())
    );
    assert_eq!(
        put_pictures(&rest),
        first,
        "the same picture, from the shelf"
    );
    assert_eq!(runner.app().pictures.state(0), Some(pictures::State::Shown));
}

#[test]
fn a_kept_picture_that_is_torn_is_forgotten_and_fetched_again() {
    let mut store = Store::default();
    let mut runner = runner_with(&mut store);
    let arrived = open_web_page(&mut runner, PLATE);
    let rest = store.pump(&mut runner, arrived);
    let shown = runner.task_outcome(fetches(&rest)[0].0, TaskOutcome::Completed(a_png(200, 150)));
    store.pump(&mut runner, shown);
    for blob in store.shelf.values_mut() {
        if pictures::unpack(blob).is_some() {
            blob.truncate(blob.len() / 2);
        }
    }
    runner.action(action_id("back"));
    let arrived = open_web_page(&mut runner, PLATE);
    let rest = store.pump(&mut runner, arrived);
    let asked = fetches(&rest);
    assert_eq!(asked.len(), 1, "fetched from the site again: {asked:?}");
    assert_eq!(asked[0].1, "https://example.com/plate.png");
    let kept = runner.app().saved.index().expect("index");
    assert!(!kept
        .entries()
        .iter()
        .any(|entry| entry.url.starts_with("picture:")));
}

#[test]
fn a_packed_picture_reads_back_and_a_short_one_is_refused() {
    let grey = pictures::Prepared {
        width: 3,
        height: 2,
        colour: false,
        pixels: vec![0, 1, 2, 3, 4, 5],
    };
    let packed = pictures::pack(&grey);
    let back = pictures::unpack(&packed).expect("reads back");
    assert_eq!(
        (back.width, back.height, back.colour, back.pixels),
        (3, 2, false, &grey.pixels[..])
    );
    assert!(pictures::unpack(&packed[..packed.len() - 1]).is_none());
    let rgb = pictures::Prepared {
        width: 2,
        height: 1,
        colour: true,
        pixels: vec![255, 0, 0, 0, 0, 255],
    };
    let back = pictures::pack(&rgb);
    assert_eq!(pictures::unpack(&back).map(|p| p.colour), Some(true));
    // Grey-sized pixels under a colour tag are a torn write.
    let mut mislabelled = pictures::pack(&grey);
    mislabelled[..4].copy_from_slice(b"KRG1");
    assert!(pictures::unpack(&mislabelled).is_none());
    assert!(pictures::unpack(b"KGR1").is_none());
    assert!(pictures::unpack(b"<html>").is_none());
}

/// The first paragraph that begins on the screen being read. A paragraph
/// carried over from the screen before does not count: it is cut where the
/// pages happen to fall.
fn first_words(runner: &AppRunner<Browser>) -> String {
    runner
        .app()
        .current_pieces()
        .iter()
        .find_map(|piece| match piece {
            Piece::Prose(runs) => {
                let text: String = runs.iter().map(|run| run.text.as_str()).collect();
                text.starts_with("Paragraph").then_some(text)
            }
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn a_saved_copy_reopens_at_the_same_words_though_its_note_moves_the_pages() {
    let mut store = Store::default();
    let mut runner = runner_with(&mut store);
    let body = String::from_utf8(long_page()).expect("utf-8");
    let arrived = open_web_page(&mut runner, &body);
    store.pump(&mut runner, arrived);
    for _ in 0..5 {
        runner.action(action_id("next-page"));
    }
    let page = runner.app().loaded.as_ref().expect("loaded").page;
    assert_eq!(page, 5);
    let words = first_words(&runner);
    assert!(words.starts_with("Paragraph"), "{words}");

    runner.action(action_id("back"));
    runner.action(action_id("forward"));
    let failed = runner.task_outcome(
        pending_task(&runner),
        TaskOutcome::Failed(TaskError::Offline),
    );
    store.pump(&mut runner, failed);
    assert_eq!(title(&runner), "Saved: Long");
    assert_eq!(first_words(&runner), words, "the same place in the text");
}

#[test]
fn form_lists_page_and_only_visible_controls_can_change() {
    let mut runner = runner(TextScale::ExtraLarge);
    runner.app_mut().retry = Some(Url::parse("https://example.com/").unwrap());
    runner.action(action_id("retry"));
    let mut html = "<title>Long form</title><form method=get action=/find>".to_owned();
    for index in 0..35 {
        write!(
            html,
            "<label>Field {index}<input name=f{index} value={index}></label>"
        )
        .unwrap();
    }
    html.push_str("</form>");
    runner.task_outcome(
        pending_task(&runner),
        TaskOutcome::Completed(html.into_bytes()),
    );
    runner.action(action_id("form-0"));
    let loaded = runner.app().loaded.as_ref().unwrap();
    let pages = forms::form_pages(loaded, 0, &runner.context().metrics());
    assert!(pages.len() > 1);
    assert!(pages[0].len() < 35);
    let later = pages[1][0];
    runner.action(action_id(&format!("field-{later}")));
    assert!(
        !runner.app().form_entry.is_open(),
        "off-page control ignored"
    );
    runner.page_turn(true);
    assert_eq!(runner.app().view, View::Form(0, 1));
    runner.action(action_id(&format!("field-{later}")));
    assert!(runner.app().form_entry.is_open());
}

#[test]
fn choices_only_accept_the_current_options_page() {
    let mut runner = runner(TextScale::ExtraLarge);
    runner.app_mut().retry = Some(Url::parse("https://example.com/").unwrap());
    runner.action(action_id("retry"));
    let mut html = "<title>Long choice</title><form><select name=item>".to_owned();
    for index in 0..30 {
        write!(html, "<option value={index}>Choice number {index}</option>").unwrap();
    }
    html.push_str("</select></form>");
    runner.task_outcome(
        pending_task(&runner),
        TaskOutcome::Completed(html.into_bytes()),
    );
    runner.action(action_id("form-0"));
    runner.action(action_id("field-0"));
    let loaded = runner.app().loaded.as_ref().unwrap();
    let form = forms::forms(&loaded.document.blocks)[0];
    let pages = forms::option_pages(form, 0, &runner.context().metrics());
    assert!(pages.len() > 1);
    let later = pages[1][0];
    runner.action(action_id(&format!("option-{later}")));
    assert_eq!(runner.app().view, View::Options(0, 0, 0));
    runner.page_turn(true);
    runner.action(action_id(&format!("option-{later}")));
    assert_eq!(runner.app().view, View::Form(0, 0));
    let form = forms::forms(&runner.app().loaded.as_ref().unwrap().document.blocks)[0];
    assert!(matches!(&form.fields[0], Field::Select { chosen: Some(i), .. } if *i == later));
}

#[test]
fn bookmarks_and_recent_pages_survive_restart_and_open() {
    let mut store = Store::default();
    let mut runner = runner_with(&mut store);
    let article = link_to(&runner, "Notes on reading");
    let commands = runner.action(article);
    store.pump(&mut runner, commands);
    assert!(title(&runner).contains("Notes on reading"));
    runner.action(action_id("navigate"));
    assert_eq!(runner.app().view, View::Navigate);
    let commands = runner.action(action_id("mark"));
    store.pump(&mut runner, commands);
    assert_eq!(runner.app().library.bookmarks.len(), 1);
    runner.action(action_id("bookmarks"));
    assert_eq!(runner.app().view, View::Bookmarks(0));
    assert!(kobo_web_layout::fits(
        &runner.app().screen(&kobo_ui::CLARA_BW_METRICS).build(),
        &kobo_ui::CLARA_BW_METRICS
    ));
    runner.action(action_id("return"));
    assert_eq!(runner.app().view, View::Page);
    let mut runner = runner_with(&mut store);
    assert_eq!(runner.app().library.bookmarks.len(), 1);
    assert_eq!(runner.app().library.recent.len(), 2);
    runner.action(action_id("navigate"));
    runner.action(action_id("bookmarks"));
    let commands = runner.action(action_id("place-0"));
    store.pump(&mut runner, commands);
    assert!(title(&runner).contains("Notes on reading"));
    runner.action(action_id("navigate"));
    runner.action(action_id("recent"));
    assert_eq!(runner.app().view, View::Recent(0));
    assert_eq!(runner.app().library.recent[0].title, title(&runner));
    let commands = runner.action(action_id("place-1"));
    store.pump(&mut runner, commands);
    assert_eq!(title(&runner), "Browse: sample pages");
}

#[test]
fn library_lists_fit_all_profiles_and_text_sizes() {
    for profile in [
        kobo_ui::CLARA_BW_METRICS,
        DisplayMetrics {
            width: 1264,
            height: 1680,
            pixels_per_inch: 300,
            ..kobo_ui::CLARA_BW_METRICS
        },
        DisplayMetrics {
            width: 1404,
            height: 1872,
            pixels_per_inch: 227,
            ..kobo_ui::CLARA_BW_METRICS
        },
    ] {
        for scale in [TextScale::Default, TextScale::Large, TextScale::ExtraLarge] {
            let metrics = DisplayMetrics {
                text_scale: scale,
                ..profile
            };
            let mut browser = Browser::default();
            for i in 0..100 {
                let url = Url::parse(&format!("https://example.com/article/{i}")).unwrap();
                browser
                    .library
                    .visit(url.clone(), &format!("A fairly long article title {i}"));
                browser
                    .library
                    .toggle(url, &format!("A fairly long article title {i}"));
            }
            assert!(
                kobo_web_layout::fits(&browser.navigate_screen().build(), &metrics),
                "navigate menu {scale:?}"
            );
            for recent in [false, true] {
                let places = if recent {
                    &browser.library.recent
                } else {
                    &browser.library.bookmarks
                };
                let pages = fit_pages(places, &metrics, |entries| {
                    place_screen(entries, places.len(), 998, 999, recent)
                });
                assert!(pages.len() > 1);
                for (page, _) in pages.iter().enumerate() {
                    let screen = browser.library_screen(recent, page, &metrics).build();
                    assert!(
                        kobo_web_layout::fits(&screen, &metrics),
                        "{recent} {page} {scale:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn failed_pages_are_not_added_and_saved_copy_titles_stay_clean() {
    let mut store = Store::default();
    let mut runner = runner_with(&mut store);
    let before = runner.app().library.recent.len();
    runner.app_mut().retry = Some(Url::parse("https://example.com/").unwrap());
    runner.action(action_id("retry"));
    let failed = runner.task_outcome(
        pending_task(&runner),
        TaskOutcome::Failed(TaskError::Offline),
    );
    store.pump(&mut runner, failed);
    assert_eq!(runner.app().library.recent.len(), before);
    let arrived = open_web_page(&mut runner, KEPT);
    store.pump(&mut runner, arrived);
    runner.action(action_id("back"));
    runner.app_mut().retry = Some(Url::parse("https://example.com/").unwrap());
    runner.action(action_id("retry"));
    let failed = runner.task_outcome(
        pending_task(&runner),
        TaskOutcome::Failed(TaskError::Offline),
    );
    store.pump(&mut runner, failed);
    assert!(title(&runner).starts_with("Saved: "));
    assert_eq!(runner.app().library.recent[0].title, "Kept page");
}

#[test]
fn library_page_turn_and_selection_only_touch_visible_rows() {
    let mut browser = Browser::default();
    let metrics = DisplayMetrics {
        text_scale: TextScale::ExtraLarge,
        ..kobo_ui::CLARA_BW_METRICS
    };
    for i in 0..100 {
        let url = Url::parse(&format!("https://example.com/{i}")).unwrap();
        browser.library.toggle(url, &format!("Article {i}"));
    }
    browser.view = View::Bookmarks(0);
    let mut runner = AppRunner::with_metrics(browser, metrics);
    let pages = fit_pages(
        &runner.app().library.bookmarks,
        &runner.context().metrics(),
        |entries| place_screen(entries, 100, 998, 999, false),
    );
    assert!(pages.len() > 1);
    let expected = pages[1][0].url.clone();
    runner.action(action_id("places-next"));
    assert_eq!(runner.app().view, View::Bookmarks(1));
    runner.action(action_id("place-0"));
    assert_eq!(runner.app().pending.as_ref().unwrap().url, expected);
}

#[test]
fn links_are_reachable_through_navigate_on_every_profile_and_text_size() {
    for profile in [
        kobo_ui::CLARA_BW_METRICS,
        DisplayMetrics {
            width: 1264,
            height: 1680,
            pixels_per_inch: 300,
            ..kobo_ui::CLARA_BW_METRICS
        },
        DisplayMetrics {
            width: 1404,
            height: 1872,
            pixels_per_inch: 227,
            ..kobo_ui::CLARA_BW_METRICS
        },
    ] {
        for scale in [TextScale::Default, TextScale::Large, TextScale::ExtraLarge] {
            let metrics = DisplayMetrics {
                text_scale: scale,
                ..profile
            };
            let mut runner = runner(scale);
            let reachable = |browser: &Browser, action: &str| {
                browser
                    .screen(&metrics)
                    .build()
                    .layout_with(&metrics, &Chrome::measuring(true))
                    .rect_of_action(action_id(action))
                    .is_some()
            };
            assert!(reachable(runner.app(), "navigate"));
            assert!(!reachable(runner.app(), "links"));
            runner.action(action_id("navigate"));
            assert!(reachable(runner.app(), "links"));
            runner.action(action_id("links"));
            assert_eq!(runner.app().view, View::Links(0));
            assert!(reachable(runner.app(), "return"));
            runner.action(action_id("return"));
            assert_eq!(runner.app().view, View::Page);
            assert!(reachable(runner.app(), "navigate"));
        }
    }
}

#[test]
fn painted_page_journey_keeps_native_reader_and_scoped_links() {
    for colour in [false, true] {
        let mut runner = runner(TextScale::Default);
        runner.app_mut().pictures.set_colour(colour);
        let mut context = runner.context();
        let url = Url::parse("https://journey.invalid/start").expect("url");
        let mut html = String::from("<title>Reading on paper</title><body><h1>Reading on paper</h1><p><a href='/next'>Open the next article</a></p>");
        for _ in 0..40 {
            html.push_str("<p>Pages keep their place. Clear text belongs on the screen, with enough room to read comfortably.</p>");
        }
        html.push_str("<p><a href='/last'>A later link</a></p></body>");
        runner
            .app_mut()
            .show_document(&mut context, &url, html.as_bytes());
        let commands = runner.action(action_id("page-view"));
        assert!(
            runner.app().loaded.as_ref().unwrap().painted.is_some(),
            "Page view refused"
        );
        assert!(commands.iter().any(|command| matches!(command, kobo_sdk::Command::PutPicture { handle, format, .. }
            if *handle == page_mode::HANDLE && *format == if colour { kobo_sdk::PictureFormat::Rgb } else { kobo_sdk::PictureFormat::Grey })));
        assert_eq!(runner.app().page_links().len(), 1);
        assert!(
            runner
                .app()
                .loaded
                .as_ref()
                .unwrap()
                .painted
                .as_ref()
                .unwrap()
                .view
                .pages()
                > 1
        );
        let first = runner.app().page_links()[0];
        save_painted_journey_frame(
            &runner,
            &commands,
            if colour { "colour-first" } else { "bw-first" },
        );
        runner.action(action_id("next-page"));
        assert_eq!(
            runner
                .app()
                .loaded
                .as_ref()
                .unwrap()
                .painted
                .as_ref()
                .unwrap()
                .page,
            1
        );
        runner.action(action_id("previous-page"));
        runner.action(action_id("links"));
        runner.action(action_id(&link_action(first)));
        assert_eq!(
            runner.app().pending.as_ref().unwrap().url.to_string(),
            "https://journey.invalid/next"
        );
        runner.action(action_id("cancel-load"));
        runner.action(action_id("native-reader"));
        assert!(runner.app().loaded.as_ref().unwrap().painted.is_none());
    }
}

fn save_painted_journey_frame(
    runner: &AppRunner<Browser>,
    commands: &[kobo_sdk::Command],
    name: &str,
) {
    if std::env::var_os("COBALT_PAGE_EVIDENCE").is_none() {
        return;
    }
    struct Picture {
        width: u32,
        height: u32,
        grey: Vec<u8>,
        colour: Option<Vec<u8>>,
    }
    impl kobo_ui::Pictures for Picture {
        fn get(&self, handle: kobo_sdk::PictureHandle) -> Option<kobo_ui::PicturePixels<'_>> {
            (handle == page_mode::HANDLE).then_some(kobo_ui::PicturePixels {
                width: self.width,
                height: self.height,
                grey: &self.grey,
                colour: self.colour.as_deref(),
            })
        }
    }
    let (width, height, format, pixels) = commands
        .iter()
        .find_map(|command| match command {
            kobo_sdk::Command::PutPicture {
                handle,
                width,
                height,
                format,
                pixels,
            } if *handle == page_mode::HANDLE => Some((*width, *height, *format, pixels)),
            _ => None,
        })
        .expect("published picture");
    let picture = Picture {
        width,
        height,
        grey: if format == kobo_sdk::PictureFormat::Grey {
            pixels.clone()
        } else {
            pixels.chunks_exact(3).map(|p| p[0]).collect()
        },
        colour: (format == kobo_sdk::PictureFormat::Rgb).then(|| pixels.clone()),
    };
    let metrics = kobo_ui::CLARA_BW_METRICS;
    let screen = runner.app().screen(&metrics).build();
    let room =
        kobo_web_layout::picture_box(&screen, &metrics, page_mode::HANDLE).expect("picture room");
    assert_eq!(room, (width, height), "raster must not be resized by UI");
    let mut surface = kobo_ui::Surface::new(metrics.width as usize, metrics.height as usize);
    kobo_ui::render_all(
        &screen,
        &metrics,
        &Chrome::measuring(true),
        &picture,
        &mut surface,
        None,
    );
    use std::io::Write as _;
    let mut file = std::fs::File::create(format!("/tmp/page-journey-{name}.ppm")).unwrap();
    write!(file, "P6\n{} {}\n255\n", metrics.width, metrics.height).unwrap();
    for index in 0..surface.pixels.len() {
        file.write_all(&surface.rgb_at(index).unwrap()).unwrap();
    }
}

#[test]
fn painted_forms_keep_native_controls_and_census_refusal_is_visible() {
    let mut runner = form_runner(TextScale::Default);
    runner.action(action_id("page-view"));
    let loaded = runner.app().loaded.as_ref().unwrap();
    assert!(loaded.painted.is_none());
    assert!(loaded.paint_refused);
    runner.action(action_id("form-0"));
    assert_eq!(runner.app().view, View::Form(0, 0));

    let mut context = runner.context();
    runner.app_mut().show_document(&mut context, &Url::parse("https://journey.invalid/refuse").unwrap(),
        b"<title>Fallback</title><div style='visibility:hidden'><p>hidden</p><p>hidden</p><p>hidden</p></div><p>Visible reader content.</p>");
    runner.app_mut().view = View::Page;
    runner.action(action_id("page-view"));
    assert!(runner.app().loaded.as_ref().unwrap().paint_refused);
    assert!(runner.app().loaded.as_ref().unwrap().painted.is_none());
}

#[test]
fn painted_fragment_link_uses_native_anchor_offsets() {
    let mut runner = runner(TextScale::Default);
    runner.action(action_id("page-view"));
    let loaded = runner.app().loaded.as_ref().unwrap();
    assert!(loaded.painted.is_some());
    let index = loaded
        .document
        .links
        .iter()
        .position(|link| {
            link.target.path().ends_with("article.html") && link.target.fragment().is_none()
        })
        .unwrap();
    // First navigate to the article, then enter its painted mode.
    runner.action(action_id(&link_action(index)));
    runner.action(action_id("page-view"));
    assert!(runner.app().loaded.as_ref().unwrap().painted.is_some());
    let target = runner
        .app()
        .loaded
        .as_ref()
        .unwrap()
        .url
        .join("#refresh")
        .unwrap();
    let mut context = runner.context();
    runner.app_mut().open(&mut context, &target);
    assert!(runner.app().loaded.as_ref().unwrap().painted.is_none());
    let loaded = runner.app().loaded.as_ref().unwrap();
    assert!(loaded.paginator.page_of("refresh").is_some());
    assert_eq!(Some(loaded.page), loaded.paginator.page_of("refresh"));
}

#[test]
fn launch_is_search_first_and_samples_remain_reachable() {
    let mut runner = AppRunner::with_metrics(Browser::default(), kobo_ui::CLARA_BW_METRICS);
    let commands = runner.start();
    assert_eq!(runner.app().view, View::Start);
    assert!(runner.app().loaded.is_none());
    assert!(fetches(&commands).is_empty());
    for scale in TextScale::STEPS {
        let metrics = DisplayMetrics {
            text_scale: scale,
            ..kobo_ui::CLARA_BW_METRICS
        };
        assert!(kobo_web_layout::fits(
            &runner.app().screen(&metrics).build(),
            &metrics
        ));
    }
    runner.action(action_id("address"));
    assert!(runner.app().address.is_open());
    assert!(runner.app().address.text().is_empty());
    runner.action(action_id("kb.cancel"));
    assert_eq!(runner.app().view, View::Start);
    runner.action(action_id("samples"));
    assert!(runner
        .app()
        .loaded
        .as_ref()
        .unwrap()
        .title
        .contains("sample pages"));
    runner.action(action_id("edit-address"));
    assert_eq!(runner.app().address.text(), SAMPLES);
}

#[test]
fn persistent_address_field_is_above_native_and_painted_content() {
    let mut runner = runner(TextScale::Default);
    for painted in [false, true] {
        if painted {
            runner.action(action_id("page-view"));
        }
        let metrics = runner.context().metrics();
        let screen = runner.app().screen(&metrics).build();
        assert!(
            matches!(screen.nodes.first(), Some(kobo_sdk::Node::Field { action, value, .. })
            if *action == action_id("edit-address") && value == SAMPLES)
        );
        assert!(kobo_web_layout::fits(&screen, &metrics));
    }
}

#[test]
fn start_search_submits_words_and_library_returns_to_start() {
    let mut runner = AppRunner::with_metrics(Browser::default(), kobo_ui::CLARA_BW_METRICS);
    runner.start();
    runner.action(action_id("bookmarks"));
    runner.action(action_id("return"));
    assert_eq!(runner.app().view, View::Start);
    runner.action(action_id("address"));
    for key in [
        "kb.r0c2", "kb.space", "kb.r0c7", "kb.r2c5", "kb.r1c7", "kb.enter",
    ] {
        runner.action(action_id(key));
    }
    assert_eq!(
        runner.app().pending.as_ref().unwrap().url.to_string(),
        "https://html.duckduckgo.com/html/?q=e+ink"
    );
    runner.action(action_id("edit-address"));
    assert_eq!(
        runner.app().address.text(),
        "https://html.duckduckgo.com/html/?q=e+ink"
    );
}

#[test]
fn inline_address_keyboard_preserves_page_and_suppresses_page_actions() {
    for metrics in [kobo_ui::CLARA_BW_METRICS] {
        let mut runner = AppRunner::with_metrics(Browser::default(), metrics);
        runner.start();
        runner.action(action_id("samples"));
        for painted in [false, true] {
            if painted {
                runner.action(action_id("page-view"));
            }
            runner.action(action_id("edit-address"));
            let screen = runner.app().screen(&metrics).build();
            assert!(screen
                .nodes
                .iter()
                .any(|n| matches!(n, kobo_sdk::Node::Grid { .. })));
            assert!(screen
                .top_bar
                .as_ref()
                .unwrap()
                .actions
                .iter()
                .any(|a| a.action == action_id("kb.cancel")));
            if painted {
                assert!(screen
                    .nodes
                    .iter()
                    .any(|n| matches!(n, kobo_sdk::Node::Picture { .. })));
            }
            let page = runner.app().loaded.as_ref().unwrap().page;
            runner.action(action_id("next-page"));
            assert_eq!(runner.app().loaded.as_ref().unwrap().page, page);
            runner.action(action_id("kb.r0c0"));
            assert_eq!(runner.app().address.text(), format!("{SAMPLES}q"));
            runner.action(action_id("kb.cancel"));
            assert!(!runner.app().address.is_open());
            assert_eq!(
                runner.app().loaded.as_ref().unwrap().url.to_string(),
                SAMPLES
            );
        }
    }
}

#[test]
fn bottom_toolbar_menu_home_and_url_keys_work() {
    let mut store = Store::default();
    let mut runner = runner_with(&mut store);
    for action in [
        "browser-more",
        "add-favourite",
        "browser-more",
        "add-favourite",
        "browser-more",
        "set-home",
    ] {
        let commands = runner.action(action_id(action));
        store.pump(&mut runner, commands);
    }
    assert_eq!(runner.app().library.bookmarks.len(), 1);
    assert_eq!(
        runner.app().home_page.as_ref().unwrap().to_string(),
        SAMPLES
    );
    runner.action(action_id("home"));
    runner.action(action_id("go-home"));
    assert_eq!(runner.app().view, View::Page);
    runner.action(action_id("edit-address"));
    for action in ["url-slash", "url-dot", "url-com", "url-colon"] {
        runner.action(action_id(action));
    }
    assert_eq!(runner.app().address.text(), format!("{SAMPLES}/..com:"));
    runner.action(action_id("kb.cancel"));
    runner.action(action_id("zoom-more"));
    assert_eq!(runner.app().zoom, 125);
    assert!(runner.app().loaded.as_ref().unwrap().painted.is_some());
    runner.action(action_id("zoom-less"));
    assert_eq!(runner.app().zoom, 100);
    runner.action(action_id("zoom-more"));
    runner.action(action_id("reload"));
    assert_eq!(
        runner.app().zoom,
        100,
        "native reload must not retain a painted zoom label"
    );
    assert_eq!(
        runner.app().loaded.as_ref().unwrap().url.to_string(),
        SAMPLES
    );
    let again = runner_with(&mut store);
    assert_eq!(again.app().home_page.as_ref().unwrap().to_string(), SAMPLES);
}

#[test]
fn inline_keyboard_splits_a_long_first_paragraph_instead_of_hiding_it() {
    let mut runner = runner(TextScale::Default);
    let body = format!(
        "<title>Long first paragraph</title><p>{}",
        "A long paragraph keeps the page visible while typing. ".repeat(100)
    );
    open_web_page(&mut runner, &body);
    runner.action(action_id("edit-address"));
    let preview = runner.app().keyboard_pieces(&runner.context().metrics());
    assert!(!preview.is_empty());
    assert!(
        matches!(preview.first(), Some(Piece::Prose(runs)) if runs.iter().map(|r| r.text.len()).sum::<usize>() > 100)
    );
    assert!(kobo_web_layout::fits(
        &runner.app().screen(&runner.context().metrics()).build(),
        &runner.context().metrics()
    ));
}

#[test]
fn page_has_browser_close_separate_from_keyboard_close() {
    let mut runner = runner(TextScale::Default);
    let metrics = runner.context().metrics();
    let screen = runner.app().screen(&metrics).build();
    assert!(screen
        .top_bar
        .unwrap()
        .actions
        .iter()
        .any(|a| a.action == action_id("browser-close")));
    runner.action(action_id("edit-address"));
    let screen = runner.app().screen(&metrics).build();
    assert!(screen
        .top_bar
        .unwrap()
        .actions
        .iter()
        .any(|a| a.action == action_id("kb.cancel")));
    runner.action(action_id("kb.cancel"));
    let commands = runner.action(action_id("browser-close"));
    assert!(commands
        .iter()
        .any(|c| matches!(c, kobo_sdk::Command::Exit)));
}

#[test]
fn compact_slider_coordinates_update_zoom_without_affecting_page_taps() {
    let mut runner = runner(TextScale::Default);
    let metrics = runner.context().metrics();
    let screen = runner.app().screen(&metrics).build();
    let layout = screen.layout_with(&metrics, &kobo_sdk::Chrome::measuring(true));
    let slider = layout
        .nodes
        .iter()
        .find(|node| matches!(node.kind, kobo_ui::LayoutKind::Slider(..)))
        .unwrap()
        .rect;
    let a = layout
        .slider_action(slider.x + slider.width / 4, slider.y + slider.height / 2)
        .unwrap();
    let b = layout
        .slider_action(
            slider.x + slider.width * 3 / 4,
            slider.y + slider.height / 2,
        )
        .unwrap();
    assert!(a.0 < b.0);
    assert!(layout.slider_action(0, 0).is_none());
    runner.action(a);
    assert!((105..=107).contains(&runner.app().zoom));
    runner.action(b);
    assert!((167..=169).contains(&runner.app().zoom));
    assert!(runner.app().loaded.as_ref().unwrap().painted.is_some());
}

#[test]
fn address_cursor_inserts_and_deletes_in_the_middle_without_navigating() {
    let mut store = Store::default();
    let mut runner = runner_with(&mut store);
    runner.action(action_id("edit-address"));
    runner.action(action_id("kb.home"));
    runner.action(action_id("kb.right"));
    runner.action(action_id("url-com"));
    let mut expected = SAMPLES.to_owned();
    expected.insert_str(1, ".com");
    assert_eq!(runner.app().address.text(), expected);
    runner.action(action_id("kb.backspace"));
    expected.remove(4);
    assert_eq!(runner.app().address.text(), expected);
    assert_eq!(runner.app().loaded.as_ref().unwrap().url.to_string(), SAMPLES);
    runner.action(action_id("kb.cancel"));
    assert!(!runner.app().address.is_open());
}


#[test]
fn native_keyboard_measurement_includes_visible_page_footer() {
    for scale in [TextScale::Default,TextScale::Large,TextScale::ExtraLarge] {
        let mut runner = runner(scale);
        runner.action(action_id("edit-address"));
        let metrics = runner.context().metrics();
        let screen = runner.app().screen(&metrics).build();
        assert!(kobo_web_layout::fits(&screen,&metrics),"{scale:?}");
        assert!(!runner.app().keyboard_pieces(&metrics).is_empty());
    }
}


#[test]
fn explicit_get_submitter_uses_only_the_chosen_button_value() {
    let mut runner = runner(TextScale::Default);
    open_web_page(&mut runner,"<!doctype html><title>Choose submitter</title><form action='https://forms.invalid/search'><input name=q value=ink><input type=submit name=mode value=title><input type=submit name=mode value=author></form>");
    runner.action(action_id("form-0"));
    runner.action(action_id("submit-field-2"));
    assert!(
        matches!(&runner.app().view,View::Loading(url) if url.to_string().ends_with("/search?q=ink&mode=author"))
    );
}

#[test]
fn offscreen_submitter_cannot_submit_and_post_choice_still_never_sends() {
    let mut runner = runner(TextScale::Default);
    let mut html =
        "<!doctype html><title>Many controls</title><form action='https://forms.invalid/search'>"
            .to_owned();
    for n in 0..30 {
        html.push_str(&format!("<input name='field{n}' value=ink>"));
    }
    html.push_str("<input type=submit name=go value=last></form>");
    open_web_page(&mut runner, &html);
    runner.action(action_id("form-0"));
    runner.action(action_id("submit-field-30"));
    assert_eq!(runner.app().view, View::Form(0, 0));
    open_web_page(&mut runner,"<!doctype html><title>POST choices</title><form method=post><input name=q><input type=submit name=go value=one><input type=submit name=go value=two></form>");
    runner.action(action_id("form-0"));
    runner.action(action_id("submit-field-2"));
    assert_eq!(runner.app().view, View::PostConfirm(0));
    assert!(runner.app().pending.is_none());
}


#[test]
fn colour_arrivals_default_to_rgb_page_but_bw_and_forms_keep_reader() {
    for colour in [false, true] {
        let mut runner = runner(TextScale::Default);
        runner.app_mut().pictures.set_colour(colour);
        let commands = open_web_page(&mut runner,"<!doctype html><title>RGB arrival</title><p style='color:red'>Red prose</p><p><a href='/target' style='color:blue'>Blue link</a></p>");
        if colour {
            assert!(commands.iter().any(|command|matches!(command,kobo_sdk::Command::PutPicture{format:kobo_sdk::PictureFormat::Rgb,pixels,..} if pixels.chunks_exact(3).any(|p|u16::from(p[0])>u16::from(p[1])+20 && u16::from(p[0])>u16::from(p[2])+20))));
        }
        assert_eq!(
            runner.app().loaded.as_ref().unwrap().painted.is_some(),
            colour
        );
        open_web_page(
            &mut runner,
            "<!doctype html><title>Form arrival</title><form><input name=q></form>",
        );
        assert!(runner.app().loaded.as_ref().unwrap().painted.is_none());
    }
}


#[test]
fn late_colour_identity_republishes_an_already_visible_rgb_page() {
    let mut runner = runner(TextScale::Default);
    open_web_page(
        &mut runner,
        "<!doctype html><title>Late colour</title><p style='color:red'>Red prose</p>",
    );
    runner.action(action_id("page-view"));
    let mut context = runner.context();
    let identity = kobo_sdk::DeviceIdentity {
        profile_id: "SIMULATOR:clara-colour-393".into(),
        model: "Simulated Colour".into(),
        firmware: String::new(),
        kernel: String::new(),
        runtime_version: String::new(),
        panel_width: 1072,
        panel_height: 1448,
        ..kobo_sdk::DeviceIdentity::default()
    };
    runner.app_mut().on_device_result(
        &mut context,
        kobo_sdk::DeviceRequest::ReadIdentity,
        kobo_sdk::DeviceResult::Identity(identity),
    );
    assert!(context.take_commands().iter().any(|command| matches!(
        command,
        kobo_sdk::Command::PutPicture {
            format: kobo_sdk::PictureFormat::Rgb,
            ..
        }
    )));
}
