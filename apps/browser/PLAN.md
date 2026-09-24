# Cobalt Browse: plan and task list

Level A reader browser for Cobalt. HTML goes into a bounded semantic Document IR,
the IR is paginated with kobo-ui metrics, and the runtime's refresh planner draws it.
No JavaScript, no author CSS in the first release, no scrolling.

Branch: `agent/browser` on mp663z/Cobalt, cut from upstream `beta` at fc88fd3.

## Crates

| Crate | Owns |
| --- | --- |
| `crates/kobo-web-url` (module of kobo-web-document if small) | WHATWG-subset URL parsing, base resolution, scheme policy |
| `crates/kobo-web-document` | Bounded parse, sanitize, normalize into Document IR; forms model; limits |
| `crates/kobo-web-layout` | IR to pages against real panel metrics; link/action ID allocation; invariants |
| `crates/kobo-web-fetch` | Runtime-mediated requests: HTTPS only, limits, MIME checks, redirects, cache |
| `crates/kobo-browser` | Navigation, history, bookmarks, session state, cache policy, errors |
| `apps/browser` | Screens: address, page, links list, history, bookmarks, errors, settings |

## Standing gates (every milestone)

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace --all-features`
- No `unsafe` (workspace forbids it). No new dependency without a measured size cost and a THIRD-PARTY.md entry.
- Simulator screenshots (`kobo drive --ideal`) for every visual change, on the default profile at 100%, 120% and 140% text scale, plus the other supported profiles for layout.
- No feature is called supported until it has a fixture, an IR or protocol assertion and a simulator journey. Hardware properties need physical-device evidence.

## M0: measurement spike

- [ ] M0.1 Build `kobod` and one simple app (hello-class) for armv7-unknown-linux-musleabihf in release; record stripped sizes
- [ ] M0.2 Build a throwaway probe binary linking html5ever vs the in-tree `kobo-doc::html` scanner; record stripped size delta on armv7
- [ ] M0.3 Measure parse time and peak heap for a fixture corpus (small article, Wikipedia-size page, 2 MiB page) on host; record RSS of simulator app process idle/peak
- [ ] M0.4 First-paint and page-turn timing in the simulator (host), labelled as host numbers; device numbers deferred to M6
- [ ] M0.5 Commit a fixture page and its expected ideal screenshot
- [ ] M0.6 Decide and write down hard limits: fetch bytes, decoded bytes, redirects, DOM nodes, depth, attributes, text, table dims, images, links, cache size
- [ ] M0.7 Record budgets doc (`apps/browser/BUDGETS.md`): binary delta target < 5 MiB, RSS delta target < 32 MiB, and the measured numbers

## M1: local document viewer (no network)

- [ ] M1.1 `kobo-web-document`: IR types (Block: Heading, Paragraph, List, Quote, Preformatted, Table, Image, Rule; Inline: Text, Emphasis, Strong, Code, Link)
- [ ] M1.2 Parser front end (html5ever if M0 allows, else bounded structured scanner) with hard limits and warnings list
- [ ] M1.3 Sanitizer: drop script, style, iframe, object, embed, canvas, media, event handlers, meta refresh, external stylesheets; hidden nodes; excessive nesting
- [ ] M1.4 URL resolution against document base (`<base href>`), unsupported schemes to inert text, fragment links
- [ ] M1.5 Title, visible text, ordered link list extraction
- [ ] M1.6 `kobo-web-layout`: paginate IR with real metrics (headings, paragraphs, lists, quotes, code, tables, rules), link action allocation, deterministic output
- [ ] M1.7 `kobo-browser`: history stack with Back/Forward, fragment navigation, page position per history entry
- [ ] M1.8 `apps/browser`: page screen with page turns, page position, top bar, Links list screen, Back/Forward, fixture index screen
- [ ] M1.9 Register the app: workspace member, `cobalt-app.json`, catalog/registry, README with screenshots
- [ ] M1.10 Golden IR snapshot tests for the fixture corpus (basic, malformed, encoding, tables, adversarial)
- [ ] M1.11 Layout invariant tests across all supported profiles x 100/120/140%: bounds, no overlap, min touch size, pagination terminates, bounded page count, every link exactly once in reading order, deterministic
- [ ] M1.12 Property tests: parser termination, depth limits, pagination termination, URL resolution, history transitions, action ID allocation
- [ ] M1.13 `apps/browser/drive.kobo` journey: open fixture, follow link, page turn, Back, Links list; screenshots
- Gate: unit + golden IR + layout profiles + drive journey green

## M2: bounded HTTPS navigation

- [x] M2.1 Address/search editor (keyboard screen), URL normalization, search fallback to a configured search engine (HTML endpoint, no JS)
- [x] M2.2 `kobo-web-fetch` over `Task::Fetch`: max bytes, timeout, cancellation, explicit user agent
- [ ] M2.3 Response metadata: final URL after redirects, status, content type. Runtime currently returns body bytes only; decide between a protocol extension (new task tag, rebuilt runtime/app/sim) and a documented limitation
- [~] M2.4 MIME and charset handling: HTML, XHTML, plain text; refuse others with a clear screen; charset from meta/BOM
- [ ] M2.5 Redirect policy and redirect-loop handling (runtime max 5), cross-origin redirect reporting
  - Where M2.3 and M2.5 stand: kobo-net follows up to 5 redirects, refuses https to http, and drops credentials and non-ordinary headers on any cross-origin hop; a loop ends as Unreachable (fixture suite covers both). What the app cannot do is know it was redirected: `Task::Fetch` answers with body bytes only. The visible cost is that the address shown and kept in history is the one asked for, and relative links on a redirected page resolve against it. That is harmless when the redirect stays in the same directory (the Wikipedia E-reader case), wrong when it does not (`/docs` to `/docs/`, or to another host). The honest fix is a protocol extension that returns final URL, status and content type; it touches kobo-protocol, the runtime, the simulator and the protocol minimums, so it is an owner decision, not something to slip in. Until then this is a documented limitation.
- [x] M2.6 Error screens: offline, timeout, denied, too large, not found, unsupported type, TLS failure; each with a way back and retry
- [x] M2.7 Loading state with Heartbeat, Cancel
- [x] M2.8 Local HTTPS fixture server for integration tests (delayed, chunked, wrong Content-Length, truncated, gzip bomb, redirect loop, cross-origin redirect, wrong MIME, connection drop)
- [x] M2.9 Simulator failure scenarios: offline, timeout, denied permission, full storage, background/foreground during fetch (`failures.kobo`; storage full does not touch fetch, since the browser stores nothing yet)
- [x] M2.10 Live-site screenshots (real pages, labelled with URL and time) (text.npr.org, lite.cnn.com, en.wikipedia.org/wiki/E-reader on clara-bw, libra-colour and extra-large text, 2026-09-25)
- Gate: deterministic local-server suite + simulator failure scenarios
- Found while taking the live shots, carried forward:
  - Typing an address is slow: the shared keyboard has no `_` at all (most Wikipedia article addresses cannot be typed) and `.` and `/` each need a layer switch. Candidate: app-local quick keys under the field; an SDK change is a release-review question.
  - Pages open on their site chrome (Wikipedia's first screen is its navigation menu). Candidate for M4: start at `<main>`, `role=main` or a skip link's target when the page has one.
  - Long pages: first screen now shows before the rest is paged (39090aab); the page count appears when paging finishes.

## M3: images and cache

- [x] M3.1 Lazy fetch of images on the visible page only, bounded count per page (6 per screen, 48 per page; sized images only, others stay a description)
- [~] M3.2 Compressed and decoded pixel limits, downsample to panel width, grey + dither via kobo-image (grey done; colour on colour panels still to do)
- [~] M3.3 `alt` fallback and broken-image placeholder (description always shown under the box; a failed picture leaves the empty frame)
- [~] M3.4 Bounded LRU disk cache for pages and transformed images (app store keys), eviction policy, cache metadata format (pages done: 24 MiB / 200 entries on the shelf, index under `page-cache-index`, strays swept at start; pictures are not kept yet)
- [x] M3.5 Offline reading of cached pages, labelled as cached with time (only for No Wi-Fi and site-did-not-answer; a slow or refusing site still shows its error)
- [~] M3.6 Cache-pressure and full-storage behavior (a refused write drops the entry quietly; eviction by size and count is tested in the core crate)
- [ ] M3.7 Image corpus tests, corrupted-image fuzz target
- Gate: image corpus, fuzz smoke, cache-pressure and full-storage scenarios

## M4: real-world compatibility corpus

- [ ] M4.1 Saved, minimized fixtures with provenance: Wikipedia, Rust docs, Hacker News, lightweight blogs, docs sites, search results, bad pages
- [ ] M4.2 Expected title, visible text, link order, page counts per fixture per profile
- [ ] M4.3 WPT subsets: URL parsing, HTML tree construction (if html5ever), encoding, entities, base URL; `tests/wpt/include.txt` and `expected-failures.txt`
- [ ] M4.4 Differential test vs html5ever (and optionally Chromium) on host for visible text and link order
- [ ] M4.5 Reader mode (main-content extraction) for article pages
- [ ] M4.6 Section navigation (heading list), link list per page
- Gate: stable text, link order, screenshots and page counts

## M5: forms and sessions

- [ ] M5.1 Form model: GET forms, single-line text, search inputs, hidden inputs, select/radio/checkbox as choose
- [ ] M5.2 URL-encoded GET submission; POST with a confirmation screen
- [ ] M5.3 Bounded cookie jar and per-origin session storage (needs runtime headers; decide protocol path)
- [ ] M5.4 Bookmarks and history screens with durable storage
- [ ] M5.5 Form URL encoding WPT subset
- Gate: forms fixtures, submission journeys, cookie policy tests

## M6: hardware qualification (needs owner hardware)

- [ ] M6.1 Clara BW, an older single-core Kobo, a larger display, a colour model
- [ ] M6.2 20-page session, rapid Back/Forward, suspend during fetch, network loss during image load, termination, low storage, exit to Cobalt, stock reader restoration, frame timing and residue
- [ ] M6.3 Device RSS, first paint and page-turn timing against M0 budgets

## Testing infrastructure (cross-cutting)

- [ ] Fuzz targets (cargo-fuzz, outside the workspace): parse_html, parse_content_type, resolve_url, decode_image, document_to_ir, paginate_document, cache_metadata
- [ ] CI lanes: PR-fast (fmt, clippy, unit, corpus, IR snapshots, invariants, small WPT), PR-simulator (drive journeys, fixture server, failure scenarios), nightly (large corpus, fuzz smoke, all profiles, size and memory reports)
