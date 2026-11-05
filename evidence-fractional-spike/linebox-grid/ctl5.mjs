import {spawn} from 'node:child_process'; import fs from 'node:fs';
const c=spawn('google-chrome',['--headless=new','--no-sandbox','--disable-gpu','--remote-debugging-port=9375','--host-resolver-rules=MAP * ~NOTFOUND','--allow-file-access-from-files','about:blank'],{stdio:'ignore'});
await new Promise(r=>setTimeout(r,2500));
const page=(await (await fetch('http://127.0.0.1:9375/json')).json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise(r=>ws.onopen=r);
let id=0; const pend={}; ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&pend[m.id]){pend[m.id](m);delete pend[m.id];}};
const send=(method,params={})=>new Promise(r=>{const i=++id;pend[i]=r;ws.send(JSON.stringify({id:i,method,params}));});
const css=['LibEmb','DejaEmb','SansEmb'].map(n=>fs.readFileSync(n+'.css','utf8')).join('');
const html=`<!doctype html><meta charset=utf-8><style>${css} body{margin:0} #b{white-space:pre;line-height:normal}</style><div id=b><span id=p style="display:inline-block;width:1px;height:0;vertical-align:baseline"></span><span id=t>Hxg</span></div>`;
fs.writeFileSync('grid.html',html);await send('Page.navigate',{url:'file:///home/sandbox/cb/fctl/grid.html'}); await new Promise(r=>setTimeout(r,800));
const sizes=[];for(let s=8;s<=40.001;s+=0.25)sizes.push(+s.toFixed(2));
const expr=`(async()=>{const out=[];const b=document.getElementById('b'),p=document.getElementById('p');
for(const fam of ['LibEmb','DejaEmb','SansEmb']){await document.fonts.load('16px '+fam);b.style.fontFamily=fam;
for(const px of ${JSON.stringify(sizes)}){b.style.fontSize=px+'px';const rb=b.getBoundingClientRect(),rp=p.getBoundingClientRect();out.push({fam,px,blockH:rb.height,asc:rp.top-rb.top})}}
return JSON.stringify(out)})()`;
const r=await send('Runtime.evaluate',{expression:expr,awaitPromise:true,returnByValue:true});
if(!r.result.result.value){console.log(JSON.stringify(r.result));process.exit(1)};fs.writeFileSync('chrome-linebox-grid.json',r.result.result.value);c.kill();process.exit(0);
