#!/usr/bin/env python3
"""Offline Chrome/Cobalt scoreboard for a frozen SingleFile corpus ZIP.

No network access is needed or allowed by this script. The ZIP manifest
contains source URLs for attribution, but rendering uses only extracted bytes.
An unsupported Cobalt page is N/A, never a fabricated white screenshot.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import zipfile

from PIL import Image, ImageChops, ImageDraw


def run(command, log):
    with open(log, "wb") as stream:
        try:
            return subprocess.run(command, stdout=stream, stderr=subprocess.STDOUT,
                                  timeout=90, check=False).returncode
        except subprocess.TimeoutExpired:
            stream.write(b"\nTIMEOUT after 90 seconds\n")
            return 124


STATIC_COMBINED_VERSION = "static-combined-v1-2026-11-01"


def static_combined(args, name, html, width, height):
    """One static-capture measurement row: paired/refused plus inspection
    evidence. Links and other interactive content render inert. The pixel
    diff is recorded for inspection only - font substitution makes it
    unsuitable as a pass gate, so this block has no pass status and never
    feeds any score numerator."""
    ppm = args.output / (name + ".static-cobalt.ppm")
    log = args.output / (name + ".static-cobalt.log")
    command = [args.cargo, "run", "-q", "--manifest-path",
               str(Path("tools/cobalt-text-preview/Cargo.toml")), "--",
               str(html), str(ppm), str(width), str(height), args.font,
               "--blocks", "--static-capture"]
    if args.font_bold:
        command += ["--font-bold", args.font_bold]
    result = {"version": STATIC_COMBINED_VERSION, "static_capture": True,
              "interactive_content": "rendered inert; not a navigation or interaction pass"}
    code = run(command, log)
    result["cobalt_status"] = code
    chrome = args.output / (name + ".chrome.png")
    if code == 0 and ppm.exists() and chrome.exists():
        with Image.open(ppm) as cobalt_image, Image.open(chrome) as chrome_image:
            left = cobalt_image.convert("RGB")
            right = chrome_image.convert("RGB")
        if left.size == (width, height) and right.size == (width, height):
            diff = ImageChops.difference(left, right)
            changed = sum(pixel != (0, 0, 0) for pixel in diff.getdata())
            result["status"] = "paired static render"
            result["diff_percent_informational_not_a_gate"] = round(
                changed * 100 / (width * height), 5)
            combined = Image.new("RGB", (2 * width, height + 24), "white")
            combined.paste(left, (0, 24))
            combined.paste(right, (width, 24))
            ImageDraw.Draw(combined).text((4, 4), "Cobalt static (inert links)", fill="black")
            ImageDraw.Draw(combined).text((width + 4, 4), "Chrome", fill="black")
            combined.save(args.output / (name + ".static-side-by-side.png"))
        else:
            result["status"] = "wrong dimensions"
    else:
        result["status"] = "no static render"
        result["cobalt_error"] = log.read_text(errors="replace").strip().splitlines()[-1:] or ["no output"]
    return result


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("corpus", type=Path, help="frozen ZIP with manifest.json")
    ap.add_argument("output", type=Path, help="new scratch result directory")
    ap.add_argument("--chrome", default="google-chrome")
    ap.add_argument("--cargo", default="cargo")
    # Measurement version static-combined-v1-2026-11-01. Opt-in: also render
    # each frozen page through the restricted text preview's --static-capture
    # path (combined text+background, interactive content inert). This is
    # inspection evidence and a refusal ladder, never an interactive pass;
    # with font substitution the unmasked diff is informational, not a gate.
    ap.add_argument("--static-combined", action="store_true",
                    help="also run the static-combined measurement (requires --font)")
    ap.add_argument("--font", help="local regular face for the static-combined path")
    ap.add_argument("--font-bold", help="local bold face for the static-combined path")
    args = ap.parse_args()
    if args.static_combined and not args.font:
        ap.error("--static-combined requires --font")
    args.output.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(args.corpus) as archive:
        manifest = json.loads(archive.read("manifest.json"))
        for row in manifest:
            name = row["name"]
            if not name.replace("-", "").isalnum():
                raise ValueError("unsafe corpus name")
            contents = archive.read(row["name"] + ".complete.html")
            if hashlib.sha256(contents).hexdigest() != row["complete_sha256"]:
                raise ValueError("snapshot hash mismatch: " + name)
            (args.output / (name + ".html")).write_bytes(contents)
    scores = []
    for row in manifest:
        name = row["name"]
        width, height = map(int, row["viewport"].split("x"))
        html = args.output / (name + ".html")
        chrome = args.output / (name + ".chrome.png")
        cobalt = args.output / (name + ".cobalt.ppm")
        status = {"name": name, "source_url": row["source_url"],
                  "complete_sha256": row["complete_sha256"],
                  "viewport": row["viewport"], "diff_percent": None}
        chrome_code = run([args.chrome, "--headless", "--disable-gpu", "--no-sandbox",
                           "--hide-scrollbars", "--disable-background-networking",
                           # Render strictly from the frozen bytes: no page may fetch.
                           "--host-resolver-rules=MAP * ~NOTFOUND",
                           f"--window-size={width},{height}", f"--screenshot={chrome}",
                           html.as_uri()], args.output / (name + ".chrome.log"))
        cobalt_code = run([args.cargo, "run", "-q", "-p", "kobo-web-document",
                            "--example", "css_background_preview", "--", str(html),
                            str(cobalt), str(width), str(height)],
                           args.output / (name + ".cobalt.log"))
        status["chrome_status"] = chrome_code
        status["cobalt_status"] = cobalt_code
        if chrome_code == 0 and cobalt_code == 0 and chrome.exists() and cobalt.exists():
            with Image.open(chrome) as chrome_image, Image.open(cobalt) as cobalt_image:
                left = cobalt_image.convert("RGB")
                right = chrome_image.convert("RGB")
            if left.size != (width, height) or right.size != (width, height):
                status["status"] = "wrong dimensions"
            else:
                diff = ImageChops.difference(left, right)
                changed = sum(pixel != (0, 0, 0) for pixel in diff.getdata())
                status["changed_pixels"] = changed
                status["diff_percent"] = round(changed * 100 / (width * height), 5)
                status["status"] = "paired render"
                diff.save(args.output / (name + ".diff.png"))
                combined = Image.new("RGB", (2 * width, height + 24), "white")
                combined.paste(left, (0, 24))
                combined.paste(right, (width, 24))
                ImageDraw.Draw(combined).text((4, 4), "Cobalt", fill="black")
                ImageDraw.Draw(combined).text((width + 4, 4), "Chrome", fill="black")
                combined.save(args.output / (name + ".side-by-side.png"))
        else:
            status["status"] = "no paired render"
            cobalt_log = (args.output / (name + ".cobalt.log")).read_text(errors="replace")
            status["cobalt_error"] = cobalt_log.strip().splitlines()[-1:] or ["no output"]
        if args.static_combined:
            status["static_combined"] = static_combined(args, name, html, width, height)
        scores.append(status)
    (args.output / "scoreboard.json").write_text(json.dumps(scores, indent=2) + "\n")
    for row in scores:
        print(row["name"], row["status"], "diff=" + str(row["diff_percent"]),
              "Cobalt=" + str(row["cobalt_status"]),
              ("static=" + row["static_combined"]["status"]) if "static_combined" in row else "")
    return 0 if all(row["status"] == "paired render" for row in scores) else 1


if __name__ == "__main__":
    sys.exit(main())
