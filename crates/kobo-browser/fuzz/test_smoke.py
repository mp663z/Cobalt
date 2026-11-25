"""Failure-propagation tests; these do not execute or qualify the fuzzers."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

TARGETS = 'parse_html paginate_document decode_image cache_metadata resolve_url sniff_body'.split()


class SmokeContract(unittest.TestCase):
    def run_smoke(self, fail='', message=''):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            shutil.copy(Path(__file__).with_name('smoke.sh'), root / 'smoke.sh')
            cargo = root / 'cargo'
            cargo.write_text('''#!/bin/sh
printf '%s\\n' "$5" >> "$CALLS"
if [ "$5" = "$FAIL_TARGET" ]; then
    printf '%s\\n' "$FAIL_MESSAGE"
    exit 42
fi
printf '%s\\n' 'stat::number_of_executed_units: 10'
''')
            cargo.chmod(0o755)
            env = dict(os.environ, PATH=str(root)+os.pathsep+os.environ['PATH'],
                       CALLS=str(root/'calls'), FAIL_TARGET=fail, FAIL_MESSAGE=message)
            result = subprocess.run(['sh', str(root/'smoke.sh'), '1'], env=env,
                                    capture_output=True, text=True)
            calls = (root/'calls').read_text().splitlines()
            return result, calls

    def test_all_success(self):
        result, calls = self.run_smoke()
        self.assertEqual(result.returncode, 0, result.stdout+result.stderr)
        self.assertEqual(calls, TARGETS)

    def test_build_error_cannot_be_filtered_to_success(self):
        result, calls = self.run_smoke('parse_html', 'linker: missing runtime')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('linker: missing runtime', result.stdout+result.stderr)
        self.assertEqual(calls, TARGETS)

    def test_last_fuzz_crash_is_nonzero(self):
        result, calls = self.run_smoke('sniff_body', 'SUMMARY: panic in sniff body')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('SUMMARY: panic in sniff body', result.stdout+result.stderr)
        self.assertEqual(calls, TARGETS)

    def test_middle_failure_does_not_hide_later_targets(self):
        result, calls = self.run_smoke('decode_image', 'fatal: image failure')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('fatal: image failure', result.stdout+result.stderr)
        self.assertEqual(calls, TARGETS)


if __name__ == '__main__':
    unittest.main()
