# Cobalt Browse: budgets and measurements

Historical baseline measured September 25, 2026 on branch `agent/browser-m0`, from upstream beta fc88fd3.
Current branch measurement is recorded separately below.
Device numbers (RSS, first paint, page turn on a real reader) are not measured yet
and are left to hardware qualification (M6). Everything below says where it came from.

## Stripped release binaries, armv7-unknown-linux-musleabihf

Built by the `Browser measurement spike` workflow, run
https://github.com/mp663z/Cobalt/actions/runs/36058259809 (thin LTO, abort on panic,
symbols stripped, the workspace release profile). The three probes link the same SDK
application; two of them also parse a page with a parser, so the difference is the parser.

| Binary | Bytes | Over baseline |
| --- | ---: | ---: |
| kobod | 5,069,452 | |
| kobo-hello (simple app) | 1,249,372 | |
| probe_baseline | 1,238,404 | |
| probe_kobodoc (in-tree `kobo-doc::html` scanner) | 1,380,732 | +142,328 |
| probe_html5ever (html5ever 0.39 + rcdom) | 1,772,524 | +534,120 |

Budget: stripped browser binary no more than 5 MiB over a simple app. html5ever costs
0.51 MiB of it.

## Host parse cost (x86_64, release, one run each)

Saved pages, not merge gates. Peak RSS is for the whole probe process.

| Page | Size | kobo-doc | html5ever + rcdom |
| --- | ---: | --- | --- |
| news.ycombinator.com | 34 KB | 0.9 ms, 9.1 MB | 1.1 ms, 9.2 MB |
| blog.rust-lang.org | 97 KB | 1.8 ms, 9.1 MB | 2.6 ms, 9.1 MB |
| Wikipedia, Electronic paper | 380 KB | 5.4 ms, 9.1 MB | 6.0 ms, 9.1 MB |
| doc.rust-lang.org Vec | 953 KB | 18 ms, 9.1 MB | 23 ms, 13.1 MB |
| 2 MiB synthetic (Wikipedia repeated) | 2 MiB | 30 ms, 9.2 MB | 35 ms, 17.4 MB |

Budget: peak RSS no more than 32 MiB over a simple app. The worst case above is
8 MB over baseline with a reference-counted tree. This does not measure the current
browser process or establish its RSS budget compliance.

## Decision

html5ever. It is 0.4 MiB more than the in-tree scanner on the device and a few
milliseconds slower on the host, and in return malformed HTML recovers the way it
does in every browser. The in-tree scanner has no tree, no implied end tags and no
table handling by design, which is right for EPUB and wrong for the open web.

## Hard limits (`kobo_web_document::Limits::DEFAULT`)

| Limit | Value |
| --- | ---: |
| Markup bytes read | 2 MiB |
| Parser nodes | 120,000 |
| Nesting read into the model | 96 |
| Text kept | 1 MiB |
| Blocks | 8,000 |
| Links | 4,000 |
| Images | 200 |
| Table rows x columns | 200 x 12 |
| Forms | 16 |
| Attributes per element | 32, each at most 2 KiB |
| URL length | 4 KiB |
| Redirects | 5 (runtime) |

Past a limit the page is still shown, with a warning saying what was left out.

## Page text fallback and shaping, November 23, 2026

The Page view font session adds the existing locked rustybuzz 0.20.1 dependency
and two renamed OFL font subsets. No new package version was resolved.

| Asset | Bytes |
| --- | ---: |
| Kobo Page Cyrillic, Noto Sans derived | 94,208 |
| Kobo Page Devanagari, Noto Sans Devanagari derived | 201,852 |
| Total embedded fallback bytes | 296,060 |

Both retain OpenType layout tables. A broad Japanese kana/CJK subset measured
13,377,744 bytes, so it is not bundled: that exceeds the entire +5 MiB binary
budget. Japanese coverage requires a caller-provided face through `add_fallback`.

A stripped x86_64 release preview measured 2,976,296 bytes with shaping and both
fallbacks, versus 1,940,008 bytes for the scalar preview on b40f4ccc, a 1,036,288
byte increase. This comparison includes intervening text-engine changes and is
not an isolated shaper cost. It is not an armv7 binary result. Device binary size
and peak RSS remain unverified; this does not establish compliance with either
the +5 MiB binary or +32 MiB RSS budget.

Shaping input is capped at 65,536 bytes per run. A shaped cluster mask is capped
at 262,144 bytes and cumulative shaped masks at 4 MiB per run. Font fallbacks
remain capped at eight. These are safety caps, not a measured device budget.

### Script fallback cuts

Cyrillic bold 95,052 B, italic 102,940 B, bold-italic 104,424 B, and
Devanagari bold 210,224 B add 512,640 font bytes. All are renamed subsets of
the same OFL Noto distribution and retain all OpenType layout tables.
Total bundled fallback bytes become 808,700 B. Devanagari italic is not
available in these sources and is not synthesized. The font catalog allows
16 explicit faces rather than eight to accommodate real style cuts; the
shaping and raster-mask bounds are unchanged. ARM size/RSS remain unmeasured.

### RTL paragraph path

The already locked unicode-bidi 0.3.18 is used for resolved paragraph levels
and visual ordering separately on each wrapped line. Arabic regular adds
215,640 B and Hebrew regular adds 22,964 B, renamed OFL Noto subsets, making
1,047,304 B of bundled fallback fonts in total. This is font bytes only;
ARM binary cost and device RSS are not established by this count.

Arabic bold adds 224,260 B and Hebrew bold adds 22,988 B, both true Noto OFL
cuts, bringing bundled fallback font bytes to 1,294,552. No italic substitute
is synthesized. As above, this is not an ARM size or RSS measurement.


## Current branch ARM cross-build, November 23, 2026

Measured source: `4810a98808b33900fb445905d8f390ce75d73ad1`,
`agent/browser-basic`. Local cross-build, not a GitHub Actions run. The fork's
Actions permissions read enabled and both enable requests returned HTTP 204,
but workflow dispatch still returned HTTP 422 reporting Actions disabled.
No upstream repository settings were changed.

Toolchain: Rust 1.85.1, `armv7-unknown-linux-musleabihf`, locked dependencies,
workspace release profile (thin LTO, panic abort, symbols stripped). C compiler:
Zig 0.16.0, archive SHA-256
`70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00`,
`cc -target arm-linux-musleabihf -fno-sanitize=undefined`, matching the spike's
pinned wrapper. All six outputs inspected as stripped, static ARM EABI5 ELF.

| Binary | Bytes | Over simple app |
| --- | ---: | ---: |
| kobod | 5,077,268 | |
| kobo-hello | 1,249,388 | baseline |
| kobo-browser | 5,110,364 | +3,860,976 |
| probe_baseline | 1,238,420 | |
| probe_kobodoc | 1,380,756 | |
| probe_html5ever | 1,772,572 | |

The browser is 3.682 MiB over the simple app, below the +5 MiB binary limit
by 1,381,904 bytes for this source revision. Probe parser deltas relative to
probe_baseline are 142,336 bytes (kobodoc) and 534,152 bytes (html5ever).
This is a binary-size result only. None of these ARM executables was run on
a reader here; device peak RSS, first paint, page-turn speed and smoothness
remain unverified. The older RSS table above is host-x86 only.

Reproduce with `cargo +1.85.1 build --locked --release
--target armv7-unknown-linux-musleabihf -p kobod -p kobo-hello -p kobo-browser`
and the same build flags with `--manifest-path tools/browser-probe/Cargo.toml`.
Set `CC_armv7_unknown_linux_musleabihf` to the pinned wrapper described above.


### Address-bar/start-page follow-up

Source `dc792fa8f` includes the persistent address field and local start page.
The same locked ARM release build produced a 5,117,932-byte browser, 7,568
bytes more than the pre-chrome binary above. Relative to the same hello
baseline, the delta is 3,868,544 bytes, leaving 1,374,336 bytes under the
+5 MiB binary limit. Device RSS and runtime behavior remain unmeasured.

### Native tables and compact browser chrome, November 23, 2026

Measured source: `4edd877177befe3fae44e724349cadf84cba3533`.
The same pinned local ARM cross-build was repeated after native HTML table
intake/grid/fallback and compact vector browser controls were integrated.
All protocol endpoints were rebuilt together. The compiler uses the wrapper
above; the archive wrapper invokes `zig ar`.

| Binary | Bytes | Over simple app |
| --- | ---: | ---: |
| kobo-browser | 5,156,900 | +3,907,520 |
| kobo-hello | 1,249,380 | baseline |
| kobod | 5,086,884 | |

The stripped browser is 1,335,360 bytes below the +5 MiB binary allowance.
`file` identifies it as a stripped, static ARM EABI5 ELF. This measures
linked size only, not runtime memory, first paint, input latency or physical
reader behavior. Device RSS and speed remain unmeasured.

### Caption, colspan, editing and row-height checkpoint, November 23, 2026

Measured source: `0982a69171a3b9e243ef3a078392814fe1594ff4`.
Repeated the same pinned local locked ARM release build of browser, hello
and daemon after caption/colspan, cell/outer border, row minimum height,
caret editing and bilinear zoom changes.

| Binary | Bytes | Over simple app |
| --- | ---: | ---: |
| kobo-browser | 5,167,204 | +3,917,824 |
| kobo-hello | 1,249,380 | baseline |
| kobod | 5,087,012 | |

The browser grew 10,304 bytes from the previous measured checkpoint and
remains 1,325,056 bytes below the +5 MiB allowance. All three files were
inspected as stripped, statically linked ARM EABI5 executables. This is a
linked-size check, not a hardware run. Device RSS, paint time, input latency
and smoothness remain unverified.

### Row groups, neutral wrappers and caption edges, November 23, 2026

Measured source: `b2433ce09d5a91bc142a1cf786f32ab41011e9a2`.
The same pinned locked ARM release build now includes row-group background
layers, neutral cell wrappers with formatting whitespace, padded solid-border
captions and the restored synthetic test fixture.

| Binary | Bytes | Over simple app |
| --- | ---: | ---: |
| kobo-browser | 5,171,348 | +3,921,968 |
| kobo-hello | 1,249,380 | baseline |
| kobod | 5,087,012 | |

The browser grew 4,144 bytes since the prior checkpoint and leaves 1,320,912
bytes under the +5 MiB allowance. All three outputs were inspected as
stripped, static ARM EABI5 executables. No physical reader was used; device
RSS, paint time, input latency and smoothness remain unverified.

### Table cell box sizing and outer padding, November 23, 2026

Measured source: `9119d5dc6ec032c686cffea488234e246168e3f7`.
Repeated the same pinned locked ARM release build after cell content-width
and content-height edge accounting and outer table padding support.

| Binary | Bytes | Over simple app |
| --- | ---: | ---: |
| kobo-browser | 5,171,028 | +3,921,648 |
| kobo-hello | 1,249,380 | baseline |
| kobod | 5,087,012 | |

The browser is 320 bytes smaller than the previous measured source and leaves
1,321,232 bytes under the +5 MiB allowance. All outputs inspected as static,
stripped ARM EABI5. All host endpoints were rebuilt together; fresh BW Page
link/pagination/Reader and Colour inline-editing/keyboard/menu/reload journeys
passed, with actual screenshots inspected. These are simulator and linked-size
checks, not physical-reader RSS, latency or panel-quality measurements.

### Counted cell minimum sizing, November 23, 2026

Measured source: `0102796be52d2fa12eef066392e4671d38aebd1e`.
The same pinned locked ARM release build includes the counted pixel cell
min-width/min-height extension with content-box and border-box edges.

| Binary | Bytes | Over simple app |
| --- | ---: | ---: |
| kobo-browser | 5,171,476 | +3,922,096 |
| kobo-hello | 1,249,380 | baseline |
| kobod | 5,087,012 | |

This is 448 browser bytes above the last checkpoint, with 1,320,784 bytes
remaining under the +5 MiB allowance. All outputs inspected as stripped,
static ARM EABI5. A fresh combined SDK/UI/layout/profile/simulator suite also
passed 652 tests with 4 ignored across 10 test-result groups. This does not
qualify hardware RSS, latency or panel behavior.


### Native PNG/JPEG data images, November 24, 2026

Measured source: `ce37735d69d1f22ef84f5f17d7cff34ccbb8bb45`.
The same pinned locked ARM release build includes native data-image decoding
and the linked-image alt-underline correction.

| Binary | Bytes | Over simple app |
| --- | ---: | ---: |
| kobo-browser | 5,183,492 | +3,934,112 |
| kobo-hello | 1,249,380 | baseline |
| kobod | 5,087,012 | |

This is 12,016 browser bytes above the last documented checkpoint, leaving
1,308,768 bytes under the +5 MiB allowance. All three outputs inspected as
stripped, static ARM EABI5. Browser tests pass 190 with 2 ignored. Aggregate
pixel exhaustion, oversized headers, baseline/progressive/CMYK/greyscale/EXIF
JPEG decoding, and malformed-payload fallback have app-level coverage. The
aggregate regression was also checked against a deliberate reset-per-image
mutation: it fails there and passes after restoration. The existing 4 MiB
compressed and 1,048,576 decoded-pixel page limits remain unchanged.

These are host tests and a cross-build. No physical-reader RSS, latency or
panel behavior is established. Image geometry remains a counted marker-based
approximation; external images and SVG are unavailable. The earlier live
simulator image/link journey is greyscale, not evidence of colour-panel fidelity.


### Image dimension overflow guard, November 24, 2026

Measured source: `4a11dc307418e07ce0bed150e6a2c62aea0f1ecf`. Image marker proportional height now
uses a u64 intermediate and clamps to the existing 4096px limit before
narrowing. A width=801, height=u32::MAX fixture panicked before the fix and
passes afterwards; equal maximum dimensions reserve 800px rather than wrapping.

The locked ARM release browser is 5,184,564 bytes, +3,935,184 over the
unchanged 1,249,380-byte hello baseline. Remaining +5 MiB headroom is 1,307,696
bytes. The browser was inspected as stripped, static ARM EABI5.
Engine suite: 1,395 passed, 0 failed, 18 ignored across 147 result groups.
Browser suite: 191 passed, 0 failed, 2 ignored. The normal native image RGB
raster was regenerated and visually inspected with no new skips, one counted
marker-geometry approximation. This does not change the image support scope
or establish physical-reader behavior.


### Page-only fitted image geometry, November 24, 2026

Measured source: `0b52916e5d0a9411d33534a9944bc435ac6c1323`. Decoded images use their first laid-out
container width for an app-only fit and bounded re-layout. Successful alt
text no longer reserves an extra line. An 80px container holding a 400x200
image now reserves 40px rather than 220px. This stays a counted marker-geometry
approximation; CSS replaced-element conformance and exact/offline paths are
unchanged. A width-dependent ancestor can still exceed this single correction.

The locked ARM release browser is 5,188,644 bytes, +3,939,264 over hello,
leaving 1,303,616 bytes under the +5 MiB allowance. Stripped static ARM EABI5
was inspected. Browser: 195 passed, 0 failed, 2 ignored. Clippy completed
with existing warnings. Narrow-container pixel bounds, multi-page images,
following text, linked-image targets and fallback tests pass. Actual normal
and narrow app rasters were inspected. The fresh Clara Colour simulator
image-to-links-to-article journey passes; its screenshots are greyscale and
do not qualify colour-panel fidelity, device RSS or latency.


### Zero-width and nested EXIF images, November 24, 2026

Measured source: `8b2e1955922339050f5dc68faf6e0a842bde7e5f`. A successful image inside a directly
containing normal block with explicit width:0 now collapses even when the
painter omits its zero-area marker fill. The fixture reserved 40px before,
then 1px document minimum after; its raster is entirely white. Nested edge
and EXIF regression checks a 240x320 oriented JPEG fitted into 60px content,
with 10px padding and 2px borders outside. It reserves 104px total and keeps
the first decoded pixel at (12,12); the actual raster was inspected.

Browser suite: 197 passed, 0 failed, 2 ignored. Clippy completed with existing
warnings. Locked ARM release browser: 5,189,612 bytes, +3,940,232 over hello,
leaving 1,302,648 bytes below +5 MiB; stripped static ARM EABI5 inspected.
The image support scope and physical-device measurement caveats are unchanged.


### Native keyboard footer measurement, November 24, 2026

Measured source: `800a2defc140630fc9f946951f58ceadbdb1846c`. Native inline URL-edit measurement now
includes the same page footer, reader action and caret address as display.
The older native drive, updated to the current edit-address action, exposed
a renderer text-fit refusal; its measurement omitted the footer and admitted
one extra prose piece. The regression fails before and passes after at
Default, Large and ExtraLarge text sizes.

Full browser: 198 passed, 0 failed, 2 ignored. Clippy completed with existing
warnings. Native navigation/fragment/link-pagination/address-edit drive now
passes on both Clara BW and Colour; keyboard screenshots were inspected.
Page/link/page-turn/Reader and start/edit/zoom/menu/reload journeys also passed
on both profiles before this native-only fix. These are simulator checks,
not physical-panel qualification.

Locked ARM release browser: 5,189,532 bytes, +3,940,152 over hello, leaving
1,302,728 bytes under +5 MiB. Stripped static ARM EABI5 inspected.


### Disabled option-group submission guard, November 24, 2026

Measured source: `b0faa9ca86c56017eb66f2048480d96a3bb4ed26`. Native form parsing skips disabled
optgroup descendants, not only individually disabled options. Before the fix,
a selected option inside a disabled group could become the chosen successful
control. Fail-before/pass-after parser coverage and app GET serialization
coverage now keep only the enabled choice. No protocol or POST changes.

Engine: 1,396 passed, 0 failed, 18 ignored across 147 result groups.
Browser: 199 passed, 0 failed, 2 ignored. Clippy completed with existing
warnings. Locked ARM browser: 5,189,684 bytes, +3,940,304 over hello, with
1,302,576 bytes left under +5 MiB. Stripped static ARM EABI5 inspected.
No new hardware measurement or visual-layout claim is made by this guard.


### Disabled fieldset first-legend exception, November 24, 2026

Measured source: `d1a7450f02da84a09b7cfcc04def932a3b0c6423`. Disabled fieldsets retain controls only
inside the first direct legend child, while explicitly disabled inputs,
later legends and nested fieldsets outside that exception remain excluded.
Parser regression fails before and passes after; app form serialization
keeps allowed=yes and outside=yes without blocked/later fields.
Specification: https://html.spec.whatwg.org/multipage/form-control-infrastructure.html
section 4.10.19.5. No public Field model, protocol or POST changes.

Engine: 1,397 passed, 0 failed, 18 ignored across 147 result groups.
Browser: 200 passed, 0 failed, 2 ignored. Clippy completed with existing
warnings. Locked ARM browser: 5,190,372 bytes, +3,940,992 over hello,
leaving 1,301,888 bytes under +5 MiB; stripped static ARM EABI5 inspected.
This is semantic form coverage, not a new visual or hardware qualification.


### Colour presentation and GET submitters, November 24, 2026

Measured production: `aee2ff4b5` plus the preceding explicit-submitter change
`13737bb7c`. Supported fresh Colour arrivals default to RGB Page view, while
BW retains Reader-first behavior. Forms, saved-copy notices and restored or
fragment positions retain native semantic Reader; arbitrary authored CSS
colours are unavailable there. Existing RGB text/link/image publication was
working; earlier ordinary simulator shots used the greyscale capture route.
The new colour journey uses shot-colour. Inspected ideal RGB screenshot has
157,121 non-grey pixels, including red heading, blue text, green link and
four-colour PNG. The equivalent BW image has zero non-grey pixels.

Explicit visible native GET submitters now serialize only the selected button
value. Offscreen actions are refused; POST remains review-only, never sent.
Fail-before/pass-after regression, offscreen/POST coverage and the local
form drive pass. Full browser: 204 passed, 0 failed, 2 ignored. Clippy completed
with existing warnings. Locked ARM browser: 5,192,076 bytes, +3,942,696 over
hello, leaving 1,300,184 bytes under +5 MiB. Stripped static ARM EABI5 inspected.
No hardware colour calibration, RSS or latency is established.


### Chromatic text contrast correction, November 24, 2026

The late-identity correction at `d2d786daa` republishes an already-painted
page after the device confirms Colour support (browser 205/0, ARM 5,192,324).
The next RGB regression found the shared contrast floor changed lighter
chromatic text to grey. The floor now applies only to neutral greys. An exact
pixel check covers regular, bold, italic, code, orange link text and a cyan
background; the orange link failed before the correction and passes after.
The 200x200 rendered test image was inspected. This is ideal RGB output,
not physical panel colour qualification. Native Reader still loses CSS colours.

Browser: 206 passed, 0 failed, 2 ignored. Engine: 1,399 passed, 0 failed,
18 ignored across 147 groups. Clippy completed with existing warnings.
Locked ARM browser: 5,192,260 bytes, +3,942,880 over the 1,249,380-byte hello,
leaving 1,300,000 bytes under +5 MiB. Stripped static ARM EABI5 inspected.
No new crates, unsafe code, allocator change or protocol change.


### Native-image late Colour identity, November 24, 2026

A native fetched image shown before the identity response stayed grey when
Colour was later confirmed. On a format change the app now retires old image
handles, cancels pending image tasks and shelf reads, then requests visible
images with the new format's cache keys. Native position and form controls
are not replaced by Page view. Repeated identical identity answers do not
refetch; stale canceled task completions do not publish. The native-image
regression fails before and passes after. The full rendered native frame was
inspected: image RGB present, 114,464 non-grey pixels; ordinary native text
remains neutral. This is a rendered app test, not a hardware panel result.

Browser 208 passed, 0 failed, 2 ignored. Clippy completed with existing warnings.
Locked ARM: 5,192,500 bytes, +3,943,120 over hello, 1,299,760 bytes headroom
under +5 MiB; stripped static ARM EABI5. Engine unchanged at 1,399/0/18 ignored.


### Colour cache-transition coverage, November 24, 2026

Two additional app regressions exercise a grey shelf read pending during
Colour confirmation: a stale grey answer cannot publish or displace the new
RGB task, and an existing RGB shelf entry satisfies the replacement without
network fetch. Grey and RGB cache keys remain separate. Removing the native
picture retirement step makes the first regression fail; restored code passes.
Full browser: 210 passed, 0 failed, 2 ignored. Test/document changes only;
production and measured ARM remain `1c2d834f3`, 5,192,500 bytes. No new pixel
behavior is claimed for this coverage increment. Native text implementation
boundaries and an incremental plan are recorded in PLAN.md.


### Same-DOM native colour provenance foundation, November 24, 2026

Crate-private optional source map emitted by the existing bounded style walk;
normal entry points supply no map and do not allocate one. Generated pseudo
nodes carry None, retained elements/text carry their actual DOM handles.
No native Reader colour behavior changes yet. Duplicate-text/hidden/generated
mapping and unchanged ordinary style-output tests pass; an index-substitution
mutation fails. Engine: 1,401 passed, 0 failed, 18 ignored across 147 groups.
Browser: 210 passed, 0 failed, 2 ignored. Clippy completed with existing warnings.
Locked stripped static ARM EABI5 remains 5,192,500 bytes, unchanged from the
preceding production checkpoint. No added crates, unsafe, allocator or wire
changes. No new visual support claim, so prior actual RGB frames remain the
latest visual evidence.


### Normalized source-range foundation, November 24, 2026

Shared whitespace normalizer extracted unchanged from semantic conversion.
Optional private source buffer preserves UTF-8 ranges through append/trim with
atomic byte/span caps; not yet installed in semantic Gather/Inline or native
RGB rendering. Five tests include 64 input partitions and a trim-rebase
mutation that fails. Engine: 1,406 passed, 0 failed, 18 ignored, 147 groups.
Browser: 210 passed, 0 failed, 2 ignored. Clippy completed with existing warnings.
Locked stripped static ARM remains 5,192,500 bytes. No new visual behavior,
protocol, crates, unsafe code or allocator changes.


### Versioned native RGB rich-text surface, November 24, 2026

Optional authored foreground/background fields in native text presentation.
Protocol 16 adds colour rich-text node tag 35; uncoloured tag 28 remains
byte-identical. Versions 11-15 remain accepted. Colour encoding on an older
version is refused, not silently downgraded. Mixed colour/style-only spans,
foreground/background flags, invalid UTF-8 ranges, truncation and old-version
refusal are tested. Native RGB glyph coverage, background and underline keep
layout geometry unchanged. Both bitmap fallback and installed typefaces were
rendered and visually inspected. Installed-font test: 13,175 non-grey pixels.
Removing colour drawing makes the pixel regression fail. This is toolkit/wire
support, not yet authored semantic browser text colour. No all-fonts claim.

Browser 211/0/2 ignored; protocol 111/0; UI 289/0/2 ignored; SDK 166/0/2 ignored;
simulator 108/0; runtime 156/0; frame host 7/0. Clippy completed with existing
warnings. Rebuilt CLI/simulator Colour image journey passes, actual RGB capture
inspected. ARM 5,194,412 bytes, delta 3,945,032 over hello, 1,297,848 bytes
headroom under +5 MiB; stripped static EABI5 inspected. New runtime is required
for protocol-16 applications. No protocol downgrade claim for new apps on old
runtimes. No new crates, unsafe code or allocator changes.


### First semantic native prose RGB, November 24, 2026

Opt-in semantic coloured text leaves now use styles from the same parsed DOM,
not word matching. Native runs carry foreground/background through splitting
and pagination to version-16 RGB rich-text spans. Colour profiles select this
parse; BW keeps the original unstyled parse. Native form/anchor models remain.
Nested emphasis/link/code and duplicate text colours are covered, including
adjacent colour changes, space collapse and ancestor background inheritance.
A native form-page raster was inspected (3,849 non-grey pixels), with red prose
and blue emphasized text; opening and serializing the GET form still passes.
Disabling the same-DOM colour mapping makes that regression fail. Fresh Colour
forms simulator journey passes. This remains partial: native headings, quotes,
tables, preformatted blocks and control labels use older plain-string paths;
link target decoration can still add a neutral underline. Unsupported CSS only
gets supported computed colours, not complete authored fidelity. Late Colour
identity does not yet reparse existing native text. No all-fonts claim.

Browser 212/0/2 ignored; engine 1,409/0/18 ignored over 148 groups; layout25/0.
Clippy completed with existing warnings. Locked stripped static ARM EABI5:
5,198,868 bytes, +3,949,488 over hello, 1,293,392 bytes under +5 MiB.
No new crate, unsafe or allocator change. Protocol-16 matching runtime required.


### Native colour link decoration and workspace repair

Native rich-text link targets now split at bounded colour-span boundaries,
retaining the same action while underlining each segment in its authored RGB.
This avoids a neutral link mark painting over colour. Pixel test covers red
and blue segments and action identity; replacing colour targets with neutral
ones fails. Actual native frame inspected: 2,108 non-grey pixels. No new wire
change. Whole-workspace cargo check caught two full TextPresentation literals
in book reader/flashcards missing optional fields from the preceding RGB API
addition. Both now explicitly default colours to None; workspace check passes.
The earlier browser-only checks did not establish whole-workspace build health.

Browser 213/0/2 ignored; UI289/0/2 ignored; reader82/0; flashcards binary suite14/0
passes. Clippy completed with existing warnings. Locked static stripped ARM:
5,199,220 bytes, delta3,949,840, headroom1,293,040 under +5 MiB. First broad
reader build hit disk exhaustion; disposable caches cleared, affected library
suite and workspace check rerun green. Not claiming full workspace tests.


### Late identity native text refresh, November 24, 2026

On confirmed panel-format changes, native text is reparsed from retained source
with the appropriate colour entry. The refresh verifies visible text, links,
images and form structure before replacing the native document, copies current
edited form values, preserves reader-view state and saved-copy notice, and
restores semantic piece position. It never forces Page mode. Missing retained
source or a semantic mismatch leaves the existing document untouched. Old image
states retire before the rebuild, so no old-format task is leaked by refresh.

Browser215/0/2 ignored, including saved notice/fragment-position/reader-view and
edited-form tests plus image race rechecks. Disabling refresh fails the RGB
regression. Actual full native frame inspected, 2,829 non-grey pixels. Clippy
completed with existing warnings. Locked static stripped ARM EABI5:5,205,148
bytes, delta3,955,768 over hello, 1,287,112 bytes headroom under +5 MiB. Native
headings/quotes/tables/preformatted/control labels remain partial plain paths.


### Native heading RGB, November 24, 2026

Native headings now carry bounded optional foreground/background spans from the
same semantic runs, preserving H1/H2 size, section anchors and native form
controls. Plain headings retain their old tag and bytes. New heading tag36 uses
protocol17, accepts earlier11-16 frames and refuses coloured headings for older
peers. Matching runtime required. Roundtrips include background-only spans
without inventing a foreground, truncation/range/span-count bounds, plain-wire
compatibility and counted payload length. A disabled-span mutation fails the
native heading regression; restoring it passes.

The Clara Colour simulator journey opens the native form after rendering red H1
and blue H2. Both full frames inspected; heading frame has18,994 non-grey pixels.
These are ideal RGB, not calibrated panel appearance. Quotes, tables,
preformatted blocks and control labels still have plain-string paths. Heading
spans preserve colours, not full per-run authored font styling. No all-fonts or
physical-panel/RSS/latency claim.

Final checks: browser216/0/2 ignored, protocol113/0, UI289/0/2 ignored,
SDK166/0/2 ignored, layout25/0, simulator108/0, runtime156/0. Whole-workspace
check passes; this is not full workspace testing. Clippy completes with existing
warnings. Locked stripped static ARM EABI5:5,211,948 bytes, delta3,962,568 over
hello, headroom1,280,312 under +5 MiB. Disk exhaustion during runtime/simulator
linking was corrected by clearing disposable outputs, then affected checks and
the simulator journey were rerun green. No new crate, unsafe or allocator change.


### Native quote RGB, November 24, 2026

Native quote text now carries authored foreground/background spans through
flattening, pagination, toolkit layout and RGB glyph/background drawing. The
quote keeps its gutter depth/rules, body/byline geometry and fold hit rectangle.
Links remain on the Links screen, as before. UTF8 pagination clips/rebases spans
without changing words or link identity. Disabling quote spans makes the browser
regression fail; restoring them passes. Plain quote tag18 stays byte-identical.
New tag37 requires protocol18;11-17 remain readable, coloured quotes refuse
older peers. Background-only roundtrip retains absent foreground. Truncated
frames, invalid ranges/flags, excess spans and encoded-length count are tested.

Actual Clara Colour full frames inspected: red quote, blue article link, nested
green quote, then form opened successfully. Frame has26,502 non-grey pixels,
including the preceding coloured headings. Ideal RGB, not calibrated panel.
Browser217/0/2 ignored, protocol114/0, UI290/0/2 ignored, layout27/0,
SDK166/0/2 ignored, sim108/0, runtime156/0. Whole-workspace check green, not full
workspace tests; clippy completed with existing warnings. Locked stripped static
ARM EABI5:5,218,356 bytes, delta3,968,976 over hello, headroom1,273,904 under
+5 MiB. No new crate, unsafe or allocator change. Native tables, preformatted
blocks and control labels remain plain paths. No full authored font-style,
all-fonts, physical-panel, RSS or latency claim.


### Native preformatted RGB, November 24, 2026

The opt-in colour parse now retains raw preformatted source runs rather than
collapsing them into prose. Leading spaces, repeated internal spaces, explicit
blank lines and the old leading-newline/trailing-trim rule survive. Native
preformatted pieces carry foreground/background spans into existing RGB rich
text; line-based pagination clips and rebases those ranges. Plain/BW parsing
stays on its previous variant. No new protocol shape. This does not add
monospaced or full authored font-style fidelity; it preserves the existing face.
Disabling the coloured-pre conversion fails the new browser regression, restored
passes. Visible text matches the uncoloured parse. Split tests keep blank lines
and exact ranges. Native form opens in test and Clara Colour simulator journey.
Actual full frame inspected:28,189 non-grey pixels including headings/quotes,
ideal RGB only, not calibrated panel appearance.

Browser218/0/2 ignored; engine1,409/0/18 ignored across148 groups; layout28/0;
whole-workspace check green. Clippy completes with existing warnings, not a
warning-free build. Locked stripped static ARM EABI5:5,224,620 bytes,
delta3,975,240 over hello, headroom1,267,640 under +5 MiB. No new crate, unsafe,
allocator or wire change. Tables/control labels still have plain paths; physical
panel/RSS/latency and complete authored CSS/font fidelity remain unverified.


### Native table RGB, November 24, 2026

Native table rows now carry bounded per-cell spans from same-DOM coloured
inlines. Grid headers/values preserve their rules and geometry. Stacked tables
rebase value spans and retain coloured repeated labels. Header-row cloning keeps
colour through page splits; links remain reachable on the Links screen.
Protocol19 tag38 adds colour spans, while plain table tag30 bytes stay unchanged.
Versions11-18 remain readable; coloured tables refuse older peers. Tests cover
background-only roundtrip, no invented foreground, old bytes, truncation,
UTF8/range/count/shape bounds and exact encoded length. Empty semantic span rows
stay on the plain path. Existing reader/gallery constructors explicitly use no
spans; whole-workspace check passes.

Removing table spans fails the browser regression; restoring them passes.
Header-repetition and stacked/grid geometry tests pass. Actual Clara Colour full
frame inspected:31,413 non-grey pixels including earlier headings/quote/pre,
red/blue column labels and green/purple values. Native form opens in the journey.
Ideal RGB, not calibrated panel appearance. Browser219/0/2 ignored,
protocol115/0, UI291/0/2 ignored, layout29/0, SDK166/0/2 ignored, sim108/0,
runtime156/0. Clippy completes with existing warnings. Locked stripped static
ARM EABI5:5,238,188 bytes, delta3,988,808 over hello, headroom1,254,072 under
+5 MiB. No new crate, unsafe or allocator change. Control labels still have
plain paths. Authored font/CSS fidelity and physical panel/RSS/latency incomplete.


### Native control title RGB, November 24, 2026

Native form-row titles now use the computed foreground/background of their
actual naming source: explicit label, aria-label/placeholder/title-bearing
control, or authored submit value. Browser-generated fallback names are not
coloured from nearby text. Text/select/checkbox/submit row actions, details and
hit rectangles stay unchanged. Radio group names remain generated/default.
This is title-level style, not nested multicolour label runs or option-value,
text-entry/keyboard, per-option or full authored control styling.

Late identity refresh now keeps newly parsed field colours while copying edited
fields, so edits survive without throwing away the source styles. Tests cover
source selection, generated-name non-attribution, exact RGB, checkbox action,
late refresh preserving edited values, unchanged row/menu geometry. Disabling
source colour lookup fails the regression. Protocol20 tag39 carries optional
row-title colours; plain row tag14 bytes unchanged,11-19 still readable, old
peers refuse coloured rows. Roundtrip covers menu/trailing/action/state,
truncation, bad flags and exact encoded size. The uncoloured golden's only
change is the explicit empty field_colours debug field; reviewed, no text or
form change.

Actual full Clara Colour form frame inspected,6,442 non-grey pixels: red search,
blue select title, green checkbox title, purple authored submit title. Neutral
browser chrome/details remain neutral. Browser221/0/2 ignored; engine1,409/0/
18 ignored148groups; protocol116/0,UI292/0/2 ignored,layout29/0,SDK166/0/
2 ignored,sim108/0,runtime156/0; wholeworkspace check green. Clippy completes
with existing warnings. Locked stripped static ARM EABI5:5,243,652 bytes,
delta3,994,272 over hello,headroom1,248,608 under +5 MiB. No new crate,unsafe
or allocator change. Ideal RGB only; physical panel/RSS/latency unverified.
No all-fonts or complete CSS/control-fidelity claim.


### Native colour fragmentation bounds, November 24, 2026

Audit found that pathological alternating colour runs could exceed the256-span
wire bound. Native semantic spans now cap at that bound, retaining all words
and appending an explicit colour-simplification note. Prose also caps encoded
spans without inserting paragraph breaks or changing link ranges. At high
layout-node pressure, heading/quote/table cells fall back to complete plain
text rather than using the remaining nodes for fragments and losing the tail.
Tests retain all text and following content with300 fragments, and bound
600-run prose/headings/pre/quote/table payloads. Disabling bounds fails the
regression; restoration passes. This is intentional partial style fidelity
under resource pressure, not a promise to draw every authored colour.

Browser221/0/2 ignored,UI293/0/2 ignored,layout30/0; whole-workspace check green.
Clippy completes with existing warnings. Locked static stripped ARM EABI5:
5,243,532 bytes,delta3,994,152 over hello,headroom1,248,728 under +5 MiB.
Actual Clara Colour combined page/form journey rerun, full page inspected.
No new crate,unsafe,allocator or wire change. Other colour/font/hardware
caveats above still apply.


### Inline font treatments on coloured native blocks, November 24, 2026

Coloured native heading/quote/table spans now use the same bold/emphasis glyph
routine as native prose, parameterized by each block's existing font size.
Previously these nodes retained strong/emphasis flags but painted plain glyphs.
The RGB path now preserves both treatments without changing measurement, rules,
backgrounds, controls or wire shape. Pixel census for red heading,blue quote and
green table text grows when bold is enabled. Removing the treatments fails the
regression; restoration passes. Full native installed-font raster inspected:
11,047 non-grey pixels. Native semantic fixture, not physical panel calibration.

Browser222/0/2 ignored,UI293/0/2 ignored,layout30/0,SDK166/0/2 ignored,
sim108/0,runtime156/0,whole-workspace check green. Clippy completes with existing
warnings. Locked stripped static ARM EABI5 remains5,243,532 bytes,delta3,994,152,
headroom1,248,728 under +5 MiB. No new crate,unsafe,allocator or wire change.
This is synthetic bold/slant,not arbitrary authored font families or true font
variant selection. Inline code/pre mono fidelity,complex control styling and
physical panel/RSS/latency remain unverified or incomplete.


### Native fixed-pitch preformatted blocks, November 24, 2026

Plain and coloured preformatted blocks use mono rich paragraphs. Measurement,
wrapping, glyph paint, RGB backgrounds, link rectangles and text-selection
rectangles share the document-mono face. It follows reading size without
changing terminal/UI mono sizing. The emergency unbroken-token wrapper now
measures its requested face instead of the interface face. Hard indentation
and blank rows remain intact; soft continuations start flush. Protocol21 adds
tag40, rejected by11-20, while existing non-mono tags retain byte shape.
Plain rich paragraphs round-trip11-21 with identical node bytes.

Browser222/0/2 ignored,UI295/0/2 ignored,font52/0,protocol117/0,layout31/0,
SDK166/0/2 ignored,sim108/0,runtime156/0,reader82/0,frame-host7/0.
Workspace check and clippy complete with existing warnings. Mutation disabling
mono routing fails the fixture; restoration passes. Actual Clara Colour
journey and both full frames inspected:31,674 non-grey page pixels and6,442
form pixels. These are ideal simulator RGB, not panel calibration.
Locked stripped static ARM EABI5:5,244,396 bytes,delta3,995,016 over hello
1,249,380,headroom1,247,864 under +5MiB. No new crate,unsafe or allocator change.
Inline code within prose remains proportional; arbitrary CSS font families,
true variant selection, complex control styling, physical RSS and latency
remain incomplete or unverified.


### Mixed native inline code in prose, November24,2026

Native prose carries the source code flag through plain and coloured inline
runs. It measures each run in its actual face during regular and emergency
wrapping, positioning, link targets and selection. Code paints mono with RGB
ink/background; neighbouring prose retains its existing face. Line height
accommodates both faces. Protocol22 tag41 carries this opt-in flag;11-21 reject
the new node, and old tags/style-byte semantics remain unchanged. Code flags on
other node kinds are explicitly refused instead of silently discarded.

Browser223/0/2 ignored,UI297/0/2 ignored,protocol118/0,layout32/0,SDK166/0/2,
sim108/0,runtime156/0,reader82/0,framehost7/0,flashcards14/0. Whole-workspace
check green; clippy completes with existing warnings. Mutation removing source
code flags fails; restoring passes. The32 layout checks include a160-character
unbroken code link across9 profile/size combinations, retaining every character
and actual mono link rectangles. Mixed selection-offset test passes.
Actual Clara Colour page/form journey passes; both full frames inspected.
Page44,984 non-grey pixels,form6,442. Ideal simulator,not panel calibration.
Locked stripped static ARM EABI5:5,247,428 bytes,delta3,998,048 over hello,
headroom1,244,832 under +5MiB. No new crate,unsafe or allocator change.
Code within headings/quotes/tables still uses its block face. Arbitrary CSS
font families,true variants,complex controls,physical RSS/latency remain open.

Mixed-code fragmentation is conservatively estimated before wrapping; when it
would exhaust layout nodes, the entire paragraph simplifies to its ordinary
face before both measurement and painting. A200-run regression confirms the
plain-bounds marker,no mono tail mismatch,and following text survives. This
fallback may remove colour/emphasis as well as mono in that paragraph.

### Native semantic styles without authored RGB, November24,2026

Heading/quote/table spans now retain strong/emphasis even when no author RGB
was supplied. Previously only coloured runs reached the existing span painter.
No foreground/background is invented; a plain raster fixture remains entirely
greyscale while showing bold/slanted words. Code-only prose overflow is now
included in the existing simplification-notice count. Code in heading/quote/
table still uses the block face; this does not change that limit.
Browser224/0/2 ignored,layout34/0; whole-workspace check green; clippy completes
with existing warnings. Removing semantic span retention fails the fixture,
restoration passes. Actual full native raster inspected. Static stripped ARM
EABI5:5,247,404 bytes,delta3,998,024 over hello,headroom1,244,856 under+5MiB.
No new crate,unsafe,allocator or protocol change. Not a physical-panel result.


### Native Colour computed font treatments, November24,2026

The same-DOM colour conversion now maps computed inherited font-weight>=600
to synthetic bold and computed italic/oblique to synthetic emphasis. Computed
normal resets take precedence over semantic strong/em tags in this path.
The ordinary uncoloured parser is unchanged. Incomplete style-tree mappings
fall back to semantic tags rather than losing them. No font file/true variant,
wire,new crate,unsafe or allocator change. Raw preformatted traversal and
complex control styling are separate limits; this does not implement arbitrary
CSS font families. RGB and ordinary prose text/links/form identity remain.
Browser225/0/2 ignored,engine1409/0/18 across148groups,layout34/0; workspace
check green;clippy completes with existing warnings. Mapping-disabled mutation
fails,restoration passes. Initial disk-full links were rerun after disposable
build output cleanup. Full native raster inspected:6,831 non-grey pixels.
Static stripped ARM EABI5:5,248,436B,delta3,999,056 over hello,headroom1,243,824
under+5MiB. No physical panel/RSS/latency qualification.

### Native preformatted computed font treatments, November24,2026

The raw source traversal now retains computed bold/emphasis beside each raw
UTF8 text slice before trimming only the existing block edges. It then wraps
those slices in semantic strong/emphasis, preserving indentation, double spaces,
blank rows and colour. Normal-reset children stay plain. Pre remains measured
and painted in the same mono face; no code flag is added to old wire shapes.
Browser226/0/2 ignored,engine1409/0/18 across148groups,layout34/0; workspace
check green;clippy completes with existing warnings. Treatment-removed mutation
fails;restoration passes. Full native raster inspected:2,531 non-grey pixels,
including accented text,blank row and child reset. Not panel calibration.
Static stripped ARM EABI5:5,248,876B,delta3,999,496 over hello,headroom1,243,384
under+5MiB. No new crate,unsafe,allocator or protocol change. Arbitrary families,
true font variants,complex controls and physical RSS/latency remain open.

### Raw native Colour pre visibility, November24,2026

Raw Colour pre traversal now applies the same element visibility check as the
semantic text walk. Hidden,aria-hidden,inline visibility:hidden and supported
stylesheet display:none descendants no longer enter its source slices.
Indentation and blank rows are preserved. Removing the visibility check fails
the four-mode fixture; restoration passes. Browser226/0/2 ignored,engine1410/0/18
across148groups,whole-workspace check green,clippy completes with existing
warnings. Locked stripped static ARM EABI5:5,248,884B,headroom1,243,376 under+5MiB.
No wire,newcrate,unsafe or allocator change. Text-exclusion regression,not a
new visual or physical-device fidelity claim.

### Styled native source boundary whitespace, November24,2026

Collapsed HTML spaces now remain single across computed strong/emphasis source
wrappers. The old boundary check saw only direct colour leaves,so a trailing
space in a bold leaf and leading space in the next italic leaf were duplicated.
The source-tail check looks through semantic/link wrappers without changing
styles,RGB,ranges or raw-pre whitespace. Exact-text regression fails before the
fix and passes after. Browser226/0/2 ignored,engine1411/0/18 across148groups;
workspace check green;clippy completes with existing warnings. Locked static
stripped ARM EABI5:5,248,708B,headroom1,243,552 under+5MiB. No new crate,unsafe,
allocator or protocol change. Text-identity check,not new hardware evidence.

## Painted direct-link interaction, November 24, 2026

Protocol23 adds optional bounded (128) source-pixel picture regions. Tag17 stays
unchanged for pictures without links; tag42 requires version23. Encoder/decoder
reject old-version rich regions, invalid geometry/counts and truncated payloads.
Layout scales/clips with the picture; ambiguous different actions refuse and do
not page-turn. Keyboard/toolbar overlays keep priority. The browser maps painted
URLs back to existing document link IDs, preserving its navigation/history path.

Genuine fail-before/pass-after: removing picture-region attachment makes
`painted_links_open_by_touch_at_every_zoom` fail on missing visible link target;
restoration passes both formats at75/100/150/200 with keyboard open/dismiss and
Back. Actual coordinate-touch Clara Colour replay and full-frame pixel inspection
passed (sample index -> tapped article -> Back). Browser228/0/2 (227 in four batches plus the new region test) in bounded
batches; UI299/0/2,protocol119/0,SDK166/0,sim108/0,framehost7/0. Workspace check and
scoped clippy complete with existing warnings. Workspace --tests remains blocked
by the existing absent panels original-pages.cbz fixture, not reported as green.
Final locked static stripped ARM EABI5 5,253,916B, delta4,004,536 over hello1,249,380,
headroom1,238,344 under+5MiB. No new crate,unsafe or allocator change. Hardware,
RSS,latency,broader HTML/CSS and independent proof gates remain open.

## Unified browser header, November 24, 2026

Protocol24 style4 explicitly requests address-in-header layout. Existing browser
style3 and prior wire versions remain accepted. Real top bar, runtime way back
and Close stay present. Only the declared first Field is relocated, preserving
its action and bounded diagnostics. The duplicate top Navigate moves under More.
Address/keyboard controls remain hit-testable; footer preserves page position.
Native pagination now compares pictures against picture-alone size in the same
final unified frame. Missing image geometry refuses instead of comparing empty
options. Painted viewport and keyboard crop measure the same footer and header.

Fail-before/pass-after: disabling unified layout fails the shared-header geometry
fixture, restored version passes. Full UI300/0/2,protocol120/0,SDK166/0; browser
four bounded batches57+58+57+56=228 passed/0failed/2ignored. This includes profile
and text-size menu reachability updated for More>Navigate, page/link/keyboard,
image fitting, form and history journeys. Sim108/0,framehost7/0. Workspace check
and scoped clippy pass with existing warnings; absent panels CBZ fixture still
prevents a blanket workspace-tests claim. Actual Colour coordinate-link/Back/
inline-keyboard/More replay and B&W page/keyboard/menu replay pass. Full pixels
inspected, Colour index38,137 non-grey pixels, B&W0. Final locked static stripped
ARM EABI5 5,252,908B,delta4,003,528,headroom1,239,352 under+5MiB.
No unsafe/new crate/allocator change. Hardware/RSS/latency/full-web/proof gates
remain open. The zoom slider is still permanently visible, not yet moved into a
panel; smooth movement/scroll interaction remains future work.

## Settled painted-viewport scrolling, November 24, 2026

Protocol25 optional Picture tag43 carries bounded scroll action base (201 IDs)
plus existing source-pixel links; prior tags17/42 are unchanged when absent.
Both simulator input and device-write runtime use one shared gesture recognizer.
A captured vertical drag emits once on release, proportional to distance; tiny
taps pass to normal link hit-testing, horizontal/moved releases are consumed.
Cancel/reflow/keyboard/overlay/foreground transitions retire capture. A real
painted CSS window, not page increment, drives both raster and visible links.
Scroll is disabled while typing/menu is open. A bounded64-entry in-session URL
map restores offsets/zoom on Back; it is not persistent across app restarts.

Fail-before/pass-after: zeroing the scroll delta fails the half-viewport offset
fixture; restored implementation passes. Full browser four batches57+58+58+57=
230 passed/0failed/2ignored, plus final changed navigation/gesture rechecks.
UI302/0/2,protocol121/0,SDK166/0,sim109/0,framehost7/0. Device-write tests26 library
and177 binary pass (1ignored); device-write check and scoped clippy pass with
existing warnings. Workspace check passes; absent panels CBZ still prevents a
blanket workspace-tests claim. Actual pointer Down/Move/Up Colour article
before/after/inline-keyboard replay passed; full frames inspected. Final locked
static stripped ARM EABI5 5,265,220B,delta4,015,840 over hello1,249,380,
headroom1,227,040 under+5MiB. No unsafe/new dependency/allocator changes.

This is settled viewport scrolling, not animation/inertia or native forms/reader
scrolling. Top/bottom clipped text is expected viewport cropping. Physical
panel quality, latency/RSS, restart restoration and independent/fullweb proof
gates remain unverified. Page-position footer names the containing page.

Follow-up scroll-state audit: explicit zoom or Reader/Page rebuild retires old
session offsets, so Back cannot resurrect a stale zoom/window. Saved-copy
notices are not bypassed by offset restoration. Foreground orientation changes
cancel gestures; background-app screen updates do not cancel the front app's
gesture. The new stale-zoom fixture failed before and passes after. Final full
browser batches58+58+58+57=231 passed,0failed,2ignored after these fixes.
Device-write26lib/177bin/1ignored, workspace check and scoped clippy pass;
Colour pointer replay rerun. Locked static stripped ARM5,265,908B,headroom
1,226,352 under+5MiB. Remaining hardware/native-scroll caveats unchanged.

## Deliberate zoom panel, November 24, 2026

Explicit BrowserCompact/style5 and Slider node/tag44 require protocol26.
Standalone sliders bound value0..100 and reject overflowing action bases in
preflight/encode/decode. Versions11..25 remain accepted, old styles/tags are
unchanged. The existing shared Slider hit/drag and paint paths serve both
standalone and legacy bar sliders. Five large main controls replace the
permanent seven-slot track. Zoom popover includes percentage, track, +/- steps,
100% reset and Done. Outside/runtime Back dismisses it, address/navigation
retires it; scrolling cannot run behind the panel.

A feature fixture failed before (no Zoom control) and passes after. Full browser
batches58+59+58+57=232 passed,0failed,2ignored. Protocol123/0,UI302/0/2,SDK166/0,
simulator110/0 (new standalone actual Down/Move/Up test),device-write26lib+
177bin/1ignored,framehost7/0. Workspace check and scoped clippy pass with existing
warnings. Locked static stripped ARM5,267,036B,headroom1,225,224 under+5MiB.
Actual Colour pointer drag reached168%, reset100%, closed and edited inline;
full frames inspected. No physical panel/latency/RSS claim. Missing panels CBZ
continues to block an all-workspace-tests claim. Fullweb and independent proof
gates are not marked complete by these UI tests.

Post-panel journey audit: all Zoom controls fit four portrait/landscape sizes
(1072x1448,1448x1072,758x1024,1024x758) at Default/Large/ExtraLarge text.
B&W actual track drag/reset/close passes (separate script because native and
painted article page counts differ; no false-green reuse of Colour wait).
Runtime Back while typing exposed an existing gap: keyboard stayed open. The
new fixture fails before and passes after routing runtime Back to keyboard
close before leaving the document. Final browser234/0/2 in four full batches,
scoped clippy and locked ARM pass. ARM5,267,044B,headroom1,225,216.

## Pending navigation keeps browser chrome, November 24, 2026

Loading now keeps the unified address/Back/Close header and compact row; Stop
replaces Reload while a fetch is pending. No new wire feature is needed, just
explicit bar actions on the existing style. Cancel/runtime Back retires the
pending task and late responses cannot replace the retained document. Existing
elapsed-wait words remain honest rather than inventing progress percentage.
A header/Stop/cancellation fixture failed before and passes after. The rendered
fixture frame was inspected (unit-render evidence, not a live-network capture).
Full browser235/0/2 across four batches,SDK166/0,workspacecheck/scopedclippy pass.
Locked static stripped ARM5,267,004B,headroom1,225,256. Earlier device/core proofs
unchanged; physical hardware/latency/RSS/fullweb proof gates remain open.

## Error recovery retains the attempted address, November 24, 2026

Failure/unsupported screens keep unified address/Back/Close and an explicit
recovery row (Back,Forward,Retry,Address,Home), not document-specific Zoom/More
commands for a previous page. Address editing starts from the attempted URL;
Reload retries that URL rather than silently loading the retained document.
Existing Try Again/return/history behavior is preserved. No wire extension.
The recovery fixture failed before (address absent) and passes after, checking
attempted-URL editing/reload and retained page. Inspected unit-rendered timeout
frame, not live-network evidence. Full browser236/0/2,SDK166/0,workspacecheck/
scopedclippy pass; locked static stripped ARM5,268,476B,headroom1,223,784.
Earlier device/core proofs unchanged. Hardware/latency/RSS/fullweb gates open.

## Abandoned navigation race audit, November 24, 2026

Local navigation or Home could leave an older network request alive and allow
its late response to replace the chosen destination. Two new fixtures fail
before and pass after. New navigation retires pending fetch/shelf wait before
choosing a local destination; Home/library/Forward paths retire abandoned work
too. The clock and transient panels retire with it. Full browser238/0/2 across
four batches, workspacecheck/scopedclippy and locked ARM pass. ARM5,268,748B,
headroom1,223,512. No new visual feature or hardware claim from this race fix.

## Runtime Back in native subviews, November 24, 2026

Native subviews/typing/internal history now declare owns_back so the shell
first offers its reserved Back instead of immediately leaving the application.
The runtime Back is mapped at the browser boundary to named Back; form typing
cancels first. More/Zoom state is retired before entering native subviews, so
a stale hidden menu cannot consume the first return tap. New fixture failed
before; actual coordinate replay exposed and then verified the hidden-menu
fix. Final replay returns Navigate to the document; pixels inspected.

The independent never-displayed artifact equality fixture initially failed on
owns_back: producer rig had a prerequisite history entry and disabled rig did
not. Mirrored the same prerequisite arrival in the disabled rig, keeping full
Screen equality intact instead of ignoring a field. Full browser239/0/2 after
all changes, workspacecheck/scopedclippy and locked ARM pass. ARM5,269,172B,
headroom1,223,088. No new wire/unsafe/dependencies. Hardware/fullweb gates open.

## Physical page keys respect panels, November 24, 2026

Page keys no longer move the document behind Zoom/More or active form typing.
The regression fixture failed before and passes after for both panels. Full
browser240/0/2 across four batches, workspacecheck/scopedclippy and locked ARM
pass. ARM5,269,204B,headroom1,223,056. This is input isolation, not physical
hardware qualification or a new visual feature. Independent gates stay open.

## First-navigation cancellation, November 24, 2026

Cancelling before any document has loaded now returns to Start rather than an
empty Page state. Existing-document cancellation still returns to that page,
and late responses stay retired. Transient panels close with cancellation.
New fixture fails before and passes after. Full browser241/0/2,workspacecheck/
scopedclippy/lockedARM pass. ARM5,269,196B,headroom1,223,064. No visual/hardware
claim from this state fix; independent gates remain open.

## Transient panels retire on navigation result, November 25, 2026

Zoom/More opened while loading no longer survives a failure or a new response,
where the recovery row has no matching anchor and the stale panel could cover
the error. Both failure-panel cases fail before and pass after. Full browser
242/0/2,workspacecheck/scopedclippy/lockedARM pass. ARM5,269,204B,headroom
1,223,056. No new visual feature/hardware claim; independent gates stay open.


## Source-anchored zoom, November 25, 2026

Zoom now maps the first visible source glyph and its clipped line fraction into
its new layout, instead of returning to the document start. A zero offset stays
at zero, including the document margin. Scroll history stores the new zoom and
offset, not the old geometry. This is a reflow anchor, not pinch-to-zoom or a
promise of exact pixel placement. Pages without a matching glyph use the first
viewport; image-only pages do not yet have an image anchor.

Regression: the reading-place test fails on the previous handler and passes on
the source-anchor handler through 100 -> 125 -> 150 -> 125 -> 100 percent.
Browser suite: all 244 passing plus 2 ignored on the final source (four batches),
including the top-margin guard and cache bound check. Existing
scoped clippy correctness gate passes (pre-existing warnings remain).
Locked stripped ARM browser: 5,270,708 bytes, +1,504 over the previous build.
This is 1,221,552 bytes below the +5 MiB-over-simple-app size limit.

The committed drive-zoom-place.kobo replay scrolls the Colour sample article,
changes to 150%, closes the panel and resets to 100%. Its inspected frames in
screenshots/zoom-reading-place show the same paragraph near the viewport top
before/after reflow, not the article title. Their sidecars retain the measured
frame and source provenance. RGB simulator output is not physical panel proof.

A separate /proc diagnostic sampled the host debug browser 902 times during
the existing Colour zoom replay: peak RSS and VmHWM were both 66,512 KiB.
This has no simple-app control and is not an ARM/release/hardware RSS result.
The device +32 MiB, latency, panel and independent full-web gates remain open.


## Display-metrics reflow, November 25, 2026

The first event after a display-metrics change now rebuilds the painted page at
the selected zoom and maps the source reading anchor into the new layout. It
retires the old geometry's session offset and stores the new one, within the
64-entry bound. If painting is refused, the zoom label returns to 100% rather
than describing a nonexistent zoomed raster.

The regression simulates portrait -> landscape -> portrait at 125% and checks
actual raster dimensions against an independently rebuilt 125% page, the source
anchor and the cached zoom/offset. It fails on the old handler, which rebuilt at
100% while keeping a 125% label, and passes on the new handler. Full browser
suite: 245 passed, 2 ignored in four batches. Scoped clippy correctness gate
passes with existing warnings. Locked stripped ARM browser: 5,271,692 bytes,
+984 from the preceding commit, 1,220,568 bytes under the binary-size limit.

This is callback/state and layout verification, not a physical rotation test.
The simulator's hardware orientation command currently changes display
composition without delivering new application metrics, so it cannot prove
this path. Existing image-only anchoring, hardware memory/latency, panel and
independent full-web qualification gaps remain unchanged.


## Painted page-button history, November 25, 2026

Page-button turns now store the selected painted reading position and zoom in
the same bounded session map used for settled scrolling. Previously a turn
removed that entry without updating native history, so Back/Forward could
return to the start at 100%. Both physical page callbacks and named page
buttons are covered by a regression that fails before the fix and passes after
it, including returning to the first page while keeping 125% zoom.

Restoring an exact pagination boundary uses complete-line pagination rather
than a continuous raster window. The first inspected replay caught a clipped
extra line at the bottom after restore; the corrected replay has pixel-equal
before/Forward/repeated-Forward frames. The inspected full images and capture
sidecars are in screenshots/page-history, with drive-page-history.kobo.
The replay uses reachable Back/Forward controls, not internal sample actions.

Final browser suite: 246 passed, 2 ignored in four batches. Scoped clippy
correctness gate passes with existing warnings. Locked stripped ARM browser:
5,272,468 bytes, +776, leaving 1,219,792 bytes below the size limit. These are
host/simulator and cross-build results, not hardware, RSS, latency, full-web
or independent-proof qualification. Session place remains per URL and does not
persist across app restart.


## Source-byte zoom correction, November 25, 2026

Correction to the earlier source-glyph description: basic text paint batches a
source leaf's whole line into a command. Its ordinal is a reflowed line number,
not a stable glyph identity. The earlier sample tests and screenshots proved
nearby passage retention, not stable source-character retention.

Basic text paint now supplies a UTF-8 byte range for each painted line fragment
in its retained text run. Other display-list producers explicitly supply no
range. The browser retains a source-node/byte anchor across consecutive zoom or
metrics rebuilds, finds the new fragment containing that byte, and clears the
anchor when the user scrolls or turns a page. No protocol/wire format changed.
A long single-paragraph regression fails with the previous ordinal lookup and
passes with byte lookup; it also checks repeated zoom does not drift its stored
byte and user movement retires it. This is vertical line-fragment anchoring,
not exact glyph positioning. Image-only anchors, bidi/ligature-specific anchor
qualification, and persistence across navigation/restart remain limited.

Browser: 247 passed, 2 ignored in four batches. Full kobo-web-document test run
passed, including exact paint bridge/display-list assertions and WPT targets.
Workspace check and scoped browser/document clippy correctness gate pass with
existing warnings. Locked stripped ARM: 5,272,508 bytes, +40, leaving 1,219,752
bytes under the binary limit. Hardware RSS/latency/panel and independent full-web
gates remain open.

Colour and BW-profile reachable-control article replays both pass at 100%,
150%, reset. Full 150% frames and reset were inspected, source-byte-zoom holds
images/sidecars, and drive-zoom-place-bw.kobo includes the explicit native
More/Navigate route. These are simulator frames, not physical panel evidence.


## Fuzz-runner failure propagation, November 26, 2026

The smoke runner previously piped each cargo/fuzzer result through a text
filter with an unconditional success fallback. Build failures or fuzz crashes
could therefore return success from the overall script. The runner now captures
the command status before filtering, prints full diagnostics on failure, runs
all six targets, and exits nonzero if any target failed.

Four isolated fake-cargo tests cover all-success, first-target build failure,
last-target crash and middle-target failure with later targets still attempted.
Three failure cases fail against the original runner; all four pass after the
fix. Shell syntax check passes. These are harness-contract tests, not fuzzing.

An actual smoke invocation on this recovered environment returns nonzero as it
should: cargo-fuzz is missing. Rustup auto-installed nightly during that
invocation, so nightly is now available, but none of the six targets ran. No
fuzz/Kani/Miri/proptest or hardware gate is marked passed.
No runtime source/binary changed; prior ARM measurement remains applicable.


## Bounded development-ASan fuzz smoke, November 26, 2026

Installed cargo-fuzz 0.13.2 as a host verification tool. Nightly is
1.102.0-nightly (739adecd4, 2026-11-24). Optimized ASan compilation did not
complete: initial attempt exceeded the execution window, then moxcms rustc was
killed by SIGKILL. Development mode with one build job and 16 codegen units
built all targets successfully, retaining default address sanitizer.

Refreshed the standalone fuzz lock for current path dependencies. New path
requirements use versions already in the runtime lock; unicode-properties and
unicode-script were explicitly pinned back to runtime 0.1.4/0.5.8 rather than
keeping newer resolver selections. No main workspace lock/runtime source changed.

Each target ran via cargo +nightly fuzz run -D --codegen-units 16 with max total
time 5s, RSS limit 512 MiB, input limit 262144 and per-input timeout 10s. Parse
and sniff use saved document fixtures, image uses the existing image corpus,
others existing target seeds, with retained generated local corpora.

| Target | Executed units | Peak host RSS MiB | Exit |
| --- | ---: | ---: | ---: |
| parse_html | 439 | 132 | 0 |
| paginate_document | 565 | 159 | 0 |
| decode_image | 19 | 70 | 0 |
| cache_metadata | 113257 | 109 | 0 |
| resolve_url | 25185 | 122 | 0 |
| sniff_body | 148921 | 84 | 0 |

No crash was reported in this short development-ASan run. decode_image spent
its budget on seed initialization (19 executions), so it is not evidence of a
mutation campaign. This is not the optimized smoke.sh qualification, a long
fuzz campaign, Kani/Miri/proptest completion or device memory/latency evidence.
Runtime binary measurement and all remaining independent/full-web gates are
unchanged. Local fuzz output logs are not committed as build artifacts.


## Image mutation smoke and partial Miri, November 26, 2026

Extended decode_image development-ASan run with the same flags/limits and
max_total_time=45: exit 0, 226 executions, 79 new corpus units, peak host RSS
159 MiB. It got past seed initialization into mutation. This short smoke is
still not a long campaign or optimized-build qualification.

Installed the Miri component for the existing nightly. Browser-core Miri tests
used default interpreter checks, --locked, no suppression flags. Completed
scopes: address/fetch/history 14 tests, library 2, cache excluding full-index
roundtrip 8. All 24 completed tests passed. The full suite was bounded at 105s
while in cache::tests::the_index_survives_being_stored_and_a_full_one_fits_a_store_value.
A separate exact run of that test also exceeded 105s with no completion or
reported diagnostic. It was terminated, not skipped as a pass. Thus browser-core
Miri is 24/25 completed, NOT an overall pass. No test or production behavior was
weakened to fit the time window. Kani, proptest, full-web and hardware gates
remain open, and no runtime binary changed.
