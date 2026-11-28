# Imported qualification evidence

Report/evidence only, from qualification d7a4c1f. No allocator or qualification
crate merged. Original paths in RESULTS.md belong to that isolated branch.
Results are worker measurements, not independently rerun here, except current
link-callback checks kept under browser-current-sheet-checks.
ARM build environments differ by1032B at02d80c61, unreconciled. Earlier+240ms
painted-time comparison is RETRACTED by interleaved A/B below. Hardware gates remain open.
Later corrections: committed drive-forms repaired and live-tested at a9752618;
its GET submitter and POST review/cancel work, but it never covered textarea
editing. The --textarea TLS fixture now covers that separately.

Updated evidence-only from qualification e747e5c, measured at d9a8ab95. Worker
originally reported a painted regression, now RETRACTED by interleaved A/B;
RSS flat, Sphinx/committed drive-forms pass. Own d9a8ab95 live Sphinx also passed.
Worker ARM5319052B vs own5320220B is a1168B environment delta at that source,
unreconciled. Do not confuse with earlier1032B delta at02d80c61. Worker test
reports are retained as measurements, not new independent integration results.
The worker's last paragraph calls the second sample form a textarea leg. That
wording is incorrect: it is the existing POST-review form. The committed route
still does not edit textarea; our separate TLS fixtures cover textarea edits.

Qualification976bf36 interleaved5reps A/B071bb0c vs02d80c6: sheets5 means2304/2305ms, sheets16means2386/2385ms. Earlier+240ms/+12% was hostmachine drift, not a demonstrated flex regression. Cross-session timing comparisons are invalid here. Evidence-only import, not independently rerun in integration lane.
