//! Sheet supply through the real App seam: arrival -> census -> bounded fetches -> ordered bodies.
//! Synthetic pages only. Nothing here enables the stage in production or paints anything.
use super::*;
use crate::css_stage::CssStage;
use crate::sheet_supply::{Outcome, Why};
use kobo_sdk::{AppRunner, Command};
use kobo_ui::TextScale;

const METRICS: DisplayMetrics = DisplayMetrics {
    width: 600,
    height: 800,
    pixels_per_inch: 300,
    text_scale: TextScale::Default,
};

fn page(links: &str) -> Vec<u8> {
    format!("<!doctype html><html><head><title>T</title>{links}</head><body><p>Body text 0123</p></body></html>")
        .into_bytes()
}

fn rig(enabled: bool) -> AppRunner<Browser> {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
    let mut runner = AppRunner::with_metrics(Browser::default(), METRICS);
    runner.start();
    if enabled {
        runner.app_mut().css = CssStage::enable_for_test();
    }
    runner
}

fn open(runner: &mut AppRunner<Browser>, address: &str) -> Vec<Command> {
    let url = Url::parse(address).expect("url");
    let mut context = runner.context();
    runner.app_mut().open(&mut context, &url);
    context.take_commands()
}

fn arrive(runner: &mut AppRunner<Browser>, html: &[u8]) -> Vec<Command> {
    let task = runner.app().pending.as_ref().expect("a fetch in flight").task;
    runner.task_outcome(task, TaskOutcome::Completed(html.to_vec()))
}

/// (task, url, max_bytes, headers, credential-free) for each spawned fetch.
fn spawned(commands: &[Command]) -> Vec<(TaskId, String, u32, Vec<Header>)> {
    commands
        .iter()
        .filter_map(|c| match c {
            Command::Spawn {
                task,
                work:
                    Task::Fetch {
                        url,
                        max_bytes,
                        credential: None,
                        offset: 0,
                        headers,
                    },
            } => Some((*task, url.clone(), *max_bytes, headers.clone())),
            _ => None,
        })
        .collect()
}

fn cancels(commands: &[Command]) -> Vec<TaskId> {
    commands
        .iter()
        .filter_map(|c| match c {
            Command::Cancel(task) => Some(*task),
            _ => None,
        })
        .collect()
}

fn outcome(runner: &AppRunner<Browser>) -> Option<Outcome> {
    runner.app().sheets.outcome().cloned()
}

const TWO: &str = "<link rel=stylesheet href=a.css><link rel=stylesheet href=\"/b.css?v=1#frag\">";

fn started() -> (AppRunner<Browser>, Vec<(TaskId, String, u32, Vec<Header>)>) {
    let mut runner = rig(true);
    open(&mut runner, "https://example.test/dir/page.html");
    let commands = arrive(&mut runner, &page(TWO));
    let fetched = spawned(&commands);
    (runner, fetched)
}

#[test]
fn enabled_stage_spawns_one_bounded_fetch_per_link_in_order() {
    let (runner, fetched) = started();
    assert_eq!(fetched.len(), 2);
    assert_eq!(fetched[0].1, "https://example.test/dir/a.css");
    assert_eq!(fetched[1].1, "https://example.test/b.css?v=1");
    for (_, _, max, headers) in &fetched {
        assert_eq!(*max, 256 * 1024);
        assert_eq!(headers, &vec![Header::new("Accept", crate::sheet_supply::ACCEPT)]);
    }
    assert_eq!(runner.app().sheets.in_flight(), 2);
    assert_eq!(outcome(&runner), None);
}

#[test]
fn out_of_order_completion_still_delivers_link_order() {
    let (mut runner, fetched) = started();
    runner.task_outcome(fetched[1].0, TaskOutcome::Completed(b"p{margin:2px}".to_vec()));
    assert_eq!(outcome(&runner), None);
    runner.task_outcome(fetched[0].0, TaskOutcome::Completed(b"p{margin:1px}".to_vec()));
    let Some(Outcome::Ready(sheets)) = outcome(&runner) else {
        panic!("not ready");
    };
    assert_eq!(sheets, vec![b"p{margin:1px}".to_vec(), b"p{margin:2px}".to_vec()]);
    // The real library accepts exactly this supply.
    let html = page(TWO);
    let tree = kobo_web_document::parse_style_tree_with_media(
        &html,
        &sheets,
        &Limits::DEFAULT,
        kobo_web_document::MediaContext::new(600).unwrap(),
    );
    assert_eq!(tree.host_sheets_claimed, Some(2));
}

#[test]
fn a_failure_settles_and_cancels_the_rest_and_late_arrivals_change_nothing() {
    let (mut runner, fetched) = started();
    let commands = runner.task_outcome(fetched[0].0, TaskOutcome::Failed(TaskError::NotFound));
    assert_eq!(outcome(&runner), Some(Outcome::Failed(Why::Fetch(TaskError::NotFound))));
    assert_eq!(cancels(&commands), vec![fetched[1].0]);
    assert_eq!(runner.app().sheets.in_flight(), 0);
    runner.task_outcome(fetched[1].0, TaskOutcome::Completed(b"p{}".to_vec()));
    assert_eq!(outcome(&runner), Some(Outcome::Failed(Why::Fetch(TaskError::NotFound))));
}

#[test]
fn a_cancelled_sheet_is_a_failure() {
    let (mut runner, fetched) = started();
    runner.task_outcome(fetched[0].0, TaskOutcome::Cancelled);
    assert_eq!(outcome(&runner), Some(Outcome::Failed(Why::Cancelled)));
}

#[test]
fn navigating_away_cancels_every_outstanding_sheet_and_stale_answers_are_ignored() {
    let (mut runner, fetched) = started();
    let commands = open(&mut runner, "https://example.test/next");
    let mut gone = cancels(&commands);
    gone.sort();
    let mut want: Vec<_> = fetched.iter().map(|f| f.0).collect();
    want.sort();
    assert!(want.iter().all(|t| gone.contains(t)), "{gone:?} vs {want:?}");
    assert_eq!(outcome(&runner), None);
    assert_eq!(runner.app().sheets.in_flight(), 0);
    runner.task_outcome(fetched[0].0, TaskOutcome::Completed(b"p{}".to_vec()));
    assert_eq!(outcome(&runner), None);
}

#[test]
fn a_new_document_replaces_the_previous_supply() {
    let (mut runner, fetched) = started();
    open(&mut runner, "https://example.test/two.html");
    let commands = arrive(&mut runner, &page("<link rel=stylesheet href=c.css>"));
    let again = spawned(&commands);
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].1, "https://example.test/c.css");
    runner.task_outcome(fetched[0].0, TaskOutcome::Completed(b"p{}".to_vec()));
    assert_eq!(outcome(&runner), None, "the old page's sheet must not count for the new page");
    runner.task_outcome(again[0].0, TaskOutcome::Completed(b"p{}".to_vec()));
    assert_eq!(outcome(&runner), Some(Outcome::Ready(vec![b"p{}".to_vec()])));
}

#[test]
fn pages_the_parse_is_certain_to_refuse_fetch_nothing() {
    for (links, why) in [
        (
            "<link rel=stylesheet href=a.css><link rel=stylesheet href=b.css><link rel=stylesheet href=c.css>",
            Why::TooManyLinks,
        ),
        (
            "<link rel=stylesheet media=\"(prefers-color-scheme: dark)\" href=d.css>",
            Why::UnknownMedia,
        ),
        ("<link rel=stylesheet>", Why::Unfetchable),
        ("<link rel=stylesheet href=\"http://example.test/plain.css\">", Why::Unfetchable),
    ] {
        let mut runner = rig(true);
        open(&mut runner, "https://example.test/p.html");
        let commands = arrive(&mut runner, &page(links));
        assert!(spawned(&commands).is_empty(), "{links}");
        assert_eq!(outcome(&runner), Some(Outcome::Failed(why)), "{links}");
    }
}

#[test]
fn a_page_without_external_sheets_is_ready_with_none() {
    let mut runner = rig(true);
    open(&mut runner, "https://example.test/p.html");
    let commands = arrive(&mut runner, &page("<style>p{margin:0}</style><link rel=stylesheet media=print href=x.css>"));
    assert!(spawned(&commands).is_empty());
    assert_eq!(outcome(&runner), Some(Outcome::Ready(Vec::new())));
}

#[test]
fn production_stage_starts_no_sheet_traffic() {
    let mut runner = rig(false);
    open(&mut runner, "https://example.test/p.html");
    let commands = arrive(&mut runner, &page(TWO));
    assert!(spawned(&commands).is_empty());
    assert_eq!(outcome(&runner), None);
    assert_eq!(runner.app().sheets.in_flight(), 0);
}

/// Runtime task limit reached mid-supply: the first sheet got a slot, the second did not. The supply settles
/// `NoSlot` and cancels the one it started (no orphan fetch), and holds no waiting state.
#[test]
fn a_partial_spawn_failure_cancels_what_it_started() {
    let runner = rig(true);
    let mut context = runner.context();
    let mut n = 0;
    while context.tasks_in_flight() + 1 < kobo_sdk::MAX_TASKS_IN_FLIGHT {
        n += 1;
        let busy = context.spawn(Task::Fetch {
            url: format!("https://example.test/busy{n}"),
            offset: 0,
            max_bytes: 10,
            credential: None,
            headers: Vec::new(),
        });
        assert!(busy.is_some());
    }
    let _ = context.take_commands();
    let html = page(TWO);
    let page_url = Url::parse("https://example.test/dir/page.html").unwrap();
    let census = kobo_web_document::host_stylesheet_links(&html, &page_url, &Limits::DEFAULT, None);
    let mut supply = crate::sheet_supply::SheetSupply::default();
    supply.begin(&mut context, &census);
    let commands = context.take_commands();
    let started = spawned(&commands);
    assert_eq!(started.len(), 1);
    assert_eq!(cancels(&commands), vec![started[0].0]);
    assert_eq!(supply.outcome(), Some(&Outcome::Failed(Why::NoSlot)));
    assert_eq!(supply.in_flight(), 0);
    // A late answer for the cancelled first sheet is not ours to consume as a result.
    let mut again = runner.context();
    assert!(!supply.on_task(&mut again, started[0].0, &TaskOutcome::Completed(Vec::new())));
}

/// Census/parse divergence guards that matter for what gets fetched.
#[test]
fn census_urls_match_what_the_fetch_layer_may_see() {
    let page_url = Url::parse("https://example.test/dir/page.html").unwrap();
    let urls = |links: &str| -> Vec<Option<String>> {
        kobo_web_document::host_stylesheet_links(&page(links), &page_url, &Limits::DEFAULT, None)
            .links
            .iter()
            .map(|u| u.as_ref().map(ToString::to_string))
            .collect()
    };
    // Non-http schemes are never fetched.
    for bad in ["file:///etc/passwd", "javascript:alert(1)", "ftp://x.test/a.css", "//"] {
        let u = urls(&format!("<link rel=stylesheet href=\"{bad}\">"));
        assert!(u.len() == 1 && (u[0].is_none() || u[0].as_deref().is_some_and(|s| s.starts_with("http"))), "{bad}: {u:?}");
        if bad != "//" {
            assert_eq!(u[0], None, "{bad}");
        }
    }
    // A base that is not http(s) is ignored, not trusted: the page URL stays the base.
    let u = urls("<base href=\"file:///tmp/\"><link rel=stylesheet href=a.css>");
    assert_eq!(u, vec![Some("https://example.test/dir/a.css".into())]);
    // Credentials in a URL are reported as parsed (the fetch sends no credential of its own).
    let u = urls("<link rel=stylesheet href=\"https://user:pw@evil.test/a.css\">");
    assert_eq!(u, vec![None], "userinfo URLs are unfetchable");
}
