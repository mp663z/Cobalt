# Cobalt web-platform benchmark

Cloudflare's Kitesurf project [used Web Platform Tests](https://blog.cloudflare.com/kitesurf/) (WPT) for conformance progress and paired real-site Chromium comparisons for practical rendering. Cobalt keeps these separate. This folder measures one **fixed, no-JavaScript CSS background reftest subset**, while `tools/cobalt-parity/score.py` owns five varied frozen actual web pages. A WPT subset rate is not an overall browser standards rate or a page-compatibility rate.

Run from the repository root:

```sh
python3 tools/cobalt-wpt/score.py /tmp/cobalt-wpt-baseline
```

Requires `python3` with Pillow, `google-chrome`, `cargo`, and the `kobo-web-document` host preview. All test and reference HTML bytes live in `fixtures/`, with paths and SHA-256 hashes in `manifest.json`; source revision `7efa3ab5de117cfd01ac23b65a7ab58d43397238` is the same WPT revision already used for the existing HTML/URL tests in this crate. WPT is BSD-3-Clause; see `https://github.com/web-platform-tests/wpt/blob/7efa3ab5de117cfd01ac23b65a7ab58d43397238/LICENSE.md`. One reference image asset is pinned in `support/` at the original relative path and hashed in the manifest. Reference `.xht` inputs are kept as original bytes in a local `.ref.html` file: the relevant HTML markup renders equivalently for these selected Chrome controls, **not** an assertion that Cobalt implements XHTML parsing. Pinning bytes avoids WPT drift and network access during measurement.

The selector is intentionally a small static CSS-background slice (11 tests) covering background propagation and background-clip, not cherry-picked passing tests. There are no scripts in selected pages. At 800x600, Chrome must produce a pixel-identical test/reference pair for the case to enter the denominator; otherwise the case is `invalid` and the runner exits nonzero. For a valid case, Cobalt's preview must emit a full image and it must match Chrome's reference pixel-for-pixel. A missing or refused Cobalt image is **unsupported, not pass**. The denominator stays fixed across milestones; do not shrink the suite when it fails. `scoreboard.json` records each case, Chrome control, Cobalt result, and changed-pixel count only when both render. These first tests contain text and auto-height; the restricted Cobalt preview currently refuses them. No score here claims the shipping reader or a complete CSS engine renders them.

Current baseline on M20 (`f10662c`): **0/11 pass, 0 paired, 11 unsupported**, all 11 Chrome controls valid. The independent five-real-page corpus remains 0/5 paired. Existing WPT-derived HTML tree, URL, entity, and encoding tests are covered by `cargo test -p kobo-web-document --all-targets`, but they count a different parser-focused set and are not added to the CSS rendering numerator.

As the renderer grows, add a pinned WPT area for CSS2 normal flow, inline text, box edges and selectors, noting each predeclared denominator and its unsupported/fail/pass split. Then run the broader WPT harness for script and DOM features once Cobalt has a JS engine and browser automation interface. Acid tests and Speedometer are not meaningful pass-rate gates for this pre-JS engine. Keep the original WPT fixtures intact; don't simplify a failing WPT page into a lab fixture and call it a WPT pass.

On this workspace, `/usr/bin/cargo` may be non-executable; add `$HOME/.cargo/bin` to `PATH` before running the benchmark. A missing executable is a setup error, not a Cobalt unsupported case.
