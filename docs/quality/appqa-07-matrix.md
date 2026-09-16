# APPQA-07 leg matrix

Per-app status of the five app-quality gate legs: install, sample,
own-data, task and offline-reopen. Rendered by
`scripts/quality/render_appqa07_matrix.py` from a
`scripts/check-apps-sim.py` sweep; the sweep reports
`state_written`, `reopened` and the route per app. This rendering
is from the sweep at `3dbfe9a` (44/44 passed, 44/44 reopened offline).

Leg meanings:

- **install** - the real signed package installed and launched through
  the Store path. UNVERIFIED in the sandbox for every app: per-app
  real-binary install belongs to the publish pipeline and attended
  device proof (`kobo beta-store-smoke`). The install machinery itself
  - catalog, signature, update, downgrade, launch-failure and state
  preservation - is covered by the kobo-cli and kobod test suites.
- **sample** - the app exercised with sample or fixture content, where
  the app has such a path.
- **own-data** - the app shown working with data from the reader’s own
  action: a journey write measured by the sweep, or own content pushed
  through the app’s real CLI path. Apps whose manifest declares no user
  data are n/a. A fetched network cache is not own data. Apps whose
  data starts on a server (feed, catalog, library, pairing) keep the
  gap until a local fixture exercises them; the first-run screen is
  their honest floor, not evidence.
- **task** - a committed simulator route exercises the core journey.
- **offline-reopen** - after the route, the same state relaunches with
  no network and renders a first screen.

| App | install | sample | own-data | task | offline-reopen |
| --- | --- | --- | --- | --- | --- |
| arxiv | UNVERIFIED | none evidenced | gap | yes - apps/arxiv/drive.kobo | yes |
| audiobook | UNVERIFIED | none evidenced | gap | yes - examples/audiobook/drive.kobo | yes |
| backgammon | UNVERIFIED | none evidenced | yes - journey wrote backgammon-autosave-v4 | yes - apps/backgammon/drive.kobo | yes |
| brief | UNVERIFIED | none evidenced | gap | yes - examples/brief/drive.kobo | yes |
| calibre-web | UNVERIFIED | none evidenced | gap | yes - apps/calibre-web/drive.kobo | yes |
| chat | UNVERIFIED | none evidenced | n/a (manifest declares no user data) | yes - examples/chat/drive.kobo | yes |
| crossword | UNVERIFIED | none evidenced | yes - journey wrote crossword-state-v1 | yes - apps/crossword/drive.kobo | yes |
| deck | UNVERIFIED | seeded config pushed through the real `kobo deck` path | yes - own content pushed via the real CLI path, shown by the route | yes - apps/deck/drive.kobo | yes |
| fanshelf | UNVERIFIED | FANSHELF_DEMO synthetic library exercised by the route | gap | yes - apps/fanshelf/drive.kobo | yes |
| fieldbook | UNVERIFIED | none evidenced | yes - journey wrote sightings | yes - apps/fieldbook/drive.kobo | yes |
| flashcards | UNVERIFIED | none evidenced | gap | yes - apps/flashcards/drive.kobo | yes |
| frame | UNVERIFIED | seeded photo pushed through the real `kobo frame push` path | yes - own content pushed via the real CLI path, shown by the route | yes - apps/frame/drive.kobo | yes |
| gallery | UNVERIFIED | none evidenced | n/a (manifest declares no user data) | yes - examples/gallery/drive.txt | yes |
| grimoire | UNVERIFIED | none evidenced | yes - journey wrote grimoire-state-v2 | yes - apps/grimoire/drive.kobo | yes |
| gutenbird | UNVERIFIED | none evidenced | gap - only the fetched catalog cache was written | yes - examples/gutenbird/drive.kobo | yes |
| habits | UNVERIFIED | none evidenced | yes - journey wrote habits-v1 | yes - apps/habits/drive.kobo | yes |
| hn | UNVERIFIED | none evidenced | gap | yes - examples/hn/drive.txt | yes |
| homepanel | UNVERIFIED | none evidenced | gap | yes - apps/homepanel/drive.kobo | yes |
| inkling | UNVERIFIED | none evidenced | yes - journey wrote inkling-state-v1 | yes - apps/inkling/drive.kobo | yes |
| kitchencard | UNVERIFIED | none evidenced | yes - journey wrote tonight | yes - apps/kitchencard/drive.kobo | yes |
| lichess | UNVERIFIED | none evidenced | gap | yes - apps/lichess/drive.kobo | yes |
| logicpack | UNVERIFIED | none evidenced | yes - journey wrote logicpack-state-v1 | yes - apps/logicpack/drive.kobo | yes |
| magnet | UNVERIFIED | none evidenced | yes - journey wrote magnet-sweep-v1 | yes - examples/magnet/drive.txt | yes |
| morse | UNVERIFIED | none evidenced | n/a (manifest declares no user data) | yes - apps/morse/drive.kobo | yes |
| musicstand | UNVERIFIED | none evidenced | yes - journey wrote musicstand-state | yes - apps/musicstand/drive.kobo | yes |
| needles | UNVERIFIED | none evidenced | yes - journey wrote counter-state-v1 | yes - apps/needles/drive.kobo | yes |
| nonograms | UNVERIFIED | none evidenced | yes - journey wrote progress-picture-house-v1 | yes - apps/nonograms/drive.kobo | yes |
| panels | UNVERIFIED | none evidenced | gap | yes - apps/panels/drive.kobo | yes |
| paperterm | UNVERIFIED | none evidenced | gap | yes - apps/paperterm/drive.kobo | yes |
| parlor | UNVERIFIED | none evidenced | yes - journey wrote parlor-autosave-v1 | yes - apps/parlor/drive.kobo | yes |
| parser | UNVERIFIED | none evidenced | gap | yes - apps/parser/drive.kobo | yes |
| post | UNVERIFIED | none evidenced | gap | yes - apps/post/drive.kobo | yes |
| pubquiz | UNVERIFIED | none evidenced | gap | yes - apps/pubquiz/drive.kobo | yes |
| readlater | UNVERIFIED | none evidenced | yes - journey wrote config | yes - apps/readlater/drive.kobo | yes |
| rss | UNVERIFIED | none evidenced | gap | yes - examples/rss/drive.kobo | yes |
| rss-miniflux | UNVERIFIED | none evidenced | gap | yes - apps/rss-miniflux/drive.kobo | yes |
| sidekick | UNVERIFIED | none evidenced | gap | yes - examples/sidekick/drive.txt | yes |
| sudoku | UNVERIFIED | none evidenced | yes - journey wrote .game.writing, game | yes - apps/sudoku/drive.kobo | yes |
| syncthing | UNVERIFIED | none evidenced | yes - journey wrote sync-config | yes - apps/syncthing/drive.kobo | yes |
| tictactoe | UNVERIFIED | none evidenced | yes - journey wrote tictactoe-v1 | yes - examples/tictactoe/drive.txt | yes |
| todo | UNVERIFIED | none evidenced | yes - journey wrote items | yes - examples/todo/drive.txt | yes |
| vault | UNVERIFIED | fixture notes pushed through the real `kobo vault push` path | yes - own content pushed via the real CLI path, shown by the route | yes - apps/vault/drive.kobo | yes |
| verses | UNVERIFIED | none evidenced | yes - journey wrote settings | yes - apps/verses/drive.kobo | yes |
| zotero-reader | UNVERIFIED | none evidenced | gap | launch only (APPQA-13) | yes |

## Open legs

- install: all 44 apps, per the method above.
- own-data gaps (manifest declares user data, no own-data evidence):
  arxiv, audiobook, brief, calibre-web, fanshelf, flashcards, gutenbird, hn, homepanel, lichess, panels, paperterm, parser, post, pubquiz, rss, rss-miniflux, sidekick, zotero-reader.
- sample: no sample-path evidence for 40 apps;
  most have no sample or demo path at all, which is a product gap the
  per-app audits should judge, not only an evidence gap.

