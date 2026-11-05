# Fractional glyph spike, step 1 (evidence only, no engine change, UNREVIEWED)

Question: does the glyph backend accept fractional pixel sizes? Backend facts only; nothing here says how Chrome rounds, snaps or positions text.

Source facts: kobo-text depends on fontdue =0.9.3. fontdue Font::metrics, horizontal_line_metrics and rasterize take `px: f32`. The adapters in this repo narrow to integers: tools/cobalt-text-preview LocalFace::pixels(size: u32) -> f32 accepts 1..=256 whole sizes, and the engine's FontProvider::shape_run(text, size: u32, bold). So the integer limit is in our traits and in the style tree (Computed.font_size: u32), not in the rasterizer.

Measured (fontdue-fractional-px-metrics.txt, probe fracpx-probe-main.rs, rustc 1.85.1, Liberation Serif Regular sha256 29d12439...88ba and DejaVuSans): at sizes 13, 13.5, 13.99, 14, 14.4, 16, 16.5, 17.6 the ascent, descent, line gap, new_line_size and advance widths are non-integer and vary continuously (Liberation Serif: new_line 14.9487 at 13px, 15.5237 at 13.5, 16.0871 at 13.99, 16.0986 at 14); rasterized bitmap width/height are integers that change at some sizes; xmin and ymin are integers (no sub-pixel glyph origin offset observed in these calls). Total coverage grows with size.

What this does not establish: Chrome's own line-height/advance rounding, whether Chrome rasterizes glyphs at the fractional size or rounds, sub-pixel glyph positioning, or the layout-unit grain. Those need controls against Chrome (getBoundingClientRect on text blocks at fractional sizes) before any design claim.
Next step if continued: a Chrome control that records line box heights and first-glyph x for fractional sizes in the same font file, compared with fontdue's numbers above; no engine change before that.
