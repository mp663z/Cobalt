. ~/.cargo/env
cd ~/cb/wt-bis
for s in ce1742e5 d8a7a1fb c8b57be5 5c2f419b 25909280 c63ad175 083601a6; do
 git checkout -q --detach $s 2>/dev/null || { echo "$s checkout-fail" >> /tmp/pg/seq.out; continue; }
 CARGO_TARGET_DIR=~/cb/tgt-z cargo test --release -p kobo-browser corpus_page_counts -- --include-ignored > /tmp/pg/seq-$s.log 2>&1
 echo "$s $(git log -1 --format=%s|cut -c1-60) | $(grep -c 'pages differs' /tmp/pg/seq-$s.log) differs | $(grep -E '^test result|error: could not' /tmp/pg/seq-$s.log|head -1) | rse: $(grep -A2 '^rust-std-option.pages differs' /tmp/pg/seq-$s.log|grep actual|grep -o '[0-9]*\"$'|head -1)" >> /tmp/pg/seq.out
done; echo DONE >> /tmp/pg/seq.out
