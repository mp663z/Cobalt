# Painted-page external images integration

Integrated external PNG/JPEG supply via Task::Fetch on current parser/font head. HTTPS source only, no credentials, 16 images, 1 MiB each, 4 MiB total, signature sniffing and navigation cancellation. SVG paths are skipped before fetch. HTTP redirect Locations are upgraded to TLS by the transport, not refused.

Own current-source verification:
- Browser full inventory: 294 tests, 288 passed, 6 ignored, no failures.
- Web document: 1451 passed, 18 ignored, 153 test groups.
- Full net suite passed, including real TLS redirect ceiling (six GETs, no seventh), HTTP Location upgraded with TLS/SNI, absence of auth/cookie headers, stalled-read cancellation under three seconds.
- Exact 180-second idle-timeout predicate boundary and progress-reset test passed. This is not a wall-clock 180-second stalled network test.
- All 13 frozen corpus census baselines match. All 13 full PNG pixel arrays are identical to the pre-image parser head.
- Real TLS actual app: two external stylesheets plus external PNG painted, SVG URL not requested; address Cancel and Back preserve painted frame. app.png inspected as actual simulator pixels. The circular black image is the transparent test PNG, not a missing-image marker.

Known caveats: native picture prefetch and painted-page supply both request the same PNG in the test journey. No request dedup claim. ARM binary size and live-image RSS qualification pending. This does not close broad HTML/CSS, hardware, Miri, Kani or POST release gates.

Post-checkpoint hardening: hidden ancestor images excluded with linear parent-state census. The image queue is pumped on every task completion, including an unrelated task freeing a slot. Both regression tests failed before their narrow fixes and pass after. Full browser rerun on updated binary: 296 tests, 290 passed, 6 ignored, zero failures. Webdoc/net/corpus implementation unchanged from the results above.
