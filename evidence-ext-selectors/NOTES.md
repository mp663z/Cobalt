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

## Revision 2 (after independent review, NOT PASS on first submission)
Reviewer findings (measured against Chrome), all fixed in the opt-in entry only; default entries untouched:
1. `[a~=v]` split on Rust `char::is_whitespace` (treated U+00A0, U+000B, U+2003 as separators). CSS whitespace is ASCII space, tab, LF, FF, CR only. Fixed for the split and for the right-hand-side rejection. Regression: word_and_class_matching_use_ascii_css_whitespace_only (NBSP, VT, em space negative; FF, tab, LF, CR positive; quoted NBSP value positive).
2. Repeated structural pseudo-classes were collapsed into bit flags, and specificity counted set bits. `p:first-child:first-child` (2) lost to `p[a]` (1) where Chrome picks the first. Now each occurrence counts. Regression: repeated_structural_pseudo_classes_count_each_occurrence_for_specificity.
3. Class tokens (`.b`) split with Unicode whitespace (inherited in the shared matcher): in the opt-in entry they now split on ASCII whitespace only; default unchanged.
4. SVG type selectors (`linearGradient`, `foreignObject`, any case) did not match html5ever's camel-case local names (inherited from the default lowercase compare): the opt-in entry now compares type selectors ASCII case-insensitively, as Chrome does. Default unchanged.
5. Found while fixing: the opt-in path trimmed selectors with Unicode `trim()`, so a selector such as `p<NBSP>` would have been accepted as `p`. The opt-in path now hands the raw text to the parser, which skips ASCII whitespace only. Regression: unicode_whitespace_around_selectors_does_not_trim_or_split. The default path still uses `trim()` and `split_whitespace()` (inherited, unchanged; same Unicode-space acceptance exists there).
My own errors, recorded: the first synthetic battery had no NBSP/VT/duplicate-pseudo cases; the cascade battery could not detect findings 2; and a later battery with tab characters inside selectors was briefly mis-parsed by my TSV splitter (fixed with rsplit; the first reported 10,875-selector table had no tab lines and is unaffected).
Controls now: element-identity control, synthetic 713 selectors (battery, battery2 whitespace/SVG/trim cases, battery3 class/VT/duplicate-pseudo cases) + 4 corpus pages unchanged: see control-summary.txt, 0 mismatches, 0 wrong-accepts. The reviewer's own 22-selector probe (review-repros/) also passes: 0 accepted mismatches. Cascade control rebuilt: 10 documents, 400 random rules each (incl. duplicate pseudos, NBSP/class cases), 580 elements, 580/580 color and 580/580 background equal to Chrome. Sensitivity check: reverting fix 2 alone makes this control fail (525/580 color, 536/580 background), so it can detect that bug class.
Tests: engine 697 passed / 0 failed / 1 ignored (kobo-web-document + kobo-web-layout + kobo-browser-core; 12 ext_selector_tests); cobalt-text-preview 32 / 0 / 1. Default full-tree identity re-run on the revised source: plain and inline, 5 pages, 10/10 byte-identical to the base tree (tree-identity/default-fulltree-identity-v2.txt).
Known remaining limits: control-character (FF, VT) selectors make the stylesheet scanner refuse the whole sheet (CssMalformed, fail closed); selectors beyond the grammar refuse; cascade only for color/background on a 58-element synthetic document.
