#!/usr/bin/env python3
"""Run the Store quality journey across the simulator profile x text-scale matrix.

Every cell writes its evidence to a unique directory, per the simulator-matrix
contract's hermeticity rule. The profile list is drift-checked against
kobo-profile's SUPPORTED_PROFILES so newly admitted hardware cannot slip past
the matrix silently.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]

# Must match SUPPORTED_PROFILES in crates/kobo-profile/src/lib.rs; the drift
# check below fails the run rather than silently testing a stale subset.
PROFILES = [
    'clara-bw-391',
    'clara-bw-395',
    'clara-hd-376',
    'clara-colour-393',
    'elipsa-2e-389',
    'libra-2-388',
    'libra-colour-390',
    'libra-colour-390-4.46.23836',
    'libra-h2o-384',
]

# Smoke scales run on every commit; --full runs all nine before merge.
SMOKE_SCALES = ['default', 'large', 'extra-large']
FULL_SCALES = ['80', '90', 'default', '110', 'large', '130', 'extra-large', '155', '170']


def supported_profile_ids():
    source = (ROOT / 'crates/kobo-profile/src/lib.rs').read_text()
    block = re.search(r'SUPPORTED_PROFILES[^=]*= &\[(.*?)\];', source, re.DOTALL)
    assert block, 'SUPPORTED_PROFILES not found'
    consts = re.findall(r'&([A-Z0-9_]+)', block.group(1))
    ids = []
    for const in consts:
        match = re.search(r'pub const ' + const + r': DeviceProfile = DeviceProfile \{\s*id: "([^"]+)"', source)
        assert match, f'{const} has no id'
        ids.append(match.group(1))
    return ids


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--full', action='store_true', help='all nine text scales instead of the smoke set')
    parser.add_argument('--profile', action='append', help='restrict to these profile IDs (repeatable)')
    args = parser.parse_args()

    declared = supported_profile_ids()
    if declared != PROFILES:
        sys.exit(f'profile list drifted from kobo-profile: code has {declared}, matrix knows {PROFILES}')

    if args.output.is_symlink() or (args.output.exists() and any(p.is_symlink() for p in args.output.rglob('*'))):
        sys.exit('output directory must not contain symlinks')
    run_dir = args.output / time.strftime('run-%Y%m%d-%H%M%S')
    run_dir.mkdir(parents=True, mode=0o700)

    profiles = args.profile or PROFILES
    scales = FULL_SCALES if args.full else SMOKE_SCALES
    cells = []
    failed = 0
    for profile in profiles:
        for scale in scales:
            cell_dir = run_dir / profile / scale
            result = subprocess.run(
                [sys.executable, str(ROOT / 'scripts/quality/check-store-sim.py'),
                 '--output', str(cell_dir), '--profile', profile, '--scale', scale],
                cwd=ROOT, capture_output=True, text=True, timeout=600)
            status = 'failed'
            detail = ''
            result_file = cell_dir / 'result.json'
            if result.returncode == 0 and result_file.is_file():
                cell = json.loads(result_file.read_text())
                status = cell['status']
            else:
                log = cell_dir / 'simulator.log'
                if log.is_file():
                    detail = log.read_text().strip().splitlines()[-1][:300]
                else:
                    detail = (result.stderr or 'no output').strip().splitlines()[-1][:300]
            if status != 'passed':
                failed += 1
            cells.append({'profile': profile, 'scale': scale, 'status': status, 'detail': detail})
            print(f'{profile} {scale}: {status}' + (f' - {detail}' if detail else ''), flush=True)

    summary = {
        'status': 'passed' if failed == 0 else 'failed',
        'basis': 'store quality journey per profile and text scale',
        'profiles': profiles,
        'scales': scales,
        'cells': cells,
        'source_head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
    }
    (run_dir / 'result.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(f'matrix: {len(cells)} cells, {failed} failed -> {run_dir}')
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
