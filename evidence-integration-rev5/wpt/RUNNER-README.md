# Cobalt web-platform benchmark

Cloudflare's Kitesurf project [used Web Platform Tests](https://blog.cloudflare.com/kitesurf/) (WPT) for conformance progress and paired real-site Chromium comparisons for practical rendering. Cobalt keeps these separate. This folder measures one **fixed, no-JavaScript CSS background reftest subset**, while `tools/cobalt-parity/score.py` owns five varied frozen actual web pages. A WPT subset rate is not an overall browser standards rate or a page-compatibility rate.

Run from the repository root:

```sh
python3 tools/cobalt-wpt/score.py /tmp/cobalt-wpt-baseline
```

Requires `python3` with Pillow, `google-chrome`, `cargo`, and the `kobo-web-document` host preview. All test and reference HTML bytes live in `fixtures/`, with paths and SHA-256 hashes in `manifest.json`; source revision `7efa3ab5de117cfd01ac23b65a7ab58d43397238` is the same WPT revision already used for the existing HTML/URL tests in this crate. WPT is BSD-3-Clause; see `https://github.com/web-platform-tests/wpt/blob/7efa3ab5de117cfd01ac23b65a7ab58d43397238/LICENSE.md`. One reference image asset is pinned in `support/` at the original relative path and hashed in the manifest. Reference `.xht` inputs are kept as original bytes in a local `.ref.html` file: the relevant HTML markup renders equivalently for these selected Chrome controls, **not** an assertion that Cobalt implements XHTML parsing. Pinning bytes avoids WPT drift and network access during measurement.

The selector is intentionally a small static CSS-background slice (11 tests) covering background propagation and background-clip, not cherry-picked passing tests. There are no scripts in selected pages. At 800x600, Chrome must produce a pixel-identical test/reference pair for the case to enter the denominator; otherwise the case is `invalid` and the runner exits nonzero. For a valid case, Cobalt's preview must emit a full image and it must match Chrome's reference pixel-for-pixel. A missing or refused Cobalt image is **unsupported, not pass**. The denominator stays fixed across milestones; do not shrink the suite when it fails. `scoreboard.json` records each case, Chrome control, Cobalt result, and changed-pixel count only when both render. These first tests contain text and auto-height; the restricted Cobalt preview currently refuses them. No score here claims the shipping reader or a complete CSS engine renders them.

Original baseline on M20 (`f10662c`): 0/11 pass, 0 paired, 11 unsupported, all 11 Chrome controls valid. On `agent/browser` after the border/background-clip/margin work: **2/11 pass, 2 paired, 9 unsupported**, 11 valid controls; the remaining 9 need text with auto height, painted borders, or absolute positioning. The independent five-real-page corpus remains 0/5 paired. Chrome runs with `--host-resolver-rules=MAP * ~NOTFOUND`: no page may fetch during measurement. Existing WPT-derived HTML tree, URL, entity, and encoding tests are covered by `cargo test -p kobo-web-document --all-targets`, but they count a different parser-focused set and are not added to the CSS rendering numerator. `python3 tools/cobalt-wpt/parser-score.py /tmp/cobalt-wpt-parser.json` runs the crate and emits independently countable parser rows (not the Rust test-function count): HTML tree 1712/1739 with 67 scripting/fragment cases skipped; HTTP(S) URL 465/506; named entities 2231/2231; Windows-1252 index 128/128. These URL cases omit non-web protocols by browser policy, and HTML tree exclusions/expected failures are documented in `crates/kobo-web-document/tests/wpt/` rather than silently dropped.

A separately labelled **masked-text secondary score** is reported alongside the
headline when an explicit local TrueType face is available (`--font`, default
`/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf`). For each case not passing
the headline, the restricted text preview (`tools/cobalt-text-preview
--blocks --mask-json`) renders the page with that face and reports glyph-ink
bands. The `.cobalt-masked.ppm` artifact is the RAW render INCLUDING text;
the mask exists only in-memory at comparison time, where Cobalt's own bands
are hidden as full-width strips (8px inflation) in BOTH the Cobalt and
Chrome-reference images. The bands are Cobalt's own reported spans applied
to both images, not a union of independently extracted Chrome glyph bounds.
All remaining pixels are compared exactly. On the current passing cases the
raw band span is rows 18..34 and the strip is rows [10, 43): 33 of 600 rows
(5.5% of pixels). A masked pass claims only non-text geometry: font
rasterization, text position, and font choice are excluded, not matched, so
a 0.0 masked diff is NOT evidence of text position or font parity. The
secondary score never feeds the headline numerator. Headline and secondary both keep the fixed 11-case denominator.
On `agent/browser` the masked pass measures three cases:
background-color-body-propagation-001/002 pass at 0.0 diff since the UA sheet
gained 1em paragraph margins, and background-clip-009 fails at 0.04 diff -
its 96px square sits one pixel lower than Chrome's because Cobalt uses the
font's integer line height where Chrome's normal line height is fractional.
The other six non-passing cases refuse the masked render as well.


On 2026-11-01, the opt-in scorer environment
`masked-bold-v2-2026-11-01` adds `--font-bold
/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf`. It keeps the regular
face, frozen Chrome controls, 11-case denominator, mask format and 8px
inflation unchanged. The regular-only environment remains the default
(`masked-regular-v1`): headline 2/11 and secondary 4/11. With the bold face,
the secondary is **6/11 background-geometry-only**: clip-008/010 now render
and pass with 0 changed pixels outside the strips. These tests have empty
red clip areas, not visible squares. Clip-009 remains a 0.04% masked failure;
four other cases still refuse. The five previously measured Cobalt raw
images, their existing masks and all corresponding Chrome controls are
byte-identical to the regular-only checkpoint. The real-page corpus stays
0/5 paired.

Mixed inline text is restricted to LTR blocks containing text nodes and
single-text inline elements. Font size, colour and line-height are uniform;
only weight 400/700 varies. Bold advances and bitmaps come from the same
real bold face. Bold loading is lazy, and unavailable bold data refuses
only a page that needs it. Nested inlines, inline edges/backgrounds, mixed
sizes, other weights, italic/oblique, bidi and inline-block still refuse.
Bold and regular line metrics must match for the shared strut. Transparent
border widths and background-clip are honoured by the combined renderer;
a zero-area clip paints nothing. Painted borders still refuse. The raw
text remains visibly different from Chrome's serif text; no masked pass
claims font or text-position parity, and no headline baseline is replaced.

As the renderer grows, add a pinned WPT area for CSS2 normal flow, inline text, box edges and selectors, noting each predeclared denominator and its unsupported/fail/pass split. Then run the broader WPT harness for script and DOM features once Cobalt has a JS engine and browser automation interface. Acid tests and Speedometer are not meaningful pass-rate gates for this pre-JS engine. Keep the original WPT fixtures intact; don't simplify a failing WPT page into a lab fixture and call it a WPT pass.

On this workspace, `/usr/bin/cargo` may be non-executable; add `$HOME/.cargo/bin` to `PATH` before running the benchmark. A missing executable is a setup error, not a Cobalt unsupported case.
