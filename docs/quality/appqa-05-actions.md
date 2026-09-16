# APPQA-05: every visible action has a handler

Spec lines: "Typed action registry; build/test fails for emitted-but-unhandled
actions unless explicitly read-only" and "Every interactive control maps to a
handled action."

## What shipped

- `scripts/quality/check_action_handlers.py` audits all 44 catalog apps
  (apps/ + examples/) for emitted action names that no handler claims.
- `scripts/test_check_action_handlers.py` unit-tests the auditor's rules.
- habits fix: the Settings page emitted an info row with id `local` that
  nothing handled. The row is now a non-interactive `.facts()` entry
  (Storage / Stored on this reader), matching the pattern syncthing and
  needles already use, and the drive route walks Today -> Stats -> Settings
  and asserts the text, so the page is covered by the live journey.

## Result at the time of writing

44 apps audited; 42 clean; 2 flagged, both documented rather than silently
dismissed:

- kitchencard `ingredient`: the Ingredients rows carry an action id and no
  handler. The view only renders with server data, and there is no kitchen
  server fixture in the sandbox, so a fix could not be verified on the
  simulator. Candidate for the per-app audit group with a fixture.
- gallery: examples/ showcase whose demo widget ids (`button-one`,
  `bar-search`, `about-gallery`, ...) have no handlers. Open design
  question: leave the showcase inert by design, or make demo controls
  respond. Not changed unilaterally.

paperterm's `term.*` ids are consumed by the SDK terminal (confirmed in
crates/kobo-sdk/src/terminal.rs SPECIALS), so they are excluded via the
SDK-id table rather than flagged.

## Honest limits

- The audit is a static literal/prefix match. It cannot see dynamically
  computed ids, dispatch outside the known SDK cases, or whether a control
  is reachable. Every flagged row needs a human reading before it counts
  as a finding.
- The behavioral guard is the drive routes: APPQA-06/07 journeys tap
  controls and assert the screen changes afterwards; a dead control on a
  routed path fails the journey.
- The spec's typed action registry (a build-time failure for
  emitted-but-unhandled actions) is a larger SDK change and was not
  attempted; this audit is the interim enforcement, run in the local gate.
