# Browser history properties

Test-only package with its own lockfile, outside the runtime workspace.
It links actual browser-core and compares it to the separate history model.

Run with the pinned lock and recorded seed:

    PROPTEST_RNG_SEED=6829501 cargo +nightly test --locked --manifest-path verification/browser-proptest/Cargo.toml

proptest is pinned to 1.12.0. Two properties run 512 cases each: random traces
up to 399 actions and capacity-crossing prefixes followed by random navigation.
The two existing bounded model tests also run here.

Mutation check: removing forward-branch truncation fails and shrinks to
[0, 1, 3, 1], meaning navigate A, navigate A#part, Back, navigate A#part.
The mutation was removed. No intentionally failing regression seed is retained.

This verifies these history properties only. It does not complete parser,
application, hardware, Kani, Miri or broader proof gates. Nightly is used only
for this isolated verification package; runtime toolchain and lock are unchanged.

Cache properties add 256 cases each for arbitrary stored bytes (up to 4095
bytes) and random insert/touch/remove traces (up to 99 actions). They check
entry/byte budgets, unique shelf names, URL-name agreement, encode/decode
roundtrips, rejected-insert stability and eviction results. Disabling byte-budget
eviction fails and shrinks to two distinct inserts totaling over the budget.
Production was restored and the artificial failure seed removed. These are
cache invariant checks, not a full independent eviction-policy model.
