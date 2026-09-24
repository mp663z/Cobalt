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
    runner.action(link_to(runner, "example.com"));
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
    runner.action(link_to(&runner, "example.com"));
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
    runner.action(link_to(&runner, "example.com"));
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
    runner.action(link_to(&runner, "example.com"));
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
    runner.action(action_id("back"));
    runner.action(link_to(&runner, "example.com"));
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
    runner.action(link_to(&runner, "example.com"));
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
    runner.action(link_to(&runner, "example.com"));
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
    let packed = pictures::pack(3, 2, &[0, 1, 2, 3, 4, 5]);
    assert_eq!(
        pictures::unpack(&packed),
        Some((3, 2, &[0, 1, 2, 3, 4, 5][..]))
    );
    assert_eq!(pictures::unpack(&packed[..packed.len() - 1]), None);
    assert_eq!(pictures::unpack(&pictures::pack(0, 2, &[])), None);
    assert_eq!(pictures::unpack(b"KGR1"), None);
    assert_eq!(pictures::unpack(b"<html>"), None);
}
