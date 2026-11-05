. ~/.cargo/env
cd ~/cb/wt-bis
F=crates/kobo-web-layout/src/lib.rs
run(){ n=$1; CARGO_TARGET_DIR=~/cb/tgt-z timeout 100 cargo test --release -p kobo-browser corpus_page_counts -- --include-ignored > /tmp/pg/hunk-$n.log 2>&1; git diff -- $F > /tmp/pg/hunk-$n.diff; echo "$n: $(grep -c 'pages differs' /tmp/pg/hunk-$n.log) differs; $(grep -E '^error(\[|:)' /tmp/pg/hunk-$n.log|head -1|cut -c1-80) rse: $(grep -A2 '^rust-std-option.pages differs' /tmp/pg/hunk-$n.log|grep actual|grep -o '[0-9]*\"$'|head -1) py: $(grep -A2 '^python-tutorial-intro.pages differs' /tmp/pg/hunk-$n.log|grep actual|grep -o '[0-9]*\"$'|head -1)" >> /tmp/pg/hunk.out; git checkout -q -- $F; }
git checkout -q -f --detach ce1742e5
python3 - <<'P'
import re
F='crates/kobo-web-layout/src/lib.rs'; s=open(F).read()
a=s.replace("""    kobo_ui::with_reading_scale(metrics.text_scale, || {
        append_at_reading_scale(builder, pieces, metrics)
    })""","    append_at_reading_scale(builder, pieces, metrics)"); assert a!=s; open('/tmp/pg/v1.rs','w').write(a)
b=s.replace("Piece::Preformatted(text) => preformatted(builder, text),","Piece::Preformatted(text) => builder.text(text),"); assert b!=s; open('/tmp/pg/v2.rs','w').write(b)
c=s.replace("Piece::Table { rows, .. } => browser_table(builder, rows, metrics),","Piece::Table { rows, .. } => builder.table(rows.clone(), Vec::new()),"); assert c!=s; open('/tmp/pg/v3.rs','w').write(c)
d=s.replace("if points.is_empty() && !page.is_empty() {","if points.is_empty() {"); d=d.replace("""            Piece::Preformatted(
                text[..at]
                    .strip_suffix('\\n')
                    .unwrap_or(&text[..at])
                    .to_owned(),
            ),""","            Piece::Preformatted(text[..at].trim_end_matches('\\n').to_owned()),"); assert d!=s and d.count("strip_suffix('\\n')")==0; open('/tmp/pg/v4.rs','w').write(d)
P
for v in v1 v2 v3 v4; do cp /tmp/pg/$v.rs $F; run $v; done; echo DONE >> /tmp/pg/hunk.out
