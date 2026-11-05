# Two positioned levels, definite geometry

An outer absolute block may contain direct absolute empty block leaves. Both
levels have definite integer width/height, zero margins and LTR. Only two
positioned levels are supported; the outer's ancestors remain static. Inner
leaves require an explicit integer inset on each axis. Left/top win when both
opposite insets are specified, as for existing outer leaves.

The inner containing block is the outer padding box: origin at the outer
border origin plus BORDER ONLY, extent equal to content plus actual padding,
excluding border. An outer content size 160x120, padding 11px 13px, border3 at
(30,40) has containing origin (33,43), extent186x142. A 28x18 outer child at
left5/top7 starts at (38,50); right9/bottom6 starts at (182,161).

Overflow stays visible outside the outer box and clips only at the final
viewport. Signed integer negative insets are allowed; no hidden clip, scroll,
unsigned wrap or saturation fallback is introduced. Checked geometry refuses
unrepresentable composed coordinates. Existing pixel/command caps stay fixed.

Auto/shrink-to-fit dimensions, child auto static anchors, percent/subpixel
insets, normal-flow children/static intermediary boxes, third positioned level,
RTL, nonzero margins, z-index, transforms and overflow controls still refuse.

The 500x500 HOST lab covers padding/border origin, opposite/negative insets,
content/border-box sizing and accepted uniform dashed/thin dotted child rings.
Fresh Chrome is pixel-identical:0/250000 changed pixels. This is not simulator
or device output and activates no production renderer.

Fixed WPT headline2/11, secondary6/11 and remaining unsupported cases stay
unchanged. The nested WPT case separately requires auto-sized outer containers
and 5px round dots, neither supported by this step.
https://www.w3.org/TR/CSS22/visudet.html#containing-block-details
https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width
https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height
