# Frozen-page comparison

Run `python3 tools/cobalt-parity/score.py <frozen-corpus.zip> <scratch-output-dir>`
from the workspace root. The ZIP must contain `manifest.json` and each
`<name>.complete.html`, with matching SHA256 hashes. The script never fetches
web pages. It renders the identical local bytes with sandbox Chrome and the
Cobalt host preview at each manifest viewport, then writes `scoreboard.json`
and, only for a successful pair, RGB side-by-side and pixel-difference PNGs.

A Cobalt failure is recorded as `no paired render`, with `diff_percent: null`.
An unpainted white placeholder must never be counted as a Cobalt render. The
host preview currently supports only definite empty block backgrounds, not
normal text or complete CSS. Chrome itself needs an installed `google-chrome`.
The frozen real-page corpus and captured screenshots are passed as artifacts,
not checked into source control.
