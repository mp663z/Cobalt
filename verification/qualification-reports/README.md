# Imported qualification evidence

Report/evidence only, from qualification d7a4c1f. No allocator or qualification
crate merged. Original paths in RESULTS.md belong to that isolated branch.
Results are worker measurements, not independently rerun here, except current
link-callback checks kept under browser-current-sheet-checks.
ARM build environments differ by1032B, unreconciled. +240ms painted-time
correlation with v5b is not causally isolated. Hardware gates remain open.
Later corrections: committed drive-forms repaired and live-tested at a9752618;
its GET submitter and POST review/cancel work, but it never covered textarea
editing. The --textarea TLS fixture now covers that separately.

Updated evidence-only from qualification e747e5c, measured at d9a8ab95. Worker
reports painted regression persists but generated hamburger adds no clear cost;
RSS flat, Sphinx/committed drive-forms pass. Own d9a8ab95 live Sphinx also passed.
Worker ARM5319052B vs own5320220B is a1168B environment delta at that source,
unreconciled. Do not confuse with earlier1032B delta at02d80c61. Worker test
reports are retained as measurements, not new independent integration results.
The worker's last paragraph calls the second sample form a textarea leg. That
wording is incorrect: it is the existing POST-review form. The committed route
still does not edit textarea; our separate TLS fixtures cover textarea edits.
