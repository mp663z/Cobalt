# Bounded absolute-leaf probes

`absolute-explicit-lab.html` and `absolute-static-lab.html` are local engine
probes, not WPT additions. At 400x320, scale 1, Chrome headless and the background
preview each measured zero changed pixels of 128,000 on 2026-11-01.

Supported: an empty block leaf with `position:absolute`, definite integer-pixel
width/height, zero margins, LTR and a static ancestor chain. The containing block
is the initial viewport. Integer left/top offsets win over opposite offsets;
right/bottom solve against the outer box dimensions. Auto offsets use a static
anchor without adding the leaf's size to normal flow. The leaf paints after
normal-flow backgrounds/text, in tree order. Padding/background clips and the
restricted solid-border ring are shared with normal blocks. Fully opaque legacy
`rgba(r,g,b,1)`, `1.0` and `1.00` are exact RGB, not alpha blending.

Nested or non-leaf absolute boxes, relative/fixed positioning, nonzero/auto
margins, shrink-to-fit/auto dimensions, RTL, percentage/subpixel offsets, z-index,
partial alpha, and unproven empty normal-flow ancestors still refuse. Positioned
inline elements are not yet blockified and refuse. The explicit content/padding
rectangle primitives also refuse positioned boxes, preserving their prior contract.
No shipping interaction, pagination or viewport-escape guard is relaxed.

Spec grounding: CSS 2.2 sections 9.6, 10.3.7, 10.6.4:
https://www.w3.org/TR/CSS22/visuren.html#absolute-positioning
https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width
https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height

The frozen WPT headline remains 2/11. The opt-in bold masked secondary remains
6/11: content-box-001 newly renders but fails with 760 changed unmasked pixels
(0.15833%). Its static square anchor is one pixel lower than Chrome because the
text strut differs. Clip-009 still fails with 192 changed pixels (0.04%); three
cases remain unsupported. No source fixtures, offsets, denominator, masks or
baselines were changed to force either failure to pass. These lab pixels are
not a shipping simulator screenshot or a claim of text/font parity.
