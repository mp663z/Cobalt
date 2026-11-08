//! TEST-ONLY arrival observation: the B0 synchronous borrowed-source
//! timing experiment.
//!
//! Under `cfg(test)` the arrival boundary's semantic parse goes through
//! [`observe`]: one immutable borrow of the arriving bytes produces BOTH
//! actual parses -- the semantic [`Document`] that continues through the
//! normal arrival flow (paginator, `Loaded`, record-only admission, all
//! unchanged) and a bounded style run (`parse_style_tree` with
//! `sheets = &[]`). Each arrival records a small fixed evidence record
//! plus wall-clock stage timings. The observation never changes
//! candidacy: a refused page stays refused, an observed page is not a
//! candidate by virtue of being observed, and `Admission`, `Stage` and
//! `Ready` semantics are untouched. This is not the 107 diagnostics
//! ledger and implements none of its policy.
//!
//! Production (`not(test)`) compiles none of this module: the parse call
//! and admission behavior are exactly what they were.
//!
//! Honesty boundaries. Renderer-identity binding is always
//! [`Binding::Unestablished`]: same-borrow association holds inside
//! `observe` by construction (one borrow, one moment, two consumers) and
//! at the reviewed local callsite, nothing more; sheet swap is out of
//! scope (`sheets = &[]`), async staleness is unexercised. No allocation
//! high-water is available (no `GlobalAlloc` -- root unsafe is forbidden
//! -- and no external profiler exists on this host), so the timing
//! harness reports RSS from `/proc/self/statm` as a HOST observation
//! only: not a device budget, not a live-bytes proof. Wall times are
//! host debug-build numbers unless stated otherwise; device-class
//! extrapolation is unproven.

use kobo_web_document::style_diagnostics::StyleDiagnostics;
use kobo_web_document::{Document, Limits, Url};
use std::time::{Duration, Instant};

/// What the arriving bytes are. Wrapper identity is caller knowledge,
/// not byte knowledge: the display path classifies the response before
/// the bytes reach the arrival boundary, and hands the classification
/// down with them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ObservationInput {
    /// Response markup, parsed directly.
    Markup { len: usize },
    /// A plain-text response the display path wrapped in generated HTML
    /// (`plain_page`). `origin_utf8_valid` records the actual UTF-8
    /// validity of the origin body; `plain_page`'s `from_utf8_lossy`
    /// converted bytes exactly when this is false. The observation sees
    /// the wrapped bytes, never the origin response, so no
    /// origin-response completeness claim exists for this kind.
    PlainWrapped {
        origin_len: usize,
        origin_utf8_valid: bool,
        wrapped_len: usize,
    },
}

/// Renderer-identity binding for an observation. The only state:
/// nothing is established. Same-borrow input association holds inside
/// `observe` by construction; renderer identity, async freshness and
/// sheet identity are unestablished and unexercised.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Binding {
    Unestablished,
}

/// How the bounded style run ended, as reported by the style tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StyleOutcome {
    pub truncated: bool,
    pub unsupported: bool,
    pub quirks: bool,
}

/// One arrival's fixed evidence record: lengths, counts, flags and
/// timings only. Content-free by construction.
#[derive(Clone, Debug)]
pub(crate) struct ArrivalObservation {
    /// Caller-classified input identity.
    pub input: ObservationInput,
    /// The input exceeded the parser's input cap and was clipped.
    pub clipped: bool,
    /// Wall time of the semantic parse (stage 1).
    pub semantic_parse: Duration,
    /// Wall time of the bounded style run (stage 2); sheets always empty.
    pub style_run: Duration,
    /// The previous `Loaded` (old document + paginator) was still
    /// installed while the observation ran. Actual old-state overlap,
    /// recorded at the moment, never computed by subtraction.
    pub old_loaded_present: bool,
    pub binding: Binding,
    /// The fixed existing diagnostic record from the style run.
    pub diagnostics: StyleDiagnostics,
    pub outcome: StyleOutcome,
}

/// Runs both actual parses off the one arrival borrow.
///
/// Phase, pinned: this runs inside `show_document_noted`, after the
/// navigation-start `css.clear()` and inline with the semantic parse,
/// BEFORE the paginator is built, before `Loaded` is replaced and before
/// record-only admission. The previous `Loaded` is still installed while
/// it runs (`old_loaded_present`); the returned `Document` is the one the
/// normal flow installs and admits, so test arrivals observe exactly the
/// document the reader shows.
pub(crate) fn observe(
    bytes: &[u8],
    url: &Url,
    limits: &Limits,
    input: ObservationInput,
    old_loaded_present: bool,
) -> (Document, ArrivalObservation) {
    let semantic_start = Instant::now();
    let document = kobo_web_document::parse_document(bytes, url, limits);
    let semantic_parse = semantic_start.elapsed();
    let style_start = Instant::now();
    let tree = kobo_web_document::parse_style_tree(bytes, &[], limits);
    let style_run = style_start.elapsed();
    let observation = ArrivalObservation {
        input,
        clipped: bytes.len() > limits.max_input_bytes,
        semantic_parse,
        style_run,
        old_loaded_present,
        binding: Binding::Unestablished,
        diagnostics: tree.diagnostics,
        outcome: StyleOutcome {
            truncated: tree.truncated,
            unsupported: tree.unsupported,
            quirks: tree.quirks,
        },
    };
    (document, observation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kobo_web_document::{parse_document, parse_style_tree};

    const URL: &str = "https://example.test/page";

    fn url() -> Url {
        Url::parse(URL).expect("url")
    }

    fn plain_body() -> Vec<u8> {
        b"plain text, caf"
            .iter()
            .copied()
            .chain([0xE9])
            .chain(b" bytes, no markup at all".iter().copied())
            .chain([0x0A])
            .chain(b"second line here".iter().copied())
            .chain([0x0A])
            .collect()
    }

    /// Two equal-length but distinct inputs, each observed separately:
    /// each observation must carry the evidence of its own bytes, and
    /// each returned Document must equal an independent semantic parse of
    /// its own bytes. No mismatch API exists; association is by the one
    /// borrow inside `observe`.
    #[test]
    fn equal_length_distinct_inputs_associate_separately() {
        let a: &[u8] = concat!(
            "<!DOCTYPE html><html><head><style>p{color:red}</style></head>",
            "<body><main><p>aaaa</p></main></body></html>"
        )
        .as_bytes();
        let b: &[u8] = concat!(
            "<!DOCTYPE html><html><head><style>p{color:blue}</style></head>",
            "<body><main><p>bbb</p></main></body></html>"
        )
        .as_bytes();
        assert_eq!(a.len(), b.len());
        assert_ne!(a, b);
        for bytes in [a, b] {
            let (document, observation) = observe(
                bytes,
                &url(),
                &Limits::DEFAULT,
                ObservationInput::Markup { len: bytes.len() },
                false,
            );
            let independent_document = parse_document(bytes, &url(), &Limits::DEFAULT);
            let independent_tree = parse_style_tree(bytes, &[], &Limits::DEFAULT);
            assert_eq!(document.title, independent_document.title);
            assert_eq!(document.visible_text(), independent_document.visible_text());
            assert_eq!(document.source_facts, independent_document.source_facts);
            assert_eq!(observation.diagnostics, independent_tree.diagnostics);
            assert_eq!(observation.outcome.truncated, independent_tree.truncated);
            assert_eq!(
                observation.outcome.unsupported,
                independent_tree.unsupported
            );
            assert_eq!(observation.outcome.quirks, independent_tree.quirks);
            assert_eq!(observation.binding, Binding::Unestablished);
            assert!(!observation.clipped);
            assert!(!observation.old_loaded_present);
        }
    }

    /// Input classification is caller knowledge: the wrapper records the
    /// actual UTF-8 validity of a plain origin and the wrapped length it
    /// was given, never a U+FFFD scan.
    #[test]
    fn plain_wrapped_records_wrapper_identity_and_utf8_validity() {
        let origin = plain_body();
        assert!(std::str::from_utf8(&origin).is_err());
        let wrapped = crate::plain_page(&origin);
        let (_, observation) = observe(
            wrapped.as_bytes(),
            &url(),
            &Limits::DEFAULT,
            ObservationInput::PlainWrapped {
                origin_len: origin.len(),
                origin_utf8_valid: std::str::from_utf8(&origin).is_ok(),
                wrapped_len: wrapped.len(),
            },
            true,
        );
        assert_eq!(
            observation.input,
            ObservationInput::PlainWrapped {
                origin_len: origin.len(),
                origin_utf8_valid: false,
                wrapped_len: wrapped.len(),
            }
        );
        assert!(observation.old_loaded_present);
    }

    /// The real arrival path: arrivals through display are observed, and
    /// the reader installs the wrapper's own parse. Overlap with the
    /// previous page's `Loaded` is recorded from the actual state.
    #[test]
    fn arrivals_through_the_real_path_are_observed_in_phase() {
        use kobo_sdk::{AppRunner, DisplayMetrics, TaskOutcome};

        kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
        let mut runner = AppRunner::with_metrics(
            crate::Browser::default(),
            DisplayMetrics {
                text_scale: kobo_ui::TextScale::default(),
                ..kobo_ui::CLARA_BW_METRICS
            },
        );
        runner.start();
        // A fresh browser may already have shown a home page; measure
        // from the actual state rather than assuming one.
        let mut seen = runner.app().observation_count;
        let mut had_loaded = runner.app().loaded.is_some();

        let styled: Vec<u8> = concat!(
            "<!DOCTYPE html><html><head><title>Styled</title>",
            "<style>p{color:red}</style></head>",
            "<body><main><p>one</p></main></body></html>"
        )
        .as_bytes()
        .to_vec();
        let plain = plain_body();
        for (address, body) in [
            ("https://arrival.test/styled", &styled),
            ("https://arrival.test/plain", &plain),
        ] {
            let url = Url::parse(address).expect("url");
            {
                let mut context = runner.context();
                runner.app_mut().open(&mut context, &url);
            }
            let task = runner
                .app()
                .pending
                .as_ref()
                .expect("a fetch in flight")
                .task;
            runner.task_outcome(task, TaskOutcome::Completed(body.clone()));

            assert_eq!(runner.app().observation_count, seen + 1);
            seen = runner.app().observation_count;
            let observation = runner
                .app()
                .last_observation
                .as_ref()
                .expect("one new observation");
            assert_eq!(observation.binding, Binding::Unestablished);
            assert_eq!(observation.old_loaded_present, had_loaded);
            had_loaded = runner.app().loaded.is_some();
            match (&observation.input, body as &Vec<u8> == &styled) {
                (ObservationInput::Markup { len }, true) => {
                    assert_eq!(*len, styled.len());
                    let expected = parse_style_tree(&styled, &[], &Limits::DEFAULT);
                    assert_eq!(observation.diagnostics, expected.diagnostics);
                }
                (
                    ObservationInput::PlainWrapped {
                        origin_len,
                        origin_utf8_valid,
                        wrapped_len,
                    },
                    false,
                ) => {
                    assert_eq!(*origin_len, plain.len());
                    assert!(!origin_utf8_valid);
                    assert_eq!(*wrapped_len, crate::plain_page(&plain).len());
                }
                (other, _) => panic!("unexpected classification: {other:?}"),
            }
        }
        // The installed document is the wrapper's own semantic parse of
        // the wrapped plain page: the reader shows what was observed.
        let loaded = runner.app().loaded.as_ref().expect("a page");
        let expected = parse_document(
            crate::plain_page(&plain).as_bytes(),
            &Url::parse("https://arrival.test/plain").expect("url"),
            &Limits::DEFAULT,
        );
        assert_eq!(loaded.document.title, expected.title);
        assert_eq!(loaded.document.blocks.len(), expected.blocks.len());
    }

    /// Focused invariants for the observation path: a refused page is
    /// observed yet stays refused exactly as before (observation never
    /// changes candidacy), and swapping to the reader view -- which
    /// clears the stage without re-admission -- records NO observation.
    #[test]
    fn refusal_is_unchanged_and_reader_toggle_records_no_observation() {
        use kobo_sdk::{AppRunner, DisplayMetrics, TaskOutcome};

        kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
        let mut runner = AppRunner::with_metrics(
            crate::Browser::default(),
            DisplayMetrics {
                text_scale: kobo_ui::TextScale::default(),
                ..kobo_ui::CLARA_BW_METRICS
            },
        );
        runner.start();
        let arrive = |runner: &mut AppRunner<crate::Browser>, address: &str, body: &[u8]| {
            let url = Url::parse(address).expect("url");
            {
                let mut context = runner.context();
                runner.app_mut().open(&mut context, &url);
            }
            let task = runner
                .app()
                .pending
                .as_ref()
                .expect("a fetch in flight")
                .task;
            runner.task_outcome(task, TaskOutcome::Completed(body.to_vec()));
        };

        // A page with links is refused Interactive exactly as before; the
        // arrival is still observed (observation precedes candidacy).
        let linked: &[u8] = concat!(
            "<!DOCTYPE html><html><head><title>Linked</title></head>",
            "<body><main><p>text</p><a href=https://example.test/x>x</a></main></body></html>"
        )
        .as_bytes();
        let before = runner.app().observation_count;
        arrive(&mut runner, "https://arrival.test/linked", linked);
        assert_eq!(runner.app().observation_count, before + 1);
        assert!(runner.app().last_observation.is_some());
        assert!(matches!(
            runner.app().css.stage(),
            crate::css_stage::Stage::Refused(crate::css_stage::Refusal::Interactive)
        ));

        // A page with a reader view: toggling to it clears the stage and
        // records no observation (the reader-view document is a distinct
        // document with no arrival and no CSS staging).
        let corpus = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/corpus/wikipedia-e-reader.html"
        ))
        .expect("corpus page");
        arrive(&mut runner, "https://arrival.test/article", &corpus);
        let before = runner.app().observation_count;
        assert!(runner
            .app()
            .loaded
            .as_ref()
            .expect("a page")
            .document
            .has_reader_view());
        {
            let mut context = runner.context();
            runner.app_mut().toggle_reader(&mut context);
        }
        assert_eq!(runner.app().observation_count, before);
        assert!(matches!(
            runner.app().css.stage(),
            crate::css_stage::Stage::Disabled
        ));
    }

    /// Clipping is recorded from the actual input length against the
    /// actual limit, not assumed.
    #[test]
    fn clipping_is_recorded_when_input_exceeds_the_cap() {
        let mut limits = Limits::DEFAULT;
        limits.max_input_bytes = 64;
        let bytes: &[u8] = concat!(
            "<!DOCTYPE html><html><head><title>Long</title></head>",
            "<body><main><p>past the cap</p></main></body></html>"
        )
        .as_bytes();
        assert!(bytes.len() > limits.max_input_bytes);
        let (_, observation) = observe(
            bytes,
            &url(),
            &limits,
            ObservationInput::Markup { len: bytes.len() },
            false,
        );
        assert!(observation.clipped);
    }
}
