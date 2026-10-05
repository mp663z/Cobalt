# Bounded browser verification

Run from the repository root with Kani 0.68.0 installed and set up:

```sh
sh tools/cobalt-verification/run-kani.sh
```

These harnesses call production code with no stubs. Safety and unwind checks
remain enabled. They do not certify a full browser or physical-panel layout.

- Protocol: arbitrary u32 payload length above MAX_FRAME_LEN, header-only input,
  current version and correct magic, unwind 16. Decoder must reject the length
  before reading a payload. It does not cover every valid frame or wire version.
- Empty paginator: empty pieces and anchors, unwind 2, includes final destruction.
- One whole text piece: one symbolic a-z byte, no anchors, one-piece fit oracle,
  unwind 2. Checks byte retention, one page, and termination.
- Whole blank-line piece: symbolic a-z byte then newline/newline/z, no anchors,
  one-piece fit oracle, unwind 5. It does not prove byte retention when split.
- Split helper: symbolic a-z byte followed by z, split at byte 1, unwind 8.
  It does not cover Unicode boundaries, newline cuts, or whole pagination.

Both nonempty paginator harnesses omit final destruction with mem::forget.
That omission is a proof boundary, not a production-code change. A one-piece
oracle is not a pixel-fit oracle. Broader symbolic/container paths previously
timed out; those historical results are not present-day verification claims.

Reconstructed on agent/browser 1d3d1f53 after loss of the unpublished package.
Fuzzing, property-based tests, Miri, and blank-line splitting regression work
remain separate outstanding parts of the verification package.

## Host campaigns

```sh
cargo +1.85.1 test --manifest-path tools/cobalt-verification/Cargo.toml
cargo +nightly-2026-08-21 miri test --manifest-path tools/cobalt-verification/Cargo.toml newline_split_loss_is_recorded_not_called_preserved
cd tools/cobalt-verification
cargo +nightly-2026-08-21 fuzz run protocol -- -max_total_time=10 -max_len=4096
cargo +nightly-2026-08-21 fuzz run html -- -max_total_time=10 -max_len=4096
```

The three property tests each generate 256 cases. They cover short protocol
inputs, Unicode/blank-line retention of whole pieces, and order of bounded
whole pieces. The fourth test records a known defect: splitting `a\n\nz`
under a three-byte oracle loses boundary newlines. Its success means the
loss was reproduced, not that preservation was proved. Replace its inequality
with equality only when production semantics are fixed.

Fresh campaigns on October 5, 2026: all four host tests passed; Miri passed
the one fixed newline-loss reproduction (three other tests filtered out).
Protocol fuzzing executed 3,553,053 inputs and HTML fuzzing 101,466 inputs,
each in 11 seconds with max input length 4096, no crash found. These short,
coverage-guided campaigns are not exhaustive proofs. HTML uses the production
parser's default limits. No network or real-account content is fetched.
