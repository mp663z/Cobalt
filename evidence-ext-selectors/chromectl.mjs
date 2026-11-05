import {spawn} from 'node:child_process'; import fs from 'node:fs';
const pages={synth:'file:///home/sandbox/cb/gate/sel/synth.html'}; for(const p of ['blog','docs','gov','news-npr']) pages[p]='file:///home/sandbox/cb/gate/corpus/'+p+'.complete.html';
const c=spawn('google-chrome',['--headless=new','--no-sandbox','--disable-gpu','--remote-debugging-port=9361','--host-resolver-rules=MAP * ~NOTFOUND','--allow-file-access-from-files','about:blank'],{stdio:'ignore'});
await new Promise(r=>setTimeout(r,2500));
const page=(await (await fetch('http://127.0.0.1:9361/json')).json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise(r=>ws.onopen=r);
let id=0; const pend={}; ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&pend[m.id]){pend[m.id](m);delete pend[m.id];}};
const send=(method,params={})=>new Promise(r=>{const i=++id;pend[i]=r;ws.send(JSON.stringify({id:i,method,params}));});
await send('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false});
for(const [p,url] of Object.entries(pages)){
 await send('Emulation.setScriptExecutionDisabled',{value:true});
 await send('Page.navigate',{url}); await new Promise(r=>setTimeout(r,3000));
 await send('Emulation.setScriptExecutionDisabled',{value:false});
 const list=fs.readFileSync(`list-${p}.txt`,'utf8').split('\n').filter((x,i,a)=>!(i===a.length-1&&x===''));
 const r=await send('Runtime.evaluate',{expression:`(()=>{const all=[...document.getElementsByTagName('*')];const idx=new Map(all.map((e,i)=>[e,i]));return JSON.stringify({compat:document.compatMode,n:all.length,res:${JSON.stringify(list)}.map(s=>{try{return [...document.querySelectorAll(s)].map(e=>idx.get(e)).join(',')}catch(e){return 'INVALID'}})})})()`,returnByValue:true,timeout:60000});
 const o=JSON.parse(r.result.result.value); fs.writeFileSync(`chrome-${p}.tsv`,list.map((s,i)=>s+'\t'+o.res[i]).join('\n')+'\n'); console.log(p,o.compat,o.n,list.length);
}
c.kill(); process.exit(0);
