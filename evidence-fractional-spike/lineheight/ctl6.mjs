import {spawn} from 'node:child_process'; import fs from 'node:fs';
const c=spawn('google-chrome',['--headless=new','--no-sandbox','--disable-gpu','--remote-debugging-port=9376','--host-resolver-rules=MAP * ~NOTFOUND','--allow-file-access-from-files','about:blank'],{stdio:'ignore'});
await new Promise(r=>setTimeout(r,2500));
const page=(await (await fetch('http://127.0.0.1:9376/json')).json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise(r=>ws.onopen=r);
let id=0; const pend={}; ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&pend[m.id]){pend[m.id](m);delete pend[m.id];}};
const send=(method,params={})=>new Promise(r=>{const i=++id;pend[i]=r;ws.send(JSON.stringify({id:i,method,params}));});
const css=['LibEmb','DejaEmb'].map(n=>fs.readFileSync(n+'.css','utf8')).join('');
fs.writeFileSync('lh.html',`<!doctype html><meta charset=utf-8><style>${css} body{margin:0}</style><div id=b style="white-space:pre"></div>`);
await send('Page.navigate',{url:'file:///home/sandbox/cb/fctl/lh.html'}); await new Promise(r=>setTimeout(r,800));
const expr=`(async()=>{const out=[];const b=document.getElementById('b');
for(const fam of ['LibEmb','DejaEmb']){await document.fonts.load('16px '+fam);
for(const px of [13,13.5,14.4,16.5,17.6]) for(const lh of ['normal','1','1.2','1.5','1.33','150%','133%','20px','21.5px','1.4em','0']){
 b.style.fontFamily=fam;b.style.fontSize=px+'px';b.style.lineHeight=lh;
 b.textContent='Hxg';const h1=b.getBoundingClientRect().height;
 b.textContent='Hxg\\nHxg\\nHxg';const h3=b.getBoundingClientRect().height;
 out.push({fam,px,lh,computed:getComputedStyle(b).lineHeight,h1,h3})}}
return JSON.stringify(out)})()`;
const r=await send('Runtime.evaluate',{expression:expr,awaitPromise:true,returnByValue:true});
if(!r.result.result.value){console.log(JSON.stringify(r.result));process.exit(1)}
fs.writeFileSync('chrome-lineheight-rows.json',r.result.result.value);c.kill();process.exit(0);
