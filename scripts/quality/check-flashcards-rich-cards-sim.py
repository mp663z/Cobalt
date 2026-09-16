#!/usr/bin/env python3
"""Flashcards rich cards, end to end: a staged bundle with a Japanese deck, a
long paginated card and a picture card. Proves the review journey renders and
grades all three kinds at the current text scale."""
import json
import os
from pathlib import Path
import re
import shutil
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
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path('/tmp/flashcards-rich')
    out.mkdir(parents=True, exist_ok=True)
    kobo = ROOT / 'target/debug/kobo'
    with tempfile.TemporaryDirectory(prefix='cb-flashcards-rich-', dir='/tmp') as state:
        shelf = Path(state) / 'cobalt-sim-data' / 'flashcards'
        shelf.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / 'scripts/fixtures/flashcards/rich.cobfc',
                        shelf / 'collection.cobfc')
        env = dict(os.environ, TMPDIR=state)
        log_path = out / 'flashcards-rich.log'
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
                drive('wait-for Japanese',
                      'expect Long reads',
                      'clean',
                      'shot rich-decks',
                      'tap-id deck-page-next',
                      'wait-for Pictures',
                      'tap-id deck-page-prev',
                      'wait-for Japanese',
                      'tap Japanese',
                      'wait-for この言葉を読みます。',
                      'clean',
                      'shot japanese-question',
                      'tap-id answer',
                      'wait-for-id good',
                      'expect 答えは日本語です。',
                      'clean',
                      'shot japanese-answer',
                      'tap-id good',
                      'wait-for-id answer',
                      'tap-id answer',
                      'wait-for-id good',
                      'tap-id easy',
                      'wait-for Review complete',
                      'tap-id back-decks',
                      'wait-for Long reads',
                      'tap Long reads',
                      'wait-for-id answer',
                      'clean',
                      'shot long-card',
                      'tap-id answer',
                      'wait-for-id good',
                      'tap-id good',
                      'wait-for Review complete',
                      'tap-id back-decks',
                      'wait-for Long reads',
                      'tap-id deck-page-next',
                      'wait-for Pictures',
                      'tap Pictures',
                      'wait-for What does this picture show?',
                      'clean',
                      'shot picture-question',
                      'tap-id answer',
                      'wait-for A sun.',
                      'clean',
                      'shot picture-answer')
                result = dict(status='passed')
            finally:
                stop_group(process)
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        print(f"flashcards rich-cards e2e passed; report: {out / 'result.json'}")


if __name__ == '__main__':
    main()
