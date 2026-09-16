#!/usr/bin/env python3
"""Render docs/quality/appqa-07-matrix.md from a check-apps-sim sweep report.

Usage: python3 scripts/quality/render_appqa07_matrix.py /path/to/results.json

The sweep measures three legs per app: the committed route (task), the
offline reopen, and state_written - the paths the driven journey created,
changed or removed past first-launch state (own data). The two legs a
simulator sweep cannot measure are spelled out rather than guessed: the
install leg belongs to the publish pipeline and the attended device proof,
and the sample leg is evidenced only where a fixture or demo path exists.
"""
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent.parent.parent

# Apps whose manifests declare no user data: the own-data leg is vacuous.
NO_DATA_DECLARED = {'chat', 'gallery', 'morse'}
# Apps whose sweep seed pushes the reader's own content through the real CLI
# path; the route then shows that content.
PUSHED_OWN = {'deck', 'frame', 'vault'}
# Sample or fixture content exercised by the sweep, and how.
SAMPLE_EVIDENCE = {
    'deck': 'seeded config pushed through the real `kobo deck` path',
    'frame': 'seeded photo pushed through the real `kobo frame push` path',
    'vault': 'fixture notes pushed through the real `kobo vault push` path',
    'fanshelf': 'FANSHELF_DEMO synthetic library exercised by the route',
    'flashcards': 'original demo bundle staged like a host import, reviewed by the route',
    'parser': 'synthetic demo story staged like a device push, played by the route',
}
# Journey writes that are fetched network caches, not the reader's own data.
CACHE_ONLY = {'gutenbird'}
# Gaps the sweep cannot close, with the reason measured against the source.
GAP_REASONS = {
    'arxiv': 'keeping a paper needs a fetched paper; no offline write path',
    'audiobook': 'the library records only finished books; composing needs provider secrets (exa/elevenlabs), no draft persists on failure',
    'brief': 'keeping an article needs a fetched digest; no offline write path',
    'calibre-web': 'settings save only after a verified server fetch',
    'hn': 'read marks need a fetched story list; no offline write path',
    'homepanel': 'requires an https Home Assistant URL; the simulator has no TLS fixture',
    'lichess': 'live play needs an account token; the offline computer game does not persist',
    'panels': 'panel content comes from the paired host; no offline write path',
    'paperterm': 'pairing needs a reachable terminal host; no offline write path',
    'rss': 'every save path (subscribe, remove, retry) needs a fetched feed first',
    'rss-miniflux': 'server-first client; no offline write path',
    'sidekick': 'pairing needs the sidekick handshake; no offline write path',
}


def classify_own_data(app, state_written):
    if app in NO_DATA_DECLARED:
        return 'n/a (manifest declares no user data)', False
    if app in PUSHED_OWN:
        return 'yes - own content pushed via the real CLI path, shown by the route', False
    if state_written and app not in CACHE_ONLY:
        names = ', '.join(Path(p).name for p in state_written)
        return f'yes - journey wrote {names}', False
    if app in CACHE_ONLY and state_written:
        return 'gap - only the fetched catalog cache was written', True
    reason = GAP_REASONS.get(app)
    if reason:
        return f'gap - {reason}', True
    return 'gap', True


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    report = json.loads(Path(sys.argv[1]).read_text())
    results = report['results']
    sha = report.get('source_sha', 'unknown')[:7]
    apps = sorted(result['app'] for result in results)
    by_app = {result['app']: result for result in results}
    passed = sum(1 for result in results if result['status'] == 'pass')
    reopened = sum(1 for result in results if result.get('reopened'))

    rows = []
    own_gaps = []
    for app in apps:
        result = by_app[app]
        own, is_gap = classify_own_data(app, result.get('state_written') or [])
        if is_gap:
            own_gaps.append(app)
        sample = SAMPLE_EVIDENCE.get(app, 'none evidenced')
        route = result.get('route')
        task = f'yes - {route}' if route else 'launch only (APPQA-13)'
        reopen = 'yes' if result.get('reopened') else 'NO'
        rows.append((app, 'UNVERIFIED', sample, own, task, reopen))

    doc = []
    doc.append('# APPQA-07 leg matrix')
    doc.append('')
    doc.append('Per-app status of the five app-quality gate legs: install, sample,')
    doc.append('own-data, task and offline-reopen. Rendered by')
    doc.append('`scripts/quality/render_appqa07_matrix.py` from a')
    doc.append('`scripts/check-apps-sim.py` sweep; the sweep reports')
    doc.append('`state_written`, `reopened` and the route per app. This rendering')
    doc.append(f'is from the sweep at `{sha}` ({passed}/{len(apps)} passed, '
               f'{reopened}/{len(apps)} reopened offline).')
    doc.append('')
    doc.append('Leg meanings:')
    doc.append('')
    doc.append('- **install** - the real signed package installed and launched through')
    doc.append('  the Store path. UNVERIFIED in the sandbox for every app: per-app')
    doc.append('  real-binary install belongs to the publish pipeline and attended')
    doc.append('  device proof (`kobo beta-store-smoke`). The install machinery itself')
    doc.append('  - catalog, signature, update, downgrade, launch-failure and state')
    doc.append('  preservation - is covered by the kobo-cli and kobod test suites.')
    doc.append('- **sample** - the app exercised with sample or fixture content, where')
    doc.append('  the app has such a path.')
    doc.append('- **own-data** - the app shown working with data from the reader\u2019s own')
    doc.append('  action: a journey write measured by the sweep, or own content pushed')
    doc.append('  through the app\u2019s real CLI path. Apps whose manifest declares no user')
    doc.append('  data are n/a. A fetched network cache is not own data. Apps whose')
    doc.append('  data starts on a server (feed, catalog, library, pairing) keep the')
    doc.append('  gap until a local fixture exercises them; the first-run screen is')
    doc.append('  their honest floor, not evidence.')
    doc.append('- **task** - a committed simulator route exercises the core journey.')
    doc.append('- **offline-reopen** - after the route, the same state relaunches with')
    doc.append('  no network and renders a first screen.')
    doc.append('')
    doc.append('| App | install | sample | own-data | task | offline-reopen |')
    doc.append('| --- | --- | --- | --- | --- | --- |')
    for row in rows:
        doc.append('| ' + ' | '.join(row) + ' |')
    doc.append('')
    doc.append('## Open legs')
    doc.append('')
    doc.append('- install: all 44 apps, per the method above.')
    doc.append('- own-data gaps (manifest declares user data, no own-data evidence):')
    doc.append('  ' + ', '.join(own_gaps) + '.')
    no_sample = [a for a in apps if a not in SAMPLE_EVIDENCE]
    doc.append(f'- sample: no sample-path evidence for {len(no_sample)} apps;')
    doc.append('  most have no sample or demo path at all, which is a product gap the')
    doc.append('  per-app audits should judge, not only an evidence gap.')
    doc.append('')
    out = ROOT / 'docs/quality/appqa-07-matrix.md'
    out.write_text('\n'.join(doc) + '\n')
    print(f'{out}: {len(rows)} rows, {len(own_gaps)} own-data gaps, from sweep {sha}')


if __name__ == '__main__':
    main()
