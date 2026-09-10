// Exercise the shared evaluator and checkpoint lifecycle in an isolated worker.
import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {spawn} from 'node:child_process';
import {chromium} from 'playwright';
const [bundle,casePath,reportPath,capturePath]=process.argv.slice(2);
assert(capturePath,'usage: experiment.mjs isolated-bundle native-case report fresh-browser-capture');
const native=JSON.parse(await readFile(casePath));assert(native.passed);
const server=spawn(process.execPath,['web/serve-viewer.mjs',bundle,'0']);
const url=await new Promise((resolve,reject)=>{server.stdout.on('data',chunk=>{const m=String(chunk).match(/http:\/\/127.0.0.1:\d+/);if(m)resolve(m[0]);});server.once('error',reject);server.once('exit',code=>reject(Error('server exit '+code)));});
let browser;
try{
 browser=await chromium.launch({headless:true,...(process.env.CHROME_EXECUTABLE?{executablePath:process.env.CHROME_EXECUTABLE}:{})});
 const page=await browser.newPage();await page.goto(url+'/OPEN.txt');
 await page.exposeFunction('reportProgress',v=>console.log(JSON.stringify(v)));
 const result=await page.evaluate(async ({experiment,proposal,split})=>{
  const worker=new Worker('/worker.js',{type:'module'});let id=0;const pending=new Map();
  worker.onmessage=({data})=>{const p=pending.get(data.id);if(!p)return;pending.delete(data.id);clearTimeout(p.timer);data.error?p.reject(Error(data.error)):p.resolve(data.result);};
  worker.onerror=e=>{for(const p of pending.values()){clearTimeout(p.timer);p.reject(Error(e.message));}pending.clear();};
  const rpc=message=>new Promise((resolve,reject)=>{const key=++id;pending.set(key,{resolve,reject,timer:setTimeout(()=>{pending.delete(key);reject(Error('experiment RPC timeout'));},60000)});worker.postMessage({...message,id:key});});
  const stable=f=>{f=structuredClone(f);delete f.stepping_wall_s;return JSON.stringify(f);};
  let ticks=0;const timer=setInterval(()=>ticks++,10);
  try{
   const bound=await rpc({type:'experiment_bind',spec:experiment.spec});
   if(bound.context_id!==experiment.context_id)throw Error('native/browser experiment identity differs');
   const frames=[await rpc({type:'experiment_start',experiment,proposal})];
   const robotInput=(await rpc({type:'experiment_metadata'})).environment.robot_input;
   const robotInspection=robotInput?.input?await rpc({type:'inspect_robot',document:experiment.spec.scene}):null;
   let invalidReceiptPreserved=true;
   if(experiment.spec.scene.robot_input?.overrides.length){
    for(const mutation of ['version','value']){
     const document=structuredClone(experiment.spec.scene);
     if(mutation==='version')document.robot_input.version=99;
     else document.robot_input.overrides[0].value='mismatched receipt';
     let rejected=false;try{await rpc({type:'inspect_robot',document});}catch{rejected=true;}
     invalidReceiptPreserved&&=rejected&&stable(await rpc({type:'experiment_frame'}))===stable(frames[0]);
    }
   }
   let materialized=null,invalidScalarPreserved=true;
   const recipe=experiment.spec.parameterization;
   if(recipe.scalars?.length){
    const request={type:'materialize_motion',scene:experiment.spec.scene,actions:experiment.spec.source_actions,recipe,values:proposal.values};
    materialized=await rpc(request);
    for(const mutation of ['reference','check']){
     const bad=structuredClone(request);
     if(mutation==='reference')bad.recipe.scalars[0].reference+=0.123;
     else bad.recipe.checks.push({name:'invalid_tolerance',left:{source:'scene_period'},right:{source:'scene_period'},tolerance:-1});
     let rejected=false;try{await rpc(bad);}catch{rejected=true;}
     invalidScalarPreserved&&=rejected&&stable(await rpc({type:'experiment_frame'}))===stable(frames[0]);
    }
   }
   let invalidSource=false;const bad=structuredClone(experiment);bad.runtime.library_source_blake3='0'.repeat(64);
   try{await rpc({type:'experiment_start',experiment:bad,proposal});}catch{invalidSource=true;}
   const invalidPreserved=invalidSource&&stable(await rpc({type:'experiment_frame'}))===stable(frames[0]);
   for(let i=0;i<experiment.spec.source_actions.length;i++){
    await rpc({type:'experiment_advance',maximum_actions:1});frames.push(await rpc({type:'experiment_frame'}));
    await window.reportProgress({phase:'complete',actions:i+1});
   }
   const full=await rpc({type:'experiment_checkpoint'});
   await rpc({type:'experiment_start',experiment,proposal});
   for(let i=0;i<split;i++)await rpc({type:'experiment_advance',maximum_actions:1});
   const partial=await rpc({type:'experiment_checkpoint'});
   await rpc({type:'experiment_resume',experiment,checkpoint:partial});
   let replayExact=true,stableCheckpoint=true;
   for(let i=0;i<split;i++){
    const s=await rpc({type:'experiment_advance',maximum_actions:1});
    replayExact&&=stable(await rpc({type:'experiment_frame'}))===stable(frames[i+1]);
    if(s.status==='replaying')stableCheckpoint&&=JSON.stringify(await rpc({type:'experiment_checkpoint'}))===JSON.stringify(partial);
   }
   let resumedExact=true;
   for(let i=split;i<experiment.spec.source_actions.length;i++){
    await rpc({type:'experiment_advance',maximum_actions:1});resumedExact&&=stable(await rpc({type:'experiment_frame'}))===stable(frames[i+1]);
   }
   const resumed=await rpc({type:'experiment_checkpoint'});
   const checkpointExact=JSON.stringify(full)===JSON.stringify(resumed);
   return {bound,frames,full,partial,resumed,robotInput,robotInspection,invalidReceiptPreserved,materialized,invalidScalarPreserved,invalidPreserved,replayExact,resumedExact,checkpointExact,stableCheckpoint,ticks};
  }finally{clearInterval(timer);worker.terminate();}
 },{experiment:native.experiment,proposal:native.proposal,split:native.split});
 let maximum=0,worst='';const differences=[];
 function compare(a,b,path){
  if(typeof a==='number'&&typeof b==='number'){
   const d=Math.abs(a-b),limit=1e-7+1e-8*Math.max(Math.abs(a),Math.abs(b));if(d>maximum){maximum=d;worst=path;}
   if(!Number.isFinite(a)||!Number.isFinite(b)||d>limit)differences.push(path);return;
  }
  if(a&&b&&typeof a==='object'&&typeof b==='object'){
   const keys=o=>Object.keys(o).filter(k=>k!=='stepping_wall_s').sort();
   if(Array.isArray(a)!==Array.isArray(b)||JSON.stringify(keys(a))!==JSON.stringify(keys(b)))differences.push(path+'.keys');
   for(const k of keys(a))compare(a[k],b[k],path+'.'+k);return;
  }
  if(a!==b)differences.push(path);
 }
 compare(native.frames,result.frames,'frames');compare(native.full,result.full,'full');
 if(native.robot_input)compare(native.robot_input,result.robotInput,'robot_input');
 if(native.robot_inspection)compare(native.robot_inspection,result.robotInspection,'robot_inspection');
 if(result.materialized)compare(native.full.recording.runtime.scene,result.materialized.variant.scene,'materialized.scene');
 const passed=!differences.length&&result.invalidReceiptPreserved&&result.invalidScalarPreserved&&result.invalidPreserved&&result.replayExact&&result.resumedExact&&result.checkpointExact&&result.stableCheckpoint&&result.ticks>0;
 await writeFile(capturePath,JSON.stringify(result)+'\n',{flag:'wx'});
 const report={passed,maximum_native_wasm_difference:maximum,worst,differences:differences.slice(0,20),invalid_source_preserved:result.invalidPreserved,
  scalar_materialization_checked:result.materialized!==null,invalid_scalar_bindings_preserved:result.invalidScalarPreserved,
  robot_input_checked:!!native.robot_input,robot_inspection_checked:!!native.robot_inspection,
  robot_input_override_count:result.robotInput?.overrides.length??null,invalid_receipts_preserved:result.invalidReceiptPreserved,
  replayed_and_resumed_frames_exact:result.replayExact&&result.resumedExact,checkpoint_exact:result.checkpointExact,
  checkpoint_preserved_during_replay:result.stableCheckpoint,main_thread_heartbeats:result.ticks,
  runtime_identity:result.bound.runtime,context_id:result.bound.context_id,capture_path:capturePath,
  scope:'Explicit numeric portability budgets and exact same-host checkpoint replay. No timing, sustained speed or physical calibration qualification.'};
 await writeFile(reportPath,JSON.stringify(report,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(report,null,2));assert(passed);
}finally{await browser?.close();server.kill();}
