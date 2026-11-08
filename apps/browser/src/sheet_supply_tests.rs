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
    let task = runner
        .app()
        .pending
        .as_ref()
        .expect("a fetch in flight")
        .task;
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

// The CSS stage refuses every page with a <link> as HeadAffectingContent (source_facts counts link|meta|style),
// so no real arrival can reach supply today. The supply machine is therefore tested directly on a real Context;
// the app seam is tested for what it must NOT do.
fn census(html: &[u8]) -> kobo_web_document::style_tree::HostSheetLinks {
    let page_url = Url::parse("https://example.test/dir/page.html").unwrap();
    kobo_web_document::host_stylesheet_links(
        html,
        &page_url,
        &Limits::DEFAULT,
        kobo_web_document::MediaContext::new(600),
    )
}

fn started() -> (
    Context,
    crate::sheet_supply::SheetSupply,
    Vec<(TaskId, String, u32, Vec<Header>)>,
) {
    let runner = rig(true);
    let mut context = runner.context();
    let mut supply = crate::sheet_supply::SheetSupply::default();
    supply.begin(&mut context, &census(&page(TWO)));
    let fetched = spawned(&context.take_commands());
    (context, supply, fetched)
}

fn done(
    supply: &mut crate::sheet_supply::SheetSupply,
    context: &mut Context,
    task: TaskId,
    o: TaskOutcome,
) -> bool {
    supply.on_task(context, task, &o)
}

#[test]
fn spawns_one_bounded_fetch_per_link_in_order() {
    let (_c, supply, fetched) = started();
    assert_eq!(fetched.len(), 2);
    assert_eq!(fetched[0].1, "https://example.test/dir/a.css");
    assert_eq!(fetched[1].1, "https://example.test/b.css?v=1");
    for (_, _, max, headers) in &fetched {
        assert_eq!(*max, 256 * 1024);
        assert_eq!(
            headers,
            &vec![Header::new("Accept", crate::sheet_supply::ACCEPT)]
        );
    }
    assert_eq!(supply.in_flight(), 2);
    assert_eq!(supply.outcome(), None);
}

#[test]
fn out_of_order_completion_still_delivers_link_order() {
    let (mut c, mut supply, fetched) = started();
    assert!(done(
        &mut supply,
        &mut c,
        fetched[1].0,
        TaskOutcome::Completed(b"p{margin:2px}".to_vec())
    ));
    assert_eq!(supply.outcome(), None);
    assert!(done(
        &mut supply,
        &mut c,
        fetched[0].0,
        TaskOutcome::Completed(b"p{margin:1px}".to_vec())
    ));
    let Some(Outcome::Ready(sheets)) = supply.outcome().cloned() else {
        panic!("not ready");
    };
    assert_eq!(
        sheets,
        vec![b"p{margin:1px}".to_vec(), b"p{margin:2px}".to_vec()]
    );
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
    let (mut c, mut supply, fetched) = started();
    assert!(done(
        &mut supply,
        &mut c,
        fetched[0].0,
        TaskOutcome::Failed(TaskError::NotFound)
    ));
    assert_eq!(
        supply.outcome(),
        Some(&Outcome::Failed(Why::Fetch(TaskError::NotFound)))
    );
    assert_eq!(cancels(&c.take_commands()), vec![fetched[1].0]);
    assert_eq!(supply.in_flight(), 0);
    assert!(!done(
        &mut supply,
        &mut c,
        fetched[1].0,
        TaskOutcome::Completed(b"p{}".to_vec())
    ));
    assert_eq!(
        supply.outcome(),
        Some(&Outcome::Failed(Why::Fetch(TaskError::NotFound)))
    );
}

#[test]
fn a_cancelled_sheet_is_a_failure() {
    let (mut c, mut supply, fetched) = started();
    done(&mut supply, &mut c, fetched[0].0, TaskOutcome::Cancelled);
    assert_eq!(supply.outcome(), Some(&Outcome::Failed(Why::Cancelled)));
}

#[test]
fn clear_cancels_every_outstanding_sheet_and_stale_answers_are_ignored() {
    let (mut c, mut supply, fetched) = started();
    supply.clear(&mut c);
    let gone = cancels(&c.take_commands());
    assert!(fetched.iter().all(|f| gone.contains(&f.0)));
    assert_eq!(supply.outcome(), None);
    assert_eq!(supply.in_flight(), 0);
    assert!(!done(
        &mut supply,
        &mut c,
        fetched[0].0,
        TaskOutcome::Completed(b"p{}".to_vec())
    ));
    assert_eq!(supply.outcome(), None);
}

#[test]
fn a_new_begin_replaces_the_previous_supply() {
    let (mut c, mut supply, fetched) = started();
    supply.begin(&mut c, &census(&page("<link rel=stylesheet href=c.css>")));
    let commands = c.take_commands();
    let again = spawned(&commands);
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].1, "https://example.test/dir/c.css");
    assert!(fetched.iter().all(|f| cancels(&commands).contains(&f.0)));
    assert!(!done(
        &mut supply,
        &mut c,
        fetched[0].0,
        TaskOutcome::Completed(b"p{}".to_vec())
    ));
    assert_eq!(supply.outcome(), None);
    done(
        &mut supply,
        &mut c,
        again[0].0,
        TaskOutcome::Completed(b"p{}".to_vec()),
    );
    assert_eq!(
        supply.outcome(),
        Some(&Outcome::Ready(vec![b"p{}".to_vec()]))
    );
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
        let runner = rig(true);
        let mut context = runner.context();
        let mut supply = crate::sheet_supply::SheetSupply::default();
        supply.begin(&mut context, &census(&page(links)));
        assert!(spawned(&context.take_commands()).is_empty(), "{links}");
        assert_eq!(supply.outcome(), Some(&Outcome::Failed(why)), "{links}");
    }
}

#[test]
fn a_page_without_external_sheets_is_ready_with_none() {
    let runner = rig(true);
    let mut context = runner.context();
    let mut supply = crate::sheet_supply::SheetSupply::default();
    supply.begin(
        &mut context,
        &census(&page(
            "<style>p{margin:0}</style><link rel=stylesheet media=print href=x.css>",
        )),
    );
    assert!(spawned(&context.take_commands()).is_empty());
    assert_eq!(supply.outcome(), Some(&Outcome::Ready(Vec::new())));
}

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
    let mut supply = crate::sheet_supply::SheetSupply::default();
    supply.begin(&mut context, &census(&page(TWO)));
    let commands = context.take_commands();
    let started = spawned(&commands);
    assert_eq!(started.len(), 1);
    assert_eq!(cancels(&commands), vec![started[0].0]);
    assert_eq!(supply.outcome(), Some(&Outcome::Failed(Why::NoSlot)));
    assert_eq!(supply.in_flight(), 0);
    let mut again = runner.context();
    assert!(!supply.on_task(
        &mut again,
        started[0].0,
        &TaskOutcome::Completed(Vec::new())
    ));
}

#[test]
fn census_urls_match_what_the_fetch_layer_may_see() {
    let urls = |links: &str| -> Vec<Option<String>> {
        census(&page(links))
            .links
            .iter()
            .map(|u| u.as_ref().map(ToString::to_string))
            .collect()
    };
    for bad in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "ftp://x.test/a.css",
    ] {
        assert_eq!(
            urls(&format!("<link rel=stylesheet href=\"{bad}\">")),
            vec![None],
            "{bad}"
        );
    }
    assert_eq!(
        urls("<base href=\"file:///tmp/\"><link rel=stylesheet href=a.css>"),
        vec![Some("https://example.test/dir/a.css".into())]
    );
    assert_eq!(
        urls("<link rel=stylesheet href=\"https://user:pw@evil.test/a.css\">"),
        vec![None]
    );
}

// ---- The app seam: what must NOT happen. ----

fn real_seam_spawns(enabled: bool, html: &[u8]) -> (usize, Option<Outcome>, bool) {
    let mut runner = rig(enabled);
    open(&mut runner, "https://example.test/dir/page.html");
    let commands = arrive(&mut runner, html);
    let refused = matches!(
        runner.app().css.stage(),
        crate::css_stage::Stage::Refused(crate::css_stage::Refusal::HeadAffectingContent)
    );
    (spawned(&commands).len(), outcome(&runner), refused)
}

/// REGRESSION (found in review of fl74c): a page whose admission is Refused never starts sheet traffic.
/// `<link>` is HeadAffectingContent today, so every stylesheet-link page is in this class.
#[test]
fn a_refused_page_never_starts_sheet_traffic() {
    let (spawns, supply, refused) = real_seam_spawns(true, &page(TWO));
    assert!(
        refused,
        "probe: eligibility refuses a stylesheet-link page as HeadAffectingContent"
    );
    assert_eq!(spawns, 0);
    assert_eq!(supply, None);
}

#[test]
fn production_stage_starts_no_sheet_traffic() {
    let (spawns, supply, _refused) = real_seam_spawns(false, &page(TWO));
    assert_eq!(spawns, 0);
    assert_eq!(supply, None);
}

/// An admitted (eligible) page with no stylesheet link starts no traffic either, and holds no supply state.
#[test]
fn an_eligible_page_without_links_has_no_supply_and_no_traffic() {
    let (spawns, supply, _) = real_seam_spawns(true, &page(""));
    assert_eq!(spawns, 0);
    assert_eq!(supply, Some(Outcome::Ready(Vec::new())));
}
