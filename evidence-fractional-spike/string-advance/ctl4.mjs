import {spawn} from 'node:child_process'; import fs from 'node:fs';
const c=spawn('google-chrome',['--headless=new','--no-sandbox','--disable-gpu','--remote-debugging-port=9374','--host-resolver-rules=MAP * ~NOTFOUND','about:blank'],{stdio:'ignore'});
await new Promise(r=>setTimeout(r,2500));
const page=(await (await fetch('http://127.0.0.1:9374/json')).json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise(r=>ws.onopen=r);
let id=0; const pend={}; ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&pend[m.id]){pend[m.id](m);delete pend[m.id];}};
const send=(method,params={})=>new Promise(r=>{const i=++id;pend[i]=r;ws.send(JSON.stringify({id:i,method,params}));});
await send('Emulation.setDeviceMetricsOverride',{width:1200,height:200,deviceScaleFactor:1,mobile:false});
const css=fs.readFileSync('LibEmb.css','utf8')+fs.readFileSync('DejaEmb.css','utf8');
const html=`<!doctype html><meta charset=utf-8><style>${css} body{margin:0} #t{position:absolute;white-space:pre;line-height:normal}</style><div id=t></div>`;
await send('Page.navigate',{url:'data:text/html;base64,'+Buffer.from(html).toString('base64')}); await new Promise(r=>setTimeout(r,800));
const strs=['H','HHHHHHHHHH','e','eeeeeeeeee','The quick brown fox jumps over the lazy dog','AVAVAVAVAV','Hello, world. 0123456789','iiiiiiiiiiiiiiiiiiii'];
const expr=`(async()=>{const out=[];const t=document.getElementById('t');
for(const fam of ['LibEmb','DejaEmb']){await document.fonts.load('16px '+fam);
for(const px of [13,13.5,13.99,14,14.4,16,16.5,17.6]) for(const s of ${JSON.stringify(strs)}){
 for(const kern of ['auto','none']){t.style.fontFamily=fam;t.style.fontSize=px+'px';t.style.fontKerning=kern;t.style.fontFeatureSettings=kern=='none'?'"liga" 0,"kern" 0':'normal';t.textContent=s;
 out.push({fam,px,s,kern,w:t.getBoundingClientRect().width});}}}
return JSON.stringify(out)})()`;
const r=await send('Runtime.evaluate',{expression:expr,awaitPromise:true,returnByValue:true});
fs.writeFileSync('chrome-string-widths.json',r.result.result.value);c.kill();process.exit(0);
