import json,re,collections
tot=collections.Counter()
def rgb(x):
    m=re.match(r'rgba?\((\d+), (\d+), (\d+)(?:, ([\d.]+))?\)',x); r,g,b=map(int,m.groups()[:3]); a=m.group(4)
    return None if a=='0' else (r<<16)|(g<<8)|b
for s in range(6):
    ch=json.load(open(f'chrome-{s}.json')); en=[l for l in open(f'engine-{s}.txt').read().split('\n')[1:] if l]
    assert len(ch)==len(en),(len(ch),len(en)); bad=0
    for c,e in zip(ch,en):
        t,col,bg,disp=c.split('\t'); et,ecol,ebg,edisp=e.split('\t'); assert t==et
        okc=int(ecol)==rgb(col); okb=(None if ebg=='None' else int(ebg[5:-1]))==rgb(bg)
        tot['elements']+=1; tot['color_ok']+=okc; tot['bg_ok']+=okb
        if not(okc and okb):
            bad+=1
            if bad<6: print('MISMATCH',s,t,col,ecol,bg,ebg)
    print('doc',s,'mismatching elements',bad,'of',len(ch))
print(dict(tot))
