import {spawn} from 'node:child_process'; import fs from 'node:fs';
const c=spawn('google-chrome',['--headless=new','--no-sandbox','--disable-gpu','--remote-debugging-port=9371','--host-resolver-rules=MAP * ~NOTFOUND','--allow-file-access-from-files','about:blank'],{stdio:'ignore'});
await new Promise(r=>setTimeout(r,2500));
const page=(await (await fetch('http://127.0.0.1:9371/json')).json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise(r=>ws.onopen=r);
let id=0; const pend={}; ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&pend[m.id]){pend[m.id](m);delete pend[m.id];}};
const send=(method,params={})=>new Promise(r=>{const i=++id;pend[i]=r;ws.send(JSON.stringify({id:i,method,params}));});
await send('Emulation.setDeviceMetricsOverride',{width:1280,height:800,deviceScaleFactor:1,mobile:false});
await send('Page.navigate',{url:'file:///home/sandbox/cb/fctl/page.html'}); await new Promise(r=>setTimeout(r,1500));
const expr=`(async()=>{
 const out=[];
 for(const fam of ['LibCtl','DejaCtl','NoSuchFontXyz']){
  await document.fonts.load('16px '+fam);
  const loaded=[...document.fonts].filter(f=>f.family==fam&&f.status=='loaded').length;
  for(const px of [13,13.5,13.99,14,14.4,16,16.5,17.6]){
   const d=document.getElementById('m'); d.innerHTML='';
   const blk=document.createElement('div'); blk.style.cssText='font-family:'+fam+';font-size:'+px+'px;line-height:normal';
   const h=document.createElement('span'); h.textContent='H'; const e=document.createElement('span'); e.textContent='e';
   const hh=document.createElement('span'); hh.textContent='HHHHHHHHHH';
   blk.append(h,e,hh); d.append(blk);
   const rb=blk.getBoundingClientRect(),rh=h.getBoundingClientRect(),re=e.getBoundingClientRect(),rr=hh.getBoundingClientRect();
   out.push({fam,loaded,px,computed:getComputedStyle(blk).fontSize,blockH:rb.height,Hw:rh.width,ew:re.width,H10w:rr.width,Hx:rh.left,ex:re.left,top:rb.top});
  }}
 return JSON.stringify(out)})()`;
const r=await send('Runtime.evaluate',{expression:expr,awaitPromise:true,returnByValue:true});
fs.writeFileSync('chrome-fractional-rows.json',r.result.result.value);
const o=JSON.parse(r.result.result.value); for(const x of o) console.log(x.fam,x.loaded,x.px,x.computed,x.blockH,x.Hw,x.ew,x.H10w);
const v=await send('Browser.getVersion'); fs.writeFileSync('chrome-version.txt',v.result.product+'\n');
c.kill(); process.exit(0);
