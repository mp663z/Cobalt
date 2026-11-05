# Review record, integrated rev5 (UNREVIEWED branch label kept; this is a candidate-level record only)

Source identity reviewed: head 083601a6de5387fb04f4fa17ef85ca2b94b84613, tree 2d5939241639fd8a3faeff62fb236ce029f9d670, branch agent/recon-clean4x-integ-S3-S2-UNREVIEWED-20261105 (= r1 70bc8f26 + S3 reviewed source 0c2a8cb + S2 f9244328 merged, text-decoration parse opt-in via parse_style_tree_with_text_decoration and parse_style_tree_with_supply_and_text_decoration, preview underline path via parse_styled).

Provenance of this record: the independent reviewer's final message of 2026-11-05 22:31:58 IST, as relayed to the build agent by the main agent. The copy available to the build agent is an excerpt cut off partway through the WPT item; points below are only those present in that excerpt or in the earlier reviewer messages quoted in the same relay. The full verbatim review is not reproduced here.

Reviewer verdict (relayed): candidate-level PASS of every reviewer-executable check, with fresh controls recomputed by the reviewer from bytes. One known red (release-profile pagination corpus_page_counts) is documented as pre-existing and not caused by the candidate; the page-count contract is unresolved. The reviewer stated the claim scope remains limited.
Reviewer-reported items in the excerpt: evidence packet sha256 4a6db7b8f7861855a53a4932102d1594aa0b03b71f7dd422361ee4e3dc7885bd matched; the pinned WPT runner (tools/cobalt-wpt/score.py, masked-bold-v2-2026-11-01) was re-run on the reviewer's own build and Chrome 156.0.8078.47, the reviewer scoreboard.json sha256 6dcbe56d150268d4c80968ab9683c8b874f1cbd3e4c5caae351a964df5d44f4e was byte-identical to the build agent's, stdout byte-identical. (The reviewer message names the packet file evidence-rev5-153f52a0.tgz; the uploaded asset is named evidence-rev5-ff7b0c20.tgz. The sha256 is the identity; the name difference is unexplained.)

Claim scope, stated by the build agent (not a reviewer statement):
- NOT all green. NOT a full-workspace result: selected packages kobo-web-document, kobo-web-layout, kobo-browser (806 passed, 1 failed, 2 ignored) plus preview crate tests.
- Known red: apps/browser corpus_tests::corpus_page_counts_on_every_reader_and_text_size. Fails at 4dc074c7 (the commit that last wrote its expected files), r1, S3 and the integration. CI runs cargo test in the debug profile, where this test is ignored. Stale-versus-environment cause is unresolved. No BLESS, no expectation change. Engineering track stays open.
- WPT headline 2/11 (textless), masked 6/11 measurement-only, real-page gate 0/5 last-known and not re-run. No render-completion claim, no canonical advance, no adoption, no PR.
- Underline: 7 PPMs byte-identical to the committed S2 PPMs; c40 refuses. Chrome not re-captured.
- Controls were run on synthetic and frozen-Chrome data as described in the evidence directory; Chrome computed-style identity for text-decoration was not built.
