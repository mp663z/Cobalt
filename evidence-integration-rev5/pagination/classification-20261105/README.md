# Pagination red classification (UNREVIEWED, read-only, no BLESS, no expectation change)
Test: apps/browser corpus_page_counts_on_every_reader_and_text_size (release-only: ignored under debug_assertions; CI runs `cargo test --workspace` without --release, so CI never runs it).
Findings:
1. Deterministic: fonts are include_bytes bundled; rev5 actual output identical on 3 runs.
2. Passes at the commit that wrote the expectations: 317059ec805c915c8a351a463c57989333956f0d (2026-09-25) -> ok in a fresh worktree (bis log).
3. First failing commit by bisect over the 73 linear commits 317059ec..4dc074c7 (bis.out; monotonicity assumed): 2510c3b60e8fc10b487bbecd8f0ab90b04b555eb "browser: model select, radio, and checkbox controls" (2026-09-25). It changed expected/wikipedia-search.reader but not .pages; at that commit wikipedia-search ExtraLarge libra panel 17 -> 18. Predecessor 2ed5a0aa passes.
4. So the inherited red at 4dc074c7 is stale expectations of upstream origin (feature commits changed paging; the pages file was not re-blessed), not environment. Still not blessed here. Whether each later change is intended is not judged.
5. rev5 differs from 4dc074c7 actual on 4 pages (python-tutorial-intro, rust-blog-post, rust-std-option, windows-1252): all 9 panel lines, counts higher (e.g. rust-std-option Clara Default 27 expected / 32 rev5 actual; base actual still 27). wikipedia-e-reader/search actual are identical between base and rev5. This extra drift is rev5-introduced and is NOT inherited. Earlier wording that the red was entirely pre-existing was incomplete: the inherited part is 4 pages with 1-4 lines each; rev5 adds 4 pages with 9 lines each.
Not done: which rev5 change causes the rust/python growth (not bisected), whether the growth is intended text/CSS layout.
