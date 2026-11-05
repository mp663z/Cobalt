import {spawn} from 'node:child_process'; import fs from 'node:fs';
const c=spawn('google-chrome',['--headless=new','--no-sandbox','--disable-gpu','--remote-debugging-port=9372','--host-resolver-rules=MAP * ~NOTFOUND','--allow-file-access-from-files','about:blank'],{stdio:'ignore'});
await new Promise(r=>setTimeout(r,2500));
const page=(await (await fetch('http://127.0.0.1:9372/json')).json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise(r=>ws.onopen=r);
let id=0; const pend={}; const events=[]; ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&pend[m.id]){pend[m.id](m);delete pend[m.id];} else events.push(m)};
const send=(method,params={})=>new Promise(r=>{const i=++id;pend[i]=r;ws.send(JSON.stringify({id:i,method,params}));});
await send('Emulation.setDeviceMetricsOverride',{width:640,height:300,deviceScaleFactor:1,mobile:false});
await send('Network.enable'); await send('DOM.enable'); await send('CSS.enable'); await send('Page.enable');
await send('Page.navigate',{url:'file:///home/sandbox/cb/fctl/page.html'}); await new Promise(r=>setTimeout(r,1500));
const sizes=[13,13.5,13.99,14,14.4,16,16.5,17.6,20,24];
const expr=`(async()=>{const out=[];
 for(const fam of ['LibCtl','DejaCtl']){await document.fonts.load('16px '+fam);
 for(const px of ${JSON.stringify(sizes)}){
  const d=document.getElementById('m'); d.innerHTML='';
  const blk=document.createElement('div'); blk.id='t'; blk.style.cssText='font-family:'+fam+';font-size:'+px+'px;line-height:normal';
  const t=document.createElement('span'); t.textContent='Hxg';
  const p=document.createElement('span'); p.style.cssText='display:inline-block;width:1px;height:0;vertical-align:baseline';
  blk.append(p,t); d.append(blk);
  const rb=blk.getBoundingClientRect(), rp=p.getBoundingClientRect(), rt=t.getBoundingClientRect();
  out.push({fam,px,blockH:rb.height,asc_to_baseline:rp.top-rb.top,below_baseline:rb.bottom-rp.top,spanH:rt.height,spanTop:rt.top-rb.top});
 }}return JSON.stringify(out)})()`;
const r=await send('Runtime.evaluate',{expression:expr,awaitPromise:true,returnByValue:true});
fs.writeFileSync('chrome-baseline-rows.json',r.result.result.value);
// platform font proof
const doc=await send('DOM.getDocument',{depth:-1});
const proof=[];
for(const fam of ['LibCtl','DejaCtl','NoSuchFontXyz']){
 await send('Runtime.evaluate',{expression:`(()=>{const d=document.getElementById('m');d.innerHTML='<div id=t style="font-family:${fam};font-size:16px">Hxg</div>'})()`});
 const doc=await send('DOM.getDocument',{depth:-1}); const n=await send('DOM.querySelector',{nodeId:doc.result.root.nodeId,selector:'#t'});
 const pf=await send('CSS.getPlatformFontsForNode',{nodeId:n.result.nodeId}); proof.push({fam,fonts:pf.result.fonts});
 if(fam==='LibCtl'||fam==='DejaCtl'){ const shot=await send('Page.captureScreenshot',{format:'png'}); fs.writeFileSync('shot-'+fam+'.png',Buffer.from(shot.result.data,'base64')); }
}
fs.writeFileSync('chrome-platform-fonts.json',JSON.stringify(proof,null,1));
fs.writeFileSync('chrome-network-events.json',JSON.stringify(events.filter(e=>/Network\.(requestWillBeSent|responseReceived|loadingFailed|loadingFinished)/.test(e.method)).map(e=>({m:e.method,u:e.params.request?.url||e.params.response?.url,s:e.params.response?.status,err:e.params.errorText})),null,1));
c.kill(); process.exit(0);
