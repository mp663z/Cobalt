#!/usr/bin/env python3
"""BW-profile (clara-bw-391) app-RSS journey checkpoints at a given checkout.

Usage: bw_journey.py ROOT KOBO_BIN OUT_JSON
Checkpoints: start-page, samples-index(native), native-article, painted-article,
form-page, form-links-list, textarea-editor. App-process VmRSS/VmHWM from /proc.
Host x86_64, sim app debug, debug drive CLI. Not device numbers.
"""
import json, os, re, subprocess, sys, tempfile, time
from pathlib import Path

ROOT = Path(sys.argv[1]); KOBO = sys.argv[2]; OUT = sys.argv[3]
APP_DIR = ROOT / "apps/browser"
ENV = dict(os.environ, PATH="/home/sandbox/bin:/home/sandbox/.cargo/bin:" + os.environ["PATH"])

def proc_status(comm):
    best = None
    for pid in os.listdir("/proc"):
        if not pid.isdigit(): continue
        try:
            if open(f"/proc/{pid}/comm").read().strip() != comm: continue
            cmdline = open(f"/proc/{pid}/cmdline").read()
            if comm == "kobo" and " dev " not in cmdline.replace("\x00", " "): continue
            st = {}
            for line in open(f"/proc/{pid}/status"):
                for k in ("VmRSS", "VmHWM", "RssAnon"):
                    if line.startswith(k + ":"): st[k] = int(line.split()[1])
            if "VmRSS" in st: best = {"pid": int(pid), **st}
        except (FileNotFoundError, PermissionError): pass
    return best

state = tempfile.mkdtemp(prefix="cb-bw-", dir="/tmp")
log = open(state + "/sim.log", "w")
env = dict(ENV, TMPDIR=state, KOBO_INKLING_DAY="2026-09-01",
           CARGO_TARGET_DIR="/home/sandbox/cb/qualification/target")
env.pop("KOBO_SIM_PROFILE", None)  # default clara-bw-391
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
print("addr", addr, "boot", round(time.monotonic() - t0, 1), flush=True)

def step(s, t=60):
    p = subprocess.run([KOBO, "drive", "--address", addr, "--step", s],
                       capture_output=True, text=True, timeout=t, env=ENV)
    if p.returncode: print(f"  step FAILED: {s}: {p.stderr.strip()[:120]}", flush=True)
    return p
def wait_for(text, timeout=30):
    t0 = time.monotonic()
    while time.monotonic() - t0 < timeout:
        if text in step("dump", 15).stdout: return time.monotonic() - t0
        time.sleep(0.15)
    print(f"  wait-for TIMEOUT: {text}", flush=True)
    return None

T0 = time.monotonic()
out = {"checkpoints": []}
def snap(label):
    rec = {"label": label, "t": round(time.monotonic() - T0, 1),
           "app": proc_status("kobo-browser"), "dev_server": proc_status("kobo")}
    out["checkpoints"].append(rec); print(json.dumps(rec), flush=True)

snap("start-page")
step("wait-for Search the web")
step("tap-id samples"); wait_for("Sample pages")
snap("samples-index")
step("tap-id link-0"); wait_for('"1 of')
snap("native-article")
step("tap-id browser-more"); step("tap-id navigate"); step("tap-id page-view")
wait_for('"1 of', 20)
snap("painted-article")
step("tap-id back"); wait_for("Sample pages", 15)
step("tap-id browser-more"); step("tap-id navigate"); step("tap-id native-reader")  # switch painted->native
wait_for("Sample pages", 10)
step("tap-id link-5"); wait_for("Sample form")
snap("form-page")
step("tap-id browser-more"); step("tap-id navigate"); step("tap-id links")
if wait_for("links on this page", 15) is not None:
    snap("form-links-list")
    step("tap-id return")  # links list publishes id return labelled "Done"; done is not an id
    wait_for("Sample form", 15)
else:
    snap("form-links-list")  # records state even when the list did not open
step("tap-id form-0")
step("wait-for Search words")
snap("form-overview")
step("tap Search words")
time.sleep(1)
snap("field-editor")
step("dump")
step("tap Cancel")  # field editor's only action is the Cancel button; tap by label

Path(OUT).write_text(json.dumps(out, indent=2))
os.killpg(proc.pid, 15); proc.wait(timeout=5)
print("DONE", flush=True)
