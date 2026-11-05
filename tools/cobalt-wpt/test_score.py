"""Runner invariants: denominators and stale artifacts cannot fake a pass."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

import score


class ScoreTests(unittest.TestCase):
    def test_pinned_inputs_have_exact_hashes(self):
        root = Path(__file__).parent / 'fixtures'
        rows = json.loads((root / 'manifest.json').read_text())['cases']
        self.assertEqual(len(rows), 11)
        for row in rows:
            for suffix, key in [('.html', 'test_sha256'), ('.ref.html', 'reference_sha256')]:
                self.assertEqual(hashlib.sha256((root / (row['name'] + suffix)).read_bytes()).hexdigest(), row[key])
            for asset in row.get('reference_assets', []):
                self.assertEqual(hashlib.sha256((root / asset['local_path']).read_bytes()).hexdigest(), asset['sha256'])

    def test_no_stale_image_can_convert_a_failed_renderer_to_a_pass(self):
        with tempfile.TemporaryDirectory() as folder:
            out = Path(folder)
            # A previous Cobalt image exists, while this run's Cobalt command fails.
            (out / 'background-color-body-propagation-001.cobalt.ppm').write_bytes(b'old')
            def fake(command, log):
                if '--example' in command:
                    log.write_text('unsupported')
                    return 1
                from PIL import Image
                target = next(part.removeprefix('--screenshot=') for part in command if part.startswith('--screenshot='))
                Image.new('RGB', (800,600), 'white').save(target)
                log.write_text('chrome')
                return 0
            with patch('score.run', side_effect=fake), patch('score.shutil.which', return_value=__file__), patch.object(sys, 'argv', ['score.py', str(out)]):
                self.assertEqual(score.main(), 0)
            results = json.loads((out / 'scoreboard.json').read_text())
            self.assertEqual(results['pass_count'], 0)
            self.assertEqual(results['paired'], 0)
            self.assertEqual(results['unsupported_or_no_render'], 11)
            self.assertFalse((out / 'background-color-body-propagation-001.cobalt.ppm').exists())


if __name__ == '__main__':
    unittest.main()
