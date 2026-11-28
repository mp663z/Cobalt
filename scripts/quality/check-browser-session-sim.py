#!/usr/bin/env python3
"""Fresh-state real-TLS mixed-mode browser session probe, host RSS only."""
import argparse
import http.server
import importlib.util
import json
import os
import re
import urllib.request
from pathlib import Path
import ssl
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[2]
HOST = 'session.example.test'


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def rss():
    found = []
    for process in Path('/proc').iterdir():
        if not process.name.isdigit():
            continue
        try:
            if (process / 'exe').resolve().name != 'kobo-browser':
                continue
            values = dict(line.split(':', 1) for line in (process / 'status').read_text().splitlines() if ':' in line)
            found.append({'pid': int(process.name), 'rss_kib': int(values['VmRSS'].split()[0]),
                          'hwm_kib': int(values['VmHWM'].split()[0])})
        except (OSError, ValueError, KeyError):
            pass
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--pages', type=int, default=12)
    parser.add_argument('--profile', default='clara-bw-391')
    parser.add_argument('--painted-only', action='store_true')
    args = parser.parse_args()
    assert 12 <= args.pages <= 30
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    target = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'target')).resolve()
    subprocess.run(['cargo', 'build', '--locked', '-p', 'kobo-cli'], cwd=ROOT, check=True)
    runner = load(ROOT / 'scripts/check-apps-sim.py', 'session_runner')
    cert = load(ROOT / 'scripts/quality/check-audiobook-sim.py', 'session_cert')
    requests = []
    checkpoints = []
    stop = threading.Event()
    peak = 0

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            requests.append({'path': self.path, 'rss': rss()})
            if not self.path.startswith('/page/'):
                self.send_error(404)
                return
            try:
                number = int(self.path.rsplit('/', 1)[1])
            except ValueError:
                self.send_error(404)
                return
            html = (f'<!doctype html><title>Session page {number}</title>'
                    '<style>h1{color:blue}p{margin-bottom:12px}</style>'
                    f'<h1>Session page {number}</h1>'
                    f'<a href=/page/{number+1}>Next session page</a>'
                    + ''.join(f'<p>Page {number} paragraph {i}. A reader keeps moving through the document and returns to previous pages.</p>' for i in range(35)))
            body = html.encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/html; charset=utf-8')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    def monitor():
        nonlocal peak
        seen = set()
        while not stop.wait(.05):
            values = rss()
            peak = max([peak] + [v['rss_kib'] for v in values])
            for shot in sorted((out / 'browser-shots').glob('*.png')):
                if shot.name not in seen:
                    seen.add(shot.name)
                    point = {'shot': shot.name, 'rss': values, 'elapsed_seconds': round(time.monotonic()-started, 3)}
                    if args.painted_only:
                        log = out / 'browser.log'
                        match = re.search(r'Kobo app simulator: http://(127\.0\.0\.1:\d+)', log.read_text())
                        if match:
                            try:
                                with urllib.request.urlopen('http://' + match.group(1) + '/layout', timeout=3) as response:
                                    point['layout'] = json.load(response)
                            except (OSError, ValueError) as error:
                                point['layout_error'] = str(error)
                    checkpoints.append(point)

    with tempfile.TemporaryDirectory(prefix='cb-session-', dir='/tmp') as temporary:
        private = Path(temporary)
        trust = cert.certificate(private, [HOST])
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain(private / f'{HOST}.pem', private / f'{HOST}.key')
        server.socket = tls.wrap_socket(server.socket, server_side=True)
        serving = threading.Thread(target=server.serve_forever, daemon=True)
        serving.start()
        seed = private / 'seed/cobalt-sim-state/browser'
        seed.mkdir(parents=True)
        (seed / 'browser-home').write_text(f'https://{HOST}/page/0')
        route = 'wait-for Search the web\nshot-colour first-run\ntap-id samples\ntap-id browser-more\ntap-id go-home\nwait-idle 30000\n'
        for number in range(args.pages):
            route += ('wait 500\nwait-idle 30000\ntap-id browser-more\ntap-id navigate\ntap-id page-view\nwait 300\nwait-idle 30000\n'
                      f'shot-colour session-{number:02}\nwait 500\n')
            if number + 1 < args.pages:
                route += 'tap-id browser-more\ntap-id navigate\ntap-id links\ntap-id link-0\nwait-idle 30000\n'
        route += ('tap-id back\nwait-idle 30000\nshot-colour session-back\n'
                  'tap-id forward\nwait-idle 30000\nshot-colour session-forward\n'
                  'tap-id browser-more\ntap-id navigate\ntap-id recent\nshot-colour session-history\n'
                  'tap-paged Session page 0\nwait-idle 30000\nshot-colour session-final\nwait 500\n')
        script = private / 'session.kobo'
        script.write_text(route)
        env = dict(os.environ, CARGO_TARGET_DIR=str(target), KOBO_SIM_PROFILE=args.profile,
                   KOBO_SIM_HTTP_FIXTURE=f'{HOST}=127.0.0.1:{server.server_port}', KOBO_SIM_TRUST_DIR=str(trust))
        env.pop('KOBO_SIM_OFFLINE', None)
        started = time.monotonic()
        watcher = threading.Thread(target=monitor, daemon=True)
        watcher.start()
        try:
            result = runner.run_app('browser', target / 'debug/kobo', out, env, 180, route_override=script, seed_root=private / 'seed')
        finally:
            stop.set()
            watcher.join(timeout=5)
            server.shutdown()
            server.server_close()
            serving.join(timeout=5)
    result.update(app_status=result['status'], status='unverified', profile=args.profile,
                  pages=args.pages, render_scope='mixed-mode; screenshots are not a painted-mode assertion', requests=requests, checkpoints=checkpoints, peak_rss_kib=peak,
                  source_sha=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                  dirty=bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True).strip()))
    destination = out / 'result.json'
    destination.write_text(json.dumps(result, indent=2)+'\n')
    assert result['app_status'] == 'pass', result
    expected = [f'/page/{i}' for i in range(args.pages)] + [f'/page/{args.pages-2}', f'/page/{args.pages-1}', '/page/0']
    assert [r['path'] for r in requests] == expected, requests
    assert any(c['shot'] == 'session-final.png' and len(c['rss']) == 1 for c in checkpoints), checkpoints
    assert all(len(c['rss']) == 1 for c in checkpoints), 'RSS not attributable to a single app'
    if args.painted_only:
        for number in range(args.pages):
            point = next(c for c in checkpoints if c['shot'] == f'session-{number:02}.png')
            layout = point.get('layout', {})
            assert any(str(node.get('kind', '')) == 'Picture(PictureHandle(2147418113))' for node in layout.get('nodes', [])), point
            metadata = json.loads((out / 'browser-shots' / f'session-{number:02}.json').read_text())
            assert int(metadata['paints']) == int(layout['paints']), 'layout changed after screenshot'
            assert any(f'https://{HOST}/page/{number}' in node.get('lines', []) for node in layout['nodes']), 'wrong page'
            assert not any(str(node.get('kind', '')).startswith(('Text', 'RichText', 'Heading')) for node in layout['nodes']), 'native prose under painted assertion'
        result['render_scope'] = 'painted-only for all sequential page checkpoints; history navigation is separate'
    result['status'] = 'pass'
    destination.write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
