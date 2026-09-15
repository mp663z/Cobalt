#!/usr/bin/env python3
"""Replay the session-version wire fixtures against the contract.

Contract: docs/quality/contracts/protocol-compatibility.md. Each
session-v<N>.bin transcript is the exact byte stream the simulator sent one
app session that greeted with protocol version N: a Welcome frame followed by
Foreground and Background lifecycle frames. Every frame in a transcript must
ride version N - the session version is fixed at greet time and never
changes afterwards.

The Rust matrix test (crates/kobo-sim/src/protocol_matrix_tests.rs) is the
producer and byte-exact comparator; this script is the structural gate for
lanes that do not build Rust: it parses each transcript frame by frame and
checks shape, ordering, and per-frame version bytes. A structural failure
here or a byte failure there means the wire format or the session-version
rule drifted - a contract change, not a fixture refresh. Regenerate only
from a known-good runtime with KOBO_BLESS=1.
"""

import json
import struct
import sys
from pathlib import Path

FIXTURE_DIR = Path(__file__).resolve().parent
HEADER_LEN = 14
MAGIC = b"KOBO"
WELCOME_KIND = 0x02
LIFECYCLE_KIND = 0x0F
EXPECTED_SEQUENCE = [
    (WELCOME_KIND, "Welcome"),
    (LIFECYCLE_KIND, "Foreground"),
    (LIFECYCLE_KIND, "Background"),
]


def parse_frames(data: bytes):
    offset = 0
    while offset < len(data):
        if offset + HEADER_LEN > len(data):
            raise ValueError(f"truncated header at byte {offset}")
        header = data[offset : offset + HEADER_LEN]
        if header[0:4] != MAGIC:
            raise ValueError(f"bad magic at byte {offset}: {header[0:4]!r}")
        version = header[4]
        kind = header[5]
        payload_len = struct.unpack(">I", header[6:10])[0]
        end = offset + HEADER_LEN + payload_len
        if end > len(data):
            raise ValueError(f"truncated payload at byte {offset}")
        yield version, kind, header[HEADER_LEN:end]
        offset = end
    if offset != len(data):
        raise ValueError("trailing bytes after final frame")


def main() -> int:
    manifest = json.loads((FIXTURE_DIR / "manifest.json").read_text())
    failures = []
    for entry in manifest["sessions"]:
        version = entry["version"]
        path = FIXTURE_DIR / entry["file"]
        label = f"session-v{version}"
        if not path.exists():
            failures.append(f"{label}: missing fixture {path.name}")
            continue
        try:
            frames = list(parse_frames(path.read_bytes()))
        except ValueError as error:
            failures.append(f"{label}: {error}")
            continue
        if len(frames) != len(EXPECTED_SEQUENCE):
            failures.append(
                f"{label}: expected {len(EXPECTED_SEQUENCE)} frames, got {len(frames)}"
            )
            continue
        for index, ((frame_version, kind, _payload), (want_kind, want_name)) in enumerate(
            zip(frames, EXPECTED_SEQUENCE)
        ):
            if kind != want_kind:
                failures.append(
                    f"{label}: frame {index} is kind {kind:#04x}, expected {want_name}"
                )
            if frame_version != version:
                failures.append(
                    f"{label}: frame {index} rides version {frame_version}, "
                    f"session greeted with {version}"
                )
    if failures:
        for failure in failures:
            print(f"FAIL {failure}", file=sys.stderr)
        return 1
    print(f"ok: {len(manifest['sessions'])} session transcripts match the contract")
    return 0


if __name__ == "__main__":
    sys.exit(main())
