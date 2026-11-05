import {spawn} from 'node:child_process'; import fs from 'node:fs';
const c=spawn('google-chrome',['--headless=new','--no-sandbox','--disable-gpu','--remote-debugging-port=9390','--host-resolver-rules=MAP * ~NOTFOUND','--allow-file-access-from-files','about:blank'],{stdio:'ignore'});
await new Promise(r=>setTimeout(r,2500));
const page=(await (await fetch('http://127.0.0.1:9390/json')).json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise(r=>ws.onopen=r);
let id=0; const pend={}; ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&pend[m.id]){pend[m.id](m);delete pend[m.id];}};
const send=(method,params={})=>new Promise(r=>{const i=++id;pend[i]=r;ws.send(JSON.stringify({id:i,method,params}));});
fs.writeFileSync('wrap.html',`<!doctype html><meta charset=utf-8><style>${['LibEmb','DejaEmb','SansEmb'].map(n=>fs.readFileSync(n+'.css','utf8')).join('')} body{margin:0} #b{font-kerning:none;font-feature-settings:"kern" 0,"liga" 0}</style><div id=b></div>`);
await send('Page.navigate',{url:'file:///home/sandbox/cb/fctl/wrap.html'}); await new Promise(r=>setTimeout(r,800));
const text='The quick brown fox jumps over the lazy dog while seven wizards quietly boxed jumping zebras near the old stone bridge at 10:45 on 2026.';
const sizes=[];for(let k=12*64;k<=24*64;k+=47)sizes.push(k/64);
const widths=[];for(let w=120;w<=420;w+=13.37)widths.push(+(Math.round(w*64)/64).toFixed(6));
const expr=`(async()=>{const out=[];const b=document.getElementById('b');const words=${JSON.stringify(text)}.split(' ');
for(const fam of ['LibEmb','DejaEmb','SansEmb']){await document.fonts.load('16px '+fam);b.style.fontFamily=fam;
for(const px of ${JSON.stringify(sizes)}){b.style.fontSize=px+'px';
 for(const W of ${JSON.stringify(widths)}){b.style.width=W+'px';
  b.textContent=words.join(' ');const tn=b.firstChild;let off=0;const tops=words.map(w=>{const r=document.createRange();r.setStart(tn,off);r.setEnd(tn,off+w.length);off+=w.length+1;return Math.round(r.getBoundingClientRect().top*64)});
  const brk=[];for(let i=1;i<tops.length;i++)if(tops[i]!==tops[i-1])brk.push(i);
  out.push({fam,px,W,brk})}}}
return JSON.stringify({words,out})})()`;
const r=await send('Runtime.evaluate',{expression:expr,awaitPromise:true,returnByValue:true});
if(!r.result.result.value){console.log(JSON.stringify(r.result));process.exit(1)}
fs.writeFileSync('chrome-wrap-rows-singlenode.json',r.result.result.value);c.kill();process.exit(0);
