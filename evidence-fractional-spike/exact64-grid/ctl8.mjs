import {spawn} from 'node:child_process'; import fs from 'node:fs';
const c=spawn('google-chrome',['--headless=new','--no-sandbox','--disable-gpu','--remote-debugging-port=9378','--host-resolver-rules=MAP * ~NOTFOUND','--allow-file-access-from-files','about:blank'],{stdio:'ignore'});
await new Promise(r=>setTimeout(r,2500));
const page=(await (await fetch('http://127.0.0.1:9378/json')).json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise(r=>ws.onopen=r);
let id=0; const pend={}; ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&pend[m.id]){pend[m.id](m);delete pend[m.id];}};
const send=(method,params={})=>new Promise(r=>{const i=++id;pend[i]=r;ws.send(JSON.stringify({id:i,method,params}));});
fs.writeFileSync('grid2.html',`<!doctype html><meta charset=utf-8><style>${['LibEmb','DejaEmb','SansEmb'].map(n=>fs.readFileSync(n+'.css','utf8')).join('')} body{margin:0}</style><div id=b style="white-space:pre"></div>`);
await send('Page.navigate',{url:'file:///home/sandbox/cb/fctl/grid2.html'}); await new Promise(r=>setTimeout(r,800));
const sizes=[];for(let k=8*64;k<=40*64;k+=7)sizes.push(k/64); // exact 1/64 multiples, stride 7/64 (covers all phases)
const expr=`(async()=>{const out=[];const b=document.getElementById('b');
const S='The quick brown fox jumps over the lazy dog 0123456789';
for(const fam of ['LibEmb','DejaEmb','SansEmb']){await document.fonts.load('16px '+fam);b.style.fontFamily=fam;
for(const px of ${JSON.stringify(sizes)}){b.style.fontSize=px+'px';b.style.lineHeight='1.5';b.textContent='H';const lh15=b.getBoundingClientRect().height;
 b.style.lineHeight='normal';const hn=b.getBoundingClientRect().height;
 b.textContent=S;const rg=document.createRange();rg.selectNodeContents(b);b.style.fontKerning='none';const w=rg.getBoundingClientRect().width;b.style.fontKerning='auto';
 out.push({fam,px,hn,lh15,w})}}
return JSON.stringify(out)})()`;
const r=await send('Runtime.evaluate',{expression:expr,awaitPromise:true,returnByValue:true});
if(!r.result.result.value){console.log(JSON.stringify(r.result));process.exit(1)}
fs.writeFileSync('chrome-exact64-grid.json',r.result.result.value);c.kill();process.exit(0);
