//! TEST-ONLY measurement driver for the B0 arrival-observation timing
//! experiment. Ignored by default; run it on demand:
//!
//! ```text
//! cargo test -p kobo-browser arrival_timing -- --ignored --nocapture
//! ```
//!
//! What is measured, exactly: the PARSE-PHASE helper cost of the
//! one-input observation wrapper, not an end-to-end app arrival. The
//! driver calls `observe` directly with `old_loaded_present = false`;
//! no paginator, no `Loaded` replacement, no task, no shelf upload and
//! no old page state exist here. End-to-end and peak claims are out of
//! scope for this table.
//!
//! Methodology, stated so the numbers can be read honestly:
//! - Fixtures are pinned by name, byte length and SHA-256 below the
//!   table, and the fixture count is asserted before measuring. Two
//!   sets, labeled: the REPO fixtures (eight `tests/corpus` pages plus
//!   the eleven non-reference WPT cases in `tools/cobalt-wpt/fixtures`)
//!   and, only when the `FROZEN_CORPUS_DIR` environment variable points
//!   at a directory of HTML files, the PRIVATE frozen corpus (never
//!   committed; labeled private). Absent the directory, the private set
//!   is reported as not provided.
//! - Limits: `Limits::DEFAULT`. Build: `cargo test` debug profile on
//!   the development host. Host numbers only; device-class
//!   extrapolation is unproven. Release-profile and target runs are
//!   not done.
//! - Per repetition, per fixture, paired: baseline = semantic parse
//!   only; observed = the whole `observe` wrapper (semantic parse plus
//!   bounded style run, `sheets = &[]`). Repetition order alternates:
//!   even repetitions run baseline first, odd repetitions observed
//!   first. In both legs the produced value is bound, black-boxed, and
//!   dropped only after the leg's timer is read, so result lifetimes
//!   match and every leg's timer excludes the same drop phase.
//! - Reported per fixture: median/min/max of baseline, of observed
//!   total, and of the SIGNED paired difference (observed total minus
//!   baseline, same repetition; negative values are printed, never
//!   clamped). No cross-fixture sums: a sum of medians is not a suite
//!   latency.
//! - Stage timings from the wrapper's own record (semantic parse,
//!   style run) are printed for information; each stage timer ends
//!   before that stage's result is dropped.
//! - Repetitions run in one shared process (warmup 2, measured 10 per
//!   fixture): allocator and cache state are shared across repetitions
//!   and fixtures. Fresh-process repetition is done by invoking the
//!   test binary repeatedly outside this driver; both are labeled.
//! - Memory: resident set is printed in PAGES from /proc/self/statm
//!   (host page size not assumed). Start/end RSS is a host observation
//!   only -- not peak, not incremental allocation, not allocator live
//!   bytes, not a device budget; no allocation high-water instrument
//!   exists (no `GlobalAlloc`, no external profiler on this host), so
//!   allocation high-water is BLOCKED. Dropping allocations does not
//!   return RSS.

#[cfg(test)]
mod tests {
    use crate::arrival_observation::{observe, ObservationInput};
    use kobo_web_document::{parse_document, parse_style_tree, Limits, Url};
    use std::time::Instant;

    const WARMUP: usize = 2;
    const REPS: usize = 10;
    const EXPECTED_REPO_FIXTURES: usize = 19;

    fn resident_pages() -> Option<u64> {
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        statm.split_whitespace().nth(1)?.parse().ok()
    }

    fn stats(times: &mut [i128]) -> (i128, i128, i128) {
        assert!(!times.is_empty(), "denominator: no repetitions recorded");
        times.sort_unstable();
        (
            times[times.len() / 2],
            *times.first().expect("non-empty"),
            *times.last().expect("non-empty"),
        )
    }

    fn sha256(path: &std::path::Path) -> String {
        std::process::Command::new("sha256sum")
            .arg(path)
            .output()
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .and_then(|line| line.split_whitespace().next().map(str::to_owned))
            .unwrap_or_else(|| "unavailable".to_owned())
    }

    fn collect_html(dir: &str, prefix: &str) -> Vec<(String, std::path::PathBuf, Vec<u8>)> {
        let mut fixtures = Vec::new();
        for entry in std::fs::read_dir(dir).expect("fixture dir") {
            let path = entry.expect("entry").path();
            let name = path
                .file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned();
            if std::path::Path::new(&name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("html"))
                && !name.ends_with(".ref.html")
            {
                fixtures.push((
                    format!("{prefix}/{name}"),
                    path.clone(),
                    std::fs::read(&path).expect("read"),
                ));
            }
        }
        fixtures.sort_by(|a, b| a.0.cmp(&b.0));
        fixtures
    }

    fn micros(elapsed: std::time::Duration) -> i128 {
        i128::try_from(elapsed.as_micros()).unwrap_or(i128::MAX)
    }

    fn print_host_notes(rss_start: Option<u64>, rss_end: Option<u64>) {
        println!(
            "RSS pages: start={rss_start:?} end={rss_end:?} (/proc/self/statm resident pages; host page size not assumed; host observation only, not peak/live bytes/device budget; drop does not return RSS)"
        );
        println!(
            "parse-phase helper cost only (old_loaded_present=false; no paginator/Loaded/task/upload); host debug build; shared-process warmup={WARMUP} reps={REPS} with per-rep order alternation; fresh-process repetition via repeated binary invocations; allocation high-water BLOCKED (no `GlobalAlloc`, no host profiler); release/target unrun"
        );
    }

    #[test]
    #[ignore = "measurement run: prints a table; run with --ignored --nocapture"]
    fn arrival_observation_timing_over_frozen_fixtures() {
        let url = Url::parse("https://example.test/fixture").expect("url");
        let mut repo = collect_html(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/corpus"),
            "repo-corpus",
        );
        repo.extend(collect_html(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../tools/cobalt-wpt/fixtures"
            ),
            "repo-wpt",
        ));
        repo.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            repo.len(),
            EXPECTED_REPO_FIXTURES,
            "denominator: repo fixture set changed"
        );
        let private = std::env::var("FROZEN_CORPUS_DIR")
            .map(|dir| collect_html(&dir, "private-frozen"))
            .unwrap_or_default();
        let rss_start = resident_pages();
        println!(
            "fixture\tbytes\tbase_med_us\tbase_min_us\tbase_max_us\tobs_med_us\tobs_min_us\tobs_max_us\tdiff_med_us\tdiff_min_us\tdiff_max_us\tsem_stage_med_us\tstyle_stage_med_us"
        );
        let measure = |fixtures: &[(String, std::path::PathBuf, Vec<u8>)]| {
            for (name, _path, bytes) in fixtures {
                for _ in 0..WARMUP {
                    std::hint::black_box(parse_document(bytes, &url, &Limits::DEFAULT));
                    std::hint::black_box(parse_style_tree(bytes, &[], &Limits::DEFAULT));
                }
                let mut base = Vec::with_capacity(REPS);
                let mut observed = Vec::with_capacity(REPS);
                let mut diff = Vec::with_capacity(REPS);
                let mut sem_stage = Vec::with_capacity(REPS);
                let mut style_stage = Vec::with_capacity(REPS);
                for rep in 0..REPS {
                    let baseline_first = rep % 2 == 0;
                    let run_base = || {
                        let start = Instant::now();
                        let document = parse_document(bytes, &url, &Limits::DEFAULT);
                        let elapsed = micros(start.elapsed());
                        std::hint::black_box(document);
                        elapsed
                    };
                    let mut run_observed = || {
                        let start = Instant::now();
                        let (document, observation) = observe(
                            bytes,
                            &url,
                            &Limits::DEFAULT,
                            ObservationInput::Markup { len: bytes.len() },
                            false,
                        );
                        let elapsed = micros(start.elapsed());
                        sem_stage.push(micros(observation.semantic_parse));
                        style_stage.push(micros(observation.style_run));
                        std::hint::black_box((document, observation));
                        elapsed
                    };
                    let (base_t, obs_t) = if baseline_first {
                        (run_base(), run_observed())
                    } else {
                        let obs = run_observed();
                        (run_base(), obs)
                    };
                    base.push(base_t);
                    observed.push(obs_t);
                    diff.push(obs_t - base_t);
                }
                let (b_med, b_min, b_max) = stats(&mut base);
                let (o_med, o_min, o_max) = stats(&mut observed);
                let (d_med, d_min, d_max) = stats(&mut diff);
                let (s_med, _, _) = stats(&mut sem_stage);
                let (t_med, _, _) = stats(&mut style_stage);
                println!(
                    "{name}\t{}\t{b_med}\t{b_min}\t{b_max}\t{o_med}\t{o_min}\t{o_max}\t{d_med}\t{d_min}\t{d_max}\t{s_med}\t{t_med}",
                    bytes.len()
                );
            }
        };
        measure(&repo);
        if private.is_empty() {
            println!("private-frozen: NOT PROVIDED (set FROZEN_CORPUS_DIR); repo fixtures only");
        } else {
            println!(
                "private-frozen: {} fixtures (private corpus, never committed)",
                private.len()
            );
            measure(&private);
        }
        println!("fixture manifest (name, bytes, sha256):");
        for (name, path, bytes) in repo.iter().chain(private.iter()) {
            println!("  {name}\t{}\t{}", bytes.len(), sha256(path));
        }
        print_host_notes(rss_start, resident_pages());
    }
}
