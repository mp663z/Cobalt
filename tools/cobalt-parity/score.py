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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("corpus", type=Path, help="frozen ZIP with manifest.json")
    ap.add_argument("output", type=Path, help="new scratch result directory")
    ap.add_argument("--chrome", default="google-chrome")
    ap.add_argument("--cargo", default="cargo")
    args = ap.parse_args()
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
        scores.append(status)
    (args.output / "scoreboard.json").write_text(json.dumps(scores, indent=2) + "\n")
    for row in scores:
        print(row["name"], row["status"], "diff=" + str(row["diff_percent"]),
              "Cobalt=" + str(row["cobalt_status"]))
    return 0 if all(row["status"] == "paired render" for row in scores) else 1


if __name__ == "__main__":
    sys.exit(main())
