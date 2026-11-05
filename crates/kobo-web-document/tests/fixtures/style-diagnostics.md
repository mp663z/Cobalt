# Visual parse refusal diagnostics

`StyleTree.diagnostics` is an additive, fixed-size evidence record. Existing
`unsupported`, `truncated`, `quirks` and all budgets remain authoritative and
unchanged. Diagnostics do not authorize rendering a partial tree.

`first_reason()` follows pipeline order: input/node limits, DOM attribute/text
loss, encoding/quirks, then stylesheet and style processing in document order.
The record also has a fixed `RefusalReason::ALL` histogram through `count(reason)`.
Counts saturate. They count observed occurrences/categories, not unique defects,
WPT failures, or compatibility rates. `DroppedAttributes` occurs once when any
attribute was lost; `dropped_attributes` is the actual total (e.g. 16). The same
category distinction applies to dropped text and scanner refusal flags.

`dom_nodes` describes the consumed parse prefix, and `retained_nodes` the retained
style arena, neither a faithful complete styled-DOM claim after refusal.
`expanded_declarations` counts selector/property pairs, not unique CSS rules.
`match_attempts` counts matching attempts, not successful matches. No URLs, CSS,
HTML, attribute values or other external content are copied into diagnostics.
The histogram is constant space; resource content is not logged or retained.

The frozen five-real-page run at this checkpoint has zero expanded declarations
and zero match attempts. Its first reasons are dropped attributes for Wikipedia,
GOV.UK, Python docs and Rust blog, and input byte budget for NPR. The snapshot's
large data-URI stylesheet attributes exceed the existing 2048-byte attribute
limit. Missing supplied stylesheet bytes and other refusals remain additional
facts. This is not evidence that those pages would render after resource loading:
CSS scanner/stylesheet-count limits, unsupported semantics, paint budgets,
pagination/navigation, images and fonts remain separate work.
