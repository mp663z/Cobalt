import {spawn} from 'node:child_process'; import fs from 'node:fs';
const c=spawn('google-chrome',['--headless=new','--no-sandbox','--disable-gpu','--remote-debugging-port=9381','--host-resolver-rules=MAP * ~NOTFOUND','--allow-file-access-from-files','about:blank'],{stdio:'ignore'});
await new Promise(r=>setTimeout(r,2500));
const page=(await (await fetch('http://127.0.0.1:9381/json')).json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise(r=>ws.onopen=r);
let id=0; const pend={}; ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&pend[m.id]){pend[m.id](m);delete pend[m.id];}};
const send=(method,params={})=>new Promise(r=>{const i=++id;pend[i]=r;ws.send(JSON.stringify({id:i,method,params}));});
await send('Page.navigate',{url:'file:///home/sandbox/cb/fctl/wrap.html'}); await new Promise(r=>setTimeout(r,800));
const expr=`(async()=>{await document.fonts.load('16px SansEmb');const b=document.getElementById('b');b.style.fontFamily='SansEmb';b.style.width='5000px';b.style.whiteSpace='pre';
const out=[];for(let k=12*64;k<=15*64;k++){const px=k/64;b.style.fontSize=px+'px';b.textContent='H'.repeat(1000);const r=document.createRange();r.selectNodeContents(b);out.push({k,px,w:r.getBoundingClientRect().width})}
return JSON.stringify(out)})()`;
const r=await send('Runtime.evaluate',{expression:expr,awaitPromise:true,returnByValue:true});
fs.writeFileSync('chrome-effsize.json',r.result.result.value);c.kill();process.exit(0);
