import {spawn} from 'node:child_process'; import fs from 'node:fs';
const sizes=process.env.SIZES.split(',').map(Number); const port=9340+Number(process.env.P||0);
const c=spawn('google-chrome',['--headless=new','--no-sandbox','--disable-gpu','--remote-debugging-port='+port,'--host-resolver-rules=MAP * ~NOTFOUND','about:blank'],{stdio:'ignore'});
await new Promise(r=>setTimeout(r,2500));
const page=(await (await fetch('http://127.0.0.1:'+port+'/json')).json()).find(t=>t.type==='page');
const ws=new WebSocket(page.webSocketDebuggerUrl); await new Promise(r=>ws.onopen=r);
let id=0; const pend={}; ws.onmessage=e=>{const m=JSON.parse(e.data); if(m.id&&pend[m.id]){pend[m.id](m);delete pend[m.id];}};
const send=(method,params={})=>new Promise(r=>{const i=++id;pend[i]=r;ws.send(JSON.stringify({id:i,method,params}));});
await send('Emulation.setDeviceMetricsOverride',{width:200,height:80,deviceScaleFactor:1,mobile:false});
fs.mkdirSync('png',{recursive:true});
for(const s of sizes){ for(let k=0x20;k<=0x7e;k++){
 const ch=String.fromCharCode(k); const esc=ch==='<'?'&lt;':ch==='&'?'&amp;':ch;
 if(fs.existsSync(`png/${s}-${k}.png`)) continue; const html=`<!doctype html><body style="margin:0"><p style="margin:0;font-size:${s}px;line-height:normal;white-space:pre;text-decoration:underline">H${esc}H</p>`;
 await send('Page.navigate',{url:'data:text/html,'+encodeURIComponent(html)}); await new Promise(r=>setTimeout(r,60));
 let r; for(let t=0;t<3&&!(r&&r.result);t++){ r=await send("Page.captureScreenshot",{format:"png",clip:{x:0,y:0,width:200,height:80,scale:1}}); if(!(r&&r.result)) await new Promise(q=>setTimeout(q,200)); } if(!(r&&r.result)) continue; fs.writeFileSync(`png/${s}-${k}.png`,Buffer.from(r.result.data,"base64"));
}}
c.kill(); process.exit(0);
