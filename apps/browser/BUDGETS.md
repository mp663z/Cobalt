# Cobalt Browse: budgets and measurements

Measured September 25, 2026 on branch `agent/browser-m0`, from upstream beta fc88fd3.
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
8 MB over baseline with a reference-counted tree; the browser's own arena tree stores less.

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
