#!/bin/sh
# Runs every browser fuzz target briefly with small memory and time limits.
# usage: smoke.sh [SECONDS_PER_TARGET]   (needs nightly and cargo-fuzz)
set -eu
cd "$(dirname "$0")"
seconds=${1:-30}
root=../../..
seeds() {
    case $1 in
        decode_image) echo "$root/apps/browser/tests/images" ;;
        parse_html | sniff_body) echo "$root/crates/kobo-web-document/tests/fixtures" ;;
        *) echo "seeds/$1" ;;
    esac
}
failed=0
log=$(mktemp)
trap 'rm -f "$log"' EXIT HUP INT TERM
for target in parse_html paginate_document decode_image cache_metadata resolve_url sniff_body; do
    mkdir -p "corpus/$target"
    echo "== $target"
    status=0
    cargo +nightly fuzz run -O "$target" "corpus/$target" "$(seeds "$target")" -- \
        -max_total_time="$seconds" -rss_limit_mb=512 -max_len=262144 -timeout=10 \
        -print_final_stats=1 > "$log" 2>&1 || status=$?
    if [ "$status" -ne 0 ]; then
        # Keep the complete diagnostic, including errors the success filter
        # does not recognize. Try later targets, but fail the overall run.
        cat "$log"
        printf 'FAIL %s (exit %s)\n' "$target" "$status" >&2
        failed=1
    else
        grep -E '^(stat::number_of_executed_units|stat::peak_rss_mb|Done)' "$log" || true
    fi
done
exit "$failed"
