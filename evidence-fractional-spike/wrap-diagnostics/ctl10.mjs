import {spawn} from 'node:child_process'; import fs from 'node:fs';
const c=spawn('google-chrome',['--headless=new','--no-sandbox','--disable-gpu','--remote-debugging-port=9380','--host-resolver-rules=MAP * ~NOTFOUND','--allow-file-access-from-files','about:blank'],{stdio:'ignore'});
await new Promise(r=>setTimeout(r,2500));
const page=(await (await fetch('http://127.0.0.1:9380/json')).json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise(r=>ws.onopen=r);
let id=0; const pend={}; ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&pend[m.id]){pend[m.id](m);delete pend[m.id];}};
const send=(method,params={})=>new Promise(r=>{const i=++id;pend[i]=r;ws.send(JSON.stringify({id:i,method,params}));});
await send('Page.navigate',{url:'file:///home/sandbox/cb/fctl/wrap.html'}); await new Promise(r=>setTimeout(r,800));
const text='The quick brown fox jumps over the lazy dog while seven wizards quietly boxed jumping zebras near the old stone bridge at 10:45 on 2026.';
const expr=`(async()=>{await document.fonts.load('16px SansEmb');const b=document.getElementById('b');b.style.fontFamily='SansEmb';b.style.fontSize='12px';b.style.width='160.109375px';
const words=${JSON.stringify(text)}.split(' ');b.innerHTML='';const spans=words.map((w,i)=>{const s=document.createElement('span');s.textContent=w;b.append(s);if(i<words.length-1)b.append(document.createTextNode(' '));return s});
const ws=spans.map(s=>({w:s.textContent,left:s.getBoundingClientRect().left,width:s.getBoundingClientRect().width,top:s.getBoundingClientRect().top}));
// isolated width of each word and of a single space and of 'a b'
const t=document.createElement('span');t.style.whiteSpace='pre';document.body.append(t);
const iso=words.map(w=>{t.textContent=w;return t.getBoundingClientRect().width});
t.textContent='a a';const aa=t.getBoundingClientRect().width;t.textContent='aa';const a2=t.getBoundingClientRect().width;
return JSON.stringify({ws,iso,space:aa-a2})})()`;
const r=await send('Runtime.evaluate',{expression:expr,awaitPromise:true,returnByValue:true});
fs.writeFileSync('chrome-flip-trace.json',r.result.result.value);c.kill();process.exit(0);
