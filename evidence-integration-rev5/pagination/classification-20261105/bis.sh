. ~/.cargo/env
cd ~/cb/wt-bis
mapfile -t C < /tmp/pg/commits.txt
lo=10; hi=12
test_at(){ s=${C[$1]}; (cd ~/cb/repo && git fetch -q --filter=blob:none origin $s) ; git checkout -q --detach $s || return 9
 out=$(CARGO_TARGET_DIR=~/cb/tgt-z cargo test --release -p kobo-browser corpus_page_counts -- --include-ignored 2>&1)
 if echo "$out" | grep -q "test result: ok. 1 passed"; then echo pass; elif echo "$out" | grep -q "pages differs"; then echo fail; else echo build-or-other; fi; }
while [ $((hi-lo)) -gt 1 ]; do mid=$(((lo+hi)/2)); r=$(test_at $mid); echo "idx $mid ${C[$mid]:0:8} $r" >> /tmp/pg/bis.out
 case $r in pass) lo=$mid;; fail) hi=$mid;; *) echo "STOP at $mid: $r" >> /tmp/pg/bis.out; exit 1;; esac; done
echo "FIRST FAIL idx $hi ${C[$hi]}" >> /tmp/pg/bis.out
