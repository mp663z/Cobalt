#!/usr/bin/env python3
"""Drive the real Store app against private local signed package transactions."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--scale', default='extra-large')
    args = parser.parse_args()
    cli = ROOT / 'target/debug/kobo'
    builder = ROOT / 'target/debug/examples/signed-store'
    payload = ROOT / 'target/debug/examples/quality-fixture'
    if not cli.is_file() or not builder.is_file() or not payload.is_file():
        parser.error('Build kobo-cli and the kobo-sim signed-store and quality-fixture examples first.')
    args.output.mkdir(parents=True, mode=0o700)
    process = None
    with tempfile.TemporaryDirectory(prefix='cobalt-store-', dir='/tmp') as private:
        fixture = Path(private) / 'fixture'
        env = dict(os.environ, TMPDIR=private, CARGO_TARGET_DIR=str(ROOT / 'target'),
                   CARGO_PROFILE_DEV_DEBUG='0', CARGO_INCREMENTAL='0',
                   KOBO_SIM_PROFILE='clara-bw-391', KOBO_TEXT_SCALE=args.scale,
                   KOBO_SIM_APP_STORE=str(fixture), KOBO_SIM_FIXTURE='original-signed-store',
                   # Installs in this journey face the launch canary: the
                   # packaged payload is the real quality-fixture application.
                   KOBO_SIM_CANARY='1',
                   KOBO_SIM_SEED='0', KOBO_SIM_CLOCK_MILLIS='1788850860000')
        with (args.output / 'simulator.log').open('w') as log:
            def publish(version, mode='publish', live=True):
                # A live payload is the real fixture application; a broken one
                # is inert bytes that cannot complete a launch canary.
                publish_env = dict(env, KOBO_FIXTURE_PAYLOAD=str(payload)) if live else env
                subprocess.run([str(builder), mode, str(fixture), version], check=True,
                               env=publish_env, stdout=log, stderr=log, timeout=15)

            def stop():
                if process is not None and process.poll() is None:
                    os.killpg(process.pid, signal.SIGTERM)
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        os.killpg(process.pid, signal.SIGKILL)
                        process.wait(timeout=5)

            try:
                publish('1.0.0', 'init')

                def start():
                    nonlocal process
                    start_offset = log.tell()
                    process = subprocess.Popen([str(cli), 'dev', '127.0.0.1:0'],
                                               cwd=ROOT / 'examples/store', env=env,
                                               stdout=log, stderr=log, start_new_session=True)
                    deadline = time.monotonic() + 120
                    while time.monotonic() < deadline:
                        text = (args.output / 'simulator.log').read_text()[start_offset:]
                        if process.poll() is not None:
                            raise RuntimeError('Store simulator stopped: ' + text[-2000:])
                        match = re.search(r'Kobo app simulator: http://(127\.0\.0\.1:\d+)', text)
                        if match:
                            return match.group(1)
                        time.sleep(.1)
                    raise RuntimeError('Store simulator did not start')

                address = start()

                def drive(*steps):
                    command = [str(cli), 'drive', '--address', address, '--ideal', '--shots', str(args.output)]
                    for step in steps:
                        command.extend(['--step', step])
                    subprocess.run(command, cwd=ROOT, env=env, check=True, timeout=30,
                                   stdout=log, stderr=log)

                def installed():
                    return json.loads((fixture / 'installed/apps/quality-fixture/manifest.json').read_text())['version']

                drive('wait-idle', 'expect-state /simulation#/appStore/mode "signed-local"',
                      'tap-id app-quality-fixture', 'shot available',
                      'tap-id install-quality-fixture', 'expect installed successfully', 'shot installed')
                assert installed() == '1.0.0'
                notes = fixture / 'installed/data/quality-fixture/notes'
                notes.parent.mkdir(parents=True)
                notes.write_text('Original owner fixture note\n')
                publish('1.1.0')
                drive('tap-id refresh', 'tap-id app-quality-fixture', 'shot update',
                      'scenario storage-full', 'tap-id install-quality-fixture', 'expect Check free space', 'shot failed')
                assert installed() == '1.0.0'
                drive('scenario normal', 'tap-id app-quality-fixture',
                      'tap-id install-quality-fixture', 'expect updated successfully', 'shot updated')
                assert installed() == '1.1.0'
                stop()
                address = start()
                drive('wait-idle', 'expect Installed · 1.1.0', 'shot reopened',
                      'tap-id app-quality-fixture', 'tap-id remove-quality-fixture',
                      'expect removed successfully', 'shot removed')
                assert not (fixture / 'installed/apps/quality-fixture').exists()
                assert notes.read_text() == 'Original owner fixture note\n'
                drive('tap-id app-quality-fixture', 'tap-id install-quality-fixture',
                      'expect installed successfully', 'shot reinstalled')
                assert installed() == '1.1.0'
                # Quarantine journey: five recorded crashes flag the listing, the
                # Store offers the recovery flow, and a reset clears the flag.
                health = fixture / 'installed/health'
                health.mkdir(parents=True)
                (health / 'quality-fixture').write_text('crashes=5\n')
                drive('tap-id refresh', 'expect Quarantined · 1.1.0', 'shot quarantined',
                      'tap-id app-quality-fixture', 'expect Recovery options',
                      'shot quarantined-detail',
                      'tap-id recovery-quality-fixture', 'expect Reset saved state',
                      'shot quarantined-recovery',
                      'tap-id recover-reset-quality-fixture', 'expect Confirm recovery',
                      'shot quarantined-confirm',
                      'tap-id recovery-confirm', 'expect state was reset',
                      'expect Installed · 1.1.0', 'shot quarantined-cleared')
                assert not (health / 'quality-fixture').exists()
                assert not notes.exists(), 'reset removes the saved state'
                # A candidate that cannot launch never replaces what runs:
                # the canary refuses it, the store says so, 1.1.0 stays.
                publish('1.2.0', live=False)
                drive('tap-id refresh', 'tap-id app-quality-fixture',
                      'tap-id install-quality-fixture',
                      'expect could not start after install', 'shot canary-failed')
                assert installed() == '1.1.0', 'the failed canary kept the previous version'
                failed = fixture / 'installed/apps/quality-fixture.failed'
                assert (failed / 'DIAGNOSTICS').is_file(), 'failed candidate keeps diagnostics'
                assert not (fixture / 'installed/apps/quality-fixture.next').exists()
                with urllib.request.urlopen(f'http://{address}/simulation', timeout=5) as response:
                    simulation = json.load(response)
                result = dict(status='passed', basis='store-app-sdk-ipc-and-runtime-transactions',
                              original_fixture=True,
                              fixture_payload='quality-fixture example; every install passes the launch canary',
                              scale=args.scale, app_store=simulation['appStore'],
                              checks=['install', 'disk-full preserves version', 'update', 'process restart',
                                      'remove preserves data', 'reinstall',
                                      'quarantine flags the listing', 'recovery flow', 'reset clears the flag',
                                      'launch canary passes on every install',
                                      'failed canary keeps the previous version'],
                              cli_sha256=hashlib.sha256(cli.read_bytes()).hexdigest(),
                              builder_sha256=hashlib.sha256(builder.read_bytes()).hexdigest(),
                              source_head=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                              source_dirty=bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT)))
                (args.output / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
            finally:
                stop()


if __name__ == '__main__':
    main()
