"""Offline corpus integrity and final-image regression tests."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from PIL import Image

ROOT = Path(__file__).resolve().parent

class CorpusTests(unittest.TestCase):
    def test_every_source_matches_its_frozen_hash(self):
        manifest = json.loads((ROOT / 'corpus' / 'manifest.json').read_text())
        self.assertEqual(len(manifest['pages']), 13)
        for page in manifest['pages']:
            directory = ROOT / 'corpus' / page['name']
            sources = [(directory / 'page.html', page)]
            sources += [(directory / f'sheet-{i}.css', sheet) for i, sheet in enumerate(page['stylesheets'])]
            for path, source in sources:
                with self.subTest(path=path):
                    data = path.read_bytes()
                    self.assertEqual(len(data), source['bytes'])
                    self.assertEqual(hashlib.sha256(data).hexdigest(), source['sha256'])
                    if 'url' in source:
                        self.assertTrue(source['url'].startswith('https://'))
                    else:
                        self.assertEqual(data, (ROOT.parent.parent / source['source_path']).read_bytes())

    def test_transparent_raster_composites_on_white(self):
        with tempfile.TemporaryDirectory() as directory:
            prefix = Path(directory) / 'page'
            prefix.with_suffix('.rgba').write_bytes(bytes([0, 0, 0, 0, 255, 0, 0, 255]))
            prefix.with_suffix('.imguris').write_text('')
            subprocess.run([sys.executable, str(ROOT / 'blit_images.py'), '--rgba', str(prefix.with_suffix('.rgba')), '--images', str(prefix.with_suffix('.imguris')), '--output', str(prefix.with_suffix('.png')), '--width', '2'], check=True, capture_output=True)
            with Image.open(prefix.with_suffix('.png')) as image:
                self.assertEqual([image.getpixel((0, 0)), image.getpixel((1, 0))], [(255, 255, 255), (255, 0, 0)])

if __name__ == '__main__':
    unittest.main()
