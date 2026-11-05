import {spawn} from 'node:child_process'; import fs from 'node:fs';
const c=spawn('google-chrome',['--headless=new','--no-sandbox','--disable-gpu','--remote-debugging-port=9373','--host-resolver-rules=MAP * ~NOTFOUND','about:blank'],{stdio:'ignore'});
await new Promise(r=>setTimeout(r,2500));
const page=(await (await fetch('http://127.0.0.1:9373/json')).json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise(r=>ws.onopen=r);
let id=0; const pend={}; ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&pend[m.id]){pend[m.id](m);delete pend[m.id];}};
const send=(method,params={})=>new Promise(r=>{const i=++id;pend[i]=r;ws.send(JSON.stringify({id:i,method,params}));});
await send('Emulation.setDeviceMetricsOverride',{width:400,height:80,deviceScaleFactor:1,mobile:false});
await send('DOM.enable'); await send('CSS.enable');
const css=fs.readFileSync('LibEmb.css','utf8')+fs.readFileSync('DejaEmb.css','utf8');
const html=`<!doctype html><meta charset=utf-8><style>${css} html,body{margin:0;background:#fff} #t{position:absolute;top:10px;color:#000;white-space:nowrap;line-height:normal;-webkit-font-smoothing:auto}</style><div id=t></div>`;
await send('Page.navigate',{url:'data:text/html;base64,'+Buffer.from(html).toString('base64')}); await new Promise(r=>setTimeout(r,1000));
fs.mkdirSync('raster',{recursive:true});
const cases=[]; const proof=[];
for(const fam of ['LibEmb','DejaEmb']) for(const px of [13,13.5,14,14.4,16,16.5,17.6]) for(const [txt,tag] of [['H','H'],['HHHHHHHHHH','H10'],['e','e']]) for(const left of [0,0.25,0.5]){
  await send('Runtime.evaluate',{expression:`(async()=>{await document.fonts.load('16px ${fam}');const t=document.getElementById('t');t.style.fontFamily='${fam}';t.style.fontSize='${px}px';t.style.left='${left}px';t.textContent='${txt}';return 1})()`,awaitPromise:true});
  const shot=await send('Page.captureScreenshot',{format:'png'});
  const name=`raster/${fam}_${px}_${tag}_${left}.png`; fs.writeFileSync(name,Buffer.from(shot.result.data,'base64')); cases.push({fam,px,tag,left,file:name});
  if(left===0&&tag==='H'){const doc=await send('DOM.getDocument',{depth:-1});const n=await send('DOM.querySelector',{nodeId:doc.result.root.nodeId,selector:'#t'});const pf=await send('CSS.getPlatformFontsForNode',{nodeId:n.result.nodeId});proof.push({fam,px,fonts:pf.result.fonts});}
}
fs.writeFileSync('raster-cases.json',JSON.stringify(cases)); fs.writeFileSync('raster-platform-fonts.json',JSON.stringify(proof));
c.kill(); process.exit(0);
