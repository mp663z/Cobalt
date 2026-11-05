import collections
for p in ['synth','blog','docs','gov','news-npr']:
    R=[l.rstrip('\n').rsplit('\t',1) for l in open(f'rust-{p}.tsv')]; C=[l.rstrip('\n').rsplit('\t',1) for l in open(f'chrome-{p}.tsv')]
    assert len(R)==len(C); cnt=collections.Counter(); bad=[]
    for r,c in zip(R,C):
        assert r[0]==c[0]; rv=r[1] if len(r)>1 else ''; cv=c[1] if len(c)>1 else ''
        if rv in('REFUSED','UNRESOLVED'): cnt['refused: chrome-invalid' if cv=='INVALID' else 'refused: chrome-valid (coverage gap)']+=1
        elif cv=='INVALID': cnt['WRONG_ACCEPT']+=1; bad.append((r[0],rv,cv))
        elif rv==cv: cnt['accepted, identical element set, non-empty' if rv else 'accepted, identical element set, empty']+=1
        else: cnt['MISMATCH']+=1; bad.append((r[0],rv[:60],cv[:60]))
    print(p,'selectors',len(R),dict(cnt)); [print('  BAD',b) for b in bad]
