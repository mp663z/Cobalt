#!/usr/bin/env python3
"""Shrinks a saved page to what the browser reads.

Drops what a browser without scripts or style sheets never uses: script
bodies, style sheets, comments, classes, event handlers and data attributes. Inline
styles and aria attributes stay, since they decide what is hidden. Text,
structure, links, images and their sizes are kept, so the page parses to the
same document it did before.

    minimize.py saved.html > fixture.html
"""
import re
import sys

KEEP = {"href", "src", "alt", "title", "id", "name", "width", "height",
        "colspan", "rowspan", "lang", "dir", "role", "type", "value",
        "action", "method", "for", "hidden", "charset", "content",
        "http-equiv", "start", "reversed", "headers", "scope", "rel", "style",
        "data-src", "srcset", "sizes", "loading", "summary", "label",
        "selected", "checked", "placeholder", "size", "maxlength", "align"}

def attrs(match):
    name, rest = match.group(1), match.group(2)
    kept = []
    for m in re.finditer(r'([^\s=/>]+)(\s*=\s*("[^"]*"|\'[^\']*\'|[^\s>]+))?', rest):
        key = m.group(1).lower()
        if key in KEEP or key.startswith("aria-"):
            kept.append(m.group(0))
    tail = "/" if rest.rstrip().endswith("/") else ""
    return "<" + name + ("" if not kept else " " + " ".join(kept)) + tail + ">"

def main():
    html = open(sys.argv[1], encoding="utf-8").read()
    html = re.sub(r"<!--.*?-->", "", html, flags=re.S)
    # Scripts are emptied, not removed, so the page still warns that it had
    # them and says how many.
    html = re.sub(r"(<script\b[^>]*>).*?(</script\s*>)", r"\1\2", html, flags=re.S | re.I)
    for tag in ("style", "template"):
        html = re.sub(rf"<{tag}\b.*?</{tag}\s*>", "", html, flags=re.S | re.I)
    html = re.sub(r"<link\b[^>]*>", "", html, flags=re.I)
    html = re.sub(r'<meta\b(?![^>]*charset)[^>]*>', "", html, flags=re.I)
    html = re.sub(
        r"""<([a-zA-Z][a-zA-Z0-9]*)((?:\s+[^\s=/>]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+))?)*\s*/?)>""",
        attrs, html)
    html = re.sub(r"[ \t]+\n", "\n", html)
    html = re.sub(r"\n{3,}", "\n\n", html)
    sys.stdout.write(html)

main()
