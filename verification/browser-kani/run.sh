#!/bin/sh
# Runs each Kani harness separately; one log per harness in $OUT (default /tmp/kani-logs).
# Usage: run.sh [harness ...]   (default: every harness in kani_checks.rs)
. "$HOME/.cargo/env"
OUT=${OUT:-/tmp/kani-logs}; mkdir -p "$OUT"
cd "$(dirname "$0")/../../crates/kobo-browser" || exit 1
H=${*:-$(grep -A8 '^#\[kani::proof\]' src/kani_checks.rs | grep -o 'fn [a-z_0-9]*' | cut -d' ' -f2)}
for h in $H; do
  s=$(date +%s)
  cargo kani -Z stubbing --solver cadical $KANI_EXTRA --harness "$h" > "$OUT/$h.log" 2>&1
  echo "$h exit=$? secs=$(( $(date +%s)-s )) $(grep -m1 '^VERIFICATION' "$OUT/$h.log")" >> "$OUT/summary.txt"
done
echo done >> "$OUT/summary.txt"
