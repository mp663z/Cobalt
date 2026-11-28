# Cobalt basic browser - dev plan

Status: living document, updated as phases land. Owner: integration branch `agent/browser-basic`.

## Goal and bar

A basic working browser for Cobalt, at least as good as the existing preview path and lightweight. We are building a real browser: real CSS parsing, real box tree, real inline layout with real shaping. No heuristics that fake behavior, no per-site special cases, no JS.

## Current checkpoint, November 29

The integration branch is `agent/browser-basic`. This document describes work;
it does not grant permission for account actions, publication or purchases.

- Browser UI: inline address editing, unified header, painted-link taps,
  settled scrolling, compact zoom panel, Back/Forward and native forms.
- Rendering: bounded external CSS, true mono regular/bold code fonts, CSS
  fractional line-height rounding, collapsed-whitespace and pre newline fixes.
- Font memory: shared regular geometry and bounded lazy bold outlines retain
  complete glyph coverage and real weights. See `vendor/fontdue/COBALT-CHANGES.md`.
- Parser: html5ever 0.40 / string_cache 0.11 integrated. The 13-page frozen
  corpus has unchanged census and byte-identical full composited pixels.
- Current parser checkpoint: web-document 1451 passed / 18 ignored; browser
  275 passed / 5 ignored; preview 47 passed / 1 ignored. These are scoped
  tests, not whole-web or device qualification.

## Done

1. Line breaker rewrite (bug 1): breaks at whitespace and after '-' '/'; all other punctuation attached; over-wide pieces go to a counted UnbreakableWord fallback. Mid-word breaks gone from the corpus.
2. Raster height cap removed (bug 2): 512px clamp gone, blit-based crop tooling reads blitted output only.
3. Unicode + run-identity text engine (bug 3, lane B): per-run color/weight/family identity through shaping and paint. Real link colors and bold runs in output; unicode-fold, color-flatten and font-size-fallback counters removed.
4. Shaped-replay advance-order fix (ec5d2750): place_runs_shaped replays advance() calls in exact request order; pinned by positive controls.
5. Lane A hygiene: portable harness CLI (basic-page --page --output), corpus pins, residue cleanup, canvas fill.

## Current order of work

1. Keep standalone workspaces buildable against the vendored font dependency.
2. Review and integrate bounded external-image loading with network/security
   tests when the implementation is ready.
3. Add the pending CSS/parser fuzz targets and inspect any failures.
4. Keep release measurements, docs, fmt/clippy and full-suite checks current.

POST stays review-only. Physical-device input, display, memory and latency
remain unverified. Complete HTML/CSS compatibility is not finished. Upstream
port/adoption is separate work, not part of the current integration.

## Verification ladder (every step)
1. Focused fixture fails before, passes after.
2. kobo-web-document full suite, untruncated output, zero failed binaries.
3. kobo-text suite; cobalt-text-preview suite incl. corpus pins.
4. Corpus re-bless if any census category moved; diffs inspected.
5. Fresh render + blit + crop of affected frozen pages; census line quoted with the crop.
6. Commit body records what, why, and any standing-order clearance used; push; report hash.

## Open risks

- Large test executables can exhaust local build storage. Preserve results,
  then remove only regenerable build artifacts before rerunning.
- Host and qemu RSS are not device RSS. No physical-device route is available.
- Broad Miri/Kani completion is not established by the scoped passing checks.
- External images, SVG and unsupported layout remain visible compatibility gaps.
- Browser clippy completes with existing warnings; it is not warning-clean.
