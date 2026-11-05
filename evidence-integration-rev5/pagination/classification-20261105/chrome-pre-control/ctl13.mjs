import {spawn} from 'node:child_process'; import fs from 'node:fs';
const c=spawn('google-chrome',['--headless=new','--no-sandbox','--disable-gpu','--remote-debugging-port=9393','--host-resolver-rules=MAP * ~NOTFOUND','--allow-file-access-from-files','about:blank'],{stdio:'ignore'});
await new Promise(r=>setTimeout(r,2500));
const page=(await (await fetch('http://127.0.0.1:9393/json')).json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise(r=>ws.onopen=r);
let id=0; const pend={}; ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&pend[m.id]){pend[m.id](m);delete pend[m.id];}};
const send=(method,params={})=>new Promise(r=>{const i=++id;pend[i]=r;ws.send(JSON.stringify({id:i,method,params}));});

const code="fn main() {\n    let v = vec![1, 2, 3];\n    for x in &v {\n        println!(\"{x}\");\n    }\n\n    let very_long = \"a long unbroken string literal that is certainly wider than any panel at any text scale ................................\";\n}";
fs.writeFileSync('pre.html',`<!doctype html><meta charset=utf-8><style>${fs.readFileSync('DejaEmb.css','utf8')} body{margin:0} pre{margin:0;font-family:DejaEmb;font-size:16px;width:400px;font-kerning:none}</style><pre id=a></pre><pre id=b></pre>`);
await send('Page.navigate',{url:'file:///home/sandbox/cb/fctl/pre.html'}); await new Promise(r=>setTimeout(r,800));
const expr=`(async()=>{await document.fonts.load('16px DejaEmb');const code=${JSON.stringify(code)};const res={};
for(const [id,ws] of [['a','pre'],['b','pre-wrap'],['c','pre-wrap']]){}
const out=[];
for(const ws of ['pre','pre-wrap','break-spaces']){const p=document.getElementById('a');p.style.whiteSpace=ws;p.textContent=code;const tn=p.firstChild;
 const cs=getComputedStyle(p);const lines=[];let off=0;
 for(let i=0;i<code.length;i++){const r=document.createRange();r.setStart(tn,i);r.setEnd(tn,i+1);const q=r.getClientRects()[0];if(!q)continue;const k=Math.round(q.top*64)/64;if(!lines.length||lines[lines.length-1].top!==k)lines.push({top:k,left:q.left,first:JSON.stringify(code.slice(i,i+12)),i});}
 out.push({ws,lh:cs.lineHeight,fs:cs.fontSize,pre_h:p.getBoundingClientRect().height,scrollW:p.scrollWidth,clientW:p.clientWidth,lines});}
return JSON.stringify(out)})()`;
const r=await send('Runtime.evaluate',{expression:expr,awaitPromise:true,returnByValue:true});
if(!r.result.result.value){console.log(JSON.stringify(r.result));process.exit(1)}
fs.writeFileSync('chrome-pre-control.json',r.result.result.value);c.kill();process.exit(0);
