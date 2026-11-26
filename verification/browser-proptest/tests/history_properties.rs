//! Randomized shrinking checks against the same separate model used by the
//! bounded exhaustive suite. Uses actual browser-core as a path dependency.
#[path = "../../../crates/kobo-browser/tests/history_model.rs"]
mod model;

use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn random_history_traces_match_model(actions in prop::collection::vec(0usize..8, 0..400)) {
        model::check_trace(&actions);
    }

    #[test]
    fn capacity_then_random_navigation_matches_model(
        prefix in 101usize..301,
        suffix in prop::collection::vec(0usize..8, 0..100),
    ) {
        let mut actions: Vec<_> = (0..prefix).map(|i| i % 3).collect();
        actions.extend(suffix);
        model::check_trace(&actions);
    }
}
