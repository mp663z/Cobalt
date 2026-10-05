#!/bin/sh
# Real production functions; no stubs or disabled safety/unwind checks.
set -eu
case "${1:-all}" in
 protocol) cargo kani -p kobo-protocol --harness oversized_header_is_rejected_before_payload_decode -Z unstable-options --harness-timeout 90s ;;
 layout) cargo kani -p kobo-web-layout -Z unstable-options --harness-timeout 90s --cbmc-args --symex-cache-dereferences ;;
 all) sh "$0" protocol; sh "$0" layout ;;
 *) printf '%s\n' 'Use all, protocol or layout.' >&2; exit 2 ;;
esac
