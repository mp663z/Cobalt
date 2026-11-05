import {spawn} from 'node:child_process'; import fs from 'node:fs';
const c=spawn('google-chrome',['--headless=new','--no-sandbox','--disable-gpu','--remote-debugging-port=9337','--host-resolver-rules=MAP * ~NOTFOUND','about:blank'],{stdio:'ignore'});
await new Promise(r=>setTimeout(r,2500));
const page=(await (await fetch('http://127.0.0.1:9337/json')).json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise(r=>ws.onopen=r);
let id=0; const pend={}; ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&pend[m.id]){pend[m.id](m);delete pend[m.id];}};
const send=(method,params={})=>new Promise(r=>{const i=++id;pend[i]=r;ws.send(JSON.stringify({id:i,method,params}));});
await send('Emulation.setDeviceMetricsOverride',{width:600,height:200,deviceScaleFactor:1,mobile:false});
const FAM=process.env.FAM; const out=[];
for(const lh of ['normal','2']) for(let s=8;s<=57;s++){
 const html=`<!doctype html><body style="margin:0"><p id=p style="margin:0;font-family:'${FAM}';font-size:${s}px;line-height:${lh==='2'?(s*2)+'px':'normal'}"><span id=a>H</span><span id=m style="display:inline-block;width:1px;height:0"></span></p>`;
 await send('Page.navigate',{url:'data:text/html,'+encodeURIComponent(html)}); await new Promise(r=>setTimeout(r,120));
 const r=await send('Runtime.evaluate',{expression:'JSON.stringify([document.getElementById("m").getBoundingClientRect().bottom,document.getElementById("p").getBoundingClientRect().height,document.getElementById("a").getBoundingClientRect().width])',returnByValue:true});
 const [b,h,w]=JSON.parse(r.result.result.value); out.push({lh,s,baseline:b,height:h,Hwidth:w});
}
fs.writeFileSync('bl-fam-'+process.env.TAG+'.json',JSON.stringify(out)); c.kill(); process.exit(0);
