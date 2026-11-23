#!/usr/bin/env python3
"""Render every frozen page and composite images. Does not update baselines."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=ROOT / 'target' / 'corpus-render')
    parser.add_argument('--binary', type=Path, help='use an already-built basic-page binary')
    args = parser.parse_args()
    binary = args.binary or ROOT / 'target' / 'debug' / 'basic-page'
    if not args.binary:
        subprocess.run(['cargo', '+1.85.1', 'build', '--locked', '--manifest-path', str(ROOT / 'Cargo.toml'), '--bin', 'basic-page'], check=True)
    args.out.mkdir(parents=True, exist_ok=True)
    for item in json.loads((ROOT / 'corpus' / 'manifest.json').read_text())['pages']:
        directory = ROOT / 'corpus' / item['name']
        prefix = args.out / item['name']
        subprocess.run([str(binary), '--page', str(directory / 'page.html'), '--stylesheet-dir', str(directory), '--output', str(prefix)], check=True)
        actual = prefix.with_suffix('.census').read_text()
        if actual != (directory / 'expected.census').read_text():
            raise RuntimeError(f'{item["name"]}: census changed; inspect output and review the diff')
        subprocess.run([sys.executable, str(ROOT / 'blit_images.py'), '--rgba', str(prefix.with_suffix('.rgba')), '--images', str(prefix.with_suffix('.imguris')), '--output', str(prefix.with_suffix('.png'))], check=True)
    print('All 10 pages match their census baselines. PNG files:', args.out)

if __name__ == '__main__':
    main()
