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
            assert!(reachable(runner.app(), "browser-more"));
            assert!(!reachable(runner.app(), "navigate"));
            runner.action(action_id("browser-more"));
            assert!(reachable(runner.app(), "navigate"));
            assert!(!reachable(runner.app(), "links"));
            runner.action(action_id("navigate"));
            assert!(reachable(runner.app(), "links"));
            runner.action(action_id("links"));
            assert_eq!(runner.app().view, View::Links(0));
            assert!(reachable(runner.app(), "return"));
            runner.action(action_id("return"));
            assert_eq!(runner.app().view, View::Page);
            assert!(reachable(runner.app(), "browser-more"));
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
        handle: kobo_sdk::PictureHandle,
        width: u32,
        height: u32,
        grey: Vec<u8>,
        colour: Option<Vec<u8>>,
    }
    impl kobo_ui::Pictures for Picture {
        fn get(&self, handle: kobo_sdk::PictureHandle) -> Option<kobo_ui::PicturePixels<'_>> {
            (handle == self.handle).then_some(kobo_ui::PicturePixels {
                width: self.width,
                height: self.height,
                grey: &self.grey,
                colour: self.colour.as_deref(),
            })
        }
    }
    let (handle, width, height, format, pixels) = commands
        .iter()
        .find_map(|command| match command {
            kobo_sdk::Command::PutPicture {
                handle,
                width,
                height,
                format,
                pixels,
            } => Some((*handle, *width, *height, *format, pixels)),
            _ => None,
        })
        .expect("published picture");
    let picture = Picture {
        handle,
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
        kobo_web_layout::picture_box(&screen, &metrics, handle).expect("picture room");
    if handle == page_mode::HANDLE {
        assert_eq!(room, (width, height), "Page raster must not be resized by UI");
    }
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
    runner.action(action_id("browser-zoom"));
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

#[test]
fn late_colour_identity_reloads_native_grey_images_as_rgb() {
    let mut runner = runner(TextScale::Default);
    let commands = open_web_page(&mut runner, PLATE);
    let first = fetches(&commands)[0].0;
    let png = kobo_image::encode_png_rgb(2, 1, &[255, 0, 0, 0, 128, 255]).unwrap();
    let commands = runner.task_outcome(first, TaskOutcome::Completed(png.clone()));
    assert!(commands.iter().any(|c|matches!(c,kobo_sdk::Command::PutPicture{format:kobo_sdk::PictureFormat::Grey,..})));
    let mut context = runner.context();
    runner.app_mut().on_device_result(&mut context,kobo_sdk::DeviceRequest::ReadIdentity,
        kobo_sdk::DeviceResult::Identity(kobo_sdk::DeviceIdentity {
            profile_id: "SIMULATOR:clara-colour-393".into(),
            ..kobo_sdk::DeviceIdentity::default()
        }));
    let commands = context.take_commands();
    assert!(commands.iter().any(|c|matches!(c,kobo_sdk::Command::DropPicture(handle) if *handle==picture_handle(0))));
    let requested = fetches(&commands);
    assert_eq!(requested.len(),1,"native grey image must be requested in confirmed colour format");
    let commands=runner.task_outcome(requested[0].0,TaskOutcome::Completed(png));
    assert!(commands.iter().any(|c|matches!(c,kobo_sdk::Command::PutPicture{format:kobo_sdk::PictureFormat::Rgb,pixels,..} if pixels.chunks_exact(3).any(|p|p[0]!=p[1]||p[1]!=p[2]))));
    assert!(runner.app().loaded.as_ref().unwrap().painted.is_none());
    save_painted_journey_frame(&runner, &commands, "late-native-rgb");
}

#[test]
fn colour_identity_change_cancels_pending_native_images_once() {
    let mut runner = runner(TextScale::Default);
    let commands = open_web_page(&mut runner, PLATE);
    let old_task = fetches(&commands)[0].0;
    let identity = kobo_sdk::DeviceIdentity {
        profile_id: "SIMULATOR:clara-colour-393".into(),
        ..kobo_sdk::DeviceIdentity::default()
    };
    let mut context = runner.context();
    runner.app_mut().on_device_result(&mut context,kobo_sdk::DeviceRequest::ReadIdentity,
        kobo_sdk::DeviceResult::Identity(identity.clone()));
    let commands = context.take_commands();
    assert!(commands.iter().any(|c|matches!(c,kobo_sdk::Command::Cancel(task) if *task==old_task)));
    let tasks = fetches(&commands);
    assert_eq!(tasks.len(),1);
    assert_ne!(tasks[0].0,old_task);
    let stale=runner.task_outcome(old_task,TaskOutcome::Completed(a_png(2,1)));
    assert!(put_pictures(&stale).is_empty());
    let mut context = runner.context();
    runner.app_mut().on_device_result(&mut context,kobo_sdk::DeviceRequest::ReadIdentity,
        kobo_sdk::DeviceResult::Identity(identity));
    assert!(fetches(&context.take_commands()).is_empty(),"same identity must not refetch");
}

#[test]
fn late_colour_identity_ignores_pending_grey_shelf_completion() {
    let mut store = Store::default();
    let mut runner = runner_with(&mut store);
    let commands = open_web_page(&mut runner, PLATE);
    let commands = store.pump(&mut runner, commands);
    let png = kobo_image::encode_png_rgb(2, 1, &[255,0,0,0,128,255]).unwrap();
    let commands = runner.task_outcome(fetches(&commands)[0].0, TaskOutcome::Completed(png.clone()));
    store.pump(&mut runner, commands);
    let commands = runner.action(action_id("back"));
    store.pump(&mut runner, commands);
    let commands = open_web_page(&mut runner, PLATE);
    assert_eq!(runner.app().pictures.state(0),Some(pictures::State::Reading));
    let shelf = commands.into_iter().find_map(|c|match c {
        kobo_sdk::Command::Store(request @ kobo_sdk::StoreRequest::ShelfRead{..})=>Some(request),
        _=>None
    }).expect("grey shelf read started");
    let stale = store.answer(shelf);
    let mut context = runner.context();
    runner.app_mut().on_device_result(&mut context,kobo_sdk::DeviceRequest::ReadIdentity,
        kobo_sdk::DeviceResult::Identity(kobo_sdk::DeviceIdentity {
            profile_id:"SIMULATOR:clara-colour-393".into(),
            ..kobo_sdk::DeviceIdentity::default()
        }));
    let changed = context.take_commands();
    let wanted = fetches(&changed);
    assert_eq!(wanted.len(),1,"grey shelf cannot satisfy a Colour request");
    let stale_commands = runner.store_result(stale);
    assert!(put_pictures(&stale_commands).is_empty(),"stale grey shelf answer must not publish");
    assert_eq!(runner.app().pictures.state(0),Some(pictures::State::Fetching(wanted[0].0)));
    let shown = runner.task_outcome(wanted[0].0,TaskOutcome::Completed(png));
    let shown = store.pump(&mut runner,shown);
    assert!(shown.iter().any(|c|matches!(c,kobo_sdk::Command::PutPicture{format:kobo_sdk::PictureFormat::Rgb,..})));
    let index=runner.app().saved.index().unwrap();
    assert!(index.entries().iter().any(|entry|entry.url.starts_with("picture:rgb:")));
    assert!(index.entries().iter().any(|entry|entry.url.starts_with("picture:grey:")));
}

#[test]
fn colour_identity_switch_prefers_existing_rgb_shelf_over_grey() {
    let mut store = Store::default();
    let mut runner = runner_with(&mut store);
    let commands = open_web_page(&mut runner,PLATE);
    let commands = store.pump(&mut runner,commands);
    let png=kobo_image::encode_png_rgb(2,1,&[255,0,0,0,128,255]).unwrap();
    let shown=runner.task_outcome(fetches(&commands)[0].0,TaskOutcome::Completed(png.clone()));
    store.pump(&mut runner,shown);
    let grey_key=runner.app().saved.index().unwrap().entries().iter()
        .find(|entry|entry.url.starts_with("picture:grey:")).unwrap().url.clone();
    let rgb_key=grey_key.replacen("picture:grey:","picture:rgb:",1);
    let prepared=pictures::prepare_for(&png,400,300,true).unwrap();
    let mut context=runner.context();
    runner.app_mut().saved.keep(&mut context,&rgb_key,&pictures::pack(&prepared));
    let commands=context.take_commands();
    store.pump(&mut runner,commands);
    let back=runner.action(action_id("back"));
    store.pump(&mut runner,back);
    let commands=open_web_page(&mut runner,PLATE);
    let stale_request=commands.into_iter().find_map(|c|match c {
        kobo_sdk::Command::Store(request @ kobo_sdk::StoreRequest::ShelfRead{..})=>Some(request),_=>None
    }).unwrap();
    let stale=store.answer(stale_request);
    let mut context=runner.context();
    runner.app_mut().on_device_result(&mut context,kobo_sdk::DeviceRequest::ReadIdentity,
        kobo_sdk::DeviceResult::Identity(kobo_sdk::DeviceIdentity {
            profile_id:"SIMULATOR:clara-colour-393".into(),
            ..kobo_sdk::DeviceIdentity::default()
        }));
    let changed=context.take_commands();
    assert!(fetches(&changed).is_empty(),"RGB cache avoids network");
    assert_eq!(runner.app().pictures.state(0),Some(pictures::State::Reading));
    assert!(put_pictures(&runner.store_result(stale)).is_empty());
    let changed=store.pump(&mut runner,changed);
    assert!(changed.iter().any(|c|matches!(c,kobo_sdk::Command::PutPicture{format:kobo_sdk::PictureFormat::Rgb,..})));
    assert_eq!(runner.app().pictures.state(0),Some(pictures::State::Shown));
}

#[test]
fn native_rgb_spans_render_with_installed_typefaces() {
    let _runner=runner(TextScale::Default);
    let screen=kobo_ui::Screen::new(81,vec![kobo_ui::Node::RichText {
        id:kobo_ui::NodeId(1),text:"Native colour text".into(),
        spans:vec![kobo_ui::RichTextSpan{start:0,end:18,presentation:kobo_ui::TextPresentation {
            foreground:Some([201,37,63]),background:Some([225,240,250]),
            strong:true,emphasis:true,underline:true,..Default::default()
        }}],links:Vec::new(),presentation:Default::default(),selection:None,formulae:Vec::new(),
    }]);
    let metrics=kobo_ui::CLARA_BW_METRICS;
    let mut surface=kobo_ui::Surface::new(metrics.width as usize,metrics.height as usize);
    kobo_ui::render_all(&screen,&metrics,&Chrome::measuring(true),&(),&mut surface,None);
    assert!((0..surface.pixels.len()).any(|i|surface.rgb_at(i)==Some([201,37,63])));
    assert!((0..surface.pixels.len()).any(|i|surface.rgb_at(i)==Some([225,240,250])));
    if std::env::var_os("COBALT_PAGE_EVIDENCE").is_some() {
        use std::io::Write;
        let mut file=std::fs::File::create("/tmp/native-rgb-fonts.ppm").unwrap();
        write!(file,"P6\n{} {}\n255\n",metrics.width,metrics.height).unwrap();
        for i in 0..surface.pixels.len(){file.write_all(&surface.rgb_at(i).unwrap()).unwrap();}
    }
}

#[test]
fn colour_native_form_page_keeps_authored_prose_rgb_and_controls() {
    let mut runner=runner(TextScale::Default);
    runner.app_mut().pictures.set_colour(true);
    open_web_page(&mut runner,"<!doctype html><style>p{color:#c9253f}em{color:#2070c0}</style><p>Native <em>colour</em> form page.</p><form action='/search'><input name=q value=ink></form>");
    assert!(runner.app().loaded.as_ref().unwrap().painted.is_none());
    let pieces=runner.app().current_pieces();
    assert!(pieces.iter().any(|p|matches!(p,Piece::Prose(runs) if runs.iter().any(|r|r.foreground==Some([201,37,63])))));
    assert!(pieces.iter().any(|p|matches!(p,Piece::Form{..})));
    let metrics=kobo_ui::CLARA_BW_METRICS;
    let screen=runner.app().screen(&metrics).build();
    let mut surface=kobo_ui::Surface::new(metrics.width as usize,metrics.height as usize);
    kobo_ui::render_all(&screen,&metrics,&Chrome::measuring(true),&(),&mut surface,None);
    assert!((0..surface.pixels.len()).any(|i|surface.rgb_at(i)==Some([201,37,63])));
    assert!((0..surface.pixels.len()).any(|i|surface.rgb_at(i)==Some([32,112,192])));
    if std::env::var_os("COBALT_PAGE_EVIDENCE").is_some() {
        use std::io::Write;
        let mut file=std::fs::File::create("/tmp/native-colour-form.ppm").unwrap();
        write!(file,"P6\n{} {}\n255\n",metrics.width,metrics.height).unwrap();
        for i in 0..surface.pixels.len(){file.write_all(&surface.rgb_at(i).unwrap()).unwrap();}
    }
    runner.action(action_id("form-0"));
    assert_eq!(runner.app().view,View::Form(0,0));
    runner.action(action_id("submit-form"));
    assert_eq!(runner.app().pending.as_ref().unwrap().url.to_string(),"https://example.com/search?q=ink");
}

#[test]
fn native_multicolour_link_keeps_each_underline_colour_and_one_action() {
    let mut runner=runner(TextScale::Default);
    runner.app_mut().pictures.set_colour(true);
    open_web_page(&mut runner,"<!doctype html><p><a href='/next'><span style='color:red'>Red </span><span style='color:blue'>blue</span></a></p><form><input name=q></form>");
    let metrics=kobo_ui::CLARA_BW_METRICS;
    let screen=runner.app().screen(&metrics).build();
    let layout=screen.layout_with(&metrics,&Chrome::measuring(true));
    let mut colours=Vec::new();
    let mut surface=kobo_ui::Surface::new(metrics.width as usize,metrics.height as usize);
    kobo_ui::render_all(&screen,&metrics,&Chrome::measuring(true),&(),&mut surface,None);
    for node in &layout.nodes {
        if let kobo_ui::LayoutKind::InlineColourLink(action,rgb)=node.kind {
            assert_eq!(action,action_id("link-0"));
            let thickness=metrics.rule_thickness();
            let y=node.rect.y+kobo_ui::FontSize::Body.line_height_in(kobo_ui::Face::Reading)-thickness*2;
            let index=(y as usize)*metrics.width as usize+node.rect.x as usize;
            assert_eq!(surface.rgb_at(index),Some(rgb));
            colours.push(rgb);
        }
    }
    assert!(colours.contains(&[255,0,0]));assert!(colours.contains(&[0,0,255]));
    if std::env::var_os("COBALT_PAGE_EVIDENCE").is_some() {
        use std::io::Write;
        let mut file=std::fs::File::create("/tmp/native-colour-link.ppm").unwrap();
        write!(file,"P6\n{} {}\n255\n",metrics.width,metrics.height).unwrap();
        for i in 0..surface.pixels.len(){file.write_all(&surface.rgb_at(i).unwrap()).unwrap();}
    }
}

#[test]
fn late_native_text_colour_preserves_edited_form_and_semantic_place() {
    let mut runner=form_runner(TextScale::Default);
    let mut context=runner.context();
    runner.app_mut().show_document(&mut context,&Url::parse("https://example.com/form").unwrap(),
        b"<!doctype html><p style='color:red'>Native late colour.</p><form action='/search'><input name=q value=original></form>");
    let form=forms::form_mut(&mut runner.app_mut().loaded.as_mut().unwrap().document.blocks,0).unwrap();
    if let Field::Text{value,..}=&mut form.fields[0]{*value="edited".into();}
    let mut context=runner.context();
    runner.app_mut().on_device_result(&mut context,kobo_sdk::DeviceRequest::ReadIdentity,
        kobo_sdk::DeviceResult::Identity(kobo_sdk::DeviceIdentity{
            profile_id:"SIMULATOR:clara-colour-393".into(),..Default::default()
        }));
    assert!(runner.app().current_pieces().iter().any(|p|matches!(p,Piece::Prose(runs) if runs.iter().any(|r|r.foreground==Some([255,0,0])))));
    let form=forms::forms(&runner.app().loaded.as_ref().unwrap().document.blocks)[0];
    assert!(matches!(&form.fields[0],Field::Text{value,..} if value=="edited"));
    assert!(runner.app().loaded.as_ref().unwrap().painted.is_none());
    let metrics=kobo_ui::CLARA_BW_METRICS;
    let screen=runner.app().screen(&metrics).build();
    let mut surface=kobo_ui::Surface::new(metrics.width as usize,metrics.height as usize);
    kobo_ui::render_all(&screen,&metrics,&Chrome::measuring(true),&(),&mut surface,None);
    assert!((0..surface.pixels.len()).any(|i|surface.rgb_at(i)==Some([255,0,0])));
    if std::env::var_os("COBALT_PAGE_EVIDENCE").is_some() {
        use std::io::Write;
        let mut file=std::fs::File::create("/tmp/late-native-colour-text.ppm").unwrap();
        write!(file,"P6\n{} {}\n255\n",metrics.width,metrics.height).unwrap();
        for i in 0..surface.pixels.len(){file.write_all(&surface.rgb_at(i).unwrap()).unwrap();}
    }
}

#[test]
fn late_native_colour_keeps_saved_notice_fragment_place_and_reader_view() {
    for reader in [false,true] {
        let mut runner=runner(TextScale::ExtraLarge);
        let mut html=String::from("<!doctype html><nav><p>Menu</p></nav><main>");
        for _ in 0..35 {html.push_str("<p style='color:red'>Long native prose keeps its position across panel identity changes.</p>");}
        html.push_str("<p id=end style='color:blue'>End section.</p></main>");
        let mut context=runner.context();
        runner.app_mut().show_document_noted(&mut context,&Url::parse("https://example.com/article#end").unwrap(),
            html.as_bytes(),Some("Saved copy: test notice.".into()));
        if reader {runner.action(action_id("reader"));}
        runner.action(action_id("next-page"));
        let before=runner.app().current_pieces().iter().filter_map(|p|match p {Piece::Prose(r)=>Some(r.iter().map(|r|r.text.as_str()).collect::<String>()),_=>None}).collect::<Vec<_>>();
        let full=runner.app().loaded.as_ref().unwrap().full.is_some();
        let mut context=runner.context();
        runner.app_mut().on_device_result(&mut context,kobo_sdk::DeviceRequest::ReadIdentity,
            kobo_sdk::DeviceResult::Identity(kobo_sdk::DeviceIdentity{profile_id:"SIMULATOR:clara-colour-393".into(),..Default::default()}));
        let loaded=runner.app().loaded.as_ref().unwrap();
        assert_eq!(loaded.note.as_deref(),Some("Saved copy: test notice."));
        assert_eq!(loaded.full.is_some(),full);
        let after=runner.app().current_pieces().iter().filter_map(|p|match p {Piece::Prose(r)=>Some(r.iter().map(|r|r.text.as_str()).collect::<String>()),_=>None}).collect::<Vec<_>>();
        assert_eq!(before,after);
        assert!(runner.app().current_pieces().iter().any(|p|matches!(p,Piece::Prose(runs) if runs.iter().any(|r|r.foreground.is_some()))));
    }
}

#[test]
fn native_headings_keep_hierarchy_rgb_and_section_anchor() {
    let mut runner=runner(TextScale::Default);runner.app_mut().pictures.set_colour(true);
    open_web_page(&mut runner,"<!doctype html><h1 id=top style='color:red'>Native heading</h1><h2 style='color:blue'>Second heading</h2><p>Prose.</p><form><input name=q></form>");
    let metrics=kobo_ui::CLARA_BW_METRICS;
    let screen=runner.app().screen(&metrics).build();
    let layout=screen.layout_with(&metrics,&Chrome::measuring(true));
    assert!(layout.nodes.iter().any(|n|matches!(n.kind,kobo_ui::LayoutKind::ColouredHeading(1,_))));
    assert!(layout.nodes.iter().any(|n|matches!(n.kind,kobo_ui::LayoutKind::ColouredHeading(2,_))));
    let mut surface=kobo_ui::Surface::new(metrics.width as usize,metrics.height as usize);
    kobo_ui::render_all(&screen,&metrics,&Chrome::measuring(true),&(),&mut surface,None);
    assert!((0..surface.pixels.len()).any(|i|surface.rgb_at(i)==Some([255,0,0])));
    assert!((0..surface.pixels.len()).any(|i|surface.rgb_at(i)==Some([0,0,255])));
    let context=runner.context();assert_eq!(runner.app_mut().fragment_page(&context,"top"),Some(0));
    if std::env::var_os("COBALT_PAGE_EVIDENCE").is_some() {
        use std::io::Write;let mut file=std::fs::File::create("/tmp/native-colour-heading.ppm").unwrap();
        write!(file,"P6\n{} {}\n255\n",metrics.width,metrics.height).unwrap();
        for i in 0..surface.pixels.len(){file.write_all(&surface.rgb_at(i).unwrap()).unwrap();}
    }
}

#[test]
fn native_quote_rgb_preserves_gutters_links_and_form() {
    let mut runner=runner(TextScale::Default);runner.app_mut().pictures.set_colour(true);
    open_web_page(&mut runner,"<!doctype html><blockquote><p><span style='color:red'>Red quote </span><a href='/next' style='color:blue'>blue link</a></p><blockquote><p style='color:green'>Nested quote</p></blockquote></blockquote><form><input name=q></form>");
    let metrics=kobo_ui::CLARA_BW_METRICS;
    let screen=runner.app().screen(&metrics).build();let layout=screen.layout_with(&metrics,&Chrome::measuring(true));
    assert!(layout.nodes.iter().any(|n|matches!(n.kind,kobo_ui::LayoutKind::ColouredQuoteText(_,style) if style.foreground==Some([255,0,0]))));
    assert!(layout.nodes.iter().any(|n|matches!(n.kind,kobo_ui::LayoutKind::Quote(2,_))));
    assert!(runner.app().current_pieces().iter().any(|p|matches!(p,Piece::Quote{links,..} if links==&vec![0])));
    let mut surface=kobo_ui::Surface::new(metrics.width as usize,metrics.height as usize);
    kobo_ui::render_all(&screen,&metrics,&Chrome::measuring(true),&(),&mut surface,None);
    for rgb in [[255,0,0],[0,0,255],[0,128,0]] {assert!((0..surface.pixels.len()).any(|i|surface.rgb_at(i)==Some(rgb)));}
    runner.action(action_id("form-0"));assert!(matches!(runner.app().view,View::Form(0,0)));
    if std::env::var_os("COBALT_PAGE_EVIDENCE").is_some(){
        use std::io::Write;let mut file=std::fs::File::create("/tmp/native-quote-rgb.ppm").unwrap();
        write!(file,"P6\n{} {}\n255\n",metrics.width,metrics.height).unwrap();
        for i in 0..surface.pixels.len(){file.write_all(&surface.rgb_at(i).unwrap()).unwrap();}
    }
}

#[test]
fn native_pre_rgb_retains_spaces_blank_line_and_form() {
    let mut runner=runner(TextScale::Default);runner.app_mut().pictures.set_colour(true);
    open_web_page(&mut runner,"<!doctype html><pre>\n<span style='color:red'>  red  x\n\n</span><span style='color:blue'>    blue y</span>\n</pre><form><input name=q></form>");
    assert!(runner.app().current_pieces().iter().any(|p|matches!(p,Piece::ColouredPreformatted{text,..} if text=="  red  x\n\n    blue y")));
    let metrics=kobo_ui::CLARA_BW_METRICS;let screen=runner.app().screen(&metrics).build();
    let layout=screen.layout_with(&metrics,&Chrome::measuring(true));
    assert!(layout.nodes.iter().any(|n|matches!(n.kind,kobo_ui::LayoutKind::MonoRichText(_))&&!n.text_lines.is_empty()));
    assert!(!layout.nodes.iter().any(|n|matches!(n.kind,kobo_ui::LayoutKind::RichText(_))&&!n.text_lines.is_empty()));
    let mut surface=kobo_ui::Surface::new(metrics.width as usize,metrics.height as usize);
    kobo_ui::render_all(&screen,&metrics,&Chrome::measuring(true),&(),&mut surface,None);
    for rgb in [[255,0,0],[0,0,255]]{assert!((0..surface.pixels.len()).any(|i|surface.rgb_at(i)==Some(rgb)));}
    runner.action(action_id("form-0"));assert!(matches!(runner.app().view,View::Form(0,0)));
    if std::env::var_os("COBALT_PAGE_EVIDENCE").is_some(){
        use std::io::Write;let mut file=std::fs::File::create("/tmp/native-pre-rgb.ppm").unwrap();
        write!(file,"P6\n{} {}\n255\n",metrics.width,metrics.height).unwrap();
        for i in 0..surface.pixels.len(){file.write_all(&surface.rgb_at(i).unwrap()).unwrap();}
    }
}

#[test]
fn native_table_rgb_keeps_header_grid_links_and_form() {
    let mut runner=runner(TextScale::Default);runner.app_mut().pictures.set_colour(true);
    open_web_page(&mut runner,"<!doctype html><table><tr><th style='color:red'>Name</th><th style='color:blue'>Value</th></tr><tr><td><a href='/next' style='color:green'>Item</a></td><td style='color:purple'>42</td></tr></table><form><input name=q></form>");
    let metrics=kobo_ui::CLARA_BW_METRICS;let screen=runner.app().screen(&metrics).build();let layout=screen.layout_with(&metrics,&Chrome::measuring(true));
    assert!(layout.nodes.iter().any(|n|matches!(n.kind,kobo_ui::LayoutKind::ColouredTableText(true,_))));
    assert!(layout.nodes.iter().any(|n|matches!(n.kind,kobo_ui::LayoutKind::TableRule)));
    assert!(runner.app().current_pieces().iter().any(|p|matches!(p,Piece::Table{links,..} if links.iter().any(|row|row==&vec![0]))));
    let mut surface=kobo_ui::Surface::new(metrics.width as usize,metrics.height as usize);
    kobo_ui::render_all(&screen,&metrics,&Chrome::measuring(true),&(),&mut surface,None);
    for rgb in [[255,0,0],[0,0,255],[0,128,0],[128,0,128]]{assert!((0..surface.pixels.len()).any(|i|surface.rgb_at(i)==Some(rgb)));}
    runner.action(action_id("form-0"));assert!(matches!(runner.app().view,View::Form(0,0)));
    if std::env::var_os("COBALT_PAGE_EVIDENCE").is_some(){
        use std::io::Write;let mut file=std::fs::File::create("/tmp/native-table-rgb.ppm").unwrap();
        write!(file,"P6\n{} {}\n255\n",metrics.width,metrics.height).unwrap();
        for i in 0..surface.pixels.len(){file.write_all(&surface.rgb_at(i).unwrap()).unwrap();}
    }
}

#[test]
fn native_control_labels_take_their_own_source_colours_not_generated_names(){
    let mut runner=runner(TextScale::Default);runner.app_mut().pictures.set_colour(true);
    open_web_page(&mut runner,"<!doctype html><form><label for=q style='color:red'>Red search</label><input id=q name=q><label style='color:blue'><input type=checkbox name=c>Blue choice</label><input name=generated style='color:green'><input type=submit value=Send style='color:purple'></form>");
    let loaded=runner.app().loaded.as_ref().unwrap();let form=forms::forms(&loaded.document.blocks)[0];
    assert_eq!(form.field_colours[0],Some(([255,0,0],None)));assert_eq!(form.field_colours[1],Some(([0,0,255],None)));assert_eq!(form.field_colours[2],None);assert_eq!(form.field_colours[3],Some(([128,0,128],None)));
    runner.action(action_id("form-0"));let metrics=kobo_ui::CLARA_BW_METRICS;
    let screen=runner.app().screen(&metrics).build();let mut surface=kobo_ui::Surface::new(metrics.width as usize,metrics.height as usize);
    kobo_ui::render_all(&screen,&metrics,&Chrome::measuring(true),&(),&mut surface,None);
    for rgb in [[255,0,0],[0,0,255],[128,0,128]]{assert!((0..surface.pixels.len()).any(|i|surface.rgb_at(i)==Some(rgb)));}
    runner.action(action_id("field-1"));assert!(matches!(forms::forms(&runner.app().loaded.as_ref().unwrap().document.blocks)[0].fields[1],Field::Checkbox{checked:true,..}));
    if std::env::var_os("COBALT_PAGE_EVIDENCE").is_some(){
        use std::io::Write;let mut file=std::fs::File::create("/tmp/native-control-rgb.ppm").unwrap();write!(file,"P6\n{} {}\n255\n",metrics.width,metrics.height).unwrap();
        for i in 0..surface.pixels.len(){file.write_all(&surface.rgb_at(i).unwrap()).unwrap();}
    }
}

#[test]
fn late_control_label_colour_keeps_edit_and_refreshes_source_style(){
    let mut runner=runner(TextScale::Default);
    open_web_page(&mut runner,"<!doctype html><form><label for=q style='color:red'>Search</label><input id=q name=q value=original></form>");
    forms::form_mut(&mut runner.app_mut().loaded.as_mut().unwrap().document.blocks,0).unwrap().fields[0]=Field::Text{name:"q".into(),label:"Search".into(),value:"edited".into(),search:false};
    runner.app_mut().pictures.set_colour(true);let mut context=runner.context();runner.app_mut().refresh_native_colour(&mut context);
    let form=forms::forms(&runner.app().loaded.as_ref().unwrap().document.blocks)[0];assert_eq!(form.field_colours[0],Some(([255,0,0],None)));assert!(matches!(&form.fields[0],Field::Text{value,..} if value=="edited"));
}

#[test]
fn native_coloured_heading_quote_table_apply_inline_font_styles(){
    let mut rendered=Vec::new();
    for styled in [false,true]{
        let mut runner=runner(TextScale::Default);runner.app_mut().pictures.set_colour(true);
        let inner=if styled{"<strong><em>Styled words</em></strong>"}else{"Styled words"};
        let html=format!("<!doctype html><h2 style='color:red'>{inner}</h2><blockquote><p style='color:blue'>{inner}</p></blockquote><table><tr><th>Title</th></tr><tr><td style='color:green'>{inner}</td></tr></table><form><input name=q></form>");
        open_web_page(&mut runner,&html);let metrics=kobo_ui::CLARA_BW_METRICS;let screen=runner.app().screen(&metrics).build();let mut surface=kobo_ui::Surface::new(metrics.width as usize,metrics.height as usize);
        kobo_ui::render_all(&screen,&metrics,&Chrome::measuring(true),&(),&mut surface,None);
        let census=|rgb|(0..surface.pixels.len()).filter(|&i|surface.rgb_at(i)==Some(rgb)).count();
        rendered.push([[255,0,0],[0,0,255],[0,128,0]].map(census));
        if styled&&std::env::var_os("COBALT_PAGE_EVIDENCE").is_some(){use std::io::Write;let mut file=std::fs::File::create("/tmp/native-block-font-styles.ppm").unwrap();write!(file,"P6\n{} {}\n255\n",metrics.width,metrics.height).unwrap();for i in 0..surface.pixels.len(){file.write_all(&surface.rgb_at(i).unwrap()).unwrap();}}
    }
    for i in 0..3{assert!(rendered[1][i]>rendered[0][i],"bold colour ink must grow: {rendered:?}");}
}

#[test]
fn native_inline_code_colours_and_mono_survive_links_form_and_paint(){
    let mut runner=runner(TextScale::Default);runner.app_mut().pictures.set_colour(true);
    open_web_page(&mut runner,"<!doctype html><p>Plain before <a href='/next'><code style='color:red;background-color:#fff0e0'>ii WWW code</code></a> after.</p><form><input name=q></form>");
    let metrics=kobo_ui::CLARA_BW_METRICS;let screen=runner.app().screen(&metrics).build();
    let layout=screen.layout_with(&metrics,&Chrome::measuring(true));
    let mono=layout.nodes.iter().find(|n|matches!(n.kind,kobo_ui::LayoutKind::MonoRichText(style) if style.foreground==Some([255,0,0]))&&!n.text_lines.is_empty()).expect("source code style carried");
    assert_eq!(mono.text_lines[0],"ii WWW code");
    assert!(layout.nodes.iter().any(|n|matches!(n.kind,kobo_ui::LayoutKind::InlineColourLink(_,rgb) if rgb==[255,0,0])&&n.rect==mono.rect));
    let mut surface=kobo_ui::Surface::new(metrics.width as usize,metrics.height as usize);
    kobo_ui::render_all(&screen,&metrics,&Chrome::measuring(true),&(),&mut surface,None);
    for rgb in [[255,0,0],[255,240,224]]{assert!((0..surface.pixels.len()).any(|i|surface.rgb_at(i)==Some(rgb)));}
    runner.action(action_id("form-0"));assert!(matches!(runner.app().view,View::Form(0,0)));
}

#[test]
fn native_plain_heading_quote_table_preserve_semantic_style_without_rgb(){
    let mut runner=runner(TextScale::Default);
    open_web_page(&mut runner,"<!doctype html><h2><strong><em>Plain heading</em></strong></h2><blockquote><p><strong><em>Plain quote</em></strong></p></blockquote><table><tr><th>Key</th><th>Value</th></tr><tr><td><strong><em>Plain cell</em></strong></td><td>42</td></tr></table><form><input name=q></form>");
    let metrics=kobo_ui::CLARA_BW_METRICS;let screen=runner.app().screen(&metrics).build();let layout=screen.layout_with(&metrics,&Chrome::measuring(true));
    let styles:Vec<_>=layout.nodes.iter().filter_map(|n|match n.kind{kobo_ui::LayoutKind::ColouredHeading(_,s)|kobo_ui::LayoutKind::ColouredQuoteText(_,s)|kobo_ui::LayoutKind::ColouredTableText(_,s) if s.strong&&s.emphasis=>Some(s),_=>None}).collect();
    assert_eq!(styles.len(),3);assert!(styles.iter().all(|s|s.foreground.is_none()&&s.background.is_none()));
    let mut surface=kobo_ui::Surface::new(metrics.width as usize,metrics.height as usize);kobo_ui::render_all(&screen,&metrics,&Chrome::measuring(true),&(),&mut surface,None);
    assert!(!(0..surface.pixels.len()).any(|i|surface.rgb_at(i).is_some_and(|[r,g,b]|r!=g||g!=b)));
    runner.action(action_id("form-0"));assert!(matches!(runner.app().view,View::Form(0,0)));
    if std::env::var_os("COBALT_PAGE_EVIDENCE").is_some(){use std::io::Write;let mut f=std::fs::File::create("/tmp/native-plain-emphasis.ppm").unwrap();write!(f,"P6\n{} {}\n255\n",metrics.width,metrics.height).unwrap();for i in 0..surface.pixels.len(){f.write_all(&surface.rgb_at(i).unwrap()).unwrap();}}
}

#[test]
fn native_colour_uses_computed_css_font_treatments_and_normal_resets(){
    let mut runner=runner(TextScale::Default);runner.app_mut().pictures.set_colour(true);
    open_web_page(&mut runner,"<!doctype html><style>.heavy{font-weight:700;font-style:italic;color:red}.normal{font-weight:normal;font-style:normal;color:blue}</style><p class=heavy>Inherited <span class=normal>reset</span> again</p><p><strong class=normal>reset tag</strong> <span style='font-weight:600;font-style:oblique;color:green'>numeric</span></p><form><input name=q></form>");
    let pieces=runner.app().current_pieces();let runs:Vec<_>=pieces.iter().filter_map(|p|if let Piece::Prose(runs)=p{Some(runs)}else{None}).flatten().collect();
    assert_eq!(runs.iter().map(|r|r.text.as_str()).collect::<String>(),"Inherited reset againreset tag numeric");
    for word in ["Inherited","again","numeric"]{assert!(runs.iter().any(|r|r.text.contains(word)&&r.strong&&r.emphasis),"{word}: {runs:?}");}
    for word in ["reset","reset tag"]{assert!(runs.iter().any(|r|r.text.contains(word)&&!r.strong&&!r.emphasis&&r.foreground==Some([0,0,255])),"{word}");}
    let metrics=kobo_ui::CLARA_BW_METRICS;let screen=runner.app().screen(&metrics).build();let mut surface=kobo_ui::Surface::new(metrics.width as usize,metrics.height as usize);kobo_ui::render_all(&screen,&metrics,&Chrome::measuring(true),&(),&mut surface,None);
    if std::env::var_os("COBALT_PAGE_EVIDENCE").is_some(){use std::io::Write;let mut f=std::fs::File::create("/tmp/native-css-fonts.ppm").unwrap();write!(f,"P6\n{} {}\n255\n",metrics.width,metrics.height).unwrap();for i in 0..surface.pixels.len(){f.write_all(&surface.rgb_at(i).unwrap()).unwrap();}}
}

#[test]
fn native_pre_computed_font_treatments_preserve_raw_utf8_whitespace(){
    let mut runner=runner(TextScale::Default);runner.app_mut().pictures.set_colour(true);
    open_web_page(&mut runner,"<!doctype html><pre style='font-weight:700;font-style:italic;color:red'>\n  café  x\n\n<span style='font-weight:normal;font-style:normal;color:blue'>    reset y</span>\n</pre><form><input name=q></form>");
    let (text,spans)=runner.app().current_pieces().iter().find_map(|p|if let Piece::ColouredPreformatted{text,spans}=p{Some((text,spans))}else{None}).unwrap();
    assert_eq!(text,"  café  x\n\n    reset y");
    assert!(spans.iter().any(|s|s.start==0&&s.presentation.strong&&s.presentation.emphasis&&s.presentation.foreground==Some([255,0,0])));
    assert!(spans.iter().any(|s|!s.presentation.strong&&!s.presentation.emphasis&&s.presentation.foreground==Some([0,0,255])));
    let metrics=kobo_ui::CLARA_BW_METRICS;let screen=runner.app().screen(&metrics).build();let mut surface=kobo_ui::Surface::new(metrics.width as usize,metrics.height as usize);kobo_ui::render_all(&screen,&metrics,&Chrome::measuring(true),&(),&mut surface,None);
    for rgb in [[255,0,0],[0,0,255]]{assert!((0..surface.pixels.len()).any(|i|surface.rgb_at(i)==Some(rgb)));}
    runner.action(action_id("form-0"));assert!(matches!(runner.app().view,View::Form(0,0)));
    if std::env::var_os("COBALT_PAGE_EVIDENCE").is_some(){use std::io::Write;let mut f=std::fs::File::create("/tmp/native-pre-fonts.ppm").unwrap();write!(f,"P6\n{} {}\n255\n",metrics.width,metrics.height).unwrap();for i in 0..surface.pixels.len(){f.write_all(&surface.rgb_at(i).unwrap()).unwrap();}}
}

#[test]
fn painted_links_open_by_touch_at_every_zoom(){
    for colour in [false,true]{for zoom in [75,100,150,200]{
        let mut runner=runner(TextScale::Default);runner.app_mut().pictures.set_colour(colour);
        runner.app_mut().zoom=zoom;
        runner.action(action_id("page-view"));
        runner.action(action_id("edit-address"));
        let metrics=runner.context().metrics();
        let keyboard=runner.app().screen(&metrics).build().layout_with(&metrics,&Chrome::measuring(true));
        let go=keyboard.rect_of_action(action_id("kb.enter")).expect("keyboard has Go");
        assert_eq!(keyboard.hit_test(go.x+go.width/2,go.y+go.height/2),Some(action_id("kb.enter")));
        runner.action(action_id("kb.cancel"));
        let commands=runner.action(action_id("previous-page"));
        let metrics=runner.context().metrics();
        let layout=runner.app().screen(&metrics).build().layout_with(&metrics,&Chrome::measuring(true));
        let region=layout.nodes.iter().find(|n|matches!(n.kind,kobo_ui::LayoutKind::PictureLink(_))).expect("visible link has a touch target");
        let x=region.rect.x+region.rect.width/2;let y=region.rect.y+region.rect.height/2;
        let action=layout.hit_test(x,y).expect("link responds to a finger");
        assert!(runner.app().link_named(action).is_some());
        if zoom==100{save_painted_journey_frame(&runner,&commands,if colour{"touch-colour-start"}else{"touch-bw-start"});}
        let index=runner.app().link_named(action).unwrap();
        let expected=runner.app().loaded.as_ref().unwrap().document.links[index].target.clone();
        runner.action(action);
        assert_eq!(runner.app().loaded.as_ref().unwrap().url,expected);
        let commands=runner.action(action_id("page-view"));
        if zoom==100{save_painted_journey_frame(&runner,&commands,if colour{"touch-colour-destination"}else{"touch-bw-destination"});}
        runner.action(action_id("back"));
        assert_eq!(runner.app().loaded.as_ref().unwrap().url.to_string(),SAMPLES);
    }}
}

#[test]
fn painted_scroll_is_a_real_offset_clamped_to_content_and_keeps_links(){
 let mut runner=runner(TextScale::Default);runner.app_mut().pictures.set_colour(true);
 runner.action(link_to(&runner,"Notes on reading"));runner.action(action_id("page-view"));
 let commands=runner.action(ActionId(page_mode::SCROLL_BASE.0+150));
 let painted=runner.app().loaded.as_ref().unwrap().painted.as_ref().unwrap();
 assert_eq!(painted.top_for_test(),painted.view.viewport_height()/2);
 let top=painted.top_for_test();assert_ne!(top,painted.view.page_top(painted.page));
 save_painted_journey_frame(&runner,&commands,"scroll-half");
 runner.action(action_id("edit-address"));runner.action(ActionId(page_mode::SCROLL_BASE.0+200));
 assert_eq!(runner.app().loaded.as_ref().unwrap().painted.as_ref().unwrap().top_for_test(),top);
 runner.action(action_id("kb.cancel"));
 for _ in 0..20{runner.action(ActionId(page_mode::SCROLL_BASE.0+200));}
 let p=runner.app().loaded.as_ref().unwrap().painted.as_ref().unwrap();assert_eq!(p.top_for_test(),p.view.max_scroll());
 for _ in 0..20{runner.action(ActionId(page_mode::SCROLL_BASE.0));}
 assert_eq!(runner.app().loaded.as_ref().unwrap().painted.as_ref().unwrap().top_for_test(),0);
}

#[test]
fn scroll_place_returns_on_back_and_only_visible_scrolled_links_open(){
 let mut runner=runner(TextScale::Default);runner.app_mut().pictures.set_colour(true);
 let url=Url::parse("https://scroll.invalid/read").unwrap();
 let mut html=String::from("<title>Scroll links</title><p><a href='/top'>Top link</a></p>");
 for _ in 0..15{html.push_str("<p>Readable content with enough lines to scroll through the middle of an article.</p>");}
 html.push_str("<p><a href='/bottom'>Bottom link</a></p>");
 let mut context=runner.context();runner.app_mut().show_document(&mut context,&url,html.as_bytes());runner.action(action_id("page-view"));
 for _ in 0..10{runner.action(ActionId(page_mode::SCROLL_BASE.0+200));}
 let p=runner.app().loaded.as_ref().unwrap().painted.as_ref().unwrap();assert!(p.scroll_position()>0);
 let metrics=runner.context().metrics();let layout=runner.app().screen(&metrics).build().layout_with(&metrics,&Chrome::measuring(true));
 let r=layout.nodes.iter().find(|n|matches!(n.kind,kobo_ui::LayoutKind::PictureLink(_))).unwrap().rect;
 let action=layout.hit_test(r.x+r.width/2,r.y+r.height/2).unwrap();let index=runner.app().link_named(action).unwrap();
 assert_eq!(runner.app().loaded.as_ref().unwrap().document.links[index].target.path(),"/bottom");
 // Real local navigation gives Back a restorable cached sample document.
 let mut runner=self::runner(TextScale::Default);runner.app_mut().pictures.set_colour(true);let target=link_to(&runner,"Notes on reading");runner.action(target);runner.action(action_id("page-view"));
 runner.action(ActionId(page_mode::SCROLL_BASE.0+150));let top=runner.app().loaded.as_ref().unwrap().painted.as_ref().unwrap().scroll_position();
 runner.action(action_id("samples"));runner.action(action_id("back"));
 assert_eq!(runner.app().loaded.as_ref().unwrap().painted.as_ref().unwrap().scroll_position(),top);
}

#[test]
fn native_reader_retires_stored_scroll_offsets(){
 for action in ["native-reader"]{
  let mut r=runner(TextScale::Default);r.app_mut().pictures.set_colour(true);
  r.action(link_to(&r,"Notes on reading"));r.action(action_id("page-view"));
  r.action(ActionId(page_mode::SCROLL_BASE.0+150));
  let key=r.app().loaded.as_ref().unwrap().url.to_string();
  assert!(r.app().scroll_places.contains_key(&key));
  r.action(action_id(action));
  assert!(!r.app().scroll_places.contains_key(&key),"{action} retained stale offset");
 }
}

#[test]
fn zoom_controls_live_in_a_deliberate_panel_not_the_browsing_row(){
 let mut r=runner(TextScale::Default);r.action(action_id("page-view"));
 let m=r.context().metrics();
 let layout=r.app().screen(&m).build().layout_with(&m,&Chrome::measuring(true));
 assert!(layout.rect_of_action(action_id("browser-zoom")).is_some());
 assert!(!layout.nodes.iter().any(|n|matches!(n.kind,kobo_ui::LayoutKind::Slider(..))));
 r.action(action_id("browser-zoom"));
 let layout=r.app().screen(&m).build().layout_with(&m,&Chrome::measuring(true));
 let slider=layout.nodes.iter().find(|n|matches!(n.kind,kobo_ui::LayoutKind::Slider(..))).expect("zoom panel has a draggable track");
 let a=layout.slider_action(slider.rect.x+slider.rect.width/2,slider.rect.y+slider.rect.height/2).unwrap();
 r.action(a);assert!(r.app().zoom>=136&&r.app().zoom<=138);
 r.action(action_id("zoom-done"));
 r.action(action_id("browser-zoom"));r.action(ActionId::BACK);assert!(!r.app().zoom_panel);
 r.action(action_id("browser-zoom"));r.action(action_id("edit-address"));assert!(!r.app().zoom_panel);r.action(action_id("kb.cancel"));
 let layout=r.app().screen(&m).build().layout_with(&m,&Chrome::measuring(true));
 assert!(!layout.nodes.iter().any(|n|matches!(n.kind,kobo_ui::LayoutKind::Slider(..))));
}

#[test]
fn zoom_panel_controls_fit_portrait_and_landscape_at_all_text_sizes(){
 for (width,height) in [(1072,1448),(1448,1072),(758,1024),(1024,758)]{
  for text_scale in [TextScale::Default,TextScale::Large,TextScale::ExtraLarge]{
   let m=DisplayMetrics{width,height,text_scale,..kobo_ui::CLARA_BW_METRICS};
   let mut r=AppRunner::with_metrics(Browser::default(),m);r.start();r.action(action_id("samples"));r.action(action_id("browser-zoom"));
   let screen=r.app().screen(&m).build();let l=screen.layout_with(&m,&Chrome::measuring(true));
   for a in ["zoom-less","zoom-more","zoom-reset","zoom-done"]{
    let rect=l.rect_of_action(action_id(a)).unwrap_or_else(||panic!("{width}x{height} {text_scale:?}: missing {a}"));
    assert!(rect.y>=0&&rect.y+rect.height<=height,"{width}x{height} {text_scale:?}: {a} offscreen {rect:?}");
   }
   let slider=l.nodes.iter().find(|n|matches!(n.kind,kobo_ui::LayoutKind::Slider(..))).unwrap();assert!(slider.rect.y+slider.rect.height<=height);
  }
 }
}

#[test]
fn runtime_back_closes_inline_keyboard_before_leaving_the_document(){
 let mut r=runner(TextScale::Default);r.action(action_id("edit-address"));
 assert!(r.app().address.is_open());let url=r.app().loaded.as_ref().unwrap().url.clone();
 r.action(ActionId::BACK);assert!(!r.app().address.is_open());assert_eq!(r.app().loaded.as_ref().unwrap().url,url);
}

#[test]
fn pending_navigation_keeps_the_browser_header_and_exposes_stop_in_the_row(){
 let mut r=runner(TextScale::Default);r.action(link_to(&r,"example.com"));let task=pending_task(&r);
 let m=r.context().metrics();let screen=r.app().screen(&m).build();let l=screen.layout_with(&m,&Chrome::measuring(true));
 let address=l.rect_of_action(action_id("edit-address")).unwrap();assert!(address.y<m.top_bar_height());
 let stop=l.rect_of_action(action_id("cancel-load")).unwrap();assert!(stop.y>m.height-m.top_bar_height()*2);
 if std::env::var_os("COBALT_PAGE_EVIDENCE").is_some(){
  use std::io::Write;let mut surface=kobo_ui::Surface::new(m.width as usize,m.height as usize);
  kobo_ui::render_all(&screen,&m,&Chrome::measuring(true),&(),&mut surface,None);
  let mut f=std::fs::File::create("/tmp/browser-loading-chrome.ppm").unwrap();write!(f,"P6\n{} {}\n255\n",m.width,m.height).unwrap();for i in 0..surface.pixels.len(){f.write_all(&surface.rgb_at(i).unwrap()).unwrap();}
 }

 assert!(l.rect_of_action(action_id("browser-close")).is_some());
 let cmds=r.action(action_id("cancel-load"));assert!(cmds.iter().any(|c|matches!(c,kobo_sdk::Command::Cancel(t) if *t==task)));
 assert_eq!(r.app().view,View::Page);assert_eq!(title(&r),"Browse: sample pages");
 r.task_outcome(task,TaskOutcome::Completed(b"<title>Late</title><p>Stale".to_vec()));assert_eq!(title(&r),"Browse: sample pages");
}

#[test]
fn failure_recovery_edits_and_reloads_the_failed_url_not_the_retained_document(){
 let mut r=runner(TextScale::Default);r.action(link_to(&r,"example.com"));let task=pending_task(&r);
 r.task_outcome(task,TaskOutcome::Failed(TaskError::TimedOut));
 let m=r.context().metrics();let l=r.app().screen(&m).build().layout_with(&m,&Chrome::measuring(true));
 assert!(l.rect_of_action(action_id("edit-address")).is_some());
 assert!(l.rect_of_action(action_id("browser-close")).is_some());
 if std::env::var_os("COBALT_PAGE_EVIDENCE").is_some(){
  use std::io::Write;let mut surface=kobo_ui::Surface::new(m.width as usize,m.height as usize);
  kobo_ui::render_all(&r.app().screen(&m).build(),&m,&Chrome::measuring(true),&(),&mut surface,None);
  let mut f=std::fs::File::create("/tmp/browser-recovery-chrome.ppm").unwrap();write!(f,"P6\n{} {}\n255\n",m.width,m.height).unwrap();for i in 0..surface.pixels.len(){f.write_all(&surface.rgb_at(i).unwrap()).unwrap();}
 }
 r.action(action_id("edit-address"));assert_eq!(r.app().address.keyboard().text(),"https://example.com/");r.action(action_id("kb.cancel"));
 let commands=r.action(action_id("reload"));let fetched=fetches(&commands);assert!(fetched.iter().any(|(_,url,_)|url=="https://example.com/"));
 assert!(matches!(r.app().view,View::Loading(_)));assert_eq!(title(&r),"Browse: sample pages");
}

#[test]
fn local_navigation_retires_an_older_network_request(){
 let mut r=runner(TextScale::Default);r.action(link_to(&r,"example.com"));let task=pending_task(&r);
 let commands=r.action(action_id("samples"));
 assert!(commands.iter().any(|c|matches!(c,kobo_sdk::Command::Cancel(t) if *t==task)));
 assert!(r.app().pending.is_none());assert!(!r.app().clock.is_running());
 r.task_outcome(task,TaskOutcome::Completed(b"<title>Wrong late destination</title><p>Stale".to_vec()));
 assert_eq!(title(&r),"Browse: sample pages");
}

#[test]
fn home_without_a_saved_home_retires_pending_navigation(){
 let mut r=runner(TextScale::Default);r.action(link_to(&r,"example.com"));let task=pending_task(&r);
 r.action(action_id("go-home"));assert_eq!(r.app().view,View::Start);assert!(r.app().pending.is_none());
 r.task_outcome(task,TaskOutcome::Completed(b"<title>Wrong late destination</title><p>Stale".to_vec()));assert_eq!(r.app().view,View::Start);
}

#[test]
fn runtime_back_is_offered_to_native_subviews_and_returns_without_leaving_browser(){
 for action in ["navigate","links","sections","bookmarks","recent"]{
  let mut r=runner(TextScale::Default);r.action(action_id("browser-more"));r.action(action_id(action));let m=r.context().metrics();
  assert!(r.app().screen(&m).build().owns_back,"{action} must accept runtime Back");
  r.action(ActionId::BACK);assert_eq!(r.app().view,View::Page,"{action}");
 }
 let mut r=form_runner(TextScale::Default);r.action(action_id("form-0"));r.action(action_id("field-0"));
 assert!(r.app().screen(&r.context().metrics()).build().owns_back);
 r.action(ActionId::BACK);assert!(!r.app().form_entry.is_open());assert!(matches!(r.app().view,View::Form(0,_)));
 r.action(ActionId::BACK);assert_eq!(r.app().view,View::Page);
}

#[test]
fn physical_page_keys_do_not_move_the_document_behind_an_open_panel(){
 for panel in ["browser-zoom","browser-more"]{
  let mut r=runner(TextScale::Default);r.app_mut().pictures.set_colour(true);
  r.action(link_to(&r,"Notes on reading"));r.action(action_id("page-view"));
  let page=r.app().loaded.as_ref().unwrap().painted.as_ref().unwrap().page;
  r.action(action_id(panel));r.page_turn(true);
  assert_eq!(r.app().loaded.as_ref().unwrap().painted.as_ref().unwrap().page,page,"{panel}");
 }
}

#[test]
fn cancelling_the_first_navigation_returns_to_start_not_an_empty_document(){
 let mut r=AppRunner::with_metrics(Browser::default(),kobo_ui::CLARA_BW_METRICS);r.start();
 let url=Url::parse("https://first.invalid/").unwrap();let mut context=r.context();r.app_mut().open(&mut context,&url);
 let task=pending_task(&r);r.action(action_id("cancel-load"));assert_eq!(r.app().view,View::Start);
 r.task_outcome(task,TaskOutcome::Completed(b"<title>Late</title><p>Old".to_vec()));assert_eq!(r.app().view,View::Start);
}

#[test]
fn pending_page_panels_do_not_survive_into_a_failure_screen(){
 for panel in ["browser-zoom","browser-more"]{
  let mut r=runner(TextScale::Default);r.action(link_to(&r,"example.com"));let task=pending_task(&r);
  r.action(action_id(panel));r.task_outcome(task,TaskOutcome::Failed(TaskError::TimedOut));
  assert!(!r.app().zoom_panel&&!r.app().browser_menu,"{panel} survived failure");
  assert!(r.app().screen(&r.context().metrics()).build().overlay.is_none());
 }
}

#[test]
fn zoom_reflows_around_the_source_glyph_instead_of_jumping_to_top(){
 let mut r=runner(TextScale::Default);r.app_mut().pictures.set_colour(true);
 r.action(link_to(&r,"Notes on reading"));r.action(action_id("page-view"));
 r.action(ActionId(page_mode::SCROLL_BASE.0+150));
 let p=r.app().loaded.as_ref().unwrap().painted.as_ref().unwrap();
 assert!(p.scroll_position()>0);
 let anchor=p.view.reading_anchor(p.scroll_position()).unwrap();
 for action in ["zoom-more","zoom-more","zoom-less","zoom-reset"] {
  r.action(action_id(action));
  let l=r.app().loaded.as_ref().unwrap();let p=l.painted.as_ref().unwrap();
  let expected=p.view.anchor_top(anchor).unwrap().min(p.view.max_scroll());
  assert!(p.scroll_position().abs_diff(expected)<=2,"{action}: lost source position");
  assert_eq!(r.app().scroll_places.get(&l.url.to_string()).copied(),Some((r.app().zoom,p.scroll_position())));
 }
}

#[test]
fn zoom_at_document_start_keeps_the_top_margin(){
 let mut r=runner(TextScale::Default);r.app_mut().pictures.set_colour(true);
 r.action(link_to(&r,"Notes on reading"));r.action(action_id("page-view"));
 for action in ["zoom-more","zoom-more","zoom-less","zoom-reset"] {
  r.action(action_id(action));
  assert_eq!(r.app().loaded.as_ref().unwrap().painted.as_ref().unwrap().scroll_position(),0);
 }
}

#[test]
fn display_reflow_keeps_selected_zoom_and_source_reading_place(){
 let mut r=runner(TextScale::Default);r.app_mut().pictures.set_colour(true);
 r.action(link_to(&r,"Notes on reading"));r.action(action_id("page-view"));
 r.action(ActionId(page_mode::SCROLL_BASE.0+150));r.action(action_id("zoom-more"));
 let before=r.app().loaded.as_ref().unwrap().painted.as_ref().unwrap();
 let anchor=before.view.reading_anchor(before.scroll_position()).unwrap();
 let original=r.context().metrics();
 for metrics in [DisplayMetrics{width:original.height,height:original.width,..original},original] {
  let host=AppRunner::with_metrics(Browser::default(),metrics);let context=host.context();
  r.app_mut().validate_css(&context);
  let loaded=r.app().loaded.as_ref().unwrap();let p=loaded.painted.as_ref().unwrap();
  let expected=page_mode::PageMode::build_zoom(loaded.page_source.as_deref().unwrap(),&loaded.title,&metrics,125).unwrap();
  assert_eq!(p.view.screen_device_px(),expected.view.screen_device_px(),"selected zoom silently changed");
  assert_eq!(r.app().zoom,125);
  let top=p.view.anchor_top(anchor).unwrap().min(p.view.max_scroll());
  assert!(p.scroll_position().abs_diff(top)<=2,"rotation lost reading place");
  assert_eq!(r.app().scroll_places.get(&loaded.url.to_string()).copied(),Some((125,p.scroll_position())));
 }
}

#[test]
fn painted_page_buttons_keep_the_reading_place_across_back_and_forward(){
 for physical in [false,true] {
  let mut r=runner(TextScale::Default);r.app_mut().pictures.set_colour(true);
  r.action(link_to(&r,"Notes on reading"));r.action(action_id("page-view"));
  r.action(action_id("zoom-more"));
  if physical {r.page_turn(true);}else{r.action(action_id("next-page"));}
  let p=r.app().loaded.as_ref().unwrap().painted.as_ref().unwrap();
  let top=p.scroll_position();assert!(top>0);assert_eq!(p.page,1);
  let expected=p.view.raster_page_rgb(p.page);
  assert!(!p.is_window_for_test());
  r.action(action_id("samples"));r.action(action_id("back"));
  let p=r.app().loaded.as_ref().unwrap().painted.as_ref().unwrap();
  assert_eq!(r.app().zoom,125);assert_eq!(p.scroll_position(),top,"Back lost painted page {physical}");
  assert!(!p.is_window_for_test(),"restored page changed to continuous window");
  let mut context=r.context();assert!(p.publish(&mut context,true,None));
  assert_eq!(p.view.raster_page_rgb(p.page),expected);
  r.action(action_id("forward"));r.action(action_id("back"));
  assert_eq!(r.app().loaded.as_ref().unwrap().painted.as_ref().unwrap().scroll_position(),top);
  r.action(action_id("previous-page"));
  r.action(action_id("samples"));r.action(action_id("back"));
  assert_eq!(r.app().zoom,125);
  assert_eq!(r.app().loaded.as_ref().unwrap().painted.as_ref().unwrap().scroll_position(),0);
 }
}

#[test]
fn zoom_anchor_is_a_source_byte_not_a_reflowed_line_number(){
 let mut r=runner(TextScale::Default);r.app_mut().pictures.set_colour(true);
 let url=Url::parse("https://anchor.invalid/article").unwrap();
 let mut html=String::from("<title>Source bytes</title><p>");
 for n in 0..160 {write!(&mut html,"word{n} reading position stays with source text. ").unwrap();}
 html.push_str("</p>");let mut context=r.context();r.app_mut().show_document(&mut context,&url,html.as_bytes());
 r.action(action_id("page-view"));r.action(ActionId(page_mode::SCROLL_BASE.0+200));
 let p=r.app().loaded.as_ref().unwrap().painted.as_ref().unwrap();let a=p.reading_anchor().unwrap();
 assert!(a.1>100,"anchor must be a UTF-8 byte, not a line ordinal: {a:?}");
 for action in ["zoom-more","zoom-more","zoom-less","zoom-reset"] {
  r.action(action_id(action));let p=r.app().loaded.as_ref().unwrap().painted.as_ref().unwrap();
  assert_eq!(p.reading_anchor(),Some(a),"repeated zoom drifted its retained source byte");
  assert_eq!(p.scroll_position(),p.view.anchor_top(a).unwrap().min(p.view.max_scroll()));
 }
 r.action(ActionId(page_mode::SCROLL_BASE.0+150));
 assert_ne!(r.app().loaded.as_ref().unwrap().painted.as_ref().unwrap().reading_anchor(),Some(a));
}
