# Partial page regression corpus

Ten frozen documentation pages, fetched without cookies or script execution on
2026-11-23. This follows `apps/browser/tests/corpus/SOURCES.md`: only pages with
redistribution permission are included. Hacker News and NPR are intentionally
not included. The private five-page comparison set is not copied into this tree.

`manifest.json` records every source URL, resolved URL, byte count and SHA-256.
The HTML and each directly linked stylesheet are stored without edits. The
ordered files `sheet-0.css`, `sheet-1.css`, ... match document stylesheet order.
There is no network access during tests or rendering. Refreshing is an explicit,
one-time operation with `../freeze_corpus.py`, never part of a test.

| Pages | Source | Content licence |
| --- | --- | --- |
| python-introduction, python-controlflow, python-datastructures, python-modules, python-errors | https://docs.python.org/3/tutorial/ | Python documentation, PSF License 2. See LICENSE-Python.html. |
| rust-installation, rust-hello, rust-guessing, rust-variables, rust-functions | https://doc.rust-lang.org/book/ | The Rust Project, MIT OR Apache-2.0. See LICENSE-Rust-MIT and LICENSE-Rust-APACHE. |

The embedded stylesheet notices are preserved. Rust book theme files come from
mdBook (MPL-2.0); Python's theme uses Sphinx (BSD-2-Clause) and Pygments
(BSD-2-Clause). Copies of their licence notices accompany this corpus. Font
stylesheet declarations are retained, but remote font files are not downloaded
or redistributed. The harness uses the already-bundled Cobalt reader fonts and
their existing notices in `crates/kobo-text/fonts`.

## What the baseline means

Each `expected.census` pins height, clamp status, block/text/glyph/fill counts,
input/output truncation, skipped content, approximations and reserved image
count. It is a change detector, not a correctness certificate. The current
baseline deliberately exposes non-ASCII drops, fallback wraps and other known
limitations. Inspect the corresponding PNG and the census diff before updating
any baseline. Do not bless a regression merely to make the test pass.

This is a first corpus, not representative coverage of the web. It covers code,
headings, lists, notes and navigation from two documentation systems. Further
licensed sources, non-Latin text and focused fixtures should broaden it.

The harness supplies directly linked stylesheets only, including links with
alternate/media attributes as the existing engine supply API expects. It does
not fetch CSS imports, remote fonts, scripts or external images. In particular,
Sphinx's imported basic.css is not resolved. These are recorded limitations,
not claims of a fully loaded browser page. The engine's temporary normalizer
and hard-coded intake media width remain until the engine lanes replace them.

The harness now uses kobo-text's PageFontSession with real regular/bold cuts and
the bundled DejaVu Sans Mono as a coverage fallback for proportional text. A
missing glyph draws a box. This is still scalar text, not complex-script shaping.
Natural line metrics include the fallback face. On the 2026-11-23 update this
changed Python page heights by 2-5 px; the ten-page before/after census diff and
representative final-composite crops were reviewed before the baseline update.
The narrow Python body columns and overlapping navigation remain known engine
layout defects, not accepted visual correctness or improvements from fonts.

## Authored language fixtures

`hindi`, `russian` and `japanese` are short original test pages copied byte for
byte from `crates/kobo-web-document/tests/fixtures/page-text/`. They use the
repository's AGPL-3.0-only licence, not fetched third-party content. Their source
paths and hashes are in the manifest. The offline integrity check compares the
copies against the originals. A source-fixture edit requires both to be updated.

The shaped harness now loads the already-bundled Noto Cyrillic and Devanagari
subsets (their OFL notice and measured sizes are in `crates/kobo-text/fonts`).
Hindi and Russian have real glyph coverage and zero dropped text blocks in the
fixtures. Japanese deliberately records missing-glyph placeholders: no CJK face
is bundled. A zero skipped-block count must never be called Japanese support.

The harness uses per-character metrics from the line-used face, while retaining
the primary strut. This avoids enlarging Latin-only lines merely because a
Devanagari fallback is installed. Census baselines were refreshed only after
reviewing all 13 renders and the diffs. The language comparison also confirms
real bold fallback cuts for Hindi and Russian. Japanese still has 48 missing
glyph placeholders, explicitly counted; no CJK support is claimed.
