// Bounded timestep experiment in the same browser Rust environment; keeps all motors and clocks.
import fs from'node:fs';import path from'node:path';import {chromium} from'playwright';
const base='examples/full-robot/measured-actuator-integration/browser-control-400hz',out=base+'/verification';
const recipe=Object.fromEntries(['scene','config','task'].map(k=>[k,JSON.parse(fs.readFileSync(`${base}/${k}.json`))]));
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
const page=await browser.newPage();await page.goto('http://127.0.0.1:4182/catalog.json');
const duration=Number(process.env.DURATION_S||1);
const profiles=[['reference',1,false],['coarse-4',4,true],['coarse-8',8,true]];
for(const [profile,factor,probes] of profiles.filter(p=>!process.env.PROFILES||process.env.PROFILES.split(',').includes(p[0]))){
 const name=(process.env.OUTPUT_PREFIX||'')+profile;
 const cfg=structuredClone(recipe);cfg.config.steps=Math.round(duration/cfg.config.step_s);cfg.config.step_s*=factor;cfg.config.steps/=factor;cfg.config.report_every/=factor;
 if(probes)Object.assign(cfg.config.implicit,{linearized_jacobian_probes:true,reuse_exact_probe_base:true,extrapolate_velocity_seed:true});
 fs.writeFileSync(`${out}/${name}.config.json`,JSON.stringify(cfg.config));
 const result=await page.evaluate(async ({cfg,duration})=>{
  const worker=new Worker('/worker.js',{type:'module'});let id=0;const pending=new Map();worker.onmessage=({data})=>{const p=pending.get(data.id);if(!p)return;pending.delete(data.id);data.error?p.reject(Error(data.error)):p.resolve(data.result);};
  const call=m=>new Promise((resolve,reject)=>{m.id=++id;pending.set(id,{resolve,reject});worker.postMessage(m);});
  const frames=[],wall=[];let metadata,inputs,error;try{
   const loaded=await call({type:'load',...cfg});metadata=loaded.metadata;inputs=loaded.inputs;frames.push(loaded.frame);
   const action=inputs.map(c=>c.initial),forward=inputs.findIndex(c=>c.name==='command.forward_speed'),seq=inputs.findIndex(c=>c.name==='command.packet_sequence');action[forward]=.1;
   for(let i=0;i<Math.round(duration/cfg.task.period_s);i++){action[seq]=i+1;const start=performance.now();const frame=await call({type:'step',action});wall.push((performance.now()-start)/1000);frames.push(frame);if(frame.error||frame.done){error=frame.error;break;}}
  }catch(e){error=e.message;}finally{worker.terminate();}
  return {metadata,inputs,frames,wall_s:wall.reduce((s,v)=>s+v,0),transition_wall_s:wall,error};
 },{cfg,duration});
 fs.writeFileSync(`${out}/${name}.browser.json`,JSON.stringify(result));console.log(name,JSON.stringify({sim_s:result.frames.at(-1)?.time_s,wall_s:result.wall_s,error:result.error}));
}
await browser.close();
