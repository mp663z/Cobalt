#!/usr/bin/env python3
"""Compare diag-tool model rows with Chrome wrap rows (chrome-wrap-rows.json from wrap-control).
Reports per face: identical/different, and for each flip the model's rejected-word overflow at the
first differing break, with a STRUCTURAL marker when the model overflow at that break exceeds 1 px
(a width-bound cannot explain it). Output carries approximate=true semantics; no bound is claimed."""
import json,sys
rows=json.load(open(sys.argv[1]))['out']
model={}
for fam,path in [x.split('=') for x in sys.argv[2:]]:
    for l in open(path):
        r=json.loads(l); model[(fam,round(r['px'],2),round(r['width'],2))]=r
st={}
for r in rows:
    m=model[(r['fam'],round(r['px'],2),round(r['W'],2))]
    s=st.setdefault(r['fam'],dict(n=0,same=0,flips=[]))
    s['n']+=1
    if m['breaks']==r['brk']: s['same']+=1; continue
    # first differing break position
    mb,cb=m['breaks'],r['brk']; i=0
    while i<min(len(mb),len(cb)) and mb[i]==cb[i]: i+=1
    cw=cb[i] if i<len(cb) else 10**9; mw=mb[i] if i<len(mb) else 10**9
    w=min(cw,mw)  # the first word on which the two decisions differ
    ov=abs(m['decision_slack'][w]) if w<len(m['decision_slack']) else None
    where='first differing decision at word %d: %s'%(w,'chrome breaks, model fits' if cw<mw else 'model breaks, chrome fits')
    s['flips'].append(dict(px=r['px'],W=r['W'],chrome=cb,model=mb,first_diff_overflow=ov,where=where,structural=(ov is None or ov>1.0)))
print('APPROXIMATE model comparison (measurement only)')
for f,s in st.items():
    print(f,'cases',s['n'],'identical',s['same'],'flips',len(s['flips']))
    for x in s['flips']:
        print('  px',x['px'],'W',x['W'],'|model decision margin| px',None if x['first_diff_overflow'] is None else round(x['first_diff_overflow'],4),'STRUCTURAL' if x['structural'] else '', '|',x['where'])
