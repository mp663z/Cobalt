# browser-kani lane (20261128, v3: no unsafe, no lint changes)
Kani 0.68.0, nightly-2026-08-21, 1 CPU / 2 GB. Run: verification/browser-kani/run.sh [harness...] (cadical, -Z stubbing).

PASS (compiled in kani_checks.rs):
- empty_history_ignores_arbitrary_place_and_rollback (real code, no stubs)
- populated_{return_to_is_ignored_out_of_range, set_position_touches_only_current, back_keeps_cursor_in_range,
  forward_keeps_cursor_in_range, navigate_cursor0, navigate_cursor1}: 2-entry history built from ONE real Url::parse("http://b").
  RELATIVE TO SAFE STUBS: Url::parse's private helpers (clean, parse_authority, normalize_path, split_tail) are identity-like,
  exact only for that literal; Url::same_document and <Url as PartialEq>::eq return an arbitrary bool. Says nothing about URL
  parsing, equality, which Go variant is chosen, or histories larger than 2.
Mutation sensitivity: navigate truncate removed / +2 (cursor0), back step 0, forward step +2, return_to bound <=, set_position place forced None: each harness FAILED on its mutant (reverted).
NOT covered: MAX_HISTORY drain (full_history_UNVERIFIED.rs.txt, OOM/segfault), Index::insert eviction and cache bounds
(cache_stubbed_UNVERIFIED.rs.txt, OOM). Not compiled. Broad Kani gate stays OPEN.
