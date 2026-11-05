# Differential against Chrome

Each corpus page is loaded in headless Chrome with scripts off and the
network off, served at the address it was fetched from, and its visible
links and words are compared with this browser's committed readings.

    node chrome-links.mjs > /tmp/chrome.json
    CHROME_VERSION="$(google-chrome --version | awk '{print $3}')" \
        python3 compare.py /tmp/chrome.json > report.md

It needs Chrome, so it is run by hand, not in the test suite. `report.md` is
the last run.

Words that differ on purpose:

- A picture here reads as its description in brackets, `[Python logo]`;
  Chrome's visible text has no alt text.
- Buttons, select boxes and other controls outside a form (theme pickers,
  "move to sidebar", language menus) are left out here; Chrome shows their
  labels.
- A literal `|` in a page is not counted, since this browser joins table
  cells with it.
