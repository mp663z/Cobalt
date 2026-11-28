#!/usr/bin/env python3
"""Real TLS fixture journey for delayed painted-page stylesheet supply.

Uses debug-only loopback routing and fresh simulator state. No public network.
The generic catalog wrapper is intentionally not used: unrelated app protocol
minimums do not decide whether this focused browser journey can run.
"""
import argparse
import http.server
import importlib.util
import json
import os
from pathlib import Path
import ssl
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[2]
HOST = 'styles.example.test'


def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--sheets', type=int, default=5)
    args = parser.parse_args()
    assert 2 <= args.sheets <= 16
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    target = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'target')).resolve()
    subprocess.run(['cargo', 'build', '--locked', '-p', 'kobo-cli'], cwd=ROOT, check=True)
    runner = module(ROOT / 'scripts/check-apps-sim.py', 'sim_runner')
    cert = module(ROOT / 'scripts/quality/check-audiobook-sim.py', 'tls_fixture')
    requests = []
    page = ('<!doctype html><title>Delayed styles</title>'
            + ''.join(f'<link rel=stylesheet href=/sheet{i}.css>' for i in range(args.sheets))
            + '<link rel=stylesheet media="(prefers-color-scheme: dark)" href=/dark.css>'
              '<h1>External stylesheet journey</h1><p>Readable before delayed styles.</p>'
              '<a href=/next>Next page</a>').encode()
    peak_rss_kib = 0
    monitoring = True

    def monitor():
        nonlocal peak_rss_kib
        while monitoring:
            for entry in Path('/proc').iterdir():
                if not entry.name.isdigit():
                    continue
                try:
                    if (entry / 'exe').resolve().name == 'kobo-browser':
                        for line in (entry / 'status').read_text().splitlines():
                            if line.startswith('VmRSS:'):
                                peak_rss_kib = max(peak_rss_kib, int(line.split()[1]))
                except (OSError, ValueError):
                    pass
            time.sleep(.02)

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            requests.append({'path': self.path, 'authorization': self.headers.get('Authorization'),
                             'accept': self.headers.get('Accept')})
            body = page if self.path == '/' else None
            if self.path.startswith('/sheet') and self.path.endswith('.css'):
                i = int(self.path[6:-4])
                body = (b'h1{color:red;font-size:36px}' if i == 0 else
                        b'p{color:blue;margin-top:70px}' if i == 1 else
                        f'h1{{margin-bottom:{i}px}}'.encode())
            if self.path.endswith('.css'):
                time.sleep(2)
            if body is None:
                self.send_error(404)
                return
            self.send_response(200)
            self.send_header('Content-Type', 'text/css' if self.path.endswith('.css') else 'text/html')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    with tempfile.TemporaryDirectory(prefix='cb-style-', dir='/tmp') as temporary:
        private = Path(temporary)
        trust = cert.certificate(private, [HOST])
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain(private / f'{HOST}.pem', private / f'{HOST}.key')
        server.socket = tls.wrap_socket(server.socket, server_side=True)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        seed = private / 'seed/cobalt-sim-state/browser'
        seed.mkdir(parents=True)
        (seed / 'browser-home').write_text(f'https://{HOST}/')
        route = private / 'styles.kobo'
        route.write_text('wait-for Search the web\ntap-id samples\ntap-id browser-more\n'
                         'tap-id go-home\nwait-for styles.example.test\n'
                         'shot-colour before-sheet\nwait-idle 30000\n'
                         'shot-colour after-sheet\ntap-id edit-address\n'
                         'tap-id kb.end\ntap-id kb.r0c0\nshot-colour address-edit\n'
                         'tap-id kb.cancel\n')
        env = dict(os.environ, CARGO_TARGET_DIR=str(target),
                   KOBO_SIM_PROFILE='libra-colour-390',
                   KOBO_SIM_HTTP_FIXTURE=f'{HOST}=127.0.0.1:{server.server_port}',
                   KOBO_SIM_TRUST_DIR=str(trust))
        env.pop('KOBO_SIM_OFFLINE', None)
        watcher = threading.Thread(target=monitor, daemon=True)
        watcher.start()
        started = time.monotonic()
        try:
            result = runner.run_app('browser', target / 'debug/kobo', out, env, 90,
                                    route_override=route, seed_root=private / 'seed')
        finally:
            monitoring = False
            watcher.join(timeout=5)
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)
        before = json.loads((out / 'browser-shots/before-sheet.json').read_text())
        after = json.loads((out / 'browser-shots/after-sheet.json').read_text())
        assert before['frame']['sha256'] != after['frame']['sha256'], 'delayed CSS did not change the frame'
        result['peak_app_rss_kib'] = peak_rss_kib
        result['whole_journey_seconds'] = round(time.monotonic() - started, 3)
        result['sheet_count'] = args.sheets
        result['requests'] = requests
        result['transport'] = 'real TLS/HTTP, debug-only loopback fixture'
        result['dirty'] = bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True).strip())
        result['source_sha'] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        assert result['status'] == 'pass', result
        assert sorted(r['path'] for r in requests) == sorted(['/'] + [f'/sheet{i}.css' for i in range(args.sheets)]), requests
        assert all(r['authorization'] is None for r in requests), requests
        print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
