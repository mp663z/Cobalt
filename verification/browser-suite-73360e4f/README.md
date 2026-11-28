# Browser and document checks at73360e4f

Document crate: cargo test --locked -p kobo-web-document, rc0. 150 test-result
groups,1431passed,0failed,18ignored. This includes doc-test empty result group.

Browser app: cargo test --locked -p kobo-browser --no-run built one test binary.
All276 names from its --list ran using --exact NAME --test-threads=1.273passed,
0failed,3ignored, zero omissions/duplicates. Runs span4bounded batches; this is
not one uninterrupted cargo test invocation. Installed-font list-fit regression
and navigation destination tests ran in this complete name set.

Temporary executable paths in browser-names.json are provenance, not portable
execution instructions. Logs retain every test result. No hardware, completeCSS,
POST or broad proof claim follows from these host tests.
