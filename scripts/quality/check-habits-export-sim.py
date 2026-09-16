#!/usr/bin/env python3
"""Habits owner export, end to end: sim journey prepares the copy, CLI receives it."""
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
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path('/tmp/habits-export-check')
    out.mkdir(parents=True, exist_ok=True)
    stale = out / 'received'
    if stale.exists():
        import shutil
        shutil.rmtree(stale)
    kobo = ROOT / 'target/debug/kobo'
    with tempfile.TemporaryDirectory(prefix='cb-habits-export-', dir='/tmp') as state:
        env = dict(os.environ, TMPDIR=state)
        log_path = out / 'habits-export.log'
        with log_path.open('w') as log:
            process = subprocess.Popen([str(kobo), 'dev', '127.0.0.1:0'],
                                       cwd=ROOT / 'apps/habits', env=env,
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

            receive = [str(kobo), 'export', '--app', 'habits', '--sim',
                       '--out', str(out / 'received')]
            try:
                drive('wait-for Habits', 'tap Add a habit', 'type Tea', 'tap Add',
                      'tap Today', 'expect Tea')
                before = subprocess.run(receive, env=env, cwd=ROOT,
                                        capture_output=True, timeout=15)
                assert before.returncode != 0 and not (out / 'received').exists(), \
                    'a copy was available before the owner asked for one'
                drive('tap Stats', 'tap Settings', 'expect Stored on this reader',
                      'tap Export a copy', 'expect Save a copy',
                      'wait-for Ready for your computer', 'shot export-ready')
                subprocess.run(receive, env=env, cwd=ROOT,
                               stdout=log, stderr=log, check=True, timeout=15)
                received = list((out / 'received').iterdir())
                assert len(received) == 1, f'expected one received file, got {received}'
                text = received[0].read_text()
                assert text.startswith('Habits\n'), text[:80]
                assert 'tea - daily\n' in text, text
                assert 'completions; current streak' in text, text
                again = subprocess.run(receive, env=env, cwd=ROOT,
                                       capture_output=True, timeout=15)
                assert again.returncode == 0
                assert list((out / 'received').iterdir()) == received, \
                    'a second receive replaced or duplicated the first copy'
                result = dict(status='passed', received=received[0].name,
                              bytes=len(text.encode()))
            finally:
                stop_group(process)
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        print(f"habits export e2e passed: received {result['received']} "
              f"({result['bytes']} bytes); report: {out / 'result.json'}")


if __name__ == '__main__':
    main()
