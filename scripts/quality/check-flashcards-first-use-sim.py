#!/usr/bin/env python3
"""Flashcards first use, end to end: no collection staged, the app offers a
setup screen (not an error), and the built-in sample deck writes, loads and
reviews."""
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


def stop_group(process):
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=5)


def main():
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path('/tmp/flashcards-first-use')
    out.mkdir(parents=True, exist_ok=True)
    kobo = ROOT / 'target/debug/kobo'
    with tempfile.TemporaryDirectory(prefix='cb-flashcards-first-', dir='/tmp') as state:
        env = dict(os.environ, TMPDIR=state)
        log_path = out / 'flashcards-first-use.log'
        with log_path.open('w') as log:
            process = subprocess.Popen([str(kobo), 'dev', '127.0.0.1:0'],
                                       cwd=ROOT / 'apps/flashcards', env=env,
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
                drive('wait-for Set up Flashcards',
                      'expect Read collection again',
                      'clean',
                      'shot first-use',
                      'tap Start with the sample deck',
                      'wait-for Nature Notes',
                      'clean',
                      'shot sample-deck',
                      'tap Nature Notes',
                      'wait-for What does a compass point toward?',
                      'clean',
                      'shot sample-card')
                result = dict(status='passed')
            finally:
                stop_group(process)
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        print(f"flashcards first-use e2e passed; report: {out / 'result.json'}")


if __name__ == '__main__':
    main()
