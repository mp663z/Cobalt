"""Frozen-page scoreboard must attribute only artifacts from this attempt."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import score


class ScoreTests(unittest.TestCase):
    def test_success_exit_without_new_image_cannot_reuse_old_render(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            out = root / "out"
            out.mkdir()
            corpus = root / "corpus.zip"
            html = b"<body>fixed bytes</body>"
            row = {"name": "page", "source_url": "https://example.invalid/",
                   "complete_sha256": hashlib.sha256(html).hexdigest(),
                   "viewport": "20x20"}
            with zipfile.ZipFile(corpus, "w") as archive:
                archive.writestr("manifest.json", json.dumps([row]))
                archive.writestr("page.complete.html", html)
            from PIL import Image
            Image.new("RGB", (20, 20), "white").save(out / "page.cobalt.ppm")
            for name in ("page.diff.png", "page.side-by-side.png"):
                (out / name).write_bytes(b"old comparison")

            def fake(command, log):
                log.write_text("successful command without output" if "--example" in command else "chrome")
                if "--example" not in command:
                    target = next(x.removeprefix("--screenshot=") for x in command if x.startswith("--screenshot="))
                    Image.new("RGB", (20, 20), "white").save(target)
                return 0

            with patch("score.run", side_effect=fake), patch.object(sys, "argv", ["score.py", str(corpus), str(out)]):
                self.assertEqual(score.main(), 1)
            result = json.loads((out / "scoreboard.json").read_text())[0]
            self.assertEqual(result["status"], "no paired render")
            self.assertIsNone(result["diff_percent"])
            for name in ("page.cobalt.ppm", "page.diff.png", "page.side-by-side.png"):
                self.assertFalse((out / name).exists())


if __name__ == "__main__":
    unittest.main()
