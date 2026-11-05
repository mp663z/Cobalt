# Exact zero-sized positioned container

Restricted addition: an outer absolute block with BOTH dimensions auto and
all four insets auto, transparent background, no padding/border, zero margins,
LTR, static block parent, and only direct empty definite-sized absolute block
children. Existing inner requirements remain: integer dimensions, explicit
inset on each axis, zero margins, LTR, no grandchildren.

With no in-flow content, preferred/minimum width and content height are zero.
This proves exactly a 0x0 containing block; it is not general shrink-to-fit.
Its origin is the hypothetical normal-flow static position: parent content
origin plus preceding in-flow sibling geometry/margins. Child right/bottom
insets solve against ZERO, not the viewport. Negative integer positions are
allowed; overflow is visible and clips only at the final viewport. Checked
coordinates refuse unrepresentable output rather than wrapping.

Ordinary empty zero-height normal-flow blocks still refuse. One-axis auto,
non-auto outer insets, padding/border/background, text/replaced/in-flow child,
inline parent, depth3, percentage/fractional offsets and general auto sizing
remain unsupported. Existing budgets/caps/font policy are unchanged.

The 400x300 HOST lab matches Chrome exactly:0/120000 changed pixels. It covers
preceding27px flow, padded/bordered static ancestry, right/bottom-negative and
visible child overflow. It is not a simulator/device screenshot; no production
renderer is activated. Fixed suite remains headline2/11, secondary6/11 and
2maskedunsupported. Its remaining nested case needs independent5px circular
dot coverage, which this slice does not supply.
https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width
https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height
