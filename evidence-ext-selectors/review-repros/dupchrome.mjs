import {spawn} from 'node:child_process'; import fs from 'node:fs';
const c=spawn('google-chrome',['--headless=new','--no-sandbox','--disable-gpu','--remote-debugging-port=9362','--host-resolver-rules=MAP * ~NOTFOUND','--allow-file-access-from-files','about:blank'],{stdio:'ignore'});
await new Promise(r=>setTimeout(r,2500));
const page=(await (await fetch('http://127.0.0.1:9362/json')).json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise(r=>ws.onopen=r);
let id=0; const pend={}; ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&pend[m.id]){pend[m.id](m);delete pend[m.id];}};
const send=(method,params={})=>new Promise(r=>{const i=++id;pend[i]=r;ws.send(JSON.stringify({id:i,method,params}));});
for(let s=0;s<1;s++){
 await send('Page.navigate',{url:`file:///tmp/s3verify/duplicate.html`}); await new Promise(r=>setTimeout(r,1500));
 const r=await send('Runtime.evaluate',{expression:`JSON.stringify([...document.getElementsByTagName('*')].map(e=>{const c=getComputedStyle(e);return e.tagName.toLowerCase()+'\t'+c.color+'\t'+c.backgroundColor+'\t'+c.display}))`,returnByValue:true});
 fs.writeFileSync(`/tmp/s3verify/duplicate-chrome.json`,r.result.result.value);
}
c.kill(); await new Promise(r=>c.on('exit',r)); process.exit(0);
