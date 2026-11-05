# v4 custom properties + var() candidate: review checkpoint (2026-11-05 19:50 IST)
Status: candidate-quality PASS from a fresh independent review (manifest 1704 files, kobo-web-document 629/0, extra adversarial tests for inline order tie and raw comments). NOT canonical, NOT a rendering-gate pass, NOT a PR. Real-page gate remains 0/5 (last-known).
Reviewed source commit: reviewed-source-commit.txt (tree: reviewed-source-tree.txt). This evidence commit only adds files under evidence/.
Known limits: engine refuses (stricter than Chrome) glued tokens, cycles even with fallback, invalid-but-losing var(), empty custom values, any raw comment in custom/var values.
