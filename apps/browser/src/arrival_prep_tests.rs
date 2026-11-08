//! 116: one actual arrival -> Stage-owned real prep artifact (design v3,
//! synthetic profile revised to 600x800 after the 512x384 actual-Screen
//! validation failure - preserved as regression evidence below).
//! PREREQUISITE FIRST: the semantic UI must work at the synthetic
//! metrics before any seam proof runs there. If this fails, the
//! positive proof stops - no silent metrics, caps or UI waiver.

use super::*;
use crate::css_stage::Stage;
use kobo_sdk::AppRunner;
use kobo_ui::TextScale;

/// Explicit synthetic whole metrics, HOST not device: 600x800 = 480,000
/// px <= `MAX_PIXELS` (1,048,576), 1,920,000 RGBA bytes. No cap raised,
/// no downscale, no silent substitution.
///
/// REVISED from the v3-original 512x384: at 512x384 the ACTUAL normal
/// Screen constructs but its own layout diagnostics carry a
/// `ContentOverflow` error (at 384px the reader chrome leaves too little
/// reading height), so the v3 prerequisite fails there. The failure is
/// preserved as regression evidence in
/// `the_earlier_512x384_profile_is_known_bad_and_preserved`. The 600x800
/// revision was independently confirmed by the reviewing worker on the
/// hash-verified 8ff9b421 base: 512x384 fails, 600x800 and CLARA clean.
pub(crate) const SYNTHETIC_METRICS: DisplayMetrics = DisplayMetrics {
    width: 600,
    height: 800,
    pixels_per_inch: 300,
    text_scale: TextScale::Default,
};

/// ONE minimal no-CSS ASCII fixture: a fixture restriction to prove the
/// real engine with engine defaults, not admission policy.
const FIXTURE: &[u8] = b"<!DOCTYPE html><html><head><title>Arrival</title></head><body><p>Real arrival text 0123</p></body></html>";

fn runner(metrics: DisplayMetrics) -> AppRunner<Browser> {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
    let mut runner = AppRunner::with_metrics(Browser::default(), metrics);
    runner.start();
    runner
}

/// Completes the in-flight fetch with `html`; the page lands through the
/// real display and document-arrival path.
fn arrive(runner: &mut AppRunner<Browser>, html: &[u8]) {
    let task = runner
        .app()
        .pending
        .as_ref()
        .expect("a fetch in flight")
        .task;
    runner.task_outcome(task, TaskOutcome::Completed(html.to_vec()));
}

fn go(runner: &mut AppRunner<Browser>, address: &str) {
    let url = Url::parse(address).expect("url");
    let mut context = runner.context();
    runner.app_mut().open(&mut context, &url);
}

/// PREREQUISITE (design v3 section 2): the semantic reader - the ACTUAL
/// normal Screen, constructed AND validated at the synthetic profile -
/// works with the seam untouched (production state). If this
/// fails, the positive proof STOPS: no metrics, caps or UI waiver.
#[test]
fn semantic_ui_works_at_the_synthetic_size() {
    let mut runner = runner(SYNTHETIC_METRICS);
    go(&mut runner, "https://example.test/arrival");
    arrive(&mut runner, FIXTURE);
    let metrics = runner.context().metrics();
    assert_eq!(
        metrics, SYNTHETIC_METRICS,
        "the rig runs the synthetic profile"
    );
    let app = runner.app();
    assert!(
        matches!(app.view, View::Page),
        "the semantic reader must show the page, found {:?}",
        app.view
    );
    let loaded = app.loaded.as_ref().expect("loaded");
    assert!(
        !loaded.paginator.pages().is_empty(),
        "paginator produced pages"
    );
    // The ACTUAL normal Screen, built by the app's real screen path, must
    // CONSTRUCT and VALIDATE at the synthetic profile; failure here stops
    // the positive proof.
    let screen = app
        .screen(&metrics)
        .build_checked()
        .expect("the actual normal Screen must construct and validate at the synthetic profile");
    let issues = screen
        .diagnostics(&metrics, &kobo_ui::Chrome::measuring(true))
        .issues;
    assert!(
        issues.is_empty(),
        "the actual Screen validates clean at the synthetic panel: {issues:?}"
    );
    let shown = format!("{screen:?}");
    assert!(
        shown.contains("Real arrival text 0123"),
        "the actual screen carries the fixture text: {shown}"
    );
    assert!(
        matches!(
            app.css.stage(),
            Stage::Disabled | Stage::PreliminarySemanticCandidate
        ),
        "production seam prepares and publishes nothing"
    );
}

// ---- 116 v3: producer mode tests (the prerequisite above runs first) ----

use crate::arrival_prep::{self, Binding, HostRefusal};
use crate::css_stage::{Admission, CssStage, PrepOutcome, Refusal};
use kobo_web_document::css_text_page::PageError;
use kobo_web_document::display_list::{MAX_COMMANDS, MAX_PIXELS};

/// Same length as `FIXTURE`, different text: association proof must tell
/// the two arrivals apart.
const FIXTURE_ALT: &[u8] = b"<!DOCTYPE html><html><head><title>Arrival</title></head><body><p>Real arrival text 4567</p></body></html>";

/// The ACTUAL normal Screen of `runner`, constructed AND validated by
/// the app's real screen path at the rig's metrics. Panics on any
/// construction failure or validation issue - the v3 prerequisite,
/// asserted in every positive rig.
fn assert_actual_screen_validates(runner: &AppRunner<Browser>) {
    let metrics = runner.context().metrics();
    let screen = runner
        .app()
        .screen(&metrics)
        .build_checked()
        .expect("the actual normal Screen must construct at the synthetic profile");
    let issues = screen
        .diagnostics(&metrics, &kobo_ui::Chrome::measuring(true))
        .issues;
    assert!(
        issues.is_empty(),
        "the actual normal Screen must validate clean at the synthetic profile: {issues:?}"
    );
}

/// Producer mode: the boundary's real producer runs through the real
/// arrival path at the synthetic metrics.
///
/// SHARED PREREQUISITE in EVERY positive rig (test order is never
/// relied on): with the seam still DISABLED - production state - the
/// FIXTURE itself arrives through the real display/document path and
/// its ACTUAL normal Screen is constructed AND validated at the
/// synthetic metrics. Only then is the producer enabled (a fresh
/// `CssStage`, so the stage is `Disabled` at handoff) and each test
/// navigates FRESH for its own scenario.
fn producer_runner() -> AppRunner<Browser> {
    let mut runner = runner(SYNTHETIC_METRICS);
    go(&mut runner, "https://example.test/prerequisite");
    arrive(&mut runner, FIXTURE);
    assert_actual_screen_validates(&runner);
    runner.app_mut().css = CssStage::enable_for_test();
    runner
}

/// Producer proof: ONE actual arrival yields `Stage::Ready` with the
/// admission's generation; the borrowed payload shows real ink, real
/// commands and a session SNAPSHOT with calls and generated bytes and no
/// error; the witness is alive while `Ready`.
#[test]
fn an_actual_arrival_yields_a_stage_owned_real_artifact() {
    let mut runner = producer_runner();
    go(&mut runner, "https://example.test/arrival");
    arrive(&mut runner, FIXTURE);
    let app = runner.app();
    let generation = match app.css.stage() {
        Stage::Ready { generation, .. } => *generation,
        other => panic!("expected Ready with the real payload, got {other:?}"),
    };
    assert_eq!(generation, 1, "the admission's generation");
    let payload = app.css.ready_payload().expect("Ready carries the payload");
    let d = payload.diagnostics();
    assert_eq!((d.viewport_width, d.viewport_height), (600, 800));
    assert_eq!(d.fingerprint_metrics, SYNTHETIC_METRICS);
    assert_eq!(d.binding, Binding::Unestablished);
    assert!(d.commands > 0 && d.commands <= MAX_COMMANDS);
    assert_eq!(payload.list().commands().len(), d.commands);
    assert_eq!(payload.list().glyph_bytes(), d.glyph_bytes);
    // The retained style-run diagnostics are the run's own fixed record.
    assert_eq!(d.style.input_bytes, FIXTURE.len());
    assert!(d.style.dom_nodes > 0);
    assert!(d.ink > 50, "real glyph ink landed, found {}", d.ink);
    let snap = payload.snapshot();
    assert!(snap.calls > 0 && snap.generated_mask_bytes > 0);
    assert_eq!(snap.first_error, None);
    assert!(
        payload.witness().upgrade().is_some(),
        "witness alive while Ready"
    );
    // The semantic reader still carries the page.
    let loaded = app.loaded.as_ref().expect("loaded");
    assert!(!loaded.paginator.pages().is_empty());
}

/// Release: a fresh second arrival releases the first payload (`Weak`
/// dead, drop logged) and owns its own; `clear` releases; a direct stale
/// `finish` drops its outcome and payload at once.
#[test]
fn replacement_clear_and_stale_finish_release_the_payload() {
    let mut runner = producer_runner();
    go(&mut runner, "https://example.test/one");
    arrive(&mut runner, FIXTURE);
    let first_witness = runner.app().css.ready_payload().expect("payload").witness();
    assert!(first_witness.upgrade().is_some());
    go(&mut runner, "https://example.test/two");
    arrive(&mut runner, FIXTURE_ALT);
    assert!(
        first_witness.upgrade().is_none(),
        "the first payload was released at replacement"
    );
    {
        let app = runner.app();
        let payload = app.css.ready_payload().expect("replacement payload");
        assert_eq!(payload.diagnostics().generation, 2);
        assert_eq!(
            app.frame_drops.borrow().as_slice(),
            &[1],
            "drop order logged: the first frame was released"
        );
    }
    let second_witness = runner.app().css.ready_payload().expect("payload").witness();
    runner.app_mut().css.clear();
    assert!(
        second_witness.upgrade().is_none(),
        "clear released the payload"
    );
    // A stale finish(): the outcome and its payload are dropped, never held.
    let drops = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let admission = Admission::for_test(99, &SYNTHETIC_METRICS, false);
    let outcome = arrival_prep::prepare_real_frame(FIXTURE, &admission, &drops);
    let stale_witness = match &outcome {
        PrepOutcome::Ready(frame) => frame.payload().expect("payload").witness(),
        other => panic!("the producer paints the fixture, got {other:?}"),
    };
    assert!(stale_witness.upgrade().is_some());
    runner.app_mut().css.finish(99, outcome);
    assert!(
        stale_witness.upgrade().is_none(),
        "the stale outcome's payload was dropped"
    );
}

/// A fixed-size numeric digest over borrowed bytes (FNV-1a): the
/// structural proof compares digests, the payload pixels are NEVER
/// cloned out of their owner.
fn rgba_digest(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Association: a same-length DISTINCT second input is the one the
/// replacement artifact is measured from - never stale first bytes.
/// The first payload is RELEASED at replacement, so the comparison is
/// between fixed numeric digests taken under each borrow; no pixel
/// buffer is cloned.
#[test]
fn a_same_length_distinct_input_yields_its_own_artifact() {
    assert_eq!(FIXTURE.len(), FIXTURE_ALT.len(), "same-length inputs");
    let mut runner = producer_runner();
    go(&mut runner, "https://example.test/one");
    arrive(&mut runner, FIXTURE);
    let first_digest = rgba_digest(runner.app().css.ready_payload().expect("payload").rgba());
    go(&mut runner, "https://example.test/two");
    arrive(&mut runner, FIXTURE_ALT);
    let payload = runner.app().css.ready_payload().expect("second payload");
    assert_eq!(payload.diagnostics().generation, 2);
    assert_eq!(
        rgba_digest(payload.rgba()),
        rgba_digest(payload.rgba()),
        "the digest is stable within one borrow"
    );
    assert_ne!(
        rgba_digest(payload.rgba()),
        first_digest,
        "the replacement is measured from the SECOND input, not stale bytes"
    );
}

/// NEGATIVE, mandatory: the actual CLARA viewport (1072x1448 =
/// 1,552,256 px over `MAX_PIXELS`) is refused with the painter's
/// `InvalidViewport` named, BEFORE any font construction. No cap raise,
/// no downscale, no substitution.
#[test]
fn the_actual_clara_viewport_is_refused_before_fonts() {
    let mut runner = runner(kobo_ui::CLARA_BW_METRICS);
    runner.app_mut().css = CssStage::enable_for_test();
    let metrics = runner.context().metrics();
    let pixels = i64::from(metrics.width) * i64::from(metrics.height);
    assert!(
        pixels > i64::try_from(MAX_PIXELS).expect("the cap fits"),
        "CLARA px exceed the painter cap, which refuses InvalidViewport before fonts"
    );
    go(&mut runner, "https://example.test/arrival");
    arrive(&mut runner, FIXTURE);
    let app = runner.app();
    assert!(matches!(
        app.css.stage(),
        Stage::Refused(Refusal::Preparation)
    ));
    assert_eq!(app.css.host_refusal(), Some(HostRefusal::InvalidViewport));
    assert!(app.css.ready_payload().is_none(), "no payload, no Ready");
    // The semantic reader still carries the page, at real metrics.
    let loaded = app.loaded.as_ref().expect("loaded");
    assert!(!loaded.paginator.pages().is_empty());
}

/// A metrics change after admission retires the artifact:
/// `StaleMetrics`, and the mismatched payload is gone.
#[test]
fn a_metrics_change_retires_the_artifact() {
    let mut runner = producer_runner();
    go(&mut runner, "https://example.test/arrival");
    arrive(&mut runner, FIXTURE);
    let witness = runner.app().css.ready_payload().expect("payload").witness();
    let changed = DisplayMetrics {
        width: 800,
        ..SYNTHETIC_METRICS
    };
    runner.app_mut().css.validate(&changed, false);
    assert!(matches!(
        runner.app().css.stage(),
        Stage::Refused(Refusal::StaleMetrics)
    ));
    assert!(
        witness.upgrade().is_none(),
        "the mismatched payload is gone"
    );
}

/// Never displayed: the same input with the seam disabled vs producer
/// enabled-and-`Ready` leaves the app's normal screen - the semantic
/// reader's view and paginator screen tree - IDENTICAL.
#[test]
fn the_artifact_is_never_displayed() {
    /// The ACTUAL normal Screen, constructed and validated by the app's
    /// real screen path at the rig's metrics.
    fn actual_screen(runner: &AppRunner<Browser>) -> kobo_ui::Screen {
        let metrics = runner.context().metrics();
        runner
            .app()
            .screen(&metrics)
            .build_checked()
            .expect("the actual normal Screen must construct and validate")
    }
    let mut disabled = runner(SYNTHETIC_METRICS);
    go(&mut disabled, "https://example.test/arrival");
    arrive(&mut disabled, FIXTURE);
    let mut enabled = producer_runner();
    go(&mut enabled, "https://example.test/arrival");
    arrive(&mut enabled, FIXTURE);
    assert!(matches!(
        disabled.app().css.stage(),
        Stage::PreliminarySemanticCandidate
    ));
    assert!(matches!(enabled.app().css.stage(), Stage::Ready { .. }));
    assert_eq!(
        actual_screen(&enabled),
        actual_screen(&disabled),
        "the actual screens - the semantic reader's full screen tree - are IDENTICAL"
    );
}

/// Pages that PASS admission but the paint path refuses whole fail the
/// stage with the reason named: NO payload, NO `Ready`, NO partial
/// publication. The session MAY have done font work first; assertions
/// target the stage and payload, not an untouched session.
#[test]
fn paint_refusals_fail_whole_with_the_reason_named() {
    for (html, name) in [
        (
            &b"<!DOCTYPE html><html><head><title>F</title></head><body><p>one</p><p>two</p></body></html>"[..],
            "two sibling blocks",
        ),
        (
            &b"<!DOCTYPE html><html><head><title>B</title></head><body><p>plain <b>bold</b></p></body></html>"[..],
            "bold markup",
        ),
    ] {
        let mut runner = producer_runner();
        go(&mut runner, "https://example.test/fail");
        arrive(&mut runner, html);
        let app = runner.app();
        assert!(
            matches!(app.css.stage(), Stage::Refused(Refusal::Preparation)),
            "{name}: refused, found {:?}",
            app.css.stage()
        );
        assert_eq!(
            app.css.host_refusal(),
            Some(HostRefusal::Paint(PageError::Unsupported)),
            "{name}: the paint refusal is named"
        );
        assert!(app.css.ready_payload().is_none(), "{name}: no payload");
        assert!(
            app.loaded.is_some(),
            "{name}: the semantic reader carries the page"
        );
    }
}

/// Failed fetch and lifecycle transitions never leave an artifact behind.
#[test]
fn failed_fetch_and_lifecycle_clear_the_artifact() {
    // Failed fetch: no arrival, nothing prepared.
    let mut runner = producer_runner();
    go(&mut runner, "https://example.test/missing");
    let task = runner
        .app()
        .pending
        .as_ref()
        .expect("a fetch in flight")
        .task;
    runner.task_outcome(task, TaskOutcome::Failed(TaskError::NotFound));
    assert!(matches!(runner.app().css.stage(), Stage::Disabled));
    assert!(runner.app().css.ready_payload().is_none());
    // Suspend and exit clear a Ready artifact.
    go(&mut runner, "https://example.test/arrival");
    arrive(&mut runner, FIXTURE);
    let witness = runner.app().css.ready_payload().expect("payload").witness();
    runner.suspend();
    assert!(matches!(runner.app().css.stage(), Stage::Disabled));
    assert!(witness.upgrade().is_none(), "suspend released the payload");
    go(&mut runner, "https://example.test/arrival-again");
    arrive(&mut runner, FIXTURE);
    let witness = runner.app().css.ready_payload().expect("payload").witness();
    runner.exit();
    assert!(matches!(runner.app().css.stage(), Stage::Disabled));
    assert!(witness.upgrade().is_none(), "exit released the payload");
}

/// Producer-mode `reprepare` is a NO-OP: fresh arrivals only, never the
/// `NullPreparation`, never bytes measured for a passed moment.
#[test]
fn producer_reprepare_is_a_no_op() {
    let mut runner = producer_runner();
    go(&mut runner, "https://example.test/arrival");
    arrive(&mut runner, FIXTURE);
    let before = runner.app().css.ready_payload().expect("payload").witness();
    runner.app_mut().css.reprepare();
    assert!(matches!(
        runner.app().css.stage(),
        Stage::Ready { generation: 1, .. }
    ));
    assert!(before.upgrade().is_some(), "the same payload is still held");
    assert!(
        runner.app().frame_drops.borrow().is_empty(),
        "nothing was released"
    );
}

/// The HOST diagnostic artifact is regenerated FROM THE READY-OWNED
/// payload during a borrow, labelled. Not SDK, not simulator, not a
/// screen capture. Written only when a directory is named.
#[test]
fn the_ready_owned_payload_yields_a_labelled_host_artifact() {
    let mut runner = producer_runner();
    go(&mut runner, "https://example.test/arrival");
    arrive(&mut runner, FIXTURE);
    let app = runner.app();
    let payload = app.css.ready_payload().expect("payload");
    if let Ok(dir) = std::env::var("NATIVE_FONT_ARTIFACT_DIR") {
        let mut ppm = b"P6\n600 800\n255\n".to_vec();
        for px in payload.rgba().chunks_exact(4) {
            ppm.extend_from_slice(&px[..3]);
        }
        std::fs::write(format!("{dir}/arrival-prep-proof.ppm"), ppm).expect("artifact");
        std::fs::write(
            format!("{dir}/arrival-prep-proof-SOURCE-LICENSE.txt"),
            "116 v3 HOST diagnostic: one actual arrival through the browser app's real\n\
             boundary, painted by the real engine path with the 115 native font session\n\
             (AtkinsonHyperlegible-Regular, OFL 1.1, bytes compiled into kobo-text) at the\n\
             explicit synthetic 600x800@300ppi metrics (revised from 512x384 after its actual-Screen validation failure, preserved as regression evidence). Regenerated from the Ready-owned\n\
             payload during a borrow. Binding::Unestablished - generic plumbing, no\n\
             byte/Document identity proof. Not an SDK handle, not a simulator or device\n\
             capture, not a screen capture, not a shipping claim.\n",
        )
        .expect("label");
    }
    assert!(payload.diagnostics().ink > 50);
}

/// REGRESSION EVIDENCE (116 v3 revision, preserved - never waived or
/// deleted): the earlier synthetic 512x384@300ppi profile is KNOWN BAD
/// for this proof. The ACTUAL normal Screen still CONSTRUCTS there, but
/// its own layout diagnostics carry a `ContentOverflow` ERROR
/// (`hidden_nodes`: 1): at 384px the reader chrome leaves too little
/// reading height for even this one-paragraph fixture. That failure is
/// why the synthetic profile is now 600x800; it is recorded here so it
/// cannot be silently waived, and the same fixture validates clean at
/// the revised profile and at the actual CLARA metrics.
#[test]
fn the_earlier_512x384_profile_is_known_bad_and_preserved() {
    let known_bad = DisplayMetrics {
        width: 512,
        height: 384,
        ..SYNTHETIC_METRICS
    };
    let mut bad_rig = runner(known_bad);
    go(&mut bad_rig, "https://example.test/arrival");
    arrive(&mut bad_rig, FIXTURE);
    let screen = bad_rig
        .app()
        .screen(&bad_rig.context().metrics())
        .build_checked()
        .expect("the 512x384 screen still CONSTRUCTS - it is validation that fails");
    let issues = screen
        .diagnostics(&known_bad, &kobo_ui::Chrome::measuring(true))
        .issues;
    assert!(
        issues.iter().any(|issue| matches!(
            issue.kind,
            kobo_ui::LayoutIssueKind::ContentOverflow { .. }
        ) && issue.severity == kobo_ui::DiagnosticSeverity::Error),
        "the preserved 512x384 failure: a ContentOverflow ERROR is present: {issues:?}"
    );
    // The same arrival validates clean at the revised synthetic profile
    // and at the actual CLARA metrics - the failure is size-specific.
    for metrics in [SYNTHETIC_METRICS, kobo_ui::CLARA_BW_METRICS] {
        let mut rig = runner(metrics);
        go(&mut rig, "https://example.test/arrival");
        arrive(&mut rig, FIXTURE);
        assert_actual_screen_validates(&rig);
    }
}

/// SYNTHETIC (not real-page): the real producer path (native font adapter) paints a
/// link-free, image-free page with `font-size:1.5rem` byte-identically to the same page
/// with the resolved `24px`; a fractional rem (1.4rem = 22.4px) is a named refusal, not a
/// rounded size. Digest comparison only; no pixel buffer is cloned.
#[test]
fn rem_font_size_paints_identically_to_resolved_px_and_fractional_refuses() {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
    let page = |size: &str| {
        format!("<!DOCTYPE html><html><head><title>t</title></head><body><p style='font-size:{size}'>Rem proof text</p></body></html>").into_bytes()
    };
    let digest_of = |bytes: &[u8]| {
        let drops = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let admission = Admission::for_test(1, &SYNTHETIC_METRICS, false);
        match arrival_prep::prepare_real_frame(bytes, &admission, &drops) {
            PrepOutcome::Ready(frame) => {
                let payload = frame.payload().expect("payload");
                if let Ok(dir) = std::env::var("REM_DUMP") {
                    let d = payload.diagnostics();
                    let mut ppm = format!("P6\n{} {}\n255\n", d.viewport_width, d.viewport_height).into_bytes();
                    for px in payload.rgba().chunks_exact(4) {
                        ppm.extend_from_slice(&px[..3]);
                    }
                    let tag = String::from_utf8_lossy(bytes).split("font-size:").nth(1).map(|t| t.split('\'').next().unwrap_or("").to_owned()).unwrap_or_default();
                    std::fs::write(format!("{dir}/app-{tag}.ppm"), ppm).unwrap();
                }
                Some((rgba_digest(payload.rgba()), payload.diagnostics().ink))
            }
            _ => None,
        }
    };
    let rem = digest_of(&page("1.5rem")).expect("rem page paints");
    let px = digest_of(&page("24px")).expect("px page paints");
    assert_eq!(rem, px, "identical pixels");
    assert!(rem.1 > 0, "ink present");
    let other = digest_of(&page("2rem")).expect("2rem paints");
    assert_ne!(other.0, rem.0, "different size, different pixels");
    assert!(digest_of(&page("1.4rem")).is_none(), "fractional rem refuses");
}
