import subprocess, numpy as np, json, html
from PIL import Image
B='/home/sandbox/cb/tgt-s/release/cobalt-text-preview'; F='/usr/share/fonts/truetype/liberation2/LiberationSerif-Regular.ttf'
def ppm(p):
    q=open(p,'rb').read().split(b'\n',3); w,h=map(int,q[1].split()); return np.frombuffer(q[3],np.uint8).reshape(h,w,3).astype(int)
tab=json.load(open('../deco-table.json')); out=[]
for s in [12,14,16,20,24,32]:
  t=tab[str(s)]; rows=list(range(t['base']+t['off'],t['base']+t['off']+t['th']))
  for k in range(0x20,0x7f):
    c=chr(k)
    open('t.html','w').write(f'<!doctype html><style>body{{margin:0}}p{{margin:0;font-size:{s}px;line-height:normal;text-decoration:underline}}</style><p>H{html.escape(c)}H</p>')
    r=subprocess.run([B,'t.html','t.ppm','200','80',F,'--blocks','--underline-chrome156-liberation'],capture_output=True,text=True)
    if r.returncode: continue
    cr=np.array(Image.open(f'png/{s}-{k}.png').convert('RGB')).astype(int)[:80,:200]; o=ppm('t.ppm')
    d=np.abs(cr-o).sum(axis=2)>0
    out.append((s,c,int(d[rows].sum())))
bad=[x for x in out if x[2]>4]
print(len(out),'accepted; underline-row diff>4:',bad)
import collections; print(collections.Counter(x[2] for x in out))
