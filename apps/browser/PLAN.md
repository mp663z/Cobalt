# Cobalt Browse: plan and task list

Original Level A reader browser for Cobalt: HTML enters a bounded semantic Document IR,
which kobo-ui paginates and draws. The original target had no JavaScript, no author CSS
and no scrolling. On September 25 the owner raised the target to "at least the beta
Nickel Kobo browser" and explicitly requested CSS parsing. The reader build remains
real, but it is NOT yet Nickel-equivalent. The extension below is a proposed
re-baseline, not a claim that the user has approved a particular engine or tradeoff.

Branch: `agent/browser` on mp663z/Cobalt, cut from upstream `beta` at fc88fd3.

## Current colour work, November 24, 2026

- [x] Supported fresh arrivals on confirmed Colour profiles open in RGB Page view.
- [x] Late Colour identity republishes an already-visible Page raster in RGB.
- [x] Native fetched images prepared before identity are retired and re-requested
  in the confirmed panel format. Stale task completions and duplicate identities
  are covered; native position and controls stay in place.
- [x] Keep authored chromatic text colours instead of the neutral contrast floor.
  Regular, bold, italic, code, links and backgrounds have exact RGB pixel coverage.
- [x] Carry supported same-DOM authored RGB into semantic Reader prose/links,
  headings, quotes, preformatted blocks, table cells and explicit control titles.
  Forms, saved-copy notices and restored/fragment positions retain native paths.
  Late identity reparses retained source while keeping edited fields/place/notices.
  Protocol20 carries the combined extensions, preserves old plain bytes and reads
  versions11-19. Matching runtime required. Bound excessive span fragmentation
  with a visible simplification note and preserve complete text.
- [x] Paint retained inline bold/slant treatments in coloured native blocks at
  their existing sizes. Keep these distinct from true font-family/variant support.
- [ ] Complete native code/preformatted monospaced face selection, nested
  multicolour control labels and per-option/editable value styling. These are
  fidelity gaps, not a reason to remove native forms or claim all-fonts support.
- [ ] Verify colour quality, memory and latency on physical hardware.

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
- [x] M3.2 Compressed and decoded pixel limits, downsample to panel width, grey + dither via kobo-image (grey and colour: a colour panel, known from the device identity, gets RGB for colour pictures and grey for grey ones; kept under picture:rgb keys)
- [~] M3.3 `alt` fallback and broken-image placeholder (description always shown under the box; a failed picture leaves the empty frame)
- [x] M3.4 Bounded LRU disk cache for pages and transformed images (app store keys), eviction policy, cache metadata format (pages and fitted pictures share one index under `page-cache-index`: 24 MiB / 200 entries, least recently read first out, strays swept at start; a picture is kept per room size, so another text size fits it afresh)
- [x] M3.5 Offline reading of cached pages, labelled as cached with time (only for No Wi-Fi and site-did-not-answer; a slow or refusing site still shows its error)
- [~] M3.6 Cache-pressure and full-storage behavior (a refused write drops the entry quietly; eviction by size and count is tested in the core crate)
- [x] M3.7 Image corpus tests, corrupted-image fuzz target (18 files in `apps/browser/tests/images`, a seeded damage test in the normal suite, and cargo-fuzz targets in `crates/kobo-browser/fuzz` with `smoke.sh`; the first smoke run found a cache index bug, fixed)
- Gate: image corpus, fuzz smoke, cache-pressure and full-storage scenarios

## M4: real-world compatibility corpus

- [x] M4.1 Saved, minimized fixtures with provenance: Wikipedia, Rust docs, Hacker News, lightweight blogs, docs sites, search results, bad pages (8 in `tests/corpus`, sources and licences in `SOURCES.md`; `minimize.py` is checked to leave the reading unchanged; Hacker News left out for licence; bad pages: cut short, Windows-1252)
- [x] M4.2 Expected title, visible text, link order, page counts per fixture per profile (`tests/corpus/expected`, `BLESS_CORPUS=1` rewrites; page counts on 3 distinct screens x 3 text sizes, release builds only since the long article takes minutes unoptimised)
- [x] M4.3 WPT subsets: URL parsing, HTML tree construction (if html5ever), encoding, entities, base URL; `tests/wpt/include.txt` and `expected-failures.txt` (in `crates/kobo-web-document/tests/wpt`. URL: 506 http(s) cases, 465 pass, 39 refused on purpose, 2 gaps. Entities: all 2231 plus numeric repairs. Encoding: the Windows-1252 index; other legacy encodings not decoded. Base URL: restated cases, two bugs fixed. Tree construction: 1739 html5lib cases through our DOM sink, 1712 pass; 14 differ by design (attribute namespaces, 32-attribute cap), 13 are rules html5ever 0.39 and 0.40 do not have yet)
- [x] M4.4 Differential test vs html5ever (and optionally Chromium) on host for visible text and link order (against Chrome, `tests/differential`, run by hand: every link on all 8 pages matches in order; words match except image descriptions, controls outside forms and literal `|`. It found links inside `<code>` being dropped and a minimiser bug, both fixed)
- [x] M4.5 Reader mode (main-content extraction) for article pages
  - main content detected (`<main>`, role=main, else the skip-link target); new pages open on the page where it starts
  - Reader in the bar shows only that content, from the top of a screen, without navigation, sidebars, site header and footer, or language lists; Whole page goes back to where the page was
  - known gap: in-article chrome with no markup to tell it apart (Wikipedia's "Edit links", "[edit]") stays
- [x] M4.6 Section navigation (heading list), link list per page (Sections lists headings in the active whole or reader view and jumps to their paginated place; the reader view uses a Navigate chooser to keep both lists reachable within the two-action bar limit; Links lists only the current screen, including links in headings and tables; app and Wikipedia fixture tests, simulator screenshots across three profiles)
- Gate: stable text, link order, screenshots and page counts

## M5: forms and sessions

- [x] M5.1 Form model: GET forms, single-line text, search inputs, hidden inputs, select/radio/checkbox as choose (bounded IR and form UI, text editor, selection lists and checkbox toggles; sample form and tests across three profiles and text sizes)
- [~] M5.2 URL-encoded GET submission; POST with a confirmation screen (GET sends successful controls in order and replaces query; POST shows a review screen but cannot send until runtime supports request bodies, so no POST effect occurs)
- [ ] M5.3 Bounded cookie jar and per-origin session storage (needs runtime headers; decide protocol path)
- [x] M5.4 Bookmarks and history screens with durable storage (bounded 100-entry lists, versioned length-prefixed store value; bookmarks toggle from Navigate; recently visited pages deduplicated; restart, malformed-store, page-fit and simulator journeys)
- [x] M5.5 Form URL encoding WPT subset (18 UTF-8 string-entry cases pinned from WPT urlencoded2.window.js; CRLF normalization, NUL, quotes, backslash, non-ASCII; file values, formdata-event duplicate, non-UTF-8 and lone surrogates out of scope).
- Gate: forms fixtures, submission journeys, cookie policy tests

## M6: hardware qualification (needs owner hardware)

- [ ] M6.1 Clara BW, an older single-core Kobo, a larger display, a colour model
- [ ] M6.2 20-page session, rapid Back/Forward, suspend during fetch, network loss during image load, termination, low storage, exit to Cobalt, stock reader restoration, frame timing and residue
- [ ] M6.3 Device RSS, first paint and page-turn timing against M0 budgets

## Re-baseline proposal: beta Nickel browser bar (September 25, 2026)

The owner now wants CSS parsing and at least beta Nickel browser capability. Kobo's
beta-feature page confirms that the browser exists on current readers but does not
specify behavior: https://help.kobo.com/hc/en-us/articles/360017763733-About-Beta-Features .
A June 2026 Libra Colour hands-on probe reports older WebKit (UA 538.1), working
JavaScript-dependent editing and localStorage, but no fetch/XHR and no CSS
Flexbox/Grid: https://robertcedwards.com/posts/eink-notepad-kobo/ . This is one
model/firmware, not a universal specification. An older Touch review reports
CSS layout and scripting features but describes a different engine/version:
https://broken-links.com/2012/07/27/browser-review-kobo-touch/ . Reports of
localStorage conflict across models/years:
https://github.com/kobolabs/Kobo-Reader/issues/59 . Use physical-device probes,
not a WebKit version or anecdotes alone, as the acceptance oracle.

- [ ] N0. Measure the target. On available current Kobo firmware and an older
  supported model, probe UA, HTML/CSS selector and layout features, media
  queries, scripting (DOM events, `contentEditable`, localStorage), native
  form GET/POST, cookies, downloads, redirects, scrolling/zoom, history and
  offline errors. Keep the probe pages, screenshots, results and firmware IDs.
  An inaccessible device stays an explicit qualification gap, not a pass.
  Confirmed firmware baseline: Kobo's official beta-browser table lists Clara BW,
  Clara Colour, Clara 2E and Nia. Official update packages for Clara BW N365
  and P365 (4.46.23836, August 2026), Nia N306 (4.38.23684, April 2026)
  and Clara 2E N506 (same April release) each contain a 19,941,528-byte
  ARM Qt WebKit shared library with a `libQt5WebKit.so.5` symlink; each
  `nickel` ELF links it and WebKitWidgets. Official model/update map:
  https://help.kobo.com/hc/en-us/articles/35059171032727-Manually-Updating-your-Kobo-eReader-device-Firmware .
  Downloaded packages inspected, not executed on a reader:
  https://ereaderfiles.kobo.com/firmwares/kobo12/Aug2026/kobo-update-4.46.23836.zip
  https://ereaderfiles.kobo.com/firmwares/kobo14/Aug2026/kobo-update-4.46.23836.zip
  https://ereaderfiles.kobo.com/firmwares/kobo7/Apr2026/kobo-update-4.38.23684.zip
  https://ereaderfiles.kobo.com/firmwares/kobo10/Apr2026/kobo-update-4.38.23684.zip .
  This verifies shipped engine bits and linkage, not active browser RSS,
  exact engine source revision, or on-device web-feature behavior. A Clara BW
  SSH report independently finds one CPU and 438 MiB usable of nominal 512 MB:
  https://leo3418.github.io/2025/12/26/kobo-clara-bw-ssh.html . Its ~178 MiB
  ordinary-session process use is not browser-active memory. Measure Nickel's
  idle and browser-active PSS/RSS with a real device before comparing engines.
- [ ] N1. Add a safe CSS parser and style model, not just token scanning.
  Measure parser/selector dependency cost on armv7 and add its license before
  adopting one. Support bounded inline `<style>`, `style` attributes and
  linked stylesheets through runtime fetch (explicit cross-origin policy,
  MIME/size/request-count limits). Parse selectors `type`, `.class`, `#id`, descendant/child and
  grouped selectors; cascade specificity, source order, inheritance and a
  bounded `!important`, with conservative unsupported-declaration fallback.
  Sanitize `url()`, `@import`, visited-link disclosure and external fonts;
  no background asset fetch until separately bounded and tested.
- [ ] N2. Reader-first CSS rendering subset: `display:none`, block/inline,
  headings/strong/emphasis, font size/weight/style, text alignment and
  decoration, line height, whitespace, margins/padding, simple borders,
  foreground/background colours mapped to panel grey or colour. Retain
  readable contrast and minimum tap targets. Check what Document IR and
  kobo-ui can actually express; do not claim `display:flex`, grid, float,
  absolute positioning, arbitrary box geometry, transitions or pixel fidelity
  from this subset. Preserve a readable unstyled mode and reader view.
- [ ] N3. Browser-critical network protocol: return final URL, status,
  content type/charset and selected bounded response headers; add a scoped
  request-body/method path for POST and a per-origin cookie jar with explicit
  persistence/clear policy. Version protocol/runtime/app/simulator together,
  restrict redirect credential/header forwarding, and test against local HTTPS
  fixtures. This is a release-review decision, not an unreviewed protocol edit.
- [ ] N4. Basic browser interaction parity: textarea/multiline editing,
  missing ordinary form controls, submitter semantics, POST review and send,
  sign-in/session behavior, images without declared dimensions, address-field
  quick keys, navigable viewport/zoom and an explicit downloads policy.
  Verify per feature against the Nickel probes, not a broad "browser" claim.
- [ ] N5. Engine feasibility gate for *actual* Nickel-level interactive
  pages. Compare two isolated prototypes before selecting a renderer, with
  device measurements and an explicit architecture review:

  | Path | Plausible gain | Unproved cost or blocker |
  | --- | --- | --- |
  | **Device Qt WebKit** in a separately supervised, firmware-native helper | Reuses the engine Nickel actually loads for HTML/CSS, DOM, JavaScript and forms; no ~20 MB WebKit ELF bundled if the installed firmware supplies it. Likely the shortest route to the *observed* beta-browser bar, not modern-web parity. | Cobalt's apps are static musl binaries in a chroot exposing only `/app` and `/runtime.sock`; `nickel` is a dynamically linked, vendor-Qt program. A glibc/Qt C++ helper would need a new tightly scoped launch/runtime model, validated loader and dependencies, framebuffer/input handoff or a bounded offscreen rendering bridge, crash recovery and network/credential isolation. Existing Cobalt app sandbox rules cannot silently be relaxed to let an untrusted engine open device nodes or sockets. |
  | **Modern embedded engine** (Servo or another demonstrably portable option) | May cover newer CSS/JS/web APIs than Nickel. | Build and port for this exact armv7 Linux target; unsupported assumptions about static-musl, graphic backend, e-ink input and fonts, architecture-specific JS/JIT, TLS and sandboxing. Bundle size, load time, PSS/RSS and ongoing patch burden are unknown until measured. Servo's general WebView description alone proves none of these: https://servo.org/ . |

  Firmware inspection establishes that official Clara BW N365/P365 4.46.23836
  packages carry identical SHA-256
  `5d97e31bd81af10dbe6390b9439146acc922b29dd230387d5156c4232ea1775a`
  for the 19,941,528-byte ARM WebKit ELF. Nia/Clara 2E 4.38.23684 carry
  identical SHA-256
  `cb030032404adc9e09c554b53aa003ce2484111d101aac8371fdd833dfbaa485`:
  same size, **different hash across firmware waves**. These are four
  inspected packages, not eight or nine targets or an ABI guarantee. A
  `libQt5WebKit.so.5` symlink ultimately points into a legacy-named Qt path;
  the path's version string is not proof of the actual Qt/WebKit source version.
  Before support, inventory exact ELF SONAME, interpreter, needed libraries,
  GLIBC/GLIBCXX and Qt symbols, loader/plugin paths and signatures/hashes per
  model and firmware branch; confirm on-device behavior and fail closed on
  unsupported or mismatched libraries. Never overwrite, shadow or redistribute
  vendor libraries as a shortcut. This is an exception to the static-musl and
  no-device-library rule in `.cargo/config.toml`, requiring explicit review.

  Budget both paths against `BUDGETS.md` (the current stripped browser budget
  is +5 MiB over the simple app; peak RSS budget +32 MiB). Device reuse avoids
  *bundling* the 19.9 MB engine file, not its loaded memory. Capture idle
  Cobalt and Nickel, browser-active and 20-page session `/proc/*/smaps_rollup`
  PSS/RSS (including helper, Qt dependencies, runtime and browser), peak
  allocations, startup/first paint, page turns, suspend/restart and low-memory
  recovery on low-end and colour devices. Do not compare a disk ELF against
  RSS or ordinary-session process use. With a helper, measure pixel or bounded
  tile transport, touch and keyboard focus, e-ink refresh, exclusive-display
  ownership and clean return to Nickel. Add origin-isolated cookies/storage,
  web-to-native IPC validation, URL/navigation limits and a clear policy on
  whether requests go through runtime N3 or a separately confined Qt stack;
  do not give WebKit direct unchecked access to Cobalt secrets or unrestricted
  network. Old WebKit may meet Nickel's beta bar but has a greater unsupported
  sites and web-content security burden than a maintained modern engine.

  Licensing is a review gate, not a dynamic-linking slogan. Qt's own LGPL
  guidance describes obligations for notices, source, replacement/relinking and
  distribution: https://www.qt.io/development/open-source-lgpl-obligations .
  Qt 5.6 also inventories separately licensed WebKit components:
  https://doc.qt.io/archives/qt-5.6/licensing.html . Establish the *actual*
  Kobo binary's applicable licenses, corresponding source and third-party
  notices, whether the library is only used from the owner's already-installed
  firmware or is distributed, and how Cobalt's signed distribution/updates
  affect replacement rights. Seek legal review before release. Kobo Qt app and
  platform-plugin projects show native GUI ports, but use separate Qt runtimes,
  not proven safe linking to Nickel's system WebKit:
  https://github.com/Szybet/nickel-qt-apps/blob/main/README.md and
  https://github.com/Rain92/qt5-kobo-platform-plugin .

  Gate the choice on an isolated *read-only* ABI/load probe, then a guarded
  physical-device prototype with stock-reader recovery and a matched Nickel
  feature/page test suite from N0. Only then choose Qt helper, modern embed,
  or a clearly named reader-plus-CSS target. Neither path is shipped or
  declared feasible by this comparison.
- [ ] N6. Qualification: CSS/JS/form WPT subsets as applicable, real-site
  screenshots against Nickel on each target profile, security fixtures for
  hostile CSS and network content, simulator regressions and actual hardware
  memory/refresh/suspend/20-page journeys. Add no new CI workflow yet.

**Execution order:** Measure N0, then run the N5 engine spike before committing
to a custom box-layout/JS implementation; network/credential policy is needed
whichever renderer wins, though the exact N3 API may differ for a guarded
engine helper. If both engine paths fail the measured device/sandbox budget,
N1-N2 remain a valuable styled-reader track, but not an honest parity claim.

**Acceptance language:** The existing M1-M5 document-reader work is a base,
not completed Nickel parity. N1-N2 alone make styled documents more useful
but do not meet the observed JavaScript/editing bar. M6 hardware work expands
into N0/N6. Keep M2.3/M2.5 and M5.2/M5.3 open until N3; keep the known
legacy-encoding limit unless a separately measured dependency decision changes
it. Do not open a PR without the owner's OK.

**Decisions now framed against the new bar:** (1) Whether to authorize the
cross-component N3 protocol change, strongly recommended for meaningful
forms/session/redirect behavior; (2) default search provider and truthful
user agent, now selected against live-site compatibility rather than a guess;
(3) how to resolve the kobo-ui release-review source pin (test 64 currently
red). In addition, the Nickel-parity request itself needs the N5 engine
feasibility result before choosing full interactive parity versus a named
reader-plus-CSS compromise. No dependency, protocol or PR authorization is
implied by recording this proposal.

## Testing infrastructure (cross-cutting)

- [ ] Fuzz targets (cargo-fuzz, outside the workspace): parse_html, parse_content_type, resolve_url, decode_image, document_to_ir, paginate_document, cache_metadata
- [ ] CI lanes: PR-fast (fmt, clippy, unit, corpus, IR snapshots, invariants, small WPT), PR-simulator (drive journeys, fixture server, failure scenarios), nightly (large corpus, fuzz smoke, all profiles, size and memory reports)


## Active implementation checkpoint, November 23, 2026

The user wants a basic lightweight real browser, at least as good as the stock
Kobo browser. This is not an exact-parity gate or a reader-only compromise.
The historical sections above retain their original status and proposals.

- Images: Page view now decodes bounded PNG/JPEG base64 data images using the
  existing decoder. Successful images preserve link targets; unsupported or
  over-budget images are counted and do not paint black sentinel boxes.
  Aggregate compressed bytes are capped at 4 MiB and decoded pixels at
  1,048,576, behind the DOM's separate lower resource-attribute retention
  limits. Successful decodes fit their first laid-out container width with
  one bounded re-layout, removing the alt-text line from layout. Geometry
  remains a counted approximation, with nearest sampling, no external image
  fetch and no native SVG decoding. The local
  image sample and image/link drive journey exercise the actual simulator.
- Engine: native HTML tables now retain their tags and use Page-only UA table
  roles with shared intrinsic columns, padding, separated solid borders,
  short rows, bounded colspan, pixel row minima, row-group backgrounds and
  top inline captions with padding/solid borders. Neutral single-block cell
  wrappers survive, including formatting whitespace. Rowspan, multiple/styled
  block contents, collapsed/dashed borders and multiple captions retain a
  counted readable stack. Cell vertical alignment remains a counted top-align
  approximation. This is useful table support, not complete CSS tables.
- Chrome: search-first launch, persistent URL field, inline caret editing,
  number/symbol keyboards, back/forward/reload, zoom controls with draggable
  slider, home/favourites menu and browser close are implemented. All endpoints
  were rebuilt together and both Clara simulator start/editing journeys plus
  a BW Page/link/pagination/Reader journey passed. Bilinear zoom reduces rough
  sampling but is not native-resolution typography or a hardware quality claim.
- Validation: latest engine checkpoint has document 1395/0 (18 ignored), browser 195/0
  (2 ignored), preview 47/0 with the unchanged 13-page corpus. Restoring the
  original synthetic Clara fixture removed the simulator compilation blocker:
  simulator 108/0, profile 65/0 and Clara safety scripts 7/0, including slider
  down/move/release. Synthetic evidence never authorizes physical checks.
- Provider stays unchanged until the pending user choice is answered.
- Measurements: current ARM binary size is in BUDGETS.md; device RSS, speed
  and hardware interaction remain unverified. Upstream app port is incomplete.


## Native authored colour implementation plan, November 24, 2026

Historical checkpoint before the November24 native RGB increments below:
the native renderer could not carry arbitrary authored RGB text. This was
not an image decoder limitation. `Inline` loses source/style identity during
semantic conversion, layout `Run` retains only strong/emphasis/link, and the
shared `TextPresentation` wire byte encodes six style flags, no colour.

A safe implementation should proceed in independently testable increments:

1. Preserve source provenance during the same bounded DOM conversion, before
   whitespace collapse, nested emphasis/link construction, trimming and code
   flattening. Style-tree indices are NOT DOM handles: generated content and
   filtered nodes change the index space. Use an explicit mapping emitted by
   the style walk over the same parsed DOM. No second parse/string matching.
   Synthesized labels and truncation notices have no author source and retain
   toolkit colours. Preserve existing unstyled parse entry-point behavior.
2. Carry optional authored foreground/background on bounded semantic spans,
   then through native flattening, paragraph splitting, link splitting and
   pagination. Do not recolour whole paragraphs from the first matching text.
   Test duplicate words with different colours, adjacent source nodes joined
   by whitespace collapse, inherited colours, nested links/emphasis/code,
   hidden content, generated content, truncation and reader-view selection.
3. Add explicit RGB span representation with versioned wire compatibility.
   The existing style byte has two spare bits, not room for RGB; never change
   its interpretation silently. Verify old peers refuse unsupported nodes
   cleanly. Runtime, app and simulator must be rebuilt and qualified together.
   Keep uncoloured native frames allocation-compatible where possible.
4. Render native RGB glyph coverage and matching link decoration against the
   span background. Use the existing colour Surface support rather than a
   full-screen picture overlay, which would discard native control semantics.
   Verify neutral and chromatic behaviour separately on BW and Colour paths.
5. Exercise native form controls, restored/fragment positions, saved-copy
   notices, small/large text, missing glyphs, cache reopen and late identity.
   Inspect actual full rendered frames and remeasure ARM size and memory.

This is a plan, not completed support. Do not remove the current native-form,
position or saved-copy safeguards to make the Colour screenshot pass.


### Native colour provenance increment

The style walk now has a crate-private opt-in source-map entry. Each retained
style node maps to its exact handle in the caller's same DOM; generated pseudo
nodes map to None. Default parse entries do not allocate this map. Tests cover
identical words with different inherited colours, hidden subtrees, generated
pseudo boxes and equality with ordinary style-walk output. Substituting the
style-arena index for the DOM handle makes the regression fail.

This does not yet preserve provenance through semantic Inline conversion and
does not paint native text in colour. The next increment must carry provenance
through semantic whitespace collapse/trim and code/link conversion before any
RGB span is sent to the native renderer. The helper is deliberately private;
no new protocol or public model fields have been introduced.


### Normalized text source-range foundation

A crate-private paragraph buffer now records normalized UTF-8 byte ranges and
same-DOM source handles through adjacent text append and edge trim. Adjacent
ranges merge only for the same source. A collapsed boundary space keeps the
first emitting source. Synthetic text has no source. Byte/span limits reject
an append atomically. The existing converter uses the exact same extracted
whitespace normalizer, with existing semantic output preserved by the full
engine/browser suites. Unicode, NBSP, repeated words, whitespace-only input,
span caps and 64 small input partitions are checked; a trim-rebasing mutation
fails the tests.

This buffer is not yet installed in Gather/Inline: nested inline, code and
link conversion still need end-to-end provenance. No native RGB span or wire
support has been claimed. Integration must carry ranges with transformations,
not reconstruct them by looking for matching strings after conversion.


### Native RGB toolkit/wire increment

Optional RGB foreground/background now draw through native TextPresentation.
Colour rich text uses protocol 16 node tag 35; ordinary rich text still uses
unchanged tag 28. Old-version colour encoding refuses, and runtime/simulator
accept versions 11-15 plus 16. Installed-font and bitmap-fallback pixel checks
pass without geometry changes. This does not yet map semantic browser text
sources to these spans. Next increment is actual semantic provenance integration,
not another renderer helper. Inline link hit targets remain intact; authored
link underline colour still needs the end-to-end link span treatment. New apps
must ship with the matching runtime, not silently target an old decoder.


### First native semantic colour integration

Colour arrivals now opt into coloured semantic text leaves produced from the
same DOM style walk. Leaf source identity is consumed directly before collapse,
not reconstructed by matching strings. Native prose keeps RGB through run
splits/pagination into versioned text spans. Form pages remain semantic and
GET controls work in the RGB regression. Unstyled/BW parser behavior remains.

Remaining native paths: headings/quotes/tables/preformatted/control labels,
neutral link-target underline, late identity text reparse, and unsupported CSS
colour fidelity. A coloured paragraph is useful support, not all-fonts support.
SourceText remains a tested normalization primitive, not a claim that every
nested conversion already routes through it. Ordinary code keeps the existing
native face; the coloured leaf preserves its code semantic flag for later work.

### Combined native colour audit, November 24, 2026, 06:53 IST

The historical increments above are superseded by the current colour checklist
at the top and README's coverage table. Native RGB now reaches prose/links,
headings,quotes,preformatted blocks,table cells and authored control titles;
late identity preserves edited forms/place/notices. Protocol20 is current.
No claim of arbitrary CSS/fonts or physical readiness. Inline code/pre mono,
complex control styling,true variant selection and device measurements remain.
Shared consumer audit after the protocol/toolkit additions: book reader82/0,
flashcards binary14/0,frame-host7/0. This supplements whole-workspace compile,
not a replacement for full workspace tests or any independent proof gate.


#### November24 native preformatted follow-through

Preformatted blocks now measure and paint with the embedded fixed-pitch face,
including RGB runs. Dedicated document-mono scaling preserves independent
reading size, while existing terminal/UI mono sizing is unchanged. Protocol21
tag40 preserves ordinary tag bytes and rejects mono blocks on older peers.
Long-token emergency wrapping also uses the selected face. Current full-frame
Colour evidence is in screenshots/native-colour-blocks.png. See BUDGETS.md for
checks and limits. Inline code inside prose, arbitrary authored font families,
true font variants, complex control styles and physical panel/RSS/latency are
still incomplete or unverified. This does not close the broad browser goal.


#### November24 native mixed inline-code follow-through

Prose code runs now retain semantic code identity and use fixed pitch for both
measurement and RGB paint, including wrapped link and selection rectangles.
Protocol22 tag41 keeps legacy style bytes unchanged; older peers reject the
new optional node. See BUDGETS.md for failure/restoration, profile coverage and
actual Colour evidence. Code in heading/quote/table blocks remains in the
block face, and arbitrary CSS families/true variants/hardware remain open.

#### November24 uncoloured semantic-span follow-through

Native headings/quotes/table cells retain semantic bold/emphasis even without
an authored RGB colour. Tests refuse invented foreground/background, and the
full raster remains greyscale. Code-only fragmentation also triggers the
existing simplification notice. See BUDGETS.md. Block-inline code face and
arbitrary font/physical-device limits remain open.


#### November24 native Colour computed-style follow-through

Same-DOM native Colour text carries inherited computed bold/italic treatments,
including normal resets overriding strong/em tags. Existing synthetic glyph
routines remain; this is not true font variants or arbitrary families. Plain
parser behavior stays stable. Incomplete style mappings retain semantic tags.
BUDGETS.md records tests,failed mutation,restoration,raster and ARM size.

#### November24 raw preformatted style follow-through

Raw preformatted source traversal now retains computed font treatments while
keeping UTF8 spacing/blank lines and normal resets. It still measures and paints
in the existing mono face. See BUDGETS.md for fail/restore,full-raster and checks.
No arbitrary font family,true-variant or hardware claim.

## Mobile-browser experience, November 24, 2026

The user's latest target is a real mobile-browser experience, not successive
colour-only screens. This sequence supersedes small font follow-through as the
main stream. The existing browser is still paginated, not a scrolling browser.

- [x] Direct taps on visible painted links, at 75/100/150/200% zoom. Source-pixel
  regions follow the raster's scaling/page crop. Keyboard/toolbar controls win;
  ambiguous overlaps refuse, with Links as fallback. Protocol23 optional tag42,
  old unlinked-picture tag17 unchanged. Requires a matching runtime for links.
- [x] Consolidate title/address into one explicit browser header, retain Back
  and Close, and route Navigate through the existing bottom More. Native and
  painted page measurement uses the final frame, including picture fit and
  page-position footer. Zoom remains directly draggable in this checkpoint.
- [ ] Add page movement/scroll interaction appropriate to an e-ink display,
  with predictable link taps and restore behavior. Do not call page turns scroll.
- [ ] Audit address/search, Back/Forward/restore, keyboard, forms and navigation
  together on complete journeys, not isolated colour samples.

Direct-link checkpoint: `drive-painted-touch.kobo` records a coordinate tap on
Clara Colour, arrival at the article, and Back. Unit journey covers both panel
formats and four zooms, plus opening/dismissing the keyboard. A deliberately
removed attachment fails the same test before restoration. Neither simulator
success nor direct taps establish hardware latency, RSS or full HTML/CSS support.

## Settled painted-viewport scrolling, November 24, 2026

- [x] Drag a painted page vertically to a real CSS offset proportional to finger
  travel, clamped to content. One refresh on release, no per-motion animation.
- [x] Share the gesture recognizer between simulator and device input paths.
  Tiny taps still activate links; horizontal/moved releases cannot hit a link.
  Cancel, overlay, keyboard, layout or foreground changes retire the gesture.
- [x] Move raster and link geometry through the same viewport window; keep
  physical/page controls as explicit page navigation, not fake scrolling.
- [x] Keep bounded offsets for Back/Forward within this running session (64 URL
  entries, with zoom); reload/page navigation retire the stored offset.
- [ ] Durable scroll restoration across app restarts, native reader/form scroll,
  inertial/continuous feedback and physical panel qualification.

Native reader/forms remain paginated. Page position reports the containing page
of the offset, not a continuous scrollbar. The initial implementation settles on
release, which is intentionally different from live phone-browser scrolling.
