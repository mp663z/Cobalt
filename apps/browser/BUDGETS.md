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
