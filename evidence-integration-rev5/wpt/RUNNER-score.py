#!/usr/bin/env python3
"""Pinned WPT no-script reftests against Chrome references and Cobalt.

Unsupported is not pass; Chrome rendering the test and reference validates the
selected test pair. A Cobalt image must exist before there is a Cobalt diff.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
from PIL import Image, ImageChops

# Full-width bands around reported glyph bounds hide text ink in BOTH images;
# the inflation absorbs line-metric drift between the two rasterizers.
MASK_INFLATE_PX = 8


def run(argv, log):
    with log.open('wb') as stream:
        try:
            return subprocess.run(argv, stdout=stream, stderr=subprocess.STDOUT,
                                  timeout=90, check=False).returncode
        except subprocess.TimeoutExpired:
            stream.write(b'\nTIMEOUT after 90 seconds\n')
            return 124


def image(path, width, height):
    with Image.open(path) as original:
        result = original.convert('RGB')
    if result.size != (width, height):
        raise ValueError(f'{path}: wrong size {result.size}')
    return result


def changed(first, second):
    diff = ImageChops.difference(first, second)
    return sum(pixel != (0, 0, 0) for pixel in diff.get_flattened_data())


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('out', type=Path)
    parser.add_argument('--chrome', default='google-chrome')
    parser.add_argument('--cargo', default='cargo')
    parser.add_argument('--font', default='/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf',
                        help='explicit local face for the masked-text secondary pass')
    parser.add_argument('--font-bold', default=None, help='optional explicit bold face; opts into masked-bold-v2-2026-11-01')
    args = parser.parse_args()
    if not all(path and Path(path).is_file() for path in (shutil.which(args.chrome), shutil.which(args.cargo))):
        parser.error('google-chrome and a working cargo must be on PATH, or supply --chrome/--cargo')
    args.out.mkdir(parents=True, exist_ok=True)
    preview_manifest = Path(__file__).resolve().parent.parent / 'cobalt-text-preview' / 'Cargo.toml'
    font_ok = Path(args.font).is_file()
    source = Path(__file__).parent / 'fixtures'
    manifest = json.loads((source / 'manifest.json').read_text())
    results = []
    for row in manifest['cases']:
        name = row['name']
        if not name.replace('-', '').replace('_', '').isalnum():
            raise ValueError('invalid case name')
        width, height = map(int, row['viewport'].split('x'))
        test = source / f'{name}.html'
        ref = source / f'{name}.ref.html'
        inputs = [(test, row['test_sha256']), (ref, row['reference_sha256'])]
        for asset in row.get('reference_assets', []):
            inputs.append((source / asset['local_path'], asset['sha256']))
        for path, expected_hash in inputs:
            if hashlib.sha256(path.read_bytes()).hexdigest() != expected_hash:
                raise ValueError(f'changed frozen input {path}')
        test_png = args.out / f'{name}.chrome-test.png'
        ref_png = args.out / f'{name}.chrome-reference.png'
        cobalt_ppm = args.out / f'{name}.cobalt.ppm'
        # A prior attempt must never masquerade as the current engine's output.
        for stale in (test_png, ref_png, cobalt_ppm, args.out / f'{name}.diff.png',
                      args.out / f'{name}.cobalt-masked.ppm', args.out / f'{name}.mask.json',
                      args.out / f'{name}.masked-diff.png'):
            stale.unlink(missing_ok=True)
        def chrome(file, output, log):
            return run([args.chrome, '--headless', '--disable-gpu', '--no-sandbox',
                        '--hide-scrollbars', '--disable-background-networking',
                        '--host-resolver-rules=MAP * ~NOTFOUND',
                        f'--window-size={width},{height}', f'--screenshot={output}', file.as_uri()], log)
        test_code = chrome(test, test_png, args.out / f'{name}.chrome-test.log')
        ref_code = chrome(ref, ref_png, args.out / f'{name}.chrome-reference.log')
        cobalt_code = run([args.cargo, 'run', '-q', '-p', 'kobo-web-document',
                           '--example', 'css_background_preview', '--', str(test),
                           str(cobalt_ppm), str(width), str(height)],
                          args.out / f'{name}.cobalt.log')
        result = dict(name=name, upstream_test=row['test_path'],
                      upstream_reference=row['reference_path'], viewport=row['viewport'],
                      chrome_test_status=test_code, chrome_reference_status=ref_code,
                      cobalt_status=cobalt_code, chrome_control_changed_pixels=None,
                      cobalt_changed_pixels=None, cobalt_diff_percent=None,
                      chrome_control_status='invalid', status='no paired render')
        if test_code == 0 and ref_code == 0 and test_png.exists() and ref_png.exists():
            a, b = image(test_png, width, height), image(ref_png, width, height)
            ctrl = changed(a, b)
            result['chrome_control_changed_pixels'] = ctrl
            if ctrl == 0:
                result['chrome_control_status'] = 'valid'
        if result['chrome_control_status'] == 'valid' and cobalt_code == 0 and cobalt_ppm.exists():
            a, b = image(cobalt_ppm, width, height), image(ref_png, width, height)
            n = changed(a, b)
            result['cobalt_changed_pixels'] = n
            result['cobalt_diff_percent'] = round(n * 100 / (width * height), 5)
            result['status'] = 'pass' if n == 0 else 'fail'
            ImageChops.difference(a, b).save(args.out / f'{name}.diff.png')
        elif cobalt_code != 0:
            result['cobalt_error'] = (args.out / f'{name}.cobalt.log').read_text(errors='replace').strip().splitlines()[-1:]
        # Masked-text secondary measurement. The restricted text preview paints
        # the proven page including text with one explicit local face and
        # reports glyph-ink bands; those bands are hidden in both images so the
        # comparison covers only non-text geometry. A masked pass is not a
        # headline pass: font rasterization is excluded, not matched. The
        # headline fields above never change because of this block.
        result['masked_status'] = 'not run'
        result['masked_changed_pixels'] = None
        result['masked_diff_percent'] = None
        if result['status'] == 'pass':
            result['masked_status'] = 'pass (unmasked)'
        elif result['chrome_control_status'] == 'valid' and font_ok:
            masked_ppm = args.out / f'{name}.cobalt-masked.ppm'
            mask_json = args.out / f'{name}.mask.json'
            masked_code = run([args.cargo, 'run', '-q', '--manifest-path', str(preview_manifest),
                               '--', str(test), str(masked_ppm), str(width), str(height),
                               args.font, '--blocks', '--mask-json', str(mask_json)] +
                              (['--font-bold', args.font_bold] if args.font_bold else []),
                              args.out / f'{name}.cobalt-masked.log')
            if masked_code == 0 and masked_ppm.exists() and mask_json.exists():
                bands = json.loads(mask_json.read_text())
                a, b = image(masked_ppm, width, height), image(ref_png, width, height)
                for band in bands:
                    top = max(0, int(band['y']) - MASK_INFLATE_PX)
                    bottom = min(height, int(band['y']) + int(band['height']) + MASK_INFLATE_PX)
                    if bottom > top:
                        for target in (a, b):
                            target.paste((255, 255, 255), (0, top, width, bottom))
                n = changed(a, b)
                result['masked_changed_pixels'] = n
                result['masked_diff_percent'] = round(n * 100 / (width * height), 5)
                result['masked_status'] = 'masked pass' if n == 0 else 'masked fail'
                ImageChops.difference(a, b).save(args.out / f'{name}.masked-diff.png')
            else:
                result['masked_status'] = 'unsupported'
        results.append(result)
    valid = sum(row['chrome_control_status'] == 'valid' for row in results)
    passed = sum(row['status'] == 'pass' for row in results)
    paired = sum(row['status'] in ('pass', 'fail') for row in results)
    masked_pass = sum(row['masked_status'] in ('pass (unmasked)', 'masked pass') for row in results)
    masked_measured = sum(row['masked_status'] in ('masked pass', 'masked fail') for row in results)
    unmasked = sum(row['masked_status'] == 'pass (unmasked)' for row in results)
    summary = dict(suite='WPT no-script CSS background reftest subset',
                   upstream_commit=manifest['upstream_commit'], selected=len(results),
                   valid_chrome_controls=valid, paired=paired, pass_count=passed,
                   unsupported_or_no_render=valid-paired,
                   pass_rate_percent=(round(100 * passed / valid, 3) if valid else None),
                   controls='test and reference Chrome images must match at selected viewport; '
                            'Cobalt must render test and match Chrome reference', cases=results)
    summary['masked_text_secondary'] = dict(
        label='masked-text secondary score - not the headline metric',
        method='restricted text preview with one explicit local face; reported glyph bands '
               f'hidden in both images (full width, {MASK_INFLATE_PX}px inflation); '
               'all remaining pixels compared exactly; font rasterization is excluded, not matched',
        font=args.font if font_ok else None,
        environment='masked-bold-v2-2026-11-01' if args.font_bold else 'masked-regular-v1',
        bold_font=args.font_bold,
        artifact='cobalt-masked.ppm is raw including text; strips applied only in memory',
        denominator=len(results),
        score=masked_pass,
        measured_masked=masked_measured,
        unmasked_pass=unmasked,
        unsupported=len(results) - masked_measured - unmasked)
    (args.out / 'scoreboard.json').write_text(json.dumps(summary, indent=2)+'\n')
    print(f"WPT CSS reftests: {passed}/{valid} passed; {paired} paired; "
          f"{valid-paired} unsupported; {len(results)-valid} invalid Chrome controls")
    if font_ok:
        print(f"Masked-text secondary: {masked_pass}/{len(results)} "
              f"({masked_measured} measured masked, {unmasked} already passing unmasked, "
              f"{len(results) - masked_measured - unmasked} unsupported)")
    else:
        print(f"Masked-text secondary: not run (font file missing: {args.font})")
    for row in results:
        print(row['name'], row['chrome_control_status'], row['status'],
              row['cobalt_diff_percent'], row['masked_status'], row['masked_diff_percent'])
    return 0 if valid == len(results) else 2


if __name__ == '__main__':
    sys.exit(main())
