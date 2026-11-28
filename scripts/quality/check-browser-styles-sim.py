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
from collections import Counter

ROOT = Path(__file__).resolve().parents[2]
HOST = 'styles.example.test'


def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def form_route(args):
    """Choose one deterministic native-form journey."""
    if args.post_review:
        return (
            'tap-id browser-more\ntap-id forms\ntap-id form-0\n'
            'tap-id field-0\ntype edited\ntap-id kb.enter\n'
            'tap-id submit-field-1\nexpect Review submission\nexpect cannot send POST\n'
            'shot-colour post-review\ntap-id cancel-post\nexpect originaledited\n'
            'tap-id submit-field-1\nexpect Review submission\ntap Back\n'
            'expect originaledited\nshot-colour post-cancelled\n'
        )
    if args.paged_form:
        return (
            'tap-id browser-more\ntap-id forms\ntap-id form-0\n'
            'tap-id field-1\ntype edited\ntap-id kb.enter\n'
            'tap-id field-0\ntap-id option-next\ntap Back\n'
            'expect Choice number 0\nexpect v0edited\n'
            'tap-id field-0\ntap-id option-next\ntap-id return-to-form\n'
            'expect Choice number 0\nexpect v0edited\n'
            'tap-id field-0\ntap-paged Choice number 29\n'
            'shot-colour long-choice-saved\ntap-paged Send last\n'
            'wait-idle 30000\nshot-colour search-result\n'
        )
    if args.sphinx:
        return (
            'tap-id browser-more\n'
            'tap-id forms\n'
            'shot-colour form-list\n'
            'tap-id form-0\n'
            'tap-id field-0\n'
            'type cobalt\n'
            'tap-id kb.enter\n'
            'shot-colour form-edited\n'
            'tap-id submit-field-1\n'
            'wait-idle 30000\n'
            'shot-colour search-result\n'
        )
    if args.textarea:
        return (
            'tap-id browser-more\n'
            'tap-id forms\n'
            'tap-id form-0\n'
            'tap-id field-0\n'
            'tap-id form-newline\n'
            'type secondx\n'
            'shot-colour textarea-limit\n'
            'tap-id kb.backspace\n'
            'type z\n'
            'tap-id kb.enter\n'
            'shot-colour textarea-saved\n'
            'tap-id submit-form\n'
            'wait-idle 30000\n'
            'shot-colour search-result\n'
        )
    if args.form_edges:
        return (
            'tap-id browser-more\n'
            'tap-id forms\n'
            'tap-id form-0\n'
            'tap-id field-0\n'
            'type draft\n'
            'tap-id kb.cancel\n'
            'tap-id field-0\n'
            'expect first\n'
            'expect-missing draft\n'
            'type wrong\n'
            'tap Back\n'
            'tap-id field-0\n'
            'expect first\n'
            'expect-missing wrong\n'
            'tap-id form-newline\n'
            'type saved\n'
            'tap-id kb.enter\n'
            'shot-colour textarea-back-cancel\n'
            'tap-id field-1\n'
            'expect Read only: fixed\n'
            'expect Done\n'
            'expect-missing New line\n'
            'tap-id field-2\n'
            'expect Read only: locked\n'
            'expect-missing New line\n'
            'shot-colour readonly-controls\n'
            'tap-id field-3\n'
            'tap ?123\n'
            'type 2\n'
            'tap-id kb.enter\n'
            'shot-colour number-maxlength\n'
            'tap-id submit-form\n'
            'wait-idle 30000\n'
            'shot-colour search-result\n'
        )
    if args.choices:
        return (
            'tap-id browser-more\n'
            'tap-id forms\n'
            'tap-id form-0\n'
            'expect-missing excluded\n'
            'tap-id field-1\n'
            'tap-id option-1\n'
            'tap-id field-2\n'
            'tap-id field-3\n'
            'tap-id option-1\n'
            'tap-id field-5\ntap-id kb.backspace\ntap-id kb.enter\n'
            'shot-colour choice-values\n'
            'tap-id submit-field-7\n'
            'wait-idle 30000\n'
            'shot-colour search-result\n'
        )
    return ""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--profile', default='libra-colour-390', help='simulator hardware profile')
    parser.add_argument('--sheets', type=int, default=5)
    parser.add_argument('--typed-address', action='store_true')
    parser.add_argument('--post-review', action='store_true', help='test POST remains gated through Cancel and runtime Back')
    parser.add_argument('--paged-form', action='store_true', help='test paginated form fields and select options')
    parser.add_argument('--choices', action='store_true', help='test native select/radio/checkbox, disabled and hidden controls and explicit submitter')
    parser.add_argument('--form-edges', action='store_true', help='test textarea cancel/Back, readonly controls and number maxlength semantics')
    parser.add_argument('--textarea', action='store_true', help='test multiline edit, maxlength and GET CRLF encoding')
    parser.add_argument('--via-link', action='store_true', help='open target through the native Links list')
    parser.add_argument('--sphinx', action='store_true', help='serve the frozen Python/Sphinx page and test native search from Page view')
    args = parser.parse_args()
    assert 2 <= args.sheets <= 16
    assert sum((args.sphinx, args.textarea, args.form_edges, args.choices, args.paged_form, args.post_review)) <= 1, 'choose one form fixture'
    assert not (args.via_link and (args.textarea or args.form_edges or args.choices or args.paged_form or args.post_review)), 'form edge fixtures open directly'
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
    if args.choices:
        args.sheets = 2
        expected_paths = ['/', '/sheet0.css', '/sheet1.css']
        page = b'<!doctype html><title>Choice form</title><link rel=stylesheet href=/sheet0.css><link rel=stylesheet href=/sheet1.css><h1>Choice form</h1><form action="/search.html?old=stale#fragment"><input type=hidden name=secret value="a&amp;b"><input disabled name=excluded value=no><label>Colour<select name=colour><option value=red>Red</option><option value=blue>Blue</option></select></label><label><input type=checkbox name=stock checked>Stock</label><label><input type=radio name=size value=small checked>Small</label><label><input type=radio name=size value=large>Large</label><fieldset disabled><legend><label>Legend<input name=legend value=keep></label></legend><input name=fieldset_excluded value=no><legend><input name=later_legend value=no></legend></fieldset><label>Empty<input name=empty value=x></label><input type=submit name=go value=First><input type=submit name=go value=Second></form>'
    if args.paged_form:
        args.sheets = 2
        expected_paths = ['/', '/sheet0.css', '/sheet1.css']
        page = ('<!doctype html><title>Paged form</title><link rel=stylesheet href=/sheet0.css><link rel=stylesheet href=/sheet1.css><h1>Paged form</h1><form action=/search.html><label>Item<select name=item>'
                + ''.join(f'<option value={i}>Choice number {i}</option>' for i in range(30))
                + '</select></label>'
                + ''.join(f'<label>Field {i}<input name=f{i} value=v{i}></label>' for i in range(24))
                + '<input type=submit name=go value="Send last"></form>').encode()
    if args.post_review:
        args.sheets = 2
        expected_paths = ['/', '/sheet0.css', '/sheet1.css']
        page = b'<!doctype html><title>POST gate</title><link rel=stylesheet href=/sheet0.css><link rel=stylesheet href=/sheet1.css><h1>POST gate</h1><form method=post action=/never-send><label>Note<input name=note value=original></label><input type=submit value=Send></form>'
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

        def do_POST(self):
            requests.append({'path': self.path, 'method': 'POST', 'authorization': self.headers.get('Authorization')})
            self.send_error(405)

        def do_GET(self):
            requests.append({'path': self.path, 'authorization': self.headers.get('Authorization'),
                             'accept': self.headers.get('Accept')})
            body = page if self.path == ('/target' if args.via_link else '/') else None
            if args.via_link and self.path=='/':
                body=b'<!doctype html><title>Fixture index</title><h1>Fixture index</h1><a href=/target>Open styled page</a>'
            if self.path.startswith('/search.html?') and (args.sphinx or args.textarea or args.form_edges or args.choices or args.paged_form or args.post_review):
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
        if args.sphinx or args.textarea or args.form_edges or args.choices or args.paged_form or args.post_review:
            opening += 'tap-id browser-more\ntap-id navigate\ntap-id page-view\n'
        route.write_text(opening +
                         'shot-colour before-sheet\nwait 1000\nwait-idle 30000\nwait 500\nwait-idle 30000\n'
                         'shot-colour after-sheet\ntap-id edit-address\n'
                         'tap-id kb.end\ntap-id kb.r0c0\nshot-colour address-edit\n'
                         'tap-id kb.cancel\nshot-colour address-cancel\n'
                         'tap-id edit-address\ntap-id kb.end\ntap-id kb.r0c1\ntap Back\n'
                         'shot-colour address-back\n' + form_route(args))
        env = dict(os.environ, CARGO_TARGET_DIR=str(target),
                   KOBO_SIM_PROFILE=args.profile,
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
        result.update(peak_app_rss_kib=peak_rss_kib, whole_journey_seconds=round(time.monotonic()-started,3), requests=requests, sphinx=args.sphinx, textarea=args.textarea, form_edges=args.form_edges, choices=args.choices, paged_form=args.paged_form, post_review=args.post_review, profile=args.profile)
        (out / 'result.json').write_text(json.dumps(result,indent=2)+'\n')
        assert app_status == 'pass', result
        before = json.loads((out / 'browser-shots/before-sheet.json').read_text())
        after = json.loads((out / 'browser-shots/after-sheet.json').read_text())
        assert before['frame']['sha256'] != after['frame']['sha256'], 'delayed CSS did not change the frame'
        result['styled_frame_changed'] = before['frame']['sha256'] != after['frame']['sha256']
        for name in ['address-cancel', 'address-back']:
            restored = json.loads((out / f'browser-shots/{name}.json').read_text())
            assert restored['frame']['sha256'] == after['frame']['sha256'], f'{name} did not restore page'
        result['address_dismissals_restore_frame'] = True
        result['peak_app_rss_kib'] = peak_rss_kib
        result['whole_journey_seconds'] = round(time.monotonic() - started, 3)
        result['sheet_count'] = sum(p.endswith('.css') for p in expected_paths)
        result['requests'] = requests
        result['transport'] = 'real TLS/HTTP, debug-only loopback fixture'
        result['dirty'] = bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True).strip())
        result['source_sha'] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
        if args.sphinx or args.textarea or args.form_edges or args.choices or args.paged_form or args.post_review:
            if args.post_review:
                assert Counter(r['path'] for r in requests) == Counter(expected_paths), requests
            elif args.paged_form:
                submitted = [parse_qs(urlsplit(r['path']).query, keep_blank_values=True)
                             for r in requests if r['path'].startswith('/search.html?')]
                expected = {'item': ['29'], **{f'f{i}': [f'v{i}'] for i in range(24)}, 'go': ['Send last']}
                expected['f0'] = ['v0edited']
                assert submitted == [expected], submitted
            elif args.form_edges:
                submitted = [parse_qs(urlsplit(r['path']).query, keep_blank_values=True)
                             for r in requests if r['path'].startswith('/search.html?')]
                assert submitted == [{'note': ['first\r\nsaved'], 'token': ['fixed'],
                                      'memo': ['locked'], 'amount': ['42']}], submitted
            elif args.choices:
                submitted = [parse_qs(urlsplit(r['path']).query, keep_blank_values=True)
                             for r in requests if r['path'].startswith('/search.html?')]
                assert submitted == [{'secret': ['a&b'], 'colour': ['blue'],
                                      'size': ['large'], 'legend': ['keep'], 'empty': [''], 'go': ['Second']}], submitted
            else:
                assert any(r['path'].startswith('/search.html?') and ('note=first%0D%0Aseconz' in r['path'] if args.textarea else 'cobalt' in r['path']) for r in requests), requests
            submissions = [r['path'] for r in requests if r['path'].startswith('/search.html?')]
            assert len(submissions) == (0 if args.post_review else 1), requests
            assert Counter(r['path'] for r in requests) == Counter(expected_paths + submissions), requests
        else:
            assert sorted(r['path'] for r in requests) == sorted(expected_paths), requests
        assert not any(r.get('method') == 'POST' for r in requests), requests
        assert all(r['authorization'] is None for r in requests), requests
        result['status'] = 'pass'
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
