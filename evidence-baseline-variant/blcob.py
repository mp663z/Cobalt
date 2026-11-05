import subprocess, json, numpy as np
BA='/home/sandbox/cb/tgt-t/release/cobalt-text-preview'
faces={'libserif':('/usr/share/fonts/truetype/liberation2/LiberationSerif-Regular.ttf','bl-chrome.json'),
'dejavusans':('/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf','bl-fam-dejavusans.json'),
'libsans':('/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf','bl-fam-libsans.json'),
'libmono':('/usr/share/fonts/truetype/liberation2/LiberationMono-Regular.ttf','bl-fam-libmono.json'),
'dejavuserif':('/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf','bl-fam-dejavuserif.json')}
def ppm(p):
    q=open(p,'rb').read().split(b'\n',3); w,h=map(int,q[1].split()); return np.frombuffer(q[3],np.uint8).reshape(h,w,3).astype(int)
def base(F,s,lh,flag):
    css="line-height:%dpx"%(2*s) if lh=='2' else "line-height:normal"
    outs=[]
    for ch in 'HE':
        open('bt.html','w').write(f"<!doctype html><style>body{{margin:0}}p{{margin:0;font-size:{s}px;{css}}}</style><p>{ch}</p>")
        r=subprocess.run([BA,'bt.html','bt.ppm','200','160',F,'--blocks']+(['--verified-normal-bytes'] if flag else []),capture_output=True,text=True)
        if r.returncode: return None
        k=ppm('bt.ppm'); rows=np.where((k.sum(axis=2)<384).any(axis=1))[0]; outs.append(int(rows.max())+1)
    return outs[0] if outs[0]==outs[1] else ('disagree',outs)
res={}
for t,(F,j) in faces.items():
    ch={(x['lh'],x['s']):x['baseline'] for x in json.load(open(j))}
    for lh in ['normal','2']:
        for flag in [0,1]:
            bad=[];ref=0;ok=0;dis=0
            for s in range(8,58):
                b=base(F,s,lh,flag)
                if b is None: ref+=1
                elif isinstance(b,tuple): dis+=1
                elif b!=ch[(lh,s)]: bad.append((s,b,ch[(lh,s)]))
                else: ok+=1
            res[f'{t}|{lh}|{"variant" if flag else "default"}']={'match':ok,'mismatch':len(bad),'refused':ref,'HE_disagree':dis,'mismatch_list':bad[:6]}
            print(t,lh,'variant' if flag else 'default','match',ok,'mismatch',len(bad),'refused',ref,'HEdis',dis,bad[:4])
json.dump(res,open('bl-variant-table.json','w'),indent=1)
