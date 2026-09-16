#!/usr/bin/env python3
"""Cross-check emitted action names against on_action handlers per app.

Heuristic, not a proof: it matches string literals and format! prefixes in
screen builders against literals and prefixes matched in the app's handler.
It cannot see dynamically computed ids, SDK-internal dispatch beyond the
known cases below, or reachability - the live drive routes prove behavior;
this audit catches affordances nothing claims. Every flagged row needs a
human reading before it counts as a finding.
"""
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[2]

# Builder methods whose first string literal (or two) name actions.
NAMED = [
    'primary_button', 'primary_button_with_state', 'secondary_button',
    'top_bar_action', 'top_bar_glyph', 'bottom_action', 'bottom_action_marked',
    'section_link', 'page_turns', 'menu_action', 'tile',
]
# Builder methods taking tuples whose first element names an action.
TUPLED = ['buttons', 'grid', 'grid_with_selection', 'chips', 'tile_grid',
          'rows', 'rows_with_menu', 'rows_with_trailing']
# Handler-side helpers that turn an index into a dynamic id.
HANDLER_HELPERS = ['indexed', 'choice']


def strip_test_modules(text):
    out = []
    cursor = 0
    for m in re.finditer(r'#\[cfg\(test\)\][^{]*\{', text):
        out.append(text[cursor:m.start()])
        cursor = matching_brace(text, m.end() - 1) + 1
    out.append(text[cursor:])
    return '\n'.join(out)


def matching_paren(text, open_at):
    depth = 0
    for i in range(open_at, len(text)):
        if text[i] == '(':
            depth += 1
        elif text[i] == ')':
            depth -= 1
            if depth == 0:
                return i
    return len(text)


def emitted(text):
    """(literal | prefix, source) pairs the builders put on screens."""
    found = []
    for name in NAMED:
        for m in re.finditer(r'\.' + name + r'\(\s*"([^"]+)"', text):
            found.append(('lit', m.group(1)))
    for name in TUPLED:
        for m in re.finditer(r'\.' + name + r'\(', text):
            region = text[m.start():matching_paren(text, m.end() - 1)]
            for lit in re.finditer(r'\(\s*"([a-z0-9][a-z0-9.\-]*)"', region):
                found.append(('lit', lit.group(1)))
            for dyn in re.finditer(r'format!\(\s*"([a-z0-9][a-z0-9.\-]*)\{', region):
                found.append(('prefix', dyn.group(1)))
    return found


def matching_brace(text, open_at):
    depth = 0
    for i in range(open_at, len(text)):
        if text[i] == '{':
            depth += 1
        elif text[i] == '}':
            depth -= 1
            if depth == 0:
                return i
    return len(text)


def handler_body(text):
    m = re.search(r'fn on_action\([^{]*\{', text)
    if not m:
        return ''
    start = m.end() - 1
    return text[start:matching_brace(text, start)]


def handled(text):
    body = handler_body(text)
    # Literals claimed anywhere: handlers dispatch through helper functions,
    # so scoping to on_action alone undercounts.
    lits = set(re.findall(r'action_id\(\s*"([^"]+)"', text))
    # Loops like `for (name, command) in [("look", ...)]` + `action_id(name)`
    # dispatch tuple first elements; only trust tuples inside on_action.
    if 'action_id(name)' in body or 'action_id(&name)' in body:
        lits.update(re.findall(r'\(\s*"([a-z0-9][a-z0-9.\-]*)"', body))
    # Closures like `let is = |name| action == action_id(name)`; handlers
    # split across helper methods each define their own.
    if re.search(r'let is = \|', text):
        lits.update(re.findall(r'\bis\(\s*"([^"]+)"', text))
    prefixes = set(re.findall(r'format!\(\s*"([a-z0-9][a-z0-9.\-]*)\{', text))
    for helper in HANDLER_HELPERS:
        prefixes.update(re.findall(helper + r'\(\s*action\s*,\s*"([^"]+)"', text))
    default_pt = re.search(r'fn on_page_turn\([^{]*\{\s*\}', text)
    delegates = {
        'keyboard': '.press(' in body,
        'page_turn': 'fn on_page_turn' in text and default_pt is None,
        'back': 'ActionId::BACK' in body,
    }
    return lits, prefixes, delegates


def sdk_ids():
    """Action names the SDK itself consumes (terminal keys, keyboard)."""
    text = '\n'.join(p.read_text()
                      for p in (ROOT / 'crates/kobo-sdk/src').glob('*.rs'))
    return set(re.findall(r'"(term\.[a-z]+|kb\.[a-z.]+)"', text))


SDK = sdk_ids()


def audit_app(directory):
    text = strip_test_modules(
        '\n'.join(p.read_text() for p in sorted(directory.rglob('*.rs'))))
    lits, prefixes, delegates = handled(text)
    unseen = []
    review = []
    for kind, name in emitted(text):
        if kind == 'lit' and (name in lits or any(name.startswith(p) for p in prefixes)):
            continue
        if kind == 'prefix' and name in prefixes:
            continue
        if kind == 'lit' and name in SDK and delegates['keyboard']:
            continue
        if kind == 'lit' and delegates['page_turn']:
            continue
        if kind == 'lit' and delegates['keyboard']:
            review.append(f'{name} ({kind}, key handler present)')
            continue
        unseen.append(f'{name} ({kind})')
    return {
        'app': directory.name,
        'emitted': len(emitted(text)),
        'handled_literals': len(lits),
        'handled_prefixes': sorted(prefixes),
        'delegates': [k for k, v in delegates.items() if v],
        'unclaimed': sorted(set(unseen)),
        'review': sorted(set(review)),
    }


def main():
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else None
    apps = sorted(p.parent for group in ('apps', 'examples')
                  for p in (ROOT / group).glob('*/cobalt-app.json'))
    report = [audit_app(directory) for directory in apps]
    flagged = [r for r in report if r['unclaimed']]
    review = [r for r in report if r['review']]
    print(f'{len(report)} apps audited; {len(flagged)} unclaimed, {len(review)} to review')
    for r in flagged:
        print(f"  UNCLAIMED {r['app']}: {', '.join(r['unclaimed'][:6])}")
    for r in review:
        print(f"  REVIEW {r['app']}: {', '.join(r['review'][:6])}")
    if out:
        out.write_text(json.dumps(report, indent=2) + '\n')
        print(f'report: {out}')


if __name__ == '__main__':
    main()
