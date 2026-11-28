# Browser qualification measurements

Measured source: `a76bbff54d587838fea6d61f69ffb76547f76609` (`agent/browser-basic`),
branch `agent/browser-qualification-20261128`. All host numbers are x86_64 host
observations on this sandbox, not device budgets. No physical Kobo route exists
in this sandbox (no adb, no tty device, no device host), so device RSS, panel
and latency gates stay open.

Toolchain: Rust 1.85.1 (matches `browser-measure.yml` and BUDGETS.md), pinned
Zig 0.16.0 for the musl cross-compile, archive SHA-256 verified against the
digest recorded in BUDGETS.md and the workflow
(`70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00`).

## ARM binary audit (armv7-unknown-linux-musleabihf, locked, workspace release profile)

Command:

```
CC_armv7_unknown_linux_musleabihf=armv7-musl-cc AR_armv7_unknown_linux_musleabihf=armv7-musl-ar \
cargo +1.85.1 build --locked --release --target armv7-unknown-linux-musleabihf \
  -p kobod -p kobo-hello -p kobo-browser
```

Wrappers match `.github/workflows/browser-measure.yml` (zig cc with the musl
target, `--target` dropped, `-fno-sanitize=undefined`; archive wrapper `zig ar`).

| Binary | Bytes | Inspected |
| --- | ---: | --- |
| kobod | 5,125,300 | stripped, static ARM EABI5 ELF |
| kobo-hello | 1,265,756 | stripped, static ARM EABI5 ELF |
| kobo-browser | 5,278,284 | stripped, static ARM EABI5 ELF |

Browser over hello baseline: 4,012,528 B, leaving 1,230,352 B under the +5 MiB
allowance. The lead's 5,273,220 B figure was recorded at the earlier baseline
`7e28165c`; the +5,064 B here is the delta to `a76bbff5`. Note: the hello
baseline itself grew from 1,249,380 B (BUDGETS.md, Nov 24) to 1,265,756 B;
the cause was not investigated in this lane.

## Host RSS and heap, basic-mode render pipeline

Method: per fixture, one fresh process each, medians of 3 reps, 96 runs, 0
failures. Two independent peak-RSS reads agree within 0.1 MB on every fixture:
(1) `basic-page` (the supported host command) via per-child `wait4` ru_maxrss;
(2) the `rss-heap` harness in this directory (same `basic_preview::prepare`
pipeline plus a counting `GlobalAlloc` measuring live-heap high-water directly,
never RSS-derived; VmHWM from `/proc/self/status`). Harness `Cargo.lock` pins
every shared crate to `tools/cobalt-text-preview/Cargo.lock` (verified zero
drift after pinning). Driver: `run_matrix.py`; raw runs: `results.jsonl`;
fixtures sha256-pinned there. Synthetic size fixtures are deterministic
(`make_synthetic.py`): 548,178 B and 1,092,178 B from python-controlflow.

| Fixture | HTML bytes | Peak RSS MB (basic-page) | VmHWM MB (harness) | Heap high-water MB | Alloc total MB |
| --- | ---: | ---: | ---: | ---: | ---: |
| baseline (minimal page) | 92 | 48.4 | 48.5 | 42.3 | 162.7 |
| japanese | 315 | 48.3 | 48.4 | 42.5 | 163.1 |
| russian | 397 | 48.8 | 48.7 | 42.6 | 163.9 |
| hindi | 517 | 48.7 | 48.8 | 42.5 | 165.9 |
| rust-installation | 30,726 | 51.1 | 51.1 | 44.3 | 203.4 |
| rust-hello | 31,980 | 51.2 | 51.2 | 44.4 | 213.8 |
| rust-variables | 34,512 | 51.2 | 51.2 | 44.7 | 234.9 |
| rust-functions | 37,077 | 51.3 | 51.4 | 44.8 | 254.9 |
| python-modules | 71,013 | 53.9 | 54.0 | 48.4 | 439.0 |
| python-introduction | 71,957 | 54.3 | 54.3 | 48.4 | 402.4 |
| rust-guessing | 83,395 | 54.8 | 54.6 | 49.5 | 505.0 |
| python-errors | 85,628 | 55.2 | 55.1 | 52.4 | 457.9 |
| python-datastructures | 111,008 | 58.2 | 58.3 | 53.5 | 515.1 |
| python-controlflow | 140,178 | 60.5 | 60.5 | 54.8 | 634.0 |
| synthetic-512k | 548,178 | 99.8 | 99.8 | 93.0 | 1675.5 |
| synthetic-1m | 1,092,178 | 152.4 | 152.4 | 143.9 | 2840.4 |

Reads:
- Fixed pipeline baseline is 48.4 MB RSS / 42.3 MB heap high-water on host.
- Worst frozen real page (140 KB): 60.5 MB RSS / 54.8 MB heap.
- Above baseline the growth is about 104 MB RSS per MB of HTML, linear across
  the two synthetic points.
- Heap high-water runs 5-9 MB below peak RSS throughout; total allocation
  churn reaches 2.84 GB for the 1.06 MB page.
- These are preview-pipeline host numbers. BUDGETS.md's +32 MiB RSS budget is
  defined for the device browser process; the preview README already disclaims
  device memory qualification. The 48.4 MB host fixed baseline alone exceeds
  32 MiB, stated as a host fact only.

Stage wall times (medians, release): t_prepare + t_rgba for the worst real
page 178 + 181 ms; synthetic-1m 1354 + 379 ms; baseline 0.3 + 0.1 ms. Host
render-side first-paint proxy only. Interactive scroll latency is not measured
in this batch (needs the simulator drive path or a device).

## Screenshot and census evidence

`python3 tools/cobalt-text-preview/render_corpus.py --out /tmp/qual-corpus
--binary <release basic-page>`: all 13 frozen pages match their census
baselines at `a76bbff5` ("All 13 pages match their census baselines").
python-controlflow renders full-length, 800 x 51,698 px, coherent from header
through footer (inspected crops in `evidence/`: top, mid, end; hindi.png shows
correct Devanagari shaping). Evidence census for python-controlflow kept in
`evidence/python-controlflow.census`.

## RETRACTION (Nov 28, resolved with the lead)

The earlier "link-tap navigation failure" framing in commit ab68641 (now
646cab4 after rebase) is retracted: it was not a regression. The simulator
defaults to clara-bw-391 and the browser only auto-enters painted mode on
colour panels, so the committed routes' painted-layout assumptions (350,378
and "1 of 3") did not hold on the default profile. The lead repaired the
routes (explicit browser-more > navigate > page-view on BW) in 547761ba.
Verified here: both committed journeys pass at 547761ba.

## Simulator latency, measured source 547761ba (Nov 28)

Host x86_64 simulator, DEBUG kobo-cli build, every step a separate CLI+IPC
round trip (dump floor 10 ms, input-step floor ~31 ms). These are host debug
numbers, NOT device latency; a release-mode sim rerun is the planned
follow-up. Probes: probes/latency_probe.py, probes/painted_latency.py,
probes/route_check.py.

Route verification at 547761ba: drive-painted-touch.kobo PASS (10.9 s wall),
drive-scroll.kobo PASS (12.8 s wall) on fresh sim state. touch-destination
evidence (painted "1 of 3") in evidence/.

Native reader (default clara-bw-391), article "1 of 4":
- link tap (samples index -> article): 695, 696 ms (2 reps)
- page turn (right-side tap, indicator advance): 260-310 ms (6 turns)
- drag on native article: delivery 366 ms, model change at 383 ms

Painted mode (explicit page-view steps per repaired route):
- page-view toggle on index: 1.19-1.22 s
- link tap to article: 650-655 ms
- page-view toggle on article -> painted pagination "1 of 3": 1.903 s (2 reps)
- drag scroll: pointer down 31-33 ms, move 31 ms, up 610-611 ms (the up step
  carries the scroll processing), total 674 ms. Semantic dump is unchanged by
  painted scrolling (expected); pixel evidence: scroll-before vs scroll-after
  differ in 398,386 px (25.7% of frame), byte-identical change set to the
  committed route's own shots. scroll-after inspected: content panned ~1/4
  page, "1 of 3" indicator stable.

### Latency rerun with RELEASE kobo-cli (app and daemon still debug-built by `kobo dev`)

Same probes, same commit 547761ba, only the drive CLI rebuilt --release.
The drop vs the debug-CLI run is mostly harness overhead (debug CLI cost
~30-40 ms per step), which is itself the lesson: these host numbers are
harness-sensitive; treat them as relative, never device latency.

| Measurement | debug CLI | release CLI |
| --- | ---: | ---: |
| Native link tap -> article "1 of 4" | 695-696 ms | 359 ms (2 reps) |
| Native page turn (side tap) | 260-310 ms | 83-84 ms (6 turns) |
| Native drag delivery -> model change | 366/383 ms | 125/135 ms |
| Painted page-view toggle (index) | 1.19-1.22 s | 792 ms |
| Painted link tap | 650-655 ms | 355 ms |
| Painted page-view toggle -> "1 of 3" | 1.903 s | 1.469 s |
| Painted drag up step (scroll processing) | 610-611 ms | 334 ms |
| Painted drag total | 674 ms | 387 ms |

Scroll pixel evidence identical across all runs: 398,386 px (25.7% of the
1072x1448 frame) changed, same change set as the committed route's shots.
App-side processing remains debug-skewed in both rows; a release app+daemon
sim is not available without changing `kobo dev` (it hardcodes a debug
build), noted as a possible follow-up.

## Sheet-supply comparison 9812d6a1 vs b2c8ec83 (2026-11-28, host, debug)

Sim libra-colour-390, debug app + debug drive CLI, TLS fixture (5- and 16-link
Sphinx-like pages, ~9 KiB corpus CSS variants), 3 reps each, probe
probes/sheet_compare.py, evidence/sheet-supply/.

- 9812d6a1 (CANONICAL, 2-link ceiling): both pages settle Failed(TooManyLinks)
  pre-traffic; 0 CSS fetches all reps; nav-sheets 1.783-1.786s.
- b2c8ec83 (16-link, 4-slot queue): 5/5 and 16/16 fetched all reps;
  nav-sheets5 7.612-7.639s, nav-sheets16 12.258-12.559s.
- App VmHWM sheets16-painted: 67,280-71,560 kB (9812d6a1) vs 71,872-72,808 kB
  (b2c8ec83); delta +1 to +5 MB, ranges nearly overlap.
- sheets16 link-tap rc=1 = drive 10s tap-ack timeout under supply load
  (tap lands; navigation + fetches complete). Not an app failure.

## qemu-arm user-mode RSS (2026-11-28)

ARM release kobo-browser at b2c8ec83 under qemu-arm-static 7.2.0, x86 sim host.
Includes emulator overhead; not a device proxy. Start page VmHWM 11,392 kB;
after sheets5 page VmHWM 75,980 kB, VmRSS 37,464 kB.

## kobo-hello ARM size growth attribution (Nov 24 -> a76bbff5)

armv7-musl release kobo-hello, 1,249,348 -> 1,265,756 B (+16,408).
Per-commit (bisect + waypoint measurement, evidence/hello-size-bisect-log.txt):

| commit | size after | delta | subject (short) |
|---|---|---|---|
| 9a5dc0d | 1,251,108 | +1,760 | versioned native RGB rich text |
| 7f93512 | 1,253,796 | +2,688 | native heading RGB versioned spans |
| 47a9aac | 1,256,172 | +2,376 | native quote geometry RGB spans |
| b3a9287 | 1,256,172 | +0 | raw preformatted whitespace RGB runs |
| d60e2c3 | 1,259,804 | +3,632 | native table RGB cells/labels/headers |
| 3d8f0e9 | 1,261,300 | +1,496 | source colours on native form titles |
| f72de53..4d5cd7c | 1,261,300 | +0 | (colour fragments, docs) |
| fefec9c | 1,262,100 | +800 | native pre blocks reading-scaled mono RGB |
| (fefec9c..457e8f7] | 1,262,052 | -48 net | |
| 82fc88a | 1,263,996 | +1,944 | painted links bounded versioned regions |
| 0d89beb | 1,264,676 | +680 | stale scroll restores on view changes |
| f592581 | 1,265,724 | +1,048 | retire abandoned requests pre-navigation |
| (b1a6bc2..a76bbff5] | 1,265,756 | +32 net | |

Theme: native-RGB/colour rendering and painted-link regions in kobo-ui /
kobo-protocol / kobo-web-document; no single outlier, largest +3,632 (d60e2c3).

## 071bb0c spot-checks (2026-11-28, host x86_64)

Origin/agent/browser-basic head 071bb0c7ef6be7d05abf367c1e8c71e16d36e27f. All sim
runs debug app + debug drive CLI, fresh state, KOBO_INKLING_DAY=2026-09-01. Host
numbers only; physical-device proof remains an open gate.

### Sheet probe rerun (typed-address landed at head)
3 reps, sheet_compare.py, PAINTED (AddressValue URL marker) not just address change:
sheets5 nav->painted 2.031-2.032 s, settled 2.20 s; sheets16 2.090-2.096 s,
settled 2.26 s; 5/5 and 16/16 stylesheet fetches, no rc=1 timeouts (lead's ce7516d
fix holds, ~6x faster than the pre-fix full-column-layout path). App VmHWM at
sheets16-painted 72,712-72,756 kB. Evidence: evidence/sheets-071bb0c/.

### ARM size audit at 071bb0c (release, stripped, musl)
kobo-browser 5,309,540 B (+25,728 vs 416f3322), kobo-hello 1,265,740 B,
kobod 5,125,300 B. Budget delta browser-hello 4,043,800 B, headroom 1,199,080 B:
WITHIN budget.

### BW-profile app-RSS journey (clara-bw-391, probes/bw_journey.py)
App (kobo-browser) VmRSS/VmHWM kB per checkpoint:
start-page 33,796/33,796; samples-index 37,564; native-article 37,628;
painted-article 63,140 (+25.5 MB over native, the big step); form-page 63,140;
form-links-list 63,140; form-overview 63,140; field-editor 63,140.
Dev server VmHWM 146,508 kB. Evidence: evidence/bw-journey-071bb0c/.
Notes: the samples form at this head has no textarea (fields: Search words, Size,
Available only, Search, Browse) and form-0 now opens a Form overview screen, so
the committed drive-forms.kobo expectations (submit-field-5, note) look stale vs
this UI (UNVERIFIED - the committed route was not run). A true textarea-editor
checkpoint came from a local TLS fixture page (same profile/debug/host):
34,268 VmRSS / 35,672 VmHWM kB on a light single-page session.

### qemu-arm RSS at 071bb0c (ARM release binary under qemu-arm-static 7.2.0)
Same journey legs, qemu process VmRSS/VmHWM kB: start-page 25,620/39,932;
samples-index 29,124/39,932; native-article 31,604/39,932;
painted-article 37,332/75,448. Emulator overhead included; not a device proxy.
Evidence: evidence/qemu-arm-071bb0c/.

### Sphinx fixture journey host RSS (scripts/quality/check-browser-styles-sim.py --sphinx)
PASS at 071bb0c (worktree dirty from probe screenshots): peak_app_rss_kib 76,232
(74.6 MiB), whole journey 56.1 s, 5 requests, styled_frame_changed true.
Evidence: evidence/sphinx-071bb0c/.

## 02d80c6 spot-checks (2026-11-28, host x86_64) - deltas vs 071bb0c

Head 02d80c615c31311067c5a9bf7245172e40f24dfb (css lane v5+v5b: flex stretch into
item layout height, bounded flex measurement; basic_page.rs +132/-45). Same
harness and labels as the 071bb0c section.

### Sheet probe (3 reps, painted = AddressValue marker)
sheets5 painted 2.269/2.301/2.270 s (071bb0c: 2.031-2.032) => +0.24-0.27 s (~+12%);
settled 2.44-2.47 s (was 2.20). sheets16 painted 2.331/2.329/2.328 s
(071bb0c: 2.090-2.096) => +0.23-0.24 s (~+11%); settled 2.499-2.502 (was 2.26).
5/5 and 16/16 fetches all reps, no timeouts. App VmHWM sheets16-painted
72,612-72,896 kB (was 72,712-72,756; unchanged). Consistent painted-time
regression on both pages, plausibly the bounded flex measurement cost
(cache key box+width, MAX_VISITS counting); causal isolation not done.

### ARM size audit (release, stripped, musl)
kobo-browser 5,316,356 B (+6,816 vs 071bb0c), kobo-hello 1,265,740 (unchanged),
kobod 5,125,300 (unchanged). Budget delta browser-hello 4,050,616 B, headroom
1,192,264 B: WITHIN budget. BUDGETS.md at this head records ARM 5,317,388 B,
1,032 B above my measurement (different build env; not reconciled).

### BW RSS journey (clara-bw-391)
App VmHWM kB: start-page 30,396 (boot variance), samples-index 39,196,
native-article 39,260, painted-article 63,208, all form checkpoints 63,208.
Deltas vs 071bb0c: +68 kB at painted; unchanged within noise.

### qemu-arm (ARM release under qemu-arm-static 7.2.0)
VmRSS/VmHWM kB: start-page 25,676/39,948; samples-index 29,316; native-article
31,792; painted-article 37,384/75,432. Unchanged vs 071bb0c (<=200 kB).

### Sphinx fixture journey
PASS. peak_app_rss_kib 74,600 (071bb0c: 76,232; -2.1%), whole journey 64.0 s
(was 56.1 s; includes build/boot variance), 5 requests.

### drive-forms.kobo staleness (committed route run as-is)
FAILS at line 4 at 02d80c6: "tap-id navigate: action 2195720326 is not
reachable on this screen" on the Sample pages index. The route predates the
browser-more menu: navigate now requires tap-id browser-more first. Later steps
are stale too: form-0 opens a Form overview (no submit-field-5), and the
samples form has no textarea/"note". Staleness predates 02d80c6 (my 071bb0c
probes already needed browser-more); introducing commit not bisected.
Evidence: evidence/drive-forms-02d80c6/failed-line-4.png.

## d9a8ab9 spot-checks (2026-11-28, host x86_64) - deltas vs 02d80c6

Head d9a8ab95e63377bbe474e67df39db18163e79928 (css lane ::before/::after
generated decoration: style_tree.rs +225, css.rs +82, box_tree.rs, basic_page.rs;
lead repaired drive-forms.kobo in a975261 and retained my evidence in 17b0a1d).
Same harness and labels as previous sections.

### Sheet probe (3 reps)
sheets5 painted 2.306/2.276/2.273 s, sheets16 painted 2.409/2.332/2.338 s
(02d80c6: 2.269-2.301 / 2.328-2.331): flat within noise, rep1 sheets16 2.409 an
outlier. 5/5 and 16/16 fetches all reps. The +12% regression vs 071bb0c from the
02d80c6 flex-measurement change persists (no further shift from ::before/::after).

### ARM size audit (release, stripped, musl)
kobo-browser 5,319,052 B (+2,696 vs 02d80c6), kobo-hello 1,265,740, kobod
5,125,300 (unchanged). Budget delta browser-hello 4,053,312 B, headroom
1,189,568 B: WITHIN budget.

### BW RSS journey / qemu-arm
BW painted-article 63,112 kB VmHWM (-96, noise), form checkpoints flat.
qemu painted 37,532 VmRSS / 75,424 VmHWM (flat, <=150 kB).

### Sphinx fixture journey
PASS. peak_app_rss_kib 76,496 (02d80c6: 74,600, +2.5%; 071bb0c: 76,232, +0.3%),
journey 61.0 s, 5 requests.

### drive-forms.kobo (repaired route, committed at d9a8ab9)
PASSES rc=0: every step passed, shots form-submitter-choices/result/post-review
taken. The repair adds tap-id browser-more before navigate and routes the
textarea leg via browser-more > forms > form-1 > note. form-post-review.png
inspected: POST form dialog "Review submission - Send form data to
https://samples.browse.invalid? ... This browser cannot send POST yet." with
Cancel. Evidence: evidence/drive-forms-d9a8ab9/.

## A/B: 071bb0c vs 02d80c6 back-to-back (2026-11-28, host x86_64)

CSS worker could not reproduce the +240 ms painted delta reported in the 02d80c6
section. Alternating A/B rerun on this box, quiet, lead's reproduce.py (their
copy of sheet_compare.py; latency = address change, kobo CLI debug binary as the
entry), 5 reps each, order A B A B ... with a debug rebuild per switch (cargo
cache, 16-30 s relink). RESULT: the delta VANISHES.
sheets5: A 2.310/2.313/2.311/2.273/2.315 (mean 2304 ms), B 2.312/2.313/2.306/
2.318/2.277 (mean 2305 ms) => +1 ms. sheets16: A 2.412/2.372/2.372/2.409/2.366
(mean 2386 ms), B 2.406/2.376/2.403/2.370/2.370 (mean 2385 ms) => -1 ms.
CORRECTION: the +12% painted-time regression attributed to 02d80c6 in the
section above was MACHINE DRIFT between sessions, not the flex-measurement
change. Consistent with the CSS worker's identical 1.64/1.64 and 1.70/1.70 s
and their +0.5-1.3 ms prepare() microbench. Also note this box's absolute
times drift between sessions: first 071bb0c session 2.03/2.09 s, A/B session
2.30/2.39 s for the same commit. Cross-session comparisons on this box are not
valid; only interleaved A/B is. Evidence: evidence/ab-071bb0c-vs-02d80c6/.
