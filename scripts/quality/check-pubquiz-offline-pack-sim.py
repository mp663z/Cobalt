#!/usr/bin/env python3
"""Pub Quiz offline pack, end to end: a previously synced pack plays with no network.

Stages the canned pack fixture as the app's saved store keys (exactly what a
successful sync would have written), launches the simulator in the offline
scenario, and proves the refreshed pack - not the built-in set - is what plays.
"""
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / 'scripts/fixtures/pubquiz/pack.json'
# rounds=7, so the round starts at fixture question 7*10 % 12 = 10.
STATE = b'1|7|20469|4|Ada|Bert|Cleo|Dev'
FIRST_QUESTION = 'How many keys does a standard fixture piano have?'


def stop_group(process):
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=5)


def main():
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path('/tmp/pubquiz-offline-check')
    out.mkdir(parents=True, exist_ok=True)
    kobo = ROOT / 'target/debug/kobo'
    with tempfile.TemporaryDirectory(prefix='cb-pubquiz-offline-', dir='/tmp') as state:
        env = dict(os.environ, TMPDIR=state)
        store = Path(state) / 'cobalt-sim-state' / 'pubquiz'
        store.mkdir(parents=True)
        (store / 'pubquiz-pack-v1').write_bytes(FIXTURE.read_bytes())
        (store / 'pubquiz-state').write_bytes(STATE)
        log_path = out / 'pubquiz-offline.log'
        with log_path.open('w') as log:
            process = subprocess.Popen([str(kobo), 'dev', '127.0.0.1:0'],
                                       cwd=ROOT / 'apps/pubquiz', env=env,
                                       stdout=log, stderr=log, start_new_session=True)
            deadline = time.monotonic() + 60
            address = None
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError(f'simulator exited early; see {log_path}')
                seen = re.findall(r'Kobo app simulator: http://(127\.0\.0\.1:\d+)',
                                  log_path.read_text())
                if seen:
                    address = seen[-1]
                    break
                time.sleep(0.2)
            if address is None:
                raise RuntimeError(f'no simulator address within 60s; see {log_path}')

            def drive(*steps):
                command = [str(kobo), 'drive', '--address', address, '--ideal',
                           '--shots', str(out)]
                for step in steps:
                    command.extend(['--step', step])
                subprocess.run(command, env=env, cwd=ROOT, stdout=log, stderr=log,
                               check=True, timeout=60)

            try:
                drive('scenario offline',
                      'wait-for Open Trivia DB',
                      'tap Solo round',
                      f'wait-for {FIRST_QUESTION}',
                      'expect Easy',
                      'clean',
                      'shot offline-pack',
                      'tap-id answer-1',
                      'expect Round result',
                      'expect Correct',
                      'tap Back',
                      'expect Pub Quiz',
                      'scenario normal')
                result = dict(status='passed', question=FIRST_QUESTION)
            finally:
                stop_group(process)
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        print(f"pubquiz offline pack e2e passed: staged pack played offline; "
              f"report: {out / 'result.json'}")


if __name__ == '__main__':
    main()
