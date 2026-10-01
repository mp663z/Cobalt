# Host text preview

This restricted TrueType preview is a standalone host tool, not a device
workspace package. Its fonts and lockfile do not change Store app dependencies.
The background-only frozen benchmarks remain unchanged.

From the repository root:

```
cargo run --locked --manifest-path tools/cobalt-text-preview/Cargo.toml -- \
  page.html preview.ppm 800 600 /path/to/font.ttf --blocks
```

Omit `--blocks` for the single-text-block mode. Unsupported content is refused,
not silently painted as an empty page.

## Experimental shipping seam (not enabled)

The library exposes `LocalFace` and `PanelFrame` to test a future browser
handoff without changing `apps/browser`, shared font APIs, Store metadata or
app versions. `LocalFace` loads a bounded explicit face at integer CSS pixel
sizes 1..=256; the same face supplies advances, natural height, baseline and
coverage. Per-glyph advance rounding is still a host restriction, not Chromium
font-metric parity or device qualification.

`PanelFrame` accepts only opaque, bounded display-list RGBA. Confirmed colour
identity (`Some(true)`) preserves RGB channels; false or unknown identity uses
Rec.709 integer grey. `put` queues exactly one existing SDK picture command;
it does not create a screen, change navigation or display an app page.
Tests use the bundled device Atkinson face and inspect RGB/grey SDK commands.

Before enabling a shipping patch, the review must cover:

- A device-compatible font source and adapter, cross-built for ARM. The current
  standalone tool is host-only and has its own lockfile.
- Browser CSS-mode state with retained input/style tree, whole-page refusal
  fallback, image-handle lifecycle, content room and pagination rules.
- A picture screen that never silently drops links/forms or invents hit targets.
- Colour-panel identity before RGB; grey fallback before identity is known.
- Real simulator and physical colour-device checks, then Store/version review.

No shipping patch is applied by this tool. The existing browser remains the
semantic reader. The exact app patch and any required version change must be
reviewed before enabling it.

`prepare_page` bounds markup and already-fetched stylesheet bytes before
building the complete style/box tree and preparing a panel frame. It follows
the parser's two-linked-sheet limit and DOM link ordering, not an arbitrary
list of styles to inject. `queue_page` prepares before queueing, so unsupported
markup produces no partial picture command and leaves semantic fallback to
the future app caller. A reserved picture handle is caller-owned and must be
dropped on replacement. The `--blocks` host preview now runs this preparation
path. Tests cover whole-page refusal, linked styles, input budgets, unknown
identity and the matching SDK drop command; no app screen is enabled.

Until a picture screen has real hit-target and scrolling/pagination rules,
preparation refuses retained interactive tags (including block-styled links
and controls) and any paint command extending beyond the supplied room.
This is deliberately conservative: even hidden interactive tags or harmless
ink overhang at the viewport edge can trigger semantic fallback. Primitive
raster clipping remains unchanged; it is not permission to drop page content
when queueing a browser picture.

`PictureSlot` tests ownership with two distinct caller-reserved handles.
It queues the prepared replacement before dropping the previous page,
alternates handles, keeps old ownership when SDK validation refuses a frame,
and clears idempotently. Navigation, semantic fallback and exit must explicitly
clear the slot. This does not reserve globally safe handles or display UI;
those remain part of the exact app patch review.
