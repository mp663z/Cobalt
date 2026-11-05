# Measurement chunk A (UNREVIEWED, diagnostic oracle only, not a shippable algorithm)
Measured quantity: for one text node, Range(0,i).getBoundingClientRect() width, rounded to 1/64 (ctl12.mjs), i.e. Chrome's ceiled cumulative character position from text start, white-space:nowrap-equivalent width 100000px, kerning and ligatures off, embedded pinned fonts. It is NOT the raw per-glyph shaping advance.
Truth rows: ctl9b.mjs = same 1173-case grid but the paragraph is ONE text node and words are probed with Ranges (the earlier grid used one span per word). Chrome's break sets differ between the two DOM shapes in 9 of 1173 cases (wrap-singlenode-compare.txt), so the DOM shape is part of the test scope.
oracle-model.py: line fits every word whose end position <= pos(line start word start)+W, greedy.
Result: oracle model vs single-node Chrome 1173/1173 identical; vs old span-per-word rows 1164/1173 (the 9 are the DOM-shape differences).
Re-baseline: fontdue sum model vs single-node Chrome 1149/1173 (24 flips), ceilpos fontdue model 1150/1173 (23 flips) (wrap-singlenode-compare.txt).
Not shown: that the position rule explains anything beyond this grid. Chrome prefix positions at sizes 12.73..23.02 are 0.86..1.10 px shorter over 135 chars than a linear hmtx*size sum (ctl12 data), so the effective size at those authored sizes is not the authored size; the cause (size quantization vs advance source) is not isolated here.

## Audit 23:28: diag tool advance mapping and effective size
The diag tool calls fontdue metrics(c, px) with px = authored k/64 and no size mapping (src/main.rs line 27-28).
effsize-audit.txt: implied effective size = Chrome total prefix width / linear hmtx total. On the 17 grid sizes: eff = k-1 (in 1/64 px) for the 15 sizes that are not multiples of 1/4 px, eff = k for 12 px and 23.75 px. The 0.86..1.10 px shortfall in the earlier note is this one-unit size shift.
Re-run of the fontdue models with eff size (k-1 unless k%16==0) against single-node Chrome truth: sum model 1173/1173, ceilpos model 1173/1173 (authored-size runs: 1149 and 1150). The rule was FIT on this grid.
Holdout (ctl12h.mjs, 25 new sizes 13.08..23.1 px, Liberation Sans only): implied eff = k-1 on 23/23 non-quarter sizes and k on 2/2 quarter sizes (holdout-effsize.txt). Measured by prefix width totals, kerning/liga off. Hypothesis, not a Chrome contract. Serif and DejaVu holdout not run.
