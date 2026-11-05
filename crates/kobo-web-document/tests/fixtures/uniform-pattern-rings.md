# Uniform complete non-solid rings

This extends the single-side integer pattern proof to four equal-width sides
of identical dashed or thin dotted style and one resolved opaque colour.
Both outer dimensions must satisfy the unchanged complete integer pattern
predicate. Full endpoint dashes cover each w*w square corner, so adjacent
same-colour strips overlap exactly without a diagonal join wedge.

Still refused: two/three nonzero sides, unequal widths, mixed styles or colours,
transparent/opaque mixtures, radius, thick circular dots, short or fractional
fits, fractional widths and partial alpha. Existing caps remain unchanged.
Transparent rings can produce no ink; no visible join claim follows from that.

Local 18-square and 18-rectangle lab controls match fresh headless Chrome with
0/480000 and 0/320000 changed pixels respectively. Engine tests assert every
pixel for 36 rings at odd offsets, plus whole-page refusals and command limits.
These are HOST restricted-engine artifacts, not simulator/device screenshots.
No browser production renderer is activated.

The fixed 11-case WPT headline stays 2/11; masked-text secondary stays 6/11.
background-clip-color newly renders but FAILS with 510 unmasked changed pixels
(0.10625%). On a separate uncommitted causal control with 19px line-height,
its masked difference is zero. Normal line-height font-profile differences
accumulate through its four text rows. No fixture, scorer, mask, baseline or
font/rounding policy was changed to hide this failure. The nested-absolute
and fractional-border cases remain unsupported independently of these joins.

https://www.w3.org/TR/css-backgrounds-3/#border-style
