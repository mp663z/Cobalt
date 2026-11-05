# EXPERIMENTAL underline profile, UNREVIEWED. Not a verified claim. No PR.
Spec bound (CSS Text Decoration 3): thickness/position are `auto`/UA-defined (font metrics or UA choice), so there is NO spec number; values below are Chrome 156 MEASUREMENT ONLY for one face.
Measured: Chrome 156 headless, DPR1, default serif Liberation Serif Regular (sha256 29d12439831b7f59194efec85872f24f54eff05738933f9a860220d2abff88ba), line-height normal.
Rule fit (Chrome pixels, sizes 8..57): thickness=max(1,floor(s/10)); top=baseline_row+max(1,ceil(s/20)). Skip-ink gaps in Chrome for Q g j p q y , ; ( ) [ ] @ -> those chars refused.
Cobalt-vs-Chrome pixels verified ONLY at 12,14,16,20,24,32px (pairs/). 40px counterexample: Cobalt baseline 1 row high -> refused.
Refused: any other text-decoration form; inline/descendant/ancestor decoration; wrapped (multi-line); bold/italic; text-align other than left; chars outside allowlist; sizes outside the six; any font whose bytes hash differs (preview CLI); default painters refuse any decorated tree.
Limits: underline x-ends differ 1-6px (advance rounding). Glyph rasters differ (known gap). Not text-decoration general, not real-page progress (gate 0/5 last-known).
Commands: pairs/cases.json; cobalt-text-preview X.html X.ppm 400 100 LiberationSerif-Regular.ttf --blocks --underline-chrome156-liberation ; Chrome via scripts/shot.mjs, measurement scripts/deco*.mjs.
