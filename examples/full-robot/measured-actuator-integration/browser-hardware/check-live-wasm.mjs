// Explicit hardware integration check using the same frozen Rust/WASM walking runtime.
// This is headless verification; it does not claim rendered-browser validation.
import fs from 'node:fs';import {performance} from 'node:perf_hooks';
const base=new URL('./walking-ui/',import.meta.url),u='http://127.0.0.1:4180';
const m=await import(new URL('sim_web.js',base));await m.default({module_or_path:fs.readFileSync(new URL('sim_web_bg.wasm',base))});
const d=JSON.parse(fs.readFileSync(new URL('data/robot-prelift-control.json',base)));
const sim=new m.EnvironmentSimulation(JSON.stringify(d.scene),JSON.stringify(d.config),JSON.stringify(d.task),0);
const metadata=JSON.parse(sim.metadata()),inputs=JSON.parse(sim.inputs()),actions=inputs.map(i=>i.initial),events=[];
let frame=JSON.parse(sim.frame()),seq=0,active=false;
const html=await(await fetch(u)).text(),token=html.match(/const token='([a-f0-9]+)'/)[1],client=crypto.randomUUID();
const api=async(path,body)=>{const r=await fetch(u+path,{method:body===undefined?'GET':'POST',headers:{'X-Control-Token':token,'X-Client-Id':client,'Content-Type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});const v=await r.json();if(!r.ok)throw Error(JSON.stringify(v));return v;};
const sample=()=>({sequence:seq,time_s:frame.time_s,targets_rad:Object.fromEntries(metadata.coordinate_names.map((n,i)=>[n,frame.servo_targets_rad[i]]))});
const bindings=metadata.coordinate_names.slice(0,3).map((coordinate,i)=>({coordinate,motor_id:10+i,polarity:1}));
let result,error=null;
try{
 await api('/live/open',{bindings,amplitude:0.03,source:'Headless check of robot-prelift-control frozen WASM; browser rendering not verified',initial:sample()});active=true;
 const origin=performance.now();for(let i=0;i<350;i++){
  actions[3]=i<200?0.00125:i<300?-0.00125:0;
  frame=JSON.parse(sim.step(new Float64Array(actions)));if(frame.error)throw Error(frame.error);seq++;
  await api('/live/sample',sample());events.push({sequence:seq,time_s:frame.time_s,wall_s:(performance.now()-origin)/1000,actions:[...actions]});
  const wait=origin+(i+1)*20-performance.now();if(wait>0)await new Promise(r=>setTimeout(r,wait));
 }
}catch(e){error=e.message;}finally{
 if(active)await api('/stop',{}).catch(()=>{});
 for(let i=0;i<50;i++){result=await api('/status');if(!result.active)break;await new Promise(r=>setTimeout(r,100));}
 fs.writeFileSync(new URL('live-wasm-check.json',import.meta.url),JSON.stringify({scope:'Headless same-WASM live control and hardware bridge check; not a browser UI test',error,events,result},null,2));
 fs.writeFileSync(new URL('live-wasm-check.recording.json',import.meta.url),sim.recording());sim.free();
}
console.log(JSON.stringify({error,run:result?.run,samples:result?.samples?.length,result:result?.result},null,2));
if(error||!result?.result?.result?.stop_verified||result.samples.length<100)process.exitCode=1;
