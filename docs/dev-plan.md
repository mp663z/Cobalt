# Cobalt basic browser - dev plan

Status: living document, updated as phases land. Owner: integration branch `agent/browser-basic`.

## Goal and bar

A basic working browser for Cobalt, at least as good as the existing preview path and lightweight. We are building a real browser: real CSS parsing, real box tree, real inline layout with real shaping. No heuristics that fake behavior, no per-site special cases, no JS.

## How we work

- Reviewer rules: no site names in code; every fix ships with a fixture that fails before and passes after; no new exact-parity gates or evidence dirs; crops only from blitted output; per step run tests + fmt/clippy on touched files, commit, push, report the hash and crops immediately.
- Three lanes: A (corpus/CI/harness hygiene), B (Unicode text engine, owns paint_text_block replacement), C (Page view). Lanes deliver bundles; integration cherry-picks or reverts, runs the suites, pushes, reports hashes. Lanes never push.
- Frozen corpus: five captured pages, re-blessed via regen_corpus after every census-changing fix, diffs inspected first. Census counters travel with every visual claim.
- Standing orders (owner-approved): dependency changes only with crates already in Cargo.lock at pinned versions; OFL Noto subsets allowed if measured against BUDGETS.md; R6 transforms in the same commit as their gate updates with tests proving them. Everything else (unsafe_code, new crates, allocator changes, public posts) waits for an explicit ask.

## Done

1. Line breaker rewrite (bug 1): breaks at whitespace and after '-' '/'; all other punctuation attached; over-wide pieces go to a counted UnbreakableWord fallback. Mid-word breaks gone from the corpus.
2. Raster height cap removed (bug 2): 512px clamp gone, blit-based crop tooling reads blitted output only.
3. Unicode + run-identity text engine (bug 3, lane B): per-run color/weight/family identity through shaping and paint. Real link colors and bold runs in output; unicode-fold, color-flatten and font-size-fallback counters removed.
4. Shaped-replay advance-order fix (ec5d2750): place_runs_shaped replays advance() calls in exact request order; pinned by positive controls.
5. Lane A hygiene: portable harness CLI (basic-page --page --output), corpus pins, residue cleanup, canvas fill.

## Current order of work

### Bugs 4 + 5 (next)
- State-aware CSS entry: replace the :link/:visited byte folds with a FreshNoHistory state-aware entry point; update the basic_preview caller per lane A's note.
- pt (and remaining absolute units) in the length parser instead of byte-level rewrites.
- Exit: new fixtures fail before/pass after; fold counters stay zero; corpus re-blessed if census moves.

### Phase 0 hygiene (parallel with above)
- Viewport plumbed end to end (harness --width already exists; engine must take it as a parameter everywhere).
- Harness args documented; corpus regen path reproducible from a fresh clone.
- fmt/clippy green on toolchain 1.85.1 for the touched crates (whole-engine freeze is a separately scheduled coordinated step).

### Phase 1: real box kinds
- Tables, images, form controls as real box kinds in the tree (not paint-time special cases).
- Delete the byte rewrites and the sentinel paste path once the box kinds cover them.
- Exit: corpus pages render with table/image/form counters replacing approximation counters; crops verified against the frozen pages.

### Rebase port (after bugs 4+5, before Phase 1 grows)
- Port onto upstream beta 3a8377b6 on NEW branch agent/browser-rebased. Branch root ecdae512 is a parentless snapshot, so this is a port via diff, not a git rebase. agent/browser-basic and bundle backups stay untouched. Push and share the link.

### Retire the exact painter (after Page view is wired)
- Remove the old exact painter by deletion in a separate commit. Parser, cascade and selectors stay.

## Verification ladder (every step)
1. Focused fixture fails before, passes after.
2. kobo-web-document full suite, untruncated output, zero failed binaries.
3. kobo-text suite; cobalt-text-preview suite incl. corpus pins.
4. Corpus re-bless if any census category moved; diffs inspected.
5. Fresh render + blit + crop of affected frozen pages; census line quoted with the crop.
6. Commit body records what, why, and any standing-order clearance used; push; report hash.

## Open risks
- Fresh-clone cold build exceeds the 120s shell cap; verification runs in pieces.
- text-layout-bounded:30 skip class on the frozen encyclopedia page needs a look (Phase 1 candidate).
- Vertical Em margins stay zeroed (resolving them exploded page height 3.5x earlier); root cause open.
