#!/usr/bin/env python3
"""Compares the browser's links and words with Chrome's for each corpus page.

    node chrome-links.mjs > /tmp/chrome.json
    compare.py /tmp/chrome.json > report.md

The browser's readings are the committed ones in ../corpus/expected. Link
targets are compared in order, and link words where the targets line up.
Visible text is compared as a sequence of words, since the two lay text
out differently (a table row here is one line joined by `|`).
"""
import difflib
import json
import os
import sys

here = os.path.dirname(os.path.abspath(__file__))
chrome = json.load(open(sys.argv[1], encoding="utf-8"))
print("# This browser against Chrome: links and words\n")
print("Chrome " + os.environ.get("CHROME_VERSION", "(version not recorded)") +
      ", headless, scripts off, network off, each page served at its real address.\n")
print("| Page | Links ours | Links Chrome | Links same, in order | Only ours | Only Chrome | Link words differ | Words ours | Words Chrome | Words same, in order |")
print("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
details = []
for name in chrome:
    ours = []
    with open(os.path.join(here, "..", "corpus", "expected", name + ".links"), encoding="utf-8") as f:
        for line in f.read().splitlines():
            text, target = line.split("\t", 1)
            ours.append((text, target))
    theirs = [tuple(pair) for pair in chrome[name]["links"]]
    with open(os.path.join(here, "..", "corpus", "expected", name + ".text"), encoding="utf-8") as f:
        body = f.read().split("\n\n", 1)[1]
    our_words = body.replace(" | ", " ").split()
    their_words = chrome[name]["text"].split()
    text_matcher = difflib.SequenceMatcher(None, our_words, their_words, autojunk=False)
    same_words = sum(block.size for block in text_matcher.get_matching_blocks())
    matcher = difflib.SequenceMatcher(None, [t for _, t in ours], [t for _, t in theirs], autojunk=False)
    same = words = 0
    only_ours, only_chrome, word_diffs = [], [], []
    for op, a1, a2, b1, b2 in matcher.get_opcodes():
        if op == "equal":
            same += a2 - a1
            for (ot, target), (ct, _) in zip(ours[a1:a2], theirs[b1:b2]):
                if " ".join(ot.split()) != " ".join(ct.split()):
                    words += 1
                    word_diffs.append((target, ot, ct))
        else:
            only_ours += ours[a1:a2]
            only_chrome += theirs[b1:b2]
    print(f"| {name} | {len(ours)} | {len(theirs)} | {same} | {len(only_ours)} | {len(only_chrome)} | {words} "
          f"| {len(our_words)} | {len(their_words)} | {same_words} |")
    text_diffs = []
    for op, a1, a2, b1, b2 in text_matcher.get_opcodes():
        if op != "equal":
            text_diffs.append((" ".join(our_words[a1:a2]), " ".join(their_words[b1:b2])))
    if only_ours or only_chrome or word_diffs or text_diffs:
        details.append((name, only_ours, only_chrome, word_diffs, text_diffs))
for name, only_ours, only_chrome, word_diffs, text_diffs in details:
    print(f"\n## {name}\n")
    for label, items in (("Only ours", only_ours), ("Only Chrome", only_chrome)):
        if items:
            print(f"{label} ({len(items)}, first 8):\n")
            for text, target in items[:8]:
                print(f"- `{text[:60]}` {target[:100]}")
            print()
    if word_diffs:
        print(f"Words differ ({len(word_diffs)}, first 8):\n")
        for target, ot, ct in word_diffs[:8]:
            print(f"- {target[:80]}: ours `{ot[:50]}`, Chrome `{ct[:50]}`")
        print()
    if text_diffs:
        print(f"Words that differ ({len(text_diffs)} places, first 10):\n")
        for ours_run, chrome_run in text_diffs[:10]:
            print(f"- ours `{ours_run[:70]}`, Chrome `{chrome_run[:70]}`")
        print()
