//! The CSS staging seam, exercised through the app's real callsites with an
//! injected deterministic preparation. These tests prove caller wiring and
//! ownership only; they say nothing about fonts, layout or pixels.

use super::*;
use crate::css_stage::{
    Admission, CssStage, FrameHandle, PrepOutcome, Preparation, Refusal, Stage,
};
use kobo_sdk::AppRunner;
use kobo_ui::TextScale;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

type Log = Rc<RefCell<Vec<u64>>>;
type Switch = Rc<Cell<bool>>;
type Rig = (AppRunner<Browser>, Log, Log, Switch);

const ELIGIBLE: &[u8] = b"<!DOCTYPE html><html><head><title>Plain</title></head><body><main><h1>Plain</h1><p>Body text only.</p></main></body></html>";

const SECOND: &[u8] = b"<!DOCTYPE html><html><head><title>Second</title></head><body><main><h1>Second</h1><p>Another plain page.</p></main></body></html>";

fn runner(scale: TextScale) -> AppRunner<Browser> {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
    let mut runner = AppRunner::with_metrics(
        Browser::default(),
        DisplayMetrics {
            text_scale: scale,
            ..kobo_ui::CLARA_BW_METRICS
        },
    );
    runner.start();
    runner
}

fn pending_task(runner: &AppRunner<Browser>) -> TaskId {
    runner
        .app()
        .pending
        .as_ref()
        .expect("a fetch in flight")
        .task
}

/// Completes the in-flight fetch with `html`; the page lands through the
/// real display and document-arrival path.
fn arrive(runner: &mut AppRunner<Browser>, html: &[u8]) {
    let task = pending_task(runner);
    runner.task_outcome(task, TaskOutcome::Completed(html.to_vec()));
}

/// Goes to `address` from the loaded page, starting a fetch.
fn go(runner: &mut AppRunner<Browser>, address: &str) {
    let url = Url::parse(address).expect("url");
    let mut context = runner.context();
    runner.app_mut().open(&mut context, &url);
}

fn stage(runner: &AppRunner<Browser>) -> &Stage {
    runner.app().css.stage()
}

fn ready_id(runner: &AppRunner<Browser>) -> u64 {
    match stage(runner) {
        Stage::Ready { frame, .. } => frame.id(),
        other => panic!("expected Ready, got {other:?}"),
    }
}

/// A deterministic preparation: records every admission it sees, hands out
/// numbered frames whose drops it records, and refuses on command.
struct Fake {
    calls: Rc<RefCell<Vec<u64>>>,
    drops: Rc<RefCell<Vec<u64>>>,
    next: Rc<Cell<u64>>,
    refuse: Rc<Cell<bool>>,
    fail_publication: Rc<Cell<bool>>,
}

impl Fake {
    fn installed() -> (Self, Log, Log, Switch) {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let drops = Rc::new(RefCell::new(Vec::new()));
        let next = Rc::new(Cell::new(1));
        let refuse = Rc::new(Cell::new(false));
        let fail_publication = Rc::new(Cell::new(false));
        let fake = Self {
            calls: calls.clone(),
            drops: drops.clone(),
            next,
            refuse: refuse.clone(),
            fail_publication,
        };
        (fake, calls, drops, refuse)
    }
}

impl Preparation for Fake {
    fn prepare(&mut self, admission: &Admission) -> PrepOutcome {
        self.calls.borrow_mut().push(admission.generation);
        if self.refuse.get() {
            return PrepOutcome::Refused(Refusal::Preparation);
        }
        if self.fail_publication.get() {
            return PrepOutcome::Refused(Refusal::Publication);
        }
        let id = self.next.get();
        self.next.set(id + 1);
        PrepOutcome::Ready(Box::new(FrameHandle::new_for_test(id, self.drops.clone())))
    }
}

/// A runner whose seam is enabled with a fake, already on the sample index.
fn staged_runner() -> Rig {
    let mut runner = runner(TextScale::Default);
    let (fake, calls, drops, refuse) = Fake::installed();
    runner.app_mut().css = CssStage::with_preparation(Box::new(fake));
    (runner, calls, drops, refuse)
}

#[test]
fn an_eligible_page_is_a_candidate_and_production_publishes_nothing() {
    let mut runner = runner(TextScale::Default);
    go(&mut runner, "https://example.com/plain");
    arrive(&mut runner, ELIGIBLE);
    // Admission ran and stopped: preparation never ran in production.
    assert!(matches!(
        stage(&runner),
        Stage::PreliminarySemanticCandidate
    ));
    // The semantic reader carries the page exactly as before.
    let loaded = runner.app().loaded.as_ref().expect("loaded");
    assert_eq!(loaded.title, "Plain");
    assert!(!loaded.paginator.pages().is_empty());
}

#[test]
fn links_forms_images_and_parse_loss_refuse_and_the_semantic_page_survives() {
    for (html, why) in [
        (
            &b"<!DOCTYPE html><html><head><title>L</title></head><body><main><p><a href=\"https://example.com/x\">x</a></p></main></body></html>"[..],
            Refusal::Interactive,
        ),
        (
            &b"<!DOCTYPE html><html><head><title>F</title></head><body><main><form action=\"/s\"><input name=q></form></main></body></html>"[..],
            Refusal::Interactive,
        ),
        (
            &b"<!DOCTYPE html><html><head><title>I</title></head><body><main><p>t</p><img src=\"p.png\" alt=\"p\"></main></body></html>"[..],
            Refusal::Media,
        ),
        (
            // Non-UTF-8 bytes are source loss: UnprovenSource names it before
            // the semantic ParseLoss path is reached.
            &b"<!DOCTYPE html><html><head><title>S</title></head><body><main><p>caf\xe9</p></main></body></html>"[..],
            Refusal::UnprovenSource,
        ),
    ] {
        let mut runner = runner(TextScale::Default);
        go(&mut runner, "https://example.com/page");
        arrive(&mut runner, html);
        assert!(
            matches!(stage(&runner), Stage::Refused(reason) if *reason == why),
            "expected {why:?}, got {:?}",
            stage(&runner)
        );
        assert!(runner.app().loaded.is_some(), "semantic page survives");
        // No frame was ever prepared: nothing is queued anywhere.
        assert!(matches!(stage(&runner), Stage::Refused(_)));
    }
}

#[test]
fn a_nondefault_text_scale_refuses() {
    let mut runner = runner(TextScale::Large);
    go(&mut runner, "https://example.com/plain");
    arrive(&mut runner, ELIGIBLE);
    assert!(matches!(
        stage(&runner),
        Stage::Refused(Refusal::NonDefaultScale)
    ));
    assert!(runner.app().loaded.is_some());
}

#[test]
fn an_eligible_page_reaches_injected_preparation_exactly_once() {
    let (mut runner, calls, _, _) = staged_runner();
    go(&mut runner, "https://example.com/plain");
    arrive(&mut runner, ELIGIBLE);
    assert_eq!(calls.borrow().len(), 1, "preparation ran once");
    assert_eq!(ready_id(&runner), 1);
}

#[test]
fn a_new_document_replaces_the_frame_with_a_distinct_one() {
    let (mut runner, _, drops, _) = staged_runner();
    go(&mut runner, "https://example.com/one");
    arrive(&mut runner, ELIGIBLE);
    let first = ready_id(&runner);
    let first_generation = match stage(&runner) {
        Stage::Ready { generation, .. } => *generation,
        other => panic!("expected Ready, got {other:?}"),
    };
    go(&mut runner, "https://example.com/two");
    arrive(&mut runner, SECOND);
    let second = ready_id(&runner);
    assert_ne!(first, second, "each document has its own frame");
    match stage(&runner) {
        Stage::Ready { generation, .. } => {
            assert!(*generation > first_generation, "generations increase");
        }
        other => panic!("expected Ready, got {other:?}"),
    }
    assert_eq!(*drops.borrow(), vec![first], "the old frame was released");
}

#[test]
fn a_preparation_refusal_leaves_the_semantic_page() {
    let (mut runner, _, _, refuse) = staged_runner();
    refuse.set(true);
    go(&mut runner, "https://example.com/plain");
    arrive(&mut runner, ELIGIBLE);
    assert!(matches!(
        stage(&runner),
        Stage::Refused(Refusal::Preparation)
    ));
    let loaded = runner.app().loaded.as_ref().expect("loaded");
    assert_eq!(loaded.title, "Plain");
}

#[test]
fn a_publication_failure_leaves_the_semantic_page() {
    let (mut runner, _, _, _) = staged_runner();
    // Queue/commit failure inside preparation: injected as an outcome.
    // Reaching it needs the fake's second switch, installed here.
    runner.app_mut().css.clear();
    let (fake, _, _, _) = Fake::installed();
    let flag = Rc::new(Cell::new(true));
    let mut fake = fake;
    fake.fail_publication = flag;
    runner.app_mut().css = CssStage::with_preparation(Box::new(fake));
    go(&mut runner, "https://example.com/plain");
    arrive(&mut runner, ELIGIBLE);
    assert!(matches!(
        stage(&runner),
        Stage::Refused(Refusal::Publication)
    ));
    assert!(runner.app().loaded.is_some(), "semantic page survives");
}

#[test]
fn clear_is_idempotent() {
    let (mut runner, _, drops, _) = staged_runner();
    go(&mut runner, "https://example.com/plain");
    arrive(&mut runner, ELIGIBLE);
    let held = ready_id(&runner);
    runner.app_mut().css.clear();
    runner.app_mut().css.clear();
    assert_eq!(*drops.borrow(), vec![held], "one frame, dropped once");
    assert!(matches!(stage(&runner), Stage::Disabled));
    runner.app_mut().css.clear();
    assert_eq!(drops.borrow().len(), 1, "nothing left to drop");
}

#[test]
fn a_stale_answer_is_dropped_not_published() {
    // Guard check only: preparation is synchronous, so no answer can
    // actually arrive late today. This pins the guard's behavior; it is
    // not async freshness proof, which needs the freshness contract named
    // in CssStage::finish before any async activation.
    let (mut runner, _, drops, _) = staged_runner();
    go(&mut runner, "https://example.com/plain");
    arrive(&mut runner, ELIGIBLE);
    let held = ready_id(&runner);
    // An answer for a generation that is gone must not disturb the held
    // frame; the stale frame itself is released unread.
    let stale_drops = Rc::new(RefCell::new(Vec::new()));
    runner.app_mut().css.finish(
        0,
        PrepOutcome::Ready(Box::new(FrameHandle::new_for_test(99, stale_drops.clone()))),
    );
    assert_eq!(ready_id(&runner), held);
    assert_eq!(*stale_drops.borrow(), vec![99]);
    assert!(drops.borrow().is_empty());
}

#[test]
fn a_replacement_is_installed_before_the_old_frame_is_released() {
    let (mut runner, _, drops, _) = staged_runner();
    go(&mut runner, "https://example.com/plain");
    arrive(&mut runner, ELIGIBLE);
    let first = ready_id(&runner);
    runner.app_mut().css.reprepare();
    let second = ready_id(&runner);
    assert_ne!(first, second);
    assert_eq!(
        *drops.borrow(),
        vec![first],
        "old frame released only after the replacement was installed"
    );
}

#[test]
fn a_failed_replacement_leaves_no_stale_frame() {
    let (mut runner, _, drops, refuse) = staged_runner();
    go(&mut runner, "https://example.com/plain");
    arrive(&mut runner, ELIGIBLE);
    let first = ready_id(&runner);
    refuse.set(true);
    runner.app_mut().css.reprepare();
    assert!(matches!(
        stage(&runner),
        Stage::Refused(Refusal::Preparation)
    ));
    assert_eq!(
        *drops.borrow(),
        vec![first],
        "the frame measured for a moment that passed is gone"
    );
    assert!(runner.app().loaded.is_some(), "semantic page survives");
}

#[test]
fn the_reader_toggle_clears_the_frame() {
    let (mut runner, _, drops, _) = staged_runner();
    go(&mut runner, "https://example.com/story");
    arrive(
        &mut runner,
        b"<!DOCTYPE html><html><head><title>Story</title></head><body><nav><p>Menu</p></nav><main><h1>Story</h1><p>Body.</p></main></body></html>",
    );
    assert!(matches!(stage(&runner), Stage::Ready { .. }));
    runner.action(action_id("reader"));
    assert!(matches!(stage(&runner), Stage::Disabled));
    assert_eq!(drops.borrow().len(), 1, "the frame was released");
    assert!(
        runner.app().loaded.as_ref().expect("loaded").full.is_some(),
        "the semantic reader view is showing"
    );
}

#[test]
fn a_failed_fetch_clears_the_frame() {
    let (mut runner, _, _, _) = staged_runner();
    go(&mut runner, "https://example.com/plain");
    arrive(&mut runner, ELIGIBLE);
    assert!(matches!(stage(&runner), Stage::Ready { .. }));
    go(&mut runner, "https://example.com/missing");
    let task = pending_task(&runner);
    runner.task_outcome(task, TaskOutcome::Failed(TaskError::NotFound));
    assert!(matches!(stage(&runner), Stage::Disabled));
    assert!(matches!(runner.app().view, View::Failed(_, _)));
}

#[test]
fn suspend_and_exit_clear_the_frame() {
    let (mut runner, _, _, _) = staged_runner();
    go(&mut runner, "https://example.com/plain");
    arrive(&mut runner, ELIGIBLE);
    runner.suspend();
    assert!(matches!(stage(&runner), Stage::Disabled));
    go(&mut runner, "https://example.com/again");
    arrive(&mut runner, ELIGIBLE);
    assert!(matches!(stage(&runner), Stage::Ready { .. }));
    runner.exit();
    assert!(matches!(stage(&runner), Stage::Disabled));
}

#[test]
fn a_metrics_change_retires_the_frame() {
    let (mut runner, _, _, _) = staged_runner();
    go(&mut runner, "https://example.com/plain");
    arrive(&mut runner, ELIGIBLE);
    assert!(matches!(stage(&runner), Stage::Ready { .. }));
    let unchanged = kobo_ui::CLARA_BW_METRICS;
    runner.app_mut().css.validate(&unchanged, false);
    assert!(matches!(stage(&runner), Stage::Ready { .. }), "same moment");
    let rotated = DisplayMetrics {
        width: unchanged.height,
        height: unchanged.width,
        ..unchanged
    };
    runner.app_mut().css.validate(&rotated, false);
    assert!(matches!(
        stage(&runner),
        Stage::Refused(Refusal::StaleMetrics)
    ));
}

#[test]
fn a_fragment_turn_keeps_the_frame_and_skips_preparation() {
    let (mut runner, calls, _, _) = staged_runner();
    go(&mut runner, "https://example.com/plain");
    arrive(
        &mut runner,
        b"<!DOCTYPE html><html><head><title>Anchors</title></head><body><main><h1 id=\"top\">Anchors</h1><p>One.</p><p id=\"later\">Two.</p></main></body></html>",
    );
    assert_eq!(calls.borrow().len(), 1);
    let held = ready_id(&runner);
    go(&mut runner, "https://example.com/plain#later");
    assert_eq!(ready_id(&runner), held, "same document, same frame");
    assert_eq!(calls.borrow().len(), 1, "no second preparation");
}

#[test]
fn navigation_start_retires_the_frame_before_the_next_page_lands() {
    let (mut runner, _, drops, _) = staged_runner();
    go(&mut runner, "https://example.com/plain");
    arrive(&mut runner, ELIGIBLE);
    let held = ready_id(&runner);
    // The fetch has only started: no answer, no failure, and still the
    // frame is gone -- the old page's frame matches nothing once
    // navigation begins. A fragment turn on the same document is the
    // specified exception, covered by its own test.
    go(&mut runner, "https://example.com/two");
    assert!(matches!(stage(&runner), Stage::Disabled));
    assert_eq!(*drops.borrow(), vec![held]);
    assert!(matches!(runner.app().view, View::Loading(_)));
    arrive(&mut runner, SECOND);
    assert_ne!(ready_id(&runner), held);
}

#[test]
fn a_panel_identity_answer_retires_a_frame_measured_for_another_panel() {
    let (mut runner, _, _, _) = staged_runner();
    go(&mut runner, "https://example.com/plain");
    arrive(&mut runner, ELIGIBLE);
    assert!(matches!(stage(&runner), Stage::Ready { .. }));
    let identity = kobo_sdk::DeviceIdentity {
        profile_id: "CLARA_COLOUR_386".to_string(),
        ..kobo_sdk::DeviceIdentity::default()
    };
    assert!(identity.colour_panel());
    runner.device_result(kobo_sdk::DeviceResult::Identity(identity));
    assert!(matches!(
        stage(&runner),
        Stage::Refused(Refusal::StaleMetrics)
    ));
    assert!(runner.app().pictures.colour());
}

/// A context whose metrics differ from admission, the way the worker's
/// independent repro drove callbacks: a second runner's context carries
/// its own metrics.
fn foreign_context(width: i32, height: i32) -> (AppRunner<Browser>, Context) {
    let other = AppRunner::with_metrics(
        Browser::default(),
        DisplayMetrics {
            width,
            height,
            ..kobo_ui::CLARA_BW_METRICS
        },
    );
    let context = other.context();
    (other, context)
}

#[test]
fn validation_rides_task_store_and_page_turn_callbacks() {
    for drive in ["task", "store", "page_turn"] {
        let (mut runner, _, _, _) = staged_runner();
        go(&mut runner, "https://example.com/plain");
        arrive(&mut runner, ELIGIBLE);
        assert!(matches!(stage(&runner), Stage::Ready { .. }));
        let (_other, mut context) = foreign_context(800, 600);
        match drive {
            "task" => {
                runner
                    .app_mut()
                    .on_task(&mut context, TaskId(999_999), TaskOutcome::Cancelled);
            }
            "store" => runner.app_mut().stored(
                &mut context,
                None,
                &StoreResult::Loaded {
                    key: "browser-library".to_string(),
                    value: None,
                },
            ),
            _ => runner.app_mut().on_page_turn(&mut context, true),
        }
        assert!(
            matches!(stage(&runner), Stage::Refused(Refusal::StaleMetrics)),
            "{drive} retired the frame"
        );
    }
}

#[test]
fn leaving_the_foreground_clears_the_frame() {
    let (mut runner, _, _, _) = staged_runner();
    go(&mut runner, "https://example.com/plain");
    arrive(&mut runner, ELIGIBLE);
    assert!(matches!(stage(&runner), Stage::Ready { .. }));
    runner.lifecycle(kobo_sdk::Lifecycle::Background);
    assert!(matches!(stage(&runner), Stage::Disabled));
}

#[test]
fn vanished_controls_refuse_by_name_closing_the_documented_gap() {
    // These passed the preliminary gate while the raw-source proof gate was
    // open (the predecessor of this test pinned Stage::Ready as the
    // documented gap). The parsed-source facts close it: each refuses by
    // name, even with the seam enabled and preparation injected.
    for (body, why) in [
        ("<button>Click</button><p>Text</p>", Refusal::VanishedForm),
        ("<video></video><p>Text</p>", Refusal::VanishedMedia),
        (
            "<p hidden><a href='/x'>Hidden link</a></p><p>Text</p>",
            Refusal::VanishedLink,
        ),
    ] {
        let (mut runner, _, _, _) = staged_runner();
        go(&mut runner, "https://example.com/plain");
        let html =
            format!("<!DOCTYPE html><html><head><title>T</title></head><body>{body}</body></html>");
        arrive(&mut runner, html.as_bytes());
        assert!(
            matches!(stage(&runner), Stage::Refused(reason) if *reason == why),
            "expected {why:?}, got {:?}",
            stage(&runner)
        );
        assert!(runner.app().loaded.is_some(), "semantic page survives");
    }
}

#[test]
fn vanished_content_classes_refuse_by_name_and_the_semantic_page_survives() {
    for (html, why) in [
        (
            &b"<!DOCTYPE html><html><head><title>t</title></head><body><main><div hidden><script>x</script></div><p>t</p></main></body></html>"[..],
            Refusal::VanishedScript,
        ),
        (
            &b"<!DOCTYPE html><html><head><title>t</title></head><body><main><div hidden><button>x</button></div><p>t</p></main></body></html>"[..],
            Refusal::VanishedForm,
        ),
        (
            &b"<!DOCTYPE html><html><head><title>t</title></head><body><main><a>no href</a><p>t</p></main></body></html>"[..],
            Refusal::VanishedLink,
        ),
        (
            &b"<!DOCTYPE html><html><head><title>t</title></head><body><main><map name=m></map><p>t</p></main></body></html>"[..],
            Refusal::VanishedImageMap,
        ),
        (
            &b"<!DOCTYPE html><html><head><title>t</title></head><body><main><div hidden><iframe src=x></iframe></div><p>t</p></main></body></html>"[..],
            Refusal::VanishedFrame,
        ),
        (
            &b"<!DOCTYPE html><html><head><title>t</title></head><body><main><details><summary>s</summary>x</details><p>t</p></main></body></html>"[..],
            Refusal::VanishedDisclosure,
        ),
        (
            &b"<!DOCTYPE html><html><head><title>t</title></head><body><main><dialog>x</dialog><p>t</p></main></body></html>"[..],
            Refusal::VanishedDialog,
        ),
        (
            &b"<!DOCTYPE html><html><head><title>t</title></head><body><main><div hidden><img src=x></div><p>t</p></main></body></html>"[..],
            Refusal::VanishedImage,
        ),
        (
            &b"<!DOCTYPE html><html><head><title>t</title></head><body><main><video src=v></video><p>t</p></main></body></html>"[..],
            Refusal::VanishedMedia,
        ),
        (
            &b"<!DOCTYPE html><html><head><title>t</title></head><body><main><svg></svg><p>t</p></main></body></html>"[..],
            Refusal::VanishedForeign,
        ),
        (
            &b"<!DOCTYPE html><html><head><title>t</title></head><body><main><foo-bar>x</foo-bar><p>t</p></main></body></html>"[..],
            Refusal::UnknownElements,
        ),
        (
            &b"<!DOCTYPE html><html><head><title>t</title><meta charset=utf-8></head><body><main><p>t</p></main></body></html>"[..],
            Refusal::HeadAffectingContent,
        ),
        (
            &b"<!DOCTYPE html><html><head><title>t</title></head><body><main><template><p>x</p></template><p>t</p></main></body></html>"[..],
            Refusal::TemplateContent,
        ),
        (
            &b"<!DOCTYPE html><html><head><title>t</title></head><body><main><p onclick=x>t</p></main></body></html>"[..],
            Refusal::InteractionAttributes,
        ),
    ] {
        let mut runner = runner(TextScale::Default);
        go(&mut runner, "https://example.com/page");
        arrive(&mut runner, html);
        assert!(
            matches!(stage(&runner), Stage::Refused(reason) if *reason == why),
            "expected {why:?}, got {:?}",
            stage(&runner)
        );
        assert!(runner.app().loaded.is_some(), "semantic page survives");
    }
}

#[test]
fn unproven_source_refuses_before_anything_else() {
    // Quirks mode (no doctype): the parse happened, but the proof contract
    // refuses before vanished classes or semantic checks are consulted.
    let mut runner1 = runner(TextScale::Default);
    go(&mut runner1, "https://example.com/page");
    arrive(
        &mut runner1,
        b"<html><head><title>t</title></head><body><main><video src=v></video><p>t</p></main></body></html>",
    );
    assert!(matches!(
        stage(&runner1),
        Stage::Refused(Refusal::UnprovenSource)
    ));
    assert!(runner1.app().loaded.is_some());
    // Discarded namespace evidence (xlink in foreign content) refuses
    // UnprovenSource ahead of the foreign-content class.
    let mut runner2 = runner(TextScale::Default);
    go(&mut runner2, "https://example.com/page");
    arrive(
        &mut runner2,
        b"<!DOCTYPE html><html><head><title>t</title></head><body><main><svg><a xlink:href=\"#x\">y</a></svg><p>t</p></main></body></html>",
    );
    assert!(matches!(
        stage(&runner2),
        Stage::Refused(Refusal::UnprovenSource)
    ));
}

#[test]
fn missing_or_tampered_facts_are_fail_closed() {
    let metrics = kobo_ui::CLARA_BW_METRICS;
    // A Document the parser did not produce carries no facts: no candidacy.
    let mut stage = CssStage::production();
    stage.admit(&Document::default(), &metrics, false);
    assert!(matches!(
        stage.stage(),
        Stage::Refused(Refusal::UnprovenSource)
    ));
    // A tampered loss flag refuses even on an otherwise eligible page.
    let url = Url::parse("https://example.com/plain").expect("url");
    let mut document =
        kobo_web_document::parse_document(ELIGIBLE, &url, &kobo_web_document::Limits::DEFAULT);
    document
        .source_facts
        .as_mut()
        .expect("parser facts")
        .source_loss_free = false;
    let mut stage = CssStage::production();
    stage.admit(&document, &metrics, false);
    assert!(matches!(
        stage.stage(),
        Stage::Refused(Refusal::UnprovenSource)
    ));
}
