#!/usr/bin/env python3
"""Takes the http and https cases out of WPT's urltestdata.json.

    vendor.py urltestdata.json > cases.tsv

One case per line: input, base (`-` for none), and the expected href or
`failure`. Backslash, tab, newline and carriage return are written as
`\\\\`, `\\t`, `\\n`, `\\r`; other control characters as `\\u{XX}`.
"""
import json
import sys

def escape(text):
    out = []
    for c in text:
        if c == "\\":
            out.append("\\\\")
        elif c == "\t":
            out.append("\\t")
        elif c == "\n":
            out.append("\\n")
        elif c == "\r":
            out.append("\\r")
        elif ord(c) < 0x20 or ord(c) == 0x7F or 0xD800 <= ord(c) <= 0xDFFF:
            out.append("\\u{%X}" % ord(c))
        else:
            out.append(c)
    return "".join(out)

def web(url):
    return url is not None and url.lower().startswith(("http:", "https:"))

cases = [c for c in json.load(open(sys.argv[1], encoding="utf-8")) if isinstance(c, dict)]
for case in cases:
    base = case.get("base")
    if base is not None and not web(base):
        continue
    if case.get("failure"):
        expected = "failure"
    elif web(case["href"]):
        expected = case["href"]
    else:
        continue
    print("\t".join([escape(case["input"]), "-" if base is None else escape(base), escape(expected)]))
