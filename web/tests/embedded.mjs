// Portability and lifecycle checks for the actual incremental Rust motor runner.
import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {spawn} from 'node:child_process';
import {resolve,dirname} from 'node:path';
import {chromium} from 'playwright';
const [directory,presetId,nativePath,reportPath]=process.argv.slice(2);
assert(directory&&presetId&&nativePath&&reportPath,'usage: embedded.mjs bundle preset-id native-capture.json report.json');
await mkdir(dirname(reportPath),{recursive:true});
const catalog=JSON.parse(await readFile(resolve(directory,'catalog.json')));
const preset=catalog.presets.find(p=>p.id===presetId);assert.equal(preset?.mode,'embedded');
const data=JSON.parse(await readFile(resolve(directory,preset.path)));
const native=JSON.parse(await readFile(nativePath));assert.equal(native.completed,true);assert.equal(native.requested_steps,data.config.steps);
assert.deepEqual(native.source,data.scene.robot.source);
const server=spawn(process.execPath,['web/serve-viewer.mjs',directory,'0']);
const url=await new Promise((resolve,reject)=>{server.stdout.on('data',c=>{const m=String(c).match(/http:\/\/127.0.0.1:\d+/);if(m)resolve(m[0]);});server.once('error',reject);server.once('exit',c=>reject(new Error(`server exited ${c}`)));});
let browser;
try {
 browser=await chromium.launch({headless:true,...(process.env.CHROME_EXECUTABLE?{executablePath:process.env.CHROME_EXECUTABLE}:{})});
 const page=await browser.newPage();await page.goto(url+'/OPEN.txt');
 await page.exposeFunction('reportProgress',p=>console.log(JSON.stringify(p)));
 const result=await page.evaluate(async ({scene,config,probeTaskObservations})=>{
  const worker=new Worker('/worker.js',{type:'module'});let sequence=0;const pending=new Map();let replayProgress=0;
  worker.onmessage=({data})=>{const p=pending.get(data.id);if(!p)return;clearTimeout(p.timer);if(data.progress){replayProgress++;p.timer=setTimeout(p.expire,30000);return;}pending.delete(data.id);data.error?p.reject(Error(data.error)):p.resolve(data.result);};
  worker.onerror=e=>{for(const p of pending.values()){clearTimeout(p.timer);p.reject(Error(e.message));}pending.clear();};
  const rpc=data=>new Promise((resolve,reject)=>{const id=++sequence;const expire=()=>{pending.delete(id);reject(Error('worker request exceeded 30 seconds without progress'));};pending.set(id,{resolve,reject,expire,timer:setTimeout(expire,30000)});worker.postMessage({...data,id});});
  const loadStart=performance.now();const loaded=await rpc({type:'load',scene,config,seed:0});const loadMs=performance.now()-loadStart;
  let rejected=false;try{await rpc({type:'step',steps:0,action:[]});}catch{rejected=true;}const afterInvalid=await rpc({type:'frame'});
  let heartbeats=0;const pulse=setInterval(()=>heartbeats++,10);const frames=[loaded.frame];const started=performance.now();let maxChunkMs=0;
  for(let n=0;n<config.steps;n+=config.report_every){
   const before=performance.now();const f=await rpc({type:'step',steps:config.report_every,action:loaded.inputs.map(c=>c.initial)});maxChunkMs=Math.max(maxChunkMs,performance.now()-before);
   if(f.error)throw Error(f.error);frames.push(f);
   if(frames.length%20===0)await window.reportProgress({phase:'advance',simulated_s:f.time_s,wall_s:(performance.now()-started)/1000});
  }
  const simulationMs=performance.now()-started;const recording=await rpc({type:'recording'});
  const changed=structuredClone(recording);changed.config.applied_generalized_loads[0]+=0.001;let changedReplayRejected=false;
  try{await rpc({type:'replay',recording:changed});}catch{changedReplayRejected=true;}
  const afterChangedReplay=await rpc({type:'frame'});
  const replay=await rpc({type:'replay',recording});const reset=await rpc({type:'reset',seed:0});
  const unknownScene=structuredClone(scene);unknownScene.robot.version=4;
  const driven=unknownScene.robot.joints.find(j=>j.name===unknownScene.robot.motors[0].joint);
  driven.physics.drive_backlash={width_rad:null,provenance:'unmeasured',reference:'Synthetic missing-physical-property probe'};
  let unknownDriveRejected=false;
  try{await rpc({type:'load',scene:unknownScene,config,seed:0});}catch(e){unknownDriveRejected=e.message.includes('drive backlash is unmeasured');}
  const afterUnknownDrive=await rpc({type:'frame'});
  let taskProbe = null;
  if(probeTaskObservations){
   const probeScene=structuredClone(scene),probeConfig=structuredClone(config);
   probeScene.robot.source.cad_sha256='synthetic-observation-test';
   probeScene.controller.sources={entry:'probe.rhai',files:{'probe.rhai':'fn control(t,s,c,state){ c["pivot.target"]=s["command.position"]+s["marker.tip.position.z"]; #{commands:c,state:state} }'}};
   probeConfig.policy.task_observations={observation_source:'ideal_rigid_body_diagnostics',expected_cad_sha256:'synthetic-observation-test',reference_link:'ground',markers:[{id:'tip',link:'pendulum',local_point_m:[0,0,0]}],floor_forces:false};
   const probeLoaded=await rpc({type:'load',scene:probeScene,config:probeConfig,seed:0});
   const probeFrame=await rpc({type:'step',steps:20,action:probeLoaded.inputs.map(c=>c.initial)});
   const record=await rpc({type:'recording'});const replayed=await rpc({type:'replay',recording:record});
   const o=probeFrame.policy?.observations;
   const stable=f=>{const c=structuredClone(f);delete c.stepping_wall_s;return JSON.stringify(c);};
   taskProbe={passed:!probeFrame.error&&o?.['body.gravity_direction.z']===-1&&Number.isFinite(o?.['marker.tip.velocity.z'])&&probeFrame.policy.targets['pivot.target']===o['command.position']+o['marker.tip.position.z']&&stable(probeFrame)===stable(replayed)&&probeLoaded.metadata.policy_contract.deployable===false,scope:'Synthetic test-only provenance; verifies observed marker input drives Rhai and replays through WASM. Not a CAD calibration artifact.'};
  }
  clearInterval(pulse);worker.terminate();return{taskProbe,loaded,afterInvalid,rejected,frames,replay,reset,unknownDriveRejected,afterUnknownDrive,changedReplayRejected,afterChangedReplay,recordedSteps:recording.completed_steps,loadMs,simulationMs,maxChunkMs,heartbeats,replayProgress};
 },{...data,probeTaskObservations:presetId==='pendulum-policy'});
 await writeFile(reportPath+'.frames.json',JSON.stringify(result));
 let maximumDifference=0,worstPath='';const differences=[];
 function compare(a,b,path){
  if(typeof a==='number'&&typeof b==='number'){
   const d=Math.abs(a-b);if(d>maximumDifference){maximumDifference=d;worstPath=path;}
   if(!Number.isFinite(a)||!Number.isFinite(b)||d>1e-7){if(differences.length<20)differences.push({path,native:a,browser:b});}return;
  }
  if(a&&typeof a==='object'&&b&&typeof b==='object'){
   if(Array.isArray(a)&&a.length!==b.length)differences.push({path,reason:'array length differs'});
   for(const k of Object.keys(a))compare(a[k],b[k],path+'.'+k);return;
  }
  if(a!==b&&differences.length<20)differences.push({path,native:a,browser:b});
 }
 assert.equal(result.frames.length,native.frames.length);
 native.frames.forEach((f,i)=>compare(f,result.frames[i],`frame[${i}]`));
 const strip=f=>{const copy=structuredClone(f);delete copy.stepping_wall_s;return copy;};
 const replayExact=JSON.stringify(strip(result.frames.at(-1)))===JSON.stringify(strip(result.replay));
 const resetExact=JSON.stringify(strip(result.loaded.frame))===JSON.stringify(strip(result.reset));
 const invalidPreserved=JSON.stringify(result.loaded.frame)===JSON.stringify(result.afterInvalid);
 const changedReplayPreserved=result.changedReplayRejected&&JSON.stringify(result.frames.at(-1))===JSON.stringify(result.afterChangedReplay);
 const unknownDrivePreserved=result.unknownDriveRejected&&JSON.stringify(strip(result.reset))===JSON.stringify(strip(result.afterUnknownDrive));
 const passed=unknownDrivePreserved&&(result.taskProbe?.passed ?? true)&&differences.length===0&&replayExact&&resetExact&&invalidPreserved&&changedReplayPreserved&&result.rejected&&result.replayProgress>0&&result.frames.at(-1).done;
 const report={passed,unknown_drive_rejected_without_mutation:unknownDrivePreserved,task_observation_probe:result.taskProbe,preset:presetId,compared_frames:native.frames.length,simulated_s:native.simulated_s,maximum_native_wasm_entry_difference:maximumDifference,worst_path:worstPath,differences,replay_exact:replayExact,reset_exact:resetExact,invalid_request_preserved:invalidPreserved,changed_replay_rejected_without_mutation:changedReplayPreserved,load_ms:result.loadMs,simulation_ms:result.simulationMs,max_chunk_ms:result.maxChunkMs,main_thread_heartbeats:result.heartbeats,replay_progress_messages:result.replayProgress,scope:'Strict 1e-7 absolute entry portability diagnostic over all native sampled frame fields; not hardware accuracy or realtime acceptance. Replay equality excludes measured wall time.'};
 await writeFile(reportPath,JSON.stringify(report,null,2));console.log(JSON.stringify(report,null,2));assert(passed,'embedded browser lifecycle or native trajectory comparison failed');
}finally{await browser?.close();server.kill();}
