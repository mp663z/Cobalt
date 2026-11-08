# Solid-border lab probes

These two standalone pages are local engine probes, not WPT fixtures or a new
benchmark denominator. They exercise integer-pixel solid borders only.

`solid-border-lab.html` covers root canvas/background separation, currentColor,
unequal widths, padding/content background clips, a border-only block, zero-width
sides, side longhands, and descendant paint overlapping an ancestor's border.
`solid-border-inherit-lab.html` checks inherited currentColor against the child's
foreground. Colours that differ between nonzero sides, including mixed opaque
and transparent sides, still refuse because diagonal corner joins are not yet
implemented. Dashed/dotted/double, partial alpha and fractional widths refuse.

At a 400x320 viewport with scale factor 1, compare `css_background_preview`
against Chrome headless with no network. The 2026-11-01 checkpoint measured zero
changed pixels of 128,000 for each page. This is background/border engine evidence,
not a shipping simulator screenshot or proof of text/font parity. The fixed
11-case WPT headline remains 2/11; the opt-in bold masked secondary remains 6/11.

The explicit content/padding rectangle primitives still preserve caller fill
order and refuse visible borders. The background painter uses a separate
restricted decorated path that visits every block in preorder, paints its
background then border, and does not skip border-only/root/propagated-body boxes.
The combined text painter uses the same border-ring code before the text phase.
All output is returned only after the whole page succeeds; geometry, allocation
and display-list budgets can refuse it without returning a partial page.
