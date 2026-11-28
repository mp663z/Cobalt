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
import re
from pathlib import Path
import ssl
import subprocess
import tempfile
import threading
import time
from urllib.parse import parse_qs, urlsplit

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
    parser.add_argument('--typed-address', action='store_true')
    parser.add_argument('--form-edges', action='store_true', help='test textarea cancel/Back, readonly controls and number maxlength semantics')
    parser.add_argument('--textarea', action='store_true', help='test multiline edit, maxlength and GET CRLF encoding')
    parser.add_argument('--via-link', action='store_true', help='open target through the native Links list')
    parser.add_argument('--sphinx', action='store_true', help='serve the frozen Python/Sphinx page and test native search from Page view')
    args = parser.parse_args()
    assert 2 <= args.sheets <= 16
    assert sum((args.sphinx, args.textarea, args.form_edges)) <= 1, 'choose one form fixture'
    assert not (args.via_link and (args.textarea or args.form_edges)), 'form edge fixtures open directly'
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
    fixture_sheets = None
    expected_paths = ['/'] + [f'/sheet{i}.css' for i in range(args.sheets)]
    if args.sphinx:
        frozen = ROOT / 'tools/cobalt-text-preview/corpus/python-introduction'
        fixture_sheets = sorted(frozen.glob('sheet-*.css'))
        html = (frozen / 'page.html').read_text()
        sheet_index = 0
        expected_paths = ['/']
        def local_sheet(match):
            nonlocal sheet_index
            tag = match.group(0)
            if not re.search(r'rel=[\"\']stylesheet[\"\']', tag, re.I):
                return tag
            index = sheet_index
            sheet_index += 1
            if 'prefers-color-scheme: dark' not in tag:
                expected_paths.append(f'/sheet{index}.css')
            return re.sub(r'href=[\"\'][^\"\']*[\"\']', f'href="/sheet{index}.css"', tag)
        page = re.sub(r'<link\b[^>]*>', local_sheet, html, flags=re.I).encode()
        assert sheet_index == len(fixture_sheets)
    if args.textarea:
        assert not args.sphinx and not args.via_link
        args.sheets=2
        expected_paths=['/','/sheet0.css','/sheet1.css']
        page=b'<!doctype html><title>Multiline form</title><link rel=stylesheet href=/sheet0.css><link rel=stylesheet href=/sheet1.css><h1>Multiline form</h1><form action=/search.html><label>Note<textarea name=note maxlength=12>first</textarea></label><button>Send</button></form>'
    if args.form_edges:
        assert not args.sphinx and not args.textarea and not args.via_link
        args.sheets = 2
        expected_paths = ['/', '/sheet0.css', '/sheet1.css']
        page = b'<!doctype html><title>Form edges</title><link rel=stylesheet href=/sheet0.css><link rel=stylesheet href=/sheet1.css><h1>Form edges</h1><form action=/search.html><label>Note<textarea name=note maxlength=12>first</textarea></label><label>Token<input name=token value=fixed readonly></label><label>Memo<textarea name=memo readonly>locked</textarea></label><label>Amount<input type=number name=amount value=4 maxlength=1></label><button>Send</button></form>'
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
            body = page if self.path == ('/target' if args.via_link else '/') else None
            if args.via_link and self.path=='/':
                body=b'<!doctype html><title>Fixture index</title><h1>Fixture index</h1><a href=/target>Open styled page</a>'
            if self.path.startswith('/search.html?') and (args.sphinx or args.textarea or args.form_edges):
                body = b'<!doctype html><title>Search received</title><h1>Search received</h1><p>Native GET reached the server.</p>'
            if self.path.startswith('/sheet') and self.path.endswith('.css'):
                i = int(self.path[6:-4])
                body = fixture_sheets[i].read_bytes() if fixture_sheets else (b'h1{color:red;font-size:36px}' if i == 0 else
                        b'p{color:blue;margin-top:70px}' if i == 1 else
                        f'h1{{margin-bottom:{i}px}}'.encode())
            if self.path.endswith('.css'):
                time.sleep(2)
            if body is None:
                self.send_error(404)
                return
            self.send_response(200)
            self.send_header('Content-Type', 'text/css' if self.path.endswith('.css') else 'text/html; charset=utf-8')
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
        opening = ('wait-for Search the web\ntap-id address\ntype styles\n'
                   'tap-id url-dot\ntype example\ntap-id url-dot\ntype test\n'
                   'tap-id kb.enter\nwait-for styles.example.test\n' if args.typed_address else
                   'wait-for Search the web\ntap-id samples\ntap-id browser-more\n'
                   'tap-id go-home\nwait-for styles.example.test\n')
        if args.via_link:
            opening += 'tap-id browser-more\ntap-id navigate\ntap-id links\ntap-id link-0\n'
            expected_paths.append('/target')
        if args.sphinx or args.textarea or args.form_edges:
            opening += 'tap-id browser-more\ntap-id navigate\ntap-id page-view\n'
        route.write_text(opening +
                         'shot-colour before-sheet\nwait 1000\nwait-idle 30000\nwait 500\nwait-idle 30000\n'
                         'shot-colour after-sheet\ntap-id edit-address\n'
                         'tap-id kb.end\ntap-id kb.r0c0\nshot-colour address-edit\n'
                         'tap-id kb.cancel\n' + (
                         'tap-id browser-more\ntap-id forms\nshot-colour form-list\n'
                         'tap-id form-0\ntap-id field-0\ntype cobalt\ntap-id kb.enter\n'
                         'shot-colour form-edited\ntap-id submit-field-1\nwait-idle 30000\nshot-colour search-result\n'
                         if args.sphinx else (
                         'tap-id browser-more\ntap-id forms\ntap-id form-0\ntap-id field-0\n'
                         'tap-id form-newline\ntype secondx\nshot-colour textarea-limit\n'
                         'tap-id kb.backspace\ntype z\ntap-id kb.enter\nshot-colour textarea-saved\n'
                         'tap-id submit-form\nwait-idle 30000\nshot-colour search-result\n'
                         if args.textarea else (
                         'tap-id browser-more\ntap-id forms\ntap-id form-0\n'
                         'tap-id field-0\ntype draft\ntap-id kb.cancel\n'
                         'tap-id field-0\nexpect first\nexpect-missing draft\n'
                         'type wrong\ntap Back\n'
                         'tap-id field-0\nexpect first\nexpect-missing wrong\n'
                         'tap-id form-newline\ntype saved\ntap-id kb.enter\n'
                         'shot-colour textarea-back-cancel\n'
                         'tap-id field-1\nexpect Read only: fixed\nexpect Done\nexpect-missing New line\n'
                         'tap-id field-2\nexpect Read only: locked\nexpect-missing New line\n'
                         'shot-colour readonly-controls\n'
                         'tap-id field-3\ntap ?123\ntype 2\ntap-id kb.enter\n'
                         'shot-colour number-maxlength\ntap-id submit-form\n'
                         'wait-idle 30000\nshot-colour search-result\n'
                         if args.form_edges else ''))))
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
        app_status = result['status']
        result['app_status'] = app_status
        result['status'] = 'unverified' if app_status == 'pass' else app_status
        result.update(peak_app_rss_kib=peak_rss_kib, whole_journey_seconds=round(time.monotonic()-started,3), requests=requests, sphinx=args.sphinx, textarea=args.textarea, form_edges=args.form_edges)
        (out / 'result.json').write_text(json.dumps(result,indent=2)+'\n')
        assert app_status == 'pass', result
        before = json.loads((out / 'browser-shots/before-sheet.json').read_text())
        after = json.loads((out / 'browser-shots/after-sheet.json').read_text())
        assert before['frame']['sha256'] != after['frame']['sha256'], 'delayed CSS did not change the frame'
        result['styled_frame_changed'] = before['frame']['sha256'] != after['frame']['sha256']
        result['peak_app_rss_kib'] = peak_rss_kib
        result['whole_journey_seconds'] = round(time.monotonic() - started, 3)
        result['sheet_count'] = sum(p.endswith('.css') for p in expected_paths)
        result['requests'] = requests
        result['transport'] = 'real TLS/HTTP, debug-only loopback fixture'
        result['dirty'] = bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True).strip())
        result['source_sha'] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
        if args.sphinx or args.textarea or args.form_edges:
            if args.form_edges:
                submitted = [parse_qs(urlsplit(r['path']).query, keep_blank_values=True)
                             for r in requests if r['path'].startswith('/search.html?')]
                assert submitted == [{'note': ['first\r\nsaved'], 'token': ['fixed'],
                                      'memo': ['locked'], 'amount': ['42']}], submitted
            else:
                assert any(r['path'].startswith('/search.html?') and ('note=first%0D%0Aseconz' in r['path'] if args.textarea else 'cobalt' in r['path']) for r in requests), requests
            actual_sheets = [r['path'] for r in requests if r['path'].endswith('.css')]
            assert sorted(actual_sheets) == sorted(expected_paths[1:]), requests
        else:
            assert sorted(r['path'] for r in requests) == sorted(expected_paths), requests
        assert all(r['authorization'] is None for r in requests), requests
        result['status'] = 'pass'
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
