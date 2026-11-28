# Scoped workspace tests at d6e7723b

Rust1.85.1, locked. Workspace --no-run compiled excluding kobo-web-document and
kobo-browser (both separately tested earlier). Ran104unit/integration binaries
in bounded batches, --test-threads=2. 4045passing test executions after reruns.
Fanshelf45s timeout resolved with100s budget:23pass in99.37s.
CLI initial436pass+2environment failures, both exact failures reran passing
with PATH and ARM crossCC/AR supplied. Gallery12pass1FAIL:11 toolkit controls
missing from gallery coverage. Fullworkspace is NOT green. No docs tests run.
Web-document full1431pass18ignored at bcdd7b47; browser273pass3ignored in
4exact-name batches at73360e4f, not rerun now. Unrun here: all doc tests,
ignored tests, currenthead browser suite. No hardware qualification.

Comic fixture restored byte-for-byte from gitblob39fc720ed28cde16ebf2c31db22829e935dd5a96;
missing fixture caused cascading unsized loop inference, no loop fix necessary.
Panels41pass; bookview30pass. State JSON preserves initial outcomes honestly.
