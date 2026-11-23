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
