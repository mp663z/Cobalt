#!/usr/bin/env python3
"""Record the resource-budget baseline metrics for one run.

The contract (docs/quality/contracts/resource-budgets.md) measures cold
start, build time and the test sweep from fixed fixtures and stores a
history; budgets are tightened only after that history exists. Metrics
that need instrumentation the runtime does not have yet (peak RSS,
layout time, search latency, refresh counts, cache growth, background
jobs) are recorded as not yet measured rather than guessed at.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]

# Pinned by CI and by the contract baseline.
RUST = '1.85.1'

PROBES = ['sim-launch', 'cli-build', 'test-sweep']

# kobo-cli tests that need an ARM C cross-compiler this host does not
# have; the contract baseline records the same gap as environmental.
# They are skipped by name and reported as skipped, never as passed.
ENVIRONMENTAL_SKIPS = {
    'every_packaged_binary_is_built_with_what_it_needs': 'needs an ARM C cross-compiler',
    'every_uploaded_artifact_is_built_from_this_workspace': 'needs an ARM C cross-compiler',
}

# Contract metrics this lane cannot measure honestly yet: the runtime
# exposes no counters for them, and a number invented here would be
# worse than none.
NOT_YET_MEASURED = [
    'peak RSS and steady-state memory (no runtime counter)',
    'screen construction and layout time (no instrumentation)',
    'local-search latency at small/medium/large libraries (no harness)',
    'full/partial refresh count per journey (no instrumentation)',
    'bytes downloaded and cache growth (needs a network fixture)',
    'background-job duration and wake frequency (needs a device runtime)',
]


def environment():
    facts = {
        'rust': subprocess.check_output(
            ['cargo', f'+{RUST}', '--version'], cwd=ROOT, text=True).strip(),
        'cpus': os.cpu_count(),
        'source_head': subprocess.check_output(
            ['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
    }
    return facts


def cargo(*arguments, timeout):
    return subprocess.run(
        ['cargo', f'+{RUST}', *arguments],
        cwd=ROOT, capture_output=True, text=True, timeout=timeout)


def probe_sim_launch(run_dir, app):
    """Warm-binary wall time for `kobo run --sim --app <app>` to a
    verified rendered frame; the command itself fails when the frame is
    short."""
    # kobo-cli drives the run, so it is built here too: a stale kobo
    # binary measuring against a fresh kobod is the failure this avoids.
    build = cargo('build', '-p', 'kobod', '-p', 'kobo-cli', '-p', f'kobo-{app}', timeout=1800)
    if build.returncode != 0:
        return {'status': 'failed', 'detail': build.stderr.strip().splitlines()[-1][:300]}
    start = time.monotonic()
    run = subprocess.run(
        [str(ROOT / 'target/debug/kobo'), 'run', '--sim', '--app', app],
        cwd=ROOT, capture_output=True, text=True, timeout=600)
    seconds = round(time.monotonic() - start, 2)
    if run.returncode != 0:
        detail = (run.stderr or run.stdout).strip().splitlines()[-1][:300]
        return {'status': 'failed', 'detail': detail}
    return {'status': 'passed', 'app': app, 'wall_seconds': seconds}


def probe_cli_build(run_dir):
    """`cargo clean -p kobo-cli` then a timed rebuild, dependencies warm:
    the change a full day of editing actually pays."""
    clean = cargo('clean', '-p', 'kobo-cli', timeout=300)
    if clean.returncode != 0:
        return {'status': 'failed', 'detail': clean.stderr.strip().splitlines()[-1][:300]}
    start = time.monotonic()
    build = cargo('build', '-p', 'kobo-cli', timeout=3600)
    seconds = round(time.monotonic() - start, 2)
    if build.returncode != 0:
        return {'status': 'failed', 'detail': build.stderr.strip().splitlines()[-1][:300]}
    return {'status': 'passed', 'wall_seconds': seconds}


def probe_test_sweep(run_dir):
    """Timed workspace unit-test sweep with parsed pass/fail counts."""
    start = time.monotonic()
    skips = [flag for name in ENVIRONMENTAL_SKIPS for flag in ('--skip', name)]
    sweep = cargo('test', '--workspace', '--', *skips, timeout=5400)
    seconds = round(time.monotonic() - start, 2)
    log = run_dir / 'test-sweep.log'
    log.write_text(sweep.stdout + sweep.stderr)
    # Parse "test result: ok. N passed; M failed; ..." lines.
    passed = failed = 0
    for match in re.finditer(r'test result: \w+\. (\d+) passed; (\d+) failed;', sweep.stdout):
        passed += int(match.group(1))
        failed += int(match.group(2))
    status = 'passed' if sweep.returncode == 0 and failed == 0 else 'failed'
    return {
        'status': status,
        'wall_seconds': seconds,
        'tests_passed': passed,
        'tests_failed': failed,
        'skipped_environmental': dict(ENVIRONMENTAL_SKIPS),
        'log': str(log),
    }


def append_history(output, record):
    history = output / 'history.jsonl'
    with history.open('a') as stream:
        stream.write(json.dumps(record) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True,
                        help='evidence root; each run gets a unique directory')
    parser.add_argument('--app', default='settings',
                        help='app for the simulator launch probe (default: settings)')
    parser.add_argument('--skip', action='append', choices=PROBES, default=[],
                        help='probe to skip (repeatable)')
    args = parser.parse_args()

    if args.output.is_symlink() or (args.output.exists() and any(p.is_symlink() for p in args.output.rglob('*'))):
        sys.exit('output directory must not contain symlinks')
    run_dir = args.output / time.strftime('run-%Y%m%d-%H%M%S')
    run_dir.mkdir(parents=True, mode=0o700)

    probes = {}
    failed = 0
    for name in PROBES:
        if name in args.skip:
            probes[name] = {'status': 'skipped'}
            continue
        probe = {'sim-launch': probe_sim_launch, 'cli-build': probe_cli_build,
                 'test-sweep': probe_test_sweep}[name]
        result = probe(run_dir) if name != 'sim-launch' else probe(run_dir, args.app)
        probes[name] = result
        if result['status'] != 'passed':
            failed += 1
        print(f'{name}: {result["status"]}'
              + (f' - {result.get("detail", "")}' if result.get('detail') else ''), flush=True)

    summary = {
        'status': 'passed' if failed == 0 else 'failed',
        'basis': 'resource-budgets contract baseline probes',
        'environment': environment(),
        'probes': probes,
        'not_yet_measured': NOT_YET_MEASURED,
    }
    (run_dir / 'result.json').write_text(json.dumps(summary, indent=2) + '\n')
    append_history(args.output, {
        'run': run_dir.name,
        'head': summary['environment']['source_head'],
        'status': summary['status'],
        'metrics': {name: result.get('wall_seconds') for name, result in probes.items()},
    })
    print(f'perf lane: {failed} failed -> {run_dir}')
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
