# Current sheet checks

Source under test: 4f223544, x86 host debug app/CLI, libra-colour-390.
Three runs of qualification probe from 4bbe635. Only target path changed to
ROOT/target. Run reproduce.py ROOT DEBUG_KOBO OUT_JSON REP_LABEL.
All link routes rc0; no 10-second tap timeout. All 5/5 and16/16 fetched.
nav5 1.606/1.603/1.684 seconds; nav16 1.668/0.728/0.701 seconds.
16-sheet app VmHWM72744/72788/72944KiB; checkpointRSS64332/64392/64300KiB.
Not device performance or causal attribution to one change. No pixel/fidelity
assertion in this probe; independent TLS simulator route has screenshot checks.
Earlier qualification b2c8ec83 exceeded10s on16-sheet link tap despite navigating.
