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
