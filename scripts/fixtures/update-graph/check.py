#!/usr/bin/env python3
"""Replay the update-graph fixtures against the contract.

Contract: docs/quality/contracts/update-graph.md. Each edge in edges.json was
produced by running the real updater (crates/kobod/src/update.rs, test
update_graph_edges_match_the_committed_contract) against the committed
archive; this script is the structural gate for lanes that do not build Rust.
It re-parses every archive, checks the path policy, cross-checks the manifest
fields against the recorded outcome, and replays the interruption edge's
checkpoint recovery. A structural failure here or a behavior failure there is
a contract change, not a fixture refresh. Regenerate only from a known-good
tree with KOBO_BLESS=1.
"""

import gzip
import hashlib
import io
import json
import sys
import tarfile
from pathlib import Path

FIXTURE_DIR = Path(__file__).resolve().parent
PREFIX = "mnt/onboard/.adds/cobalt"
LAUNCHER = "mnt/onboard/.adds/cobalt-launch.sh"
RELEASE_SCHEMA = 1
UPDATER_CAPABILITY = 1
ALLOWED_ROOTS = {"cobalt", "launcher"}
KNOWN_MIGRATIONS = {"nickelmenu"}
STAGES = {
    "discovery", "dns", "tls", "signature", "archive policy",
    "disk", "migration", "activation", "launch canary", "hand-back",
}


def fail(message):
    print(f"update-graph contract: {message}", file=sys.stderr)
    sys.exit(1)


def check_archive_edge(edge):
    archive_path = FIXTURE_DIR / edge["archive"]
    raw = archive_path.read_bytes()
    if hashlib.sha256(raw).hexdigest() != edge["archive_sha256"]:
        fail(f"{edge['id']}: archive bytes do not match the recorded digest")
    try:
        tar = tarfile.open(fileobj=io.BytesIO(gzip.decompress(raw)))
    except (OSError, tarfile.TarError) as error:
        fail(f"{edge['id']}: archive does not expand: {error}")
    members = [
        member for member in tar.getmembers()
        if member.name == LAUNCHER or member.name == PREFIX
        or member.name.startswith(PREFIX + "/")
    ]
    all_members = tar.getmembers()
    if len(members) != len(all_members):
        outside = [m.name for m in all_members if m not in members]
        fail(f"{edge['id']}: members outside the allowed roots: {outside}")
    # Tar readers normalize the trailing slash on folder names.
    recorded = {m["path"].rstrip("/"): m for m in edge["members"]}
    if {m.name for m in members} != set(recorded):
        fail(f"{edge['id']}: member listing drifted from the record")
    for member in members:
        expect = recorded[member.name]
        if member.isdir():
            if expect["kind"] != 53:
                fail(f"{edge['id']}: {member.name} kind drifted")
            continue
        payload = tar.extractfile(member).read()
        if len(payload) != expect["bytes"]:
            fail(f"{edge['id']}: {member.name} size drifted")
        if hashlib.sha256(payload).hexdigest() != expect["sha256"]:
            fail(f"{edge['id']}: {member.name} payload drifted")
        if member.name == f"{PREFIX}/release.json":
            manifest = json.loads(payload)
            if manifest != edge["manifest"]:
                fail(f"{edge['id']}: manifest drifted from the record")
    if not edge["manifest"] and f"{PREFIX}/release.json" in recorded:
        fail(f"{edge['id']}: archive carries a manifest the record denies")

    outcome = edge["outcome"]
    manifest = edge["manifest"]
    refuses = False
    if manifest:
        refuses = (
            manifest.get("schema", 0) > RELEASE_SCHEMA
            or manifest.get("requiresUpdater", 0) > UPDATER_CAPABILITY
            or any(root not in ALLOWED_ROOTS for root in manifest.get("roots", []))
            or any(m not in KNOWN_MIGRATIONS for m in manifest.get("migrations", []))
        )
    if refuses:
        if outcome.get("result") != "refused":
            fail(f"{edge['id']}: an archive this updater cannot honor installed")
        if outcome.get("stage") != "archive policy":
            fail(f"{edge['id']}: refusal is not staged archive policy")
        ledger = outcome.get("ledger", "")
        if not ledger.startswith("archive policy: "):
            fail(f"{edge['id']}: ledger line is not staged: {ledger!r}")
    else:
        if outcome.get("result") != "installed":
            fail(f"{edge['id']}: an honorable archive was refused")


def check_interruption_edge(edge):
    checkpoints = edge["checkpoints"]
    if not checkpoints:
        fail("interrupted-at-every-checkpoint: no checkpoints recorded")
    if edge["outcome"].get("result") != "recovered":
        fail("interrupted-at-every-checkpoint: recovery was not the outcome")
    steps = [c["step"] for c in checkpoints]
    if len(steps) != len(set(steps)):
        fail("interrupted-at-every-checkpoint: a checkpoint ran twice")
    if steps[0] != "SetForward" or steps[-1] != "ClearJournal":
        fail("interrupted-at-every-checkpoint: not the full transaction")
    for checkpoint in checkpoints:
        if checkpoint["recovered"] not in ("old", "new"):
            fail(f"checkpoint {checkpoint['step']}: recovered to neither release")
        # Power lost before the journal lands must restore the old release;
        # every later boundary promotes the new one.
        expected = "old" if checkpoint["step"] == "SetForward" else "new"
        if checkpoint["recovered"] != expected:
            fail(
                f"checkpoint {checkpoint['step']}: recovered "
                f"{checkpoint['recovered']}, contract says {expected}"
            )


def main():
    index = json.loads((FIXTURE_DIR / "edges.json").read_text())
    if index.get("contract") != "update-graph":
        fail("edges.json is not an update-graph contract record")
    edges = index["edges"]
    if len(edges) < 6:
        fail(f"only {len(edges)} edges recorded; the contract names more")
    for edge in edges:
        if edge.get("archive"):
            check_archive_edge(edge)
        else:
            check_interruption_edge(edge)
        stage = edge["outcome"].get("stage")
        if stage and stage not in STAGES:
            fail(f"{edge['id']}: stage {stage!r} is outside the taxonomy")
    ids = {edge["id"] for edge in edges}
    for required in (
        "stable-to-candidate",
        "pre-bootstrap-layout-to-current-bootstrap",
        "interrupted-at-every-checkpoint",
    ):
        if required not in ids:
            fail(f"required edge {required} is missing")
    print(f"update-graph contract: {len(edges)} edges replayed and held")


if __name__ == "__main__":
    main()
