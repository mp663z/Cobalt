# Fractional text layout, candidate spec DRAFT (UNREVIEWED, no code exists)
Source base: reviewed rev5 083601a6. Evidence: directories in this folder. Nothing here changes the WPT pixel gate, the real-page gate (0/5 last-known), the frozen profile, caps, budgets, or default behaviour.

## Two tiers, kept separate
A. Measurement-only approximate preview (the only tier proposed now). Opt-in entry only. Output is labelled approximate in the API and in any artifact. It must never be used as a pass/fail result, never as a faithful render, never to claim Chrome geometry equality. Its residuals vs Chrome are reported per run.
B. Admitted exact layout: NOT proposed. No rule measured so far is exact: normal line height 875/879 (+-1 px misses), string width residual up to 0.46 px growing with length, wrap breaks equal in 98.6%, authored-to-used font size mapping has path-dependent phase. Tier B stays empty until a controlled test shows an exact rule over a stated envelope.

## Tier A envelope (fail closed, each item measured above)
- Faces: the three pinned faces only (hashes in font-sha256.txt). Any other face refuses.
- Authored font-size: exact k/64 px, 8..40. Decimals not on that grid (14.4, 17.6) refuse (used size differs by path, see fontsize-quantization).
- Characters: ASCII letters, digits, space only until kern/liga/shaping controls exist for punctuation. Refuse any pair that the face kerns and refuse f-ligature sequences (fi, fl, ff, ffi, ffl) until measured.
- line-height: only unitless 1.5 at exact k/64 sizes has an exact measured rule (floor(size*1.5*64)/64, 879/879). Other unitless numbers, percent, em, px forms refuse until separately measured. normal: allowed only as flagged approximate (+-1 px observed), excluded for Liberation Serif 35.5..40 px.
- Width and wrap: unrounded advance sums; report the cumulative residual per string (not a fixed 0.5), flag wraps where the width margin is below the measured residual bound for that length as "boundary uncertain" and refuse them in tier A output.
- Origin: integer pixel origin only; sub-pixel origin unmeasured, refuse fractional origins.
- Raster: fontdue at the f32 size; reported as NOT matching Chrome pixels (coverage ratio 0.88..1.06, bbox rows differ).

## Representation (for review, not implemented)
New newtype (e.g. FontSize64(u32), px*64) carried only by an opt-in entry beside the integer Computed; checked arithmetic with explicit overflow refusal; all ~48 font_size sites audited; default, inline and ext entries untouched, proven by identity tests (Shaped and Legacy paths, both input classes) versus rev5. Existing Shaped path (size list 8,10,12,13,14,15,16,17,18,20,24,32,48) must keep byte identity.

## Tests required before any claim
Identity vs rev5 default on the 10 captures; mutation checks that each refusal fires; Chrome-controlled fixtures for each admitted form with the residual recorded; independent review of API scope, overflow and refusals before code is called reviewed.
