import sys, base64, io, subprocess, tempfile, os
from PIL import Image

page = sys.argv[1]
W = 800
rgba = open(f'/tmp/basic-{page}.rgba','rb').read()
H = len(rgba)//(W*4)
im = Image.frombytes('RGBA',(W,H),rgba).convert('RGB')
px = im.load()
uris = open(f'/tmp/basic-{page}.imguris').read().splitlines()
report=[]
# Pass 1: locate every marker box on the PRISTINE raster. Pasted photos
# contain near-black pixels that collide with the sentinel colors, so
# scanning after pasting contaminates later boxes. A marker box is a
# solid sentinel rectangle; sparse sentinel-colored text pixels are
# excluded by taking the row cluster with the most sentinel pixels and
# requiring it to be mostly solid.
boxes = {}
for idx in range(len(uris)):
    col = (1,(idx>>8)&0xff, idx&0xff)
    rows = {}
    for y in range(H):
        n = 0
        xa, xb = W, -1
        for x in range(W):
            if px[x,y]==col:
                n += 1
                if x<xa: xa=x
                if x>xb: xb=x
        if n:
            rows[y] = (n, xa, xb)
    if not rows:
        continue
    clusters = []
    for y in sorted(rows):
        if clusters and y-clusters[-1][-1]<=2: clusters[-1].append(y)
        else: clusters.append([y])
    best = None
    for c in clusters:
        total = sum(rows[y][0] for y in c)
        xa = min(rows[y][1] for y in c); xb = max(rows[y][2] for y in c)
        area = (xb-xa+1)*len(c)
        fill = total/area if area else 0
        score = total if fill >= 0.5 else 0
        if best is None or score > best[0]:
            best = (score, c[0], c[-1], xa, xb, fill)
    if best and best[0] > 0:
        _, y0, y1, x0, x1, _fill = best
        boxes[idx] = (x0, y0, x1, y1)

# Pass 2: clear and paste.
for idx, uri in enumerate(uris):
    if idx not in boxes:
        report.append((idx,'no-sentinel-pixels',uri[:40])); continue
    x0,y0,x1,y1 = boxes[idx]
    rh = y1-y0+1; rw = x1-x0+1
    head,_,payload = uri.partition(',')
    data = base64.b64decode(payload)
    if 'svg' in head:
        img=None
        try:
            import cairosvg
            # Render at 2x the target box so the later LANCZOS pass
            # only ever downscales; intrinsic-size renders (often ~100px)
            # upscaled 5-8x are the blur the user sees.
            png = cairosvg.svg2png(bytestring=data, background_color='white', output_width=max(rw*2, 64))
            img = Image.open(io.BytesIO(png)).convert('RGB')
            dec='svg(cairosvg)'
        except Exception as e1:
            try:
                with tempfile.NamedTemporaryFile(suffix='.svg',delete=False) as f:
                    f.write(data); sp=f.name
                pp = sp+'.png'
                subprocess.run(['convert','-background','white',sp,pp],check=True,capture_output=True,timeout=60)
                img = Image.open(pp).convert('RGB')
                os.unlink(sp); os.unlink(pp)
                dec='svg(imagemagick)'
            except Exception as e:
                dec=f'svg-FAILED:{e1}/{e}'
    else:
        try:
            img = Image.open(io.BytesIO(data)).convert('RGB')
            dec=head.split(';')[0].replace('data:','')
        except Exception as e:
            img=None; dec=f'decode-FAILED:{e}'
    # clear sentinel region
    for y in range(y0,y1+1):
        for x in range(x0,x1+1):
            px[x,y]=(255,255,255)
    if img is None:
        report.append((idx,f'{rw}x{rh}@{x0},{y0}',dec)); continue
    iw,ih = img.size
    tw = rw
    th = max(1, round(ih*tw/iw))
    if th > rh:  # cap to reserved height, recentre the crop
        tw = max(1, round(iw*rh/ih))
        if tw > rw:
            tw = rw
        th = rh
    img = img.resize((tw,th), Image.LANCZOS)
    im.paste(img,(x0+(rw-tw)//2,y0))
    report.append((idx,f'{rw}x{rh}@{x0},{y0} pasted {tw}x{th}',dec))
im.save(f'/tmp/basic-{page}-blit.png')
for r in report: print(r)
print('saved', f'/tmp/basic-{page}-blit.png', im.size)
