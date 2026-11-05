# Extended selector grammar (opt-in candidate) - UNREVIEWED

Entry: `parse_style_tree_with_inline_sheets_and_selectors` (inline-sheet retention plus the grammar below). Default entries are unchanged.
Base: 70bc8f26 (evidence child of 5c2f419b); source delta is only css.rs, lib.rs, style_tree.rs under crates/kobo-web-document/src.

Grammar added (opt-in only): `*`, `[a]`, `[a=v]`, `~=`, `|=`, `^=`, `$=`, `*=` (quoted or ident values, no `i`/`s` flags, no namespaces, no escapes), `:first-child`, `:last-child`, `:only-child` (element siblings only, text and comments ignored), combinators `+` and `~`, plus a backtracking, fuel-bounded matcher (fuel 4096, sibling lists over 4096 refuse as `MatchWork`, never as a mismatch). Identifiers starting with a digit are refused.
Still refused as CssSelector: `:hover :active :focus :focus-visible :focus-within :visited :link`, all functional pseudos (`:not :is :where :has :nth-*`), `:first-of-type` etc, `:empty`, pseudo-elements, namespaces, escapes, case flags. State pseudos are refused, not assumed never-match.
HTML rules implemented: attribute names ASCII case-insensitive (Chrome also does this on SVG elements); values case-insensitive for the HTML spec list of attributes (type, lang, rel, target, ...) on HTML elements; empty value with `~= ^= $= *=` matches nothing.

## Control against Chrome (control-summary.txt, compare.py)
Same documents, same selector strings, script disabled at load. Compared the SET OF ELEMENT IDENTITIES (preorder index of every element), not counts. Result across 5 documents (synth + blog, docs, gov, news-npr) and 10,875 selectors: 0 mismatches, 0 wrong-accepts (accepted selector that Chrome rejects). 961 accepted selectors select a non-empty set identically. Refused-but-Chrome-valid selectors are the coverage gap.
One real bug found and fixed by this control: attribute names on SVG elements (`[viewbox]`, `[Id]`) were compared case-sensitively; Chrome matches case-insensitively.
NOT controlled: cascade and computed-style outcomes against Chrome (specificity is covered only by unit tests), and wikipedia (no extractable sheets by my crude extractor). Selector extraction is an upper bound (includes selectors inside non-screen @media) and a diagnostic, not verified coverage.

## Counters (refusal-counters.txt, crude sheet extraction, dark sheets skipped, not the review harness)
CssSelector count, inline entry -> ext entry: blog 361->321, docs 46->27, news-npr 446->262, wikipedia 300->284, gov 0 (my extractor finds no sheets). No page changes first refusal reason (DroppedAttributes / InputBytes) and no page becomes supported. No page-flip claim. Default plain and inline rows are byte-identical to the base tree (checked by diff of the three counters on 5 pages).

## Observed in the default matcher (unchanged, not fixed here)
The default descendant matcher is greedy and never backtracks: `div > section span` misses a span whose nearest `section` ancestor is not a child of `div` while an outer one is (test `default_entry_greedy_descendant_chain_gap_is_unchanged`). The opt-in matcher backtracks.

## Tests
kobo-web-document + kobo-web-layout + kobo-browser-core: see engine-tests.log (693 passed / 0 failed / 1 ignored; includes 8 new ext_selector_tests; the ignored one is the Chrome control dump harness `control_dump`). cobalt-text-preview: preview-tests.log (32 passed / 0 failed / 1 ignored by target sums: 29+2+1 passed, 1 ignored).
Not run: full workspace (kobo-cli needs an assets dir absent from these trees).

## Privacy note
Per-selector lists and result files for the four private-corpus pages (list-/rust-/chrome-{blog,docs,gov,news-npr}) are NOT in this public branch; they ship in the private release packet. Only the synthetic document (synth.html, battery.txt) results are here, plus the summary and the scripts.

## Addendum: full-tree default identity and cascade control
- tree-identity/default-fulltree-identity.txt: Debug serialization of every retained StyledNode (tag, text, parent/children, full Computed) plus diagnostics, truncated/unsupported/quirks and host_sheets_claimed, for the plain and inline entries on the 5 corpus pages, base tree (clean4r) vs this tree: 10 of 10 byte-identical (sha256 compared). For the ext entry: gov equals inline; the other four differ, as expected. Probes: treedump.rs (both trees), casc.rs.
- cascade-control/: 6 generated documents, each with 300 random rules drawn from the 169 ext-grammar selectors that select a non-empty set on the synthetic document (25% grouped, 15% !important, color and background-color, random order and ties), run through the ext entry. Per element, engine color and background-color vs Chrome getComputedStyle: 270 elements, 270/270 color and 270/270 background equal. Limits: synthetic document only (45 elements), two properties, no display/layout; real-page cascade is not controlled.
