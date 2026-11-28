#!/usr/bin/env python3
"""Sheet-supply comparison probe: one rep per invocation.

Usage: sheet_compare.py ROOT KOBO_BIN OUT_JSON REP_LABEL
- colour profile libra-colour-390, TLS fixture index seeded as browser-home
- route: samples > browser-more > go-home, then link taps to /sheets5 /sheets16
- measures app-process (comm kobo-browser, VmRSS-present PIDs only) VmRSS/VmHWM
  at checkpoints, nav latencies, and fetched-css counts from the fixture log.
Host x86_64, sim app debug, CLI = KOBO_BIN (debug; release CLI refuses the
fixture by design). Not device numbers.
"""
import http.server, importlib.util, json, os, re, ssl, subprocess, sys, tempfile, threading, time
from pathlib import Path

ROOT = Path(sys.argv[1])
KOBO = sys.argv[2]
OUT = sys.argv[3]
REP = sys.argv[4]
APP_DIR = ROOT / "apps/browser"
CORPUS_CSS = ROOT / "tools/cobalt-text-preview/corpus/python-controlflow"
HOST = "qual.example.test"
ENV = dict(os.environ, PATH="/home/sandbox/bin:/home/sandbox/.cargo/bin:" + os.environ["PATH"])
sys.path.insert(0, str(ROOT / "scripts/quality"))

def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m); return m

base_html = (CORPUS_CSS / "page.html").read_text()
css_bodies = [p.read_bytes() for p in sorted(CORPUS_CSS.glob("sheet-*.css"))]

def variant(i):
    b = bytearray(css_bodies[i % len(css_bodies)])
    b += f"\n/* fixture variant {i} */\nbody{{--qual-variant-{i}:1}}\n".encode()
    return bytes(b)

def page_with_sheets(n, prefix):
    # small Sphinx-like page: auto-paints on colour profiles, full census
    links = "".join(f'<link rel="stylesheet" href="/{prefix}-{i}.css">' for i in range(n))
    title = f"{prefix.upper()} PAGE"
    return f'''<!doctype html><html><head><title>{title}</title>{links}</head>
<body><div class="document"><h1>{title}</h1>
<p>Sphinx-like body text: control flow tools, while statements, for loops, and ranges.</p>
<pre>def f(n):\n    return n * 2</pre>
<p>More prose to give the painter something to lay out across pages.</p>
<table><tr><th>Name</th><th>Value</th></tr><tr><td>alpha</td><td>1</td></tr></table>
</div></body></html>'''.encode()

INDEX_PAGE = b'''<!doctype html><html><head><title>qual fixtures</title></head>
<body><h1>Qualification fixtures</h1>
<p style="font-size:48px"><a href="/sheets5">OPEN SHEETS5 PAGE</a></p>
<p style="font-size:48px"><a href="/sheets16">OPEN SHEETS16 PAGE</a></p>
</body></html>'''

REQUESTS = []
class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_): pass
    def do_GET(self):
        REQUESTS.append(self.path)
        body, ctype = None, "text/html"
        if self.path == "/": body = INDEX_PAGE
        elif self.path == "/sheets5": body = page_with_sheets(5, "s5")
        elif self.path == "/sheets16": body = page_with_sheets(16, "s16")
        else:
            m = re.fullmatch(r"/(s5|s16)-(\d+)\.css", self.path)
            if m: body, ctype = variant(int(m.group(2))), "text/css"
        if body is None: self.send_error(404); return
        self.send_response(200); self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body))); self.end_headers()
        self.wfile.write(body)

def proc_status(comm):
    best = None
    for pid in os.listdir("/proc"):
        if not pid.isdigit(): continue
        try:
            if open(f"/proc/{pid}/comm").read().strip() != comm: continue
            cmdline = open(f"/proc/{pid}/cmdline").read()
            if comm == "kobo" and " dev " not in cmdline.replace("\x00", " "):
                continue  # transient `kobo drive` CLI, not the dev server
            st = {}
            for line in open(f"/proc/{pid}/status"):
                for k in ("VmRSS", "VmHWM", "RssAnon"):
                    if line.startswith(k + ":"): st[k] = int(line.split()[1])
            if "VmRSS" in st:
                best = {"pid": int(pid), **st}
        except (FileNotFoundError, PermissionError): pass
    return best

cert = module(ROOT / "scripts/quality/check-audiobook-sim.py", "tls_fixture")
priv = Path(tempfile.mkdtemp(prefix="cb-tlsc-", dir="/tmp"))
trust = cert.certificate(priv, [HOST])
srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
c = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); c.load_cert_chain(priv / f"{HOST}.pem", priv / f"{HOST}.key")
srv.socket = c.wrap_socket(srv.socket, server_side=True)
threading.Thread(target=srv.serve_forever, daemon=True).start()

state = tempfile.mkdtemp(prefix="cb-cmp-", dir="/tmp")
seed = Path(state) / "cobalt-sim-state/browser"
seed.mkdir(parents=True)
(seed / "browser-home").write_text(f"https://{HOST}/")
log = open(state + "/sim.log", "w")
env = dict(ENV, TMPDIR=state, KOBO_INKLING_DAY="2026-09-01", KOBO_SIM_PROFILE="libra-colour-390",
           KOBO_SIM_HTTP_FIXTURE=f"{HOST}=127.0.0.1:{srv.server_port}", KOBO_SIM_TRUST_DIR=str(trust),
           CARGO_TARGET_DIR=str(ROOT / "target"))
env.pop("KOBO_SIM_OFFLINE", None)
proc = subprocess.Popen([KOBO, "dev", "127.0.0.1:0"], cwd=APP_DIR, env=env,
                        stdout=log, stderr=log, start_new_session=True)
addr = None
t0 = time.monotonic()
while time.monotonic() - t0 < 240:
    log.flush(); m = re.search(r"127\.0\.0\.1:\d+", open(log.name).read())
    if m:
        addr = m.group(0)
        if subprocess.run([KOBO, "drive", "--address", addr, "--step", "dump"],
                          capture_output=True, text=True, timeout=10, env=ENV).returncode == 0:
            break
        addr = None
    time.sleep(0.3)
assert addr, "sim not ready"
boot_s = time.monotonic() - t0
print("addr", addr, "boot", round(boot_s, 1), flush=True)

def step(s, t=60):
    return subprocess.run([KOBO, "drive", "--address", addr, "--step", s],
                          capture_output=True, text=True, timeout=t, env=ENV)
def wait_for(text, timeout=30):
    t0 = time.monotonic()
    while time.monotonic() - t0 < timeout:
        if text in step("dump", 15).stdout: return time.monotonic() - t0
        time.sleep(0.1)
    return None

T0 = time.monotonic()
out = {"rep": REP, "boot_s": round(boot_s, 2), "checkpoints": [], "latencies": {}, "fetches": {}}
def snap(label):
    rec = {"label": label, "t": round(time.monotonic() - T0, 1),
           "app": proc_status("kobo-browser"), "dev_server": proc_status("kobo")}
    out["checkpoints"].append(rec); print(json.dumps(rec), flush=True)

snap("start-page")
step("wait-for Search the web")
step("tap-id samples")
t0 = time.monotonic()
step("tap-id browser-more"); step("tap-id go-home")
lat = wait_for("qual.example.test/")
out["latencies"]["nav-home"] = None if lat is None else round(time.monotonic() - t0, 3)
print("LAT nav-home", out["latencies"]["nav-home"], flush=True)
snap("fixture-index")

for label, linkid in (("sheets5", "link-0"), ("sheets16", "link-1")):
    before = len(REQUESTS)
    r0 = step("tap-id browser-more"); r1 = step("tap-id navigate"); r2 = step("tap-id links")
    t0 = time.monotonic()
    r = step(f"tap-id {linkid}")
    if r.returncode != 0:
        print(label, "link tap failed; dump excerpt:", step("dump").stdout[:500], flush=True)
    lat = wait_for(f'"https://{HOST}/{label}"', 60)
    out["latencies"][f"nav-{label}"] = None if lat is None else round(time.monotonic() - t0, 3)
    print(f"LAT nav-{label}", out["latencies"][f"nav-{label}"], "routes", r0.returncode, r1.returncode, r2.returncode, r.returncode, flush=True)
    step("wait-idle 30000")
    snap(f"{label}-painted")
    fetched = [x for x in REQUESTS[before:] if x.endswith(".css")]
    out["fetches"][label] = fetched
    out["fetches"][label + "-all"] = REQUESTS[before:]
    print(f"{label}: css fetched {len(fetched)}; all: {REQUESTS[before:]}", flush=True)
    step("tap-id back")
    tb = time.monotonic()
    while time.monotonic() - tb < 20:
        if f'"https://{HOST}/"' in step("dump", 15).stdout: break
        time.sleep(0.2)

Path(OUT).write_text(json.dumps(out, indent=2))
os.killpg(proc.pid, 15); proc.wait(timeout=5); srv.shutdown()
print("DONE", flush=True)
