# Single-side integer border patterns

The 400x320 HTML lab is a local restricted-engine probe, not a WPT addition
or a simulator/device screenshot. Seven style/width/length patterns on each
of four sides match headless Chrome pixel-for-pixel: 0/128000 changed pixels.

Computed border styles retain none, solid, dashed and dotted through shorthand,
longhand, inherit, initial, unset and importance. Existing solid rings retain
unequal widths with a common resolved colour. Non-solid paint is narrower:
exactly one nonzero-width side, with no corner or mixed-side join.

Supported dotted widths are 1..3 integer pixels, in complete equal dot/gap
patterns with an odd number of thickness units (at least two dots). These are
thin pixel-grid dots, not a circular coverage claim. Dashed lengths are complete
square-ended dashes and gaps: dash/gap 3w/2w for widths 1..2, 2w/w otherwise.
The length must be n*dash+(n-1)*gap, n>=2. There is no fit adjustment or new
rounding policy. CurrentColor resolves at the element. Transparent single sides
paint no ink. Zero, none and hidden keep their prior used-width semantics.

Non-solid corners, multiple nonzero sides, thick circular dots, shortened end
patterns, fractional fitting, partial alpha, fractional widths and other border
styles still refuse. Existing command/glyph/pixel caps are unchanged; command
exhaustion refuses the whole page. The named fixed WPT tests still need broader
joins/nested absolute support or fractional border widths. The fixed suite stays
headline 2/11 and masked-text secondary 6/11. No shipping renderer is activated.

Grounding: CSS Backgrounds 3 border-style defines square-ended dashes and round
dots, with spacing/length left to implementations. Thin pixel-grid dot behavior
is specifically checked against Chrome, not inferred from the word "round".
https://www.w3.org/TR/css-backgrounds-3/#border-style
https://www.w3.org/TR/CSS22/box.html#border-style-properties
