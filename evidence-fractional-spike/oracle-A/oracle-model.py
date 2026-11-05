import json,sys
T='The quick brown fox jumps over the lazy dog while seven wizards quietly boxed jumping zebras near the old stone bridge at 10:45 on 2026.'
words=T.split(' ')
ends=[];o=0
for w in words: ends.append(o+len(w)); o+=len(w)+1   # end offset (exclusive)
starts=[e-len(w) for e,w in zip(ends,words)]
ch={ (r['fam'],r['px']):r['pos'] for r in json.load(open('chrome-charpos.json'))['out']}
def P(pos,k): return 0.0 if k==0 else pos[k-1]
def model(pos,W):
    brk=[];ls=0;cur=0  # word index of line start
    i=0
    while i<len(words):
        # line starts at word i; fits words j while P(end_j)<=P(start_i)+W (always take first word)
        base=P(pos,starts[i]);j=i
        while j+1<len(words) and P(pos,ends[j+1])<=base+W+1e-9: j+=1
        if j+1<len(words): brk.append(j+1)
        i=j+1
    return brk
for fn in ['chrome-wrap-rows-singlenode.json','chrome-wrap-rows.json']:
    rows=json.load(open(fn))['out'];tot=0;ok=0;bad=[]
    for r in rows:
        k=(r['fam'],r['px']);W=r['W']
        m=model(ch[k],W);tot+=1
        b=r['brk']
        if m==b: ok+=1
        else: bad.append((r['fam'],r['px'],W))
    print(fn,'oracle-model identical',ok,'/',tot,'flips',len(bad),bad[:8])
