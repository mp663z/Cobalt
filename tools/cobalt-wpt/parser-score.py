#!/usr/bin/env python3
"""Parse per-case WPT-derived counts from the crate's existing Rust tests."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

EXPECTED = {'html-tree', 'http-url', 'html-named-entities', 'windows-1252-index'}
ROW = re.compile(r'WPT_SCORE suite=([a-z0-9-]+) passed=(\d+) run=(\d+) skipped=(\d+)')


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('output', type=Path)
    ap.add_argument('--cargo', default='cargo')
    args = ap.parse_args()
    command = [args.cargo, 'test', '-p', 'kobo-web-document', '--all-targets', '--', '--nocapture']
    env = os.environ.copy()
    env['WPT_SCORE'] = '1'
    completed = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                               check=False, env=env, timeout=120)
    output = completed.stdout.decode('utf-8', errors='replace')
    rows = {}
    for suite, passed, run, skipped in ROW.findall(output):
        if suite in rows:
            raise ValueError('duplicate row: '+suite)
        rows[suite] = {'passed': int(passed), 'run': int(run), 'skipped': int(skipped)}
    if completed.returncode != 0 or set(rows) != EXPECTED:
        raise RuntimeError(f'Rust tests failed or incomplete score rows: status={completed.returncode}, rows={list(rows)}\n{output[-2500:]}')
    if any(row['passed'] > row['run'] for row in rows.values()):
        raise ValueError('impossible score')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    result = {'suite': 'WPT-derived parser cases, separate from CSS visual pass rate',
              'upstream_commit': '7efa3ab5de117cfd01ac23b65a7ab58d43397238',
              'cases': rows}
    args.output.write_text(json.dumps(result, indent=2)+'\n')
    for suite, row in rows.items():
        print(f"{suite}: {row['passed']}/{row['run']} passed, {row['skipped']} skipped")
    return 0


if __name__ == '__main__':
    sys.exit(main())
