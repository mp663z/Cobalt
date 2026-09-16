import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('perf_lane', Path(__file__).with_name('perf-lane.py'))
LANE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LANE)


class Completed:
    def __init__(self, returncode=0, stdout='', stderr=''):
        self.returncode = returncode
        self.stdout = stdout
        self.stderr = stderr


class PerfLane(unittest.TestCase):
    def test_history_appends_one_line_per_run(self):
        with tempfile.TemporaryDirectory() as private:
            output = Path(private)
            LANE.append_history(output, {'run': 'run-1', 'status': 'passed', 'metrics': {}})
            LANE.append_history(output, {'run': 'run-2', 'status': 'failed', 'metrics': {}})
            lines = (output / 'history.jsonl').read_text().splitlines()
            self.assertEqual(len(lines), 2)
            self.assertEqual(json.loads(lines[1])['run'], 'run-2')

    def test_symlinked_output_is_refused(self):
        with tempfile.TemporaryDirectory() as private:
            target = Path(private) / 'elsewhere'
            target.mkdir()
            link = Path(private) / 'link'
            link.symlink_to(target)
            with patch.object(LANE.sys, 'argv', ['perf-lane.py', '--output', str(link)]):
                with self.assertRaises(SystemExit) as raised:
                    LANE.main()
            self.assertIn('symlink', str(raised.exception))

    def test_skipped_probes_do_not_run_and_unmeasured_metrics_are_recorded(self):
        with tempfile.TemporaryDirectory() as private:
            output = Path(private)
            calls = []
            def fake_run(argv, **kwargs):
                calls.append(argv)
                return Completed(0, stdout='host runtime completed for kobo-settings; frame: ok')
            argv = ['perf-lane.py', '--output', str(output),
                    '--skip', 'cli-build', '--skip', 'test-sweep']
            with patch.object(LANE.sys, 'argv', argv), \
                    patch.object(LANE.subprocess, 'run', fake_run), \
                    patch.object(LANE, 'environment', return_value={'rust': 'test', 'cpus': 2, 'source_head': 'x'}):
                with self.assertRaises(SystemExit) as raised:
                    LANE.main()
            self.assertEqual(raised.exception.code, 0)
            run_dir = next(output.glob('run-*'))
            result = json.loads((run_dir / 'result.json').read_text())
            self.assertEqual(result['probes']['sim-launch']['status'], 'passed')
            self.assertEqual(result['probes']['cli-build']['status'], 'skipped')
            self.assertEqual(result['probes']['test-sweep']['status'], 'skipped')
            self.assertTrue(result['not_yet_measured'])
            history = json.loads((output / 'history.jsonl').read_text().splitlines()[0])
            self.assertEqual(history['status'], 'passed')

    def test_a_failed_probe_fails_the_run(self):
        with tempfile.TemporaryDirectory() as private:
            output = Path(private)
            def fake_run(argv, **kwargs):
                return Completed(1, stdout='', stderr='simulation failed: app=exit')
            argv = ['perf-lane.py', '--output', str(output),
                    '--skip', 'cli-build', '--skip', 'test-sweep']
            with patch.object(LANE.sys, 'argv', argv), \
                    patch.object(LANE.subprocess, 'run', fake_run), \
                    patch.object(LANE, 'environment', return_value={'rust': 'test', 'cpus': 2, 'source_head': 'x'}):
                with self.assertRaises(SystemExit) as raised:
                    LANE.main()
            self.assertEqual(raised.exception.code, 1)
            run_dir = next(output.glob('run-*'))
            result = json.loads((run_dir / 'result.json').read_text())
            self.assertEqual(result['probes']['sim-launch']['status'], 'failed')

    def test_sweep_parser_counts_every_suite(self):
        with tempfile.TemporaryDirectory() as private:
            run_dir = Path(private)
            sweep_output = ('test result: ok. 12 passed; 0 failed; 0 ignored\n'
                            'test result: ok. 3 passed; 1 failed; 0 ignored\n')
            def fake_cargo(*arguments, timeout):
                return Completed(0, stdout=sweep_output)
            with patch.object(LANE, 'cargo', fake_cargo):
                result = LANE.probe_test_sweep(run_dir)
            self.assertEqual(result['tests_passed'], 15)
            self.assertEqual(result['tests_failed'], 1)
            self.assertEqual(result['status'], 'failed')
            self.assertTrue((run_dir / 'test-sweep.log').is_file())


if __name__ == '__main__':
    unittest.main()
