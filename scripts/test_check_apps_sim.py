"""The simulator sweep must fail on a bad route and clean only its own state."""
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('check_apps_sim', Path(__file__).with_name('check-apps-sim.py'))
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class SimulatorRunnerTests(unittest.TestCase):
    def run_fixture(self, startup_failure=False):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            app = root / 'apps/fixture'
            app.mkdir(parents=True)
            (app / 'drive.kobo').write_text('expect Home\n')
            out = root / 'output'
            out.mkdir()
            marker = root / 'state.json'
            fake = root / 'kobo'
            fake.write_text(f'''#!{sys.executable}
import json, os, sys, time
from pathlib import Path
if sys.argv[1] == 'dev':
    Path(os.environ['TEST_STATE_FILE']).write_text(json.dumps([os.environ['TMPDIR'], os.getpid()]))
    if os.environ.get('TEST_STARTUP_FAILURE') == '1':
        raise SystemExit(9)
    print('Kobo app simulator: http://127.0.0.1:12345', flush=True)
    time.sleep(60)
elif '--script' in sys.argv:
    raise SystemExit(7)
else:
    print('text ["Home"]')
''')
            fake.chmod(0o755)
            env = dict(os.environ, TEST_STATE_FILE=str(marker), TEST_STARTUP_FAILURE=str(int(startup_failure)))
            with patch.object(runner, 'ROOT', root):
                result = runner.run_app('fixture', fake, out, env, 3)
            self.assertEqual(result['status'], 'fail')
            self.assertEqual(result['launched'], not startup_failure)
            state, pid = json.loads(marker.read_text())
            self.assertFalse(Path(state).exists(), 'temporary app data leaked')
            with self.assertRaises(ProcessLookupError):
                os.kill(pid, 0)
            return result

    def test_failed_route_is_not_reported_as_success(self):
        result = self.run_fixture()
        self.assertEqual(result['exit_code'], 7)

    def test_early_simulator_exit_cleans_temporary_store(self):
        result = self.run_fixture(startup_failure=True)
        self.assertIn('simulator exited with 9', result['error'])

    def passing_fixture(self):
        """A fake whose route passes and writes journey state into TMPDIR."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            app = root / 'apps/fixture'
            app.mkdir(parents=True)
            (app / 'drive.kobo').write_text('expect Home\n')
            out = root / 'output'
            out.mkdir()
            fake = root / 'kobo'
            fake.write_text(f"""#!{sys.executable}
import os, sys, time
from pathlib import Path
if sys.argv[1] == 'dev':
    Path(os.environ['TMPDIR'], 'launch-state').write_text('launch')
    print('Kobo app simulator: http://127.0.0.1:12345', flush=True)
    time.sleep(60)
elif '--script' in sys.argv:
    Path(os.environ['TMPDIR'], 'journey-state').write_text('journey')
    Path(os.environ['TMPDIR'], 'launch-state').write_text('changed')
else:
    print('text ["Home"]')
""")
            fake.chmod(0o755)
            with patch.object(runner, 'ROOT', root):
                result = runner.run_app('fixture', fake, out, dict(os.environ), 3)
            self.assertEqual(result['status'], 'pass', result)
            self.assertTrue(result['reopened'])
            self.assertEqual(sorted(result['state_written']),
                             ['journey-state', 'launch-state'])
            return result

    def test_passing_route_reports_journey_state_writes(self):
        self.passing_fixture()

    def test_flashcards_seed_stages_the_demo_bundle(self):
        import io
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fixture = root / 'scripts/fixtures/flashcards/collection.cobfc'
            fixture.parent.mkdir(parents=True)
            fixture.write_bytes(b'demo bundle bytes')
            state = root / 'state'
            state.mkdir()
            with patch.object(runner, 'ROOT', root):
                runner.seed('flashcards', state, root / 'kobo', dict(os.environ), io.StringIO())
            staged = state / 'cobalt-sim-data/flashcards/collection.cobfc'
            self.assertEqual(staged.read_bytes(), b'demo bundle bytes')

    def test_snapshot_state_hashes_regular_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'a').write_text('one')
            first = runner.snapshot_state(root)
            self.assertEqual(list(first), ['a'])
            (root / 'a').write_text('two')
            (root / 'b').write_text('new')
            second = runner.snapshot_state(root)
            changed = [path for path in set(first) | set(second)
                       if first.get(path) != second.get(path)]
            self.assertEqual(sorted(changed), ['a', 'b'])


if __name__ == '__main__':
    unittest.main()
