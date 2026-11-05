import {spawn} from 'node:child_process'; import fs from 'node:fs';
const c=spawn('google-chrome',['--headless=new','--no-sandbox','--disable-gpu','--remote-debugging-port=9377','--host-resolver-rules=MAP * ~NOTFOUND','--allow-file-access-from-files','about:blank'],{stdio:'ignore'});
await new Promise(r=>setTimeout(r,2500));
const page=(await (await fetch('http://127.0.0.1:9377/json')).json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise(r=>ws.onopen=r);
let id=0; const pend={}; ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&pend[m.id]){pend[m.id](m);delete pend[m.id];}};
const send=(method,params={})=>new Promise(r=>{const i=++id;pend[i]=r;ws.send(JSON.stringify({id:i,method,params}));});
await send('Page.navigate',{url:'file:///home/sandbox/cb/fctl/lh.html'}); await new Promise(r=>setTimeout(r,800));
const expr=`(async()=>{const out=[];const b=document.getElementById('b');await document.fonts.load('16px LibEmb');b.style.fontFamily='LibEmb';
for(let i=0;i<=40;i++){const px=+(14.38+i*0.001).toFixed(3);b.style.fontSize=px+'px';
 b.style.lineHeight='1';b.textContent='H';const hLH=b.getBoundingClientRect().height;
 b.style.lineHeight='20px';b.textContent='HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH';
 const w=b.firstChild?document.createRange():null;let wd=0;if(w){w.selectNodeContents(b);wd=w.getBoundingClientRect().width}
 out.push({px,cs:getComputedStyle(b).fontSize,lh1:hLH,w100:wd})}
return JSON.stringify(out)})()`;
const r=await send('Runtime.evaluate',{expression:expr,awaitPromise:true,returnByValue:true});
fs.writeFileSync('chrome-fontsize-fine.json',r.result.result.value);c.kill();process.exit(0);
