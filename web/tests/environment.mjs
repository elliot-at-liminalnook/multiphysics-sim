// Actual worker/native parity for sampled task transitions, including task replay.
import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {spawn} from 'node:child_process';
import {resolve,dirname} from 'node:path';
import {chromium} from 'playwright';
import {cpus,platform,arch} from 'node:os';
import {createHash} from 'node:crypto';
const [directory,presetId,nativePath,reportPath,configOverride]=process.argv.slice(2);
assert(directory&&presetId&&nativePath&&reportPath,'usage: environment.mjs bundle preset native-capture report [explicit-config-override]');
const frameEncoding=process.env.FRAME_ENCODING??'object';
assert(['object','json'].includes(frameEncoding),'FRAME_ENCODING must be object or json');
await mkdir(dirname(reportPath),{recursive:true});
const read=async p=>JSON.parse(await readFile(p));
const catalog=await read(resolve(directory,'catalog.json'));
const preset=catalog.presets.find(p=>p.id===presetId);assert(preset?.task);
const data=await read(resolve(directory,preset.path)),native=await read(nativePath);
const forecastCase=process.env.FORECAST_CASE_PATH?await read(process.env.FORECAST_CASE_PATH):null;
const motionCase=process.env.MOTION_CASE_PATH?await read(process.env.MOTION_CASE_PATH):null;
const fidelityPlan=process.env.FIDELITY_PLAN_PATH?await read(process.env.FIDELITY_PLAN_PATH):null;
if(fidelityPlan)assert(process.env.FIDELITY_CAPTURE_PATH&&process.env.FIDELITY_REPORT_PATH,'fidelity plan requires capture and report output paths');
if(forecastCase){assert(forecastCase.passed);assert.equal(forecastCase.source_capture,nativePath);}
const seed=native.recording.seed;
assert(Number.isInteger(seed)&&seed>=0&&seed<=0xffffffff,'browser seed must fit the existing u32 API');
// Scenario checks may use a versioned shorter horizon without altering the
// packaged preset. Always report the override; default preset checks stay exact.
let overrideEvidence;
if(configOverride){
 const bytes=await readFile(configOverride);data.config=JSON.parse(bytes);
 overrideEvidence={path:configOverride,sha256:createHash('sha256').update(bytes).digest('hex')};
}
assert(native.completed);assert.deepEqual(native.task,data.task);
// Both Rust hosts normalize optional schema defaults. Compare their recorded
// recipes below, not a typed recording against unnormalized source JSON.
const server=spawn(process.execPath,['web/serve-viewer.mjs',directory,'0']);
const url=await new Promise((resolve,reject)=>{server.stdout.on('data',c=>{const m=String(c).match(/http:\/\/127.0.0.1:\d+/);if(m)resolve(m[0]);});server.once('error',reject);server.once('exit',c=>reject(Error(`server exited ${c}`)));});
let browser;
try {
 browser=await chromium.launch({headless:true,...(process.env.CHROME_EXECUTABLE?{executablePath:process.env.CHROME_EXECUTABLE}:{})});
 const page=await browser.newPage();await page.goto(url+'/OPEN.txt');
 await page.exposeFunction('reportProgress',p=>console.log(JSON.stringify(p)));
 const result=await page.evaluate(async ({data,events,frameEncoding,seed,forecastCase,motionCase,fidelityPlan,fidelityReference})=>{
  const decode = frameEncoding==='json' ? (await import('/worker-message.mjs')).decodeWorkerResult : d=>d.result;
  const worker=new Worker('/worker.js',{type:'module'});let id=0;const pending=new Map();let progress=0;
  worker.onmessage=({data})=>{const p=pending.get(data.id);if(!p)return;clearTimeout(p.timer);if(data.progress){progress++;p.timer=setTimeout(p.expire,30000);return;}pending.delete(data.id);try{data.error?p.reject(Error(data.error)):p.resolve(decode(data));}catch(error){p.reject(error);}};
  worker.onerror=e=>{for(const p of pending.values()){clearTimeout(p.timer);p.reject(Error(e.message));}pending.clear();};
  const rpc=d=>new Promise((resolve,reject)=>{const key=++id;const expire=()=>{pending.delete(key);reject(Error('worker response timed out'));};pending.set(key,{resolve,reject,expire,timer:setTimeout(expire,30000)});worker.postMessage({...(d.type==='step'?{response_encoding:frameEncoding}:{}),...d,id:key});});
  const stable=f=>{const x=structuredClone(f);delete x.stepping_wall_s;return JSON.stringify(x);};
  let ticks=0;const pulse=setInterval(()=>ticks++,10);
  try {
   const loaded=await rpc({type:'load',...data,seed});
   let motion=null;
   if(motionCase){
    const materialized=await rpc({type:'materialize_motion',...motionCase.request});
    const bad=structuredClone(motionCase.request);bad.recipe.space.parameters[0].kind='Time';
    let invalidRejected=false;try{await rpc({type:'materialize_motion',...bad});}catch{invalidRejected=true;}
    motion={materialized,invalidRejected,framePreserved:stable(await rpc({type:'frame'}))===stable(loaded.frame)};
   }
   let invalid=false;try {await rpc({type:'step',action:[]});}catch {invalid=true;}
   const preserved=invalid&&stable(await rpc({type:'frame'}))===stable(loaded.frame);
   let encodingPreserved=null;
   if(frameEncoding==='json'){
    let rejected=false;try{await rpc({type:'step',action:loaded.inputs.map(c=>c.initial),response_encoding:'unsupported'});}catch{rejected=true;}
    encodingPreserved=rejected&&stable(await rpc({type:'frame'}))===stable(loaded.frame);
    let resetRejected=false;try{await rpc({type:'reset',seed:1,response_encoding:'json'});}catch{resetRejected=true;}
    encodingPreserved=encodingPreserved&&resetRejected&&stable(await rpc({type:'frame'}))===stable(loaded.frame);
   }
   const frames=[loaded.frame],transitionWall=[],forecasts=[];
   let forecastPreserved=true,invalidForecastPreserved=true;
   let held=loaded.inputs.map(c=>c.initial),eventIndex=0;
   const stride=Math.round(data.task.period_s/data.config.step_s);
   for(let at=0;at<data.config.steps;at+=stride){
    if(events[eventIndex]?.at_step===at)held=events[eventIndex++].values;
    const started=performance.now();
    frames.push(await rpc({type:'step',action:held}));
    transitionWall.push((performance.now()-started)/1000);
    const query=forecastCase?.queries[frames.length-2];
    if(query){
     const previous=frames.at(-2),before=stable(frames.at(-1));
     forecasts.push(await rpc({type:'predict_controller_trajectory',model:forecastCase.model,previous,actions:query.future_actions}));
     forecastPreserved&&=stable(await rpc({type:'frame'}))===before;
     const bad=structuredClone(forecastCase.model);bad.recipe.controller_inputs[0].kind='Angle';
     // A changed declaration or network feature contract must be rejected.
     if(bad.recipe.controller_inputs[0].kind===forecastCase.model.recipe.controller_inputs[0].kind)bad.recipe.controller_inputs[0].kind='LinearVelocity';
     let rejected=false;try{await rpc({type:'predict_controller_trajectory',model:bad,previous,actions:query.future_actions});}catch{rejected=true;}
     invalidForecastPreserved&&=rejected&&stable(await rpc({type:'frame'}))===before;
     const wrongPhysics=structuredClone(forecastCase.model);
     wrongPhysics.recipe.physics_context.sections['/config/step_s']='0'.repeat(64);
     rejected=false;try{await rpc({type:'predict_controller_trajectory',model:wrongPhysics,previous,actions:query.future_actions});}catch{rejected=true;}
     invalidForecastPreserved&&=rejected&&stable(await rpc({type:'frame'}))===before;
     const wrongSource=structuredClone(forecastCase.model);
     wrongSource.recipe.physics_context.runtime.library_source_blake3='0'.repeat(64);
     rejected=false;try{await rpc({type:'predict_controller_trajectory',model:wrongSource,previous,actions:query.future_actions});}catch{rejected=true;}
     invalidForecastPreserved&&=rejected&&stable(await rpc({type:'frame'}))===before;
    }
    if(frames.length%20===0)await window.reportProgress({phase:'environment',time_s:frames.at(-1).time_s});
   }
   const record=await rpc({type:'recording'});
   let fidelity=null;
   if(fidelityPlan){
    const capture={version:1,kind:'sampled_environment_capture',completed:record.runtime.completed_steps===record.runtime.config.steps&&record.error===null,
     error:record.error,recording:record.runtime,task:record.task,metadata:loaded.metadata,frames,transitions:frames.map(f=>f.learning),
     wall_s:transitionWall.reduce((a,b)=>a+b,0)};
    const before=stable(await rpc({type:'frame'}));
    const comparison=await rpc({type:'compare_environment_fidelity',reference:fidelityReference,candidate:capture,plan:fidelityPlan});
    const bad=structuredClone(capture);bad.recording.config.step_s*=2;
    let rejected=false;try{await rpc({type:'compare_environment_fidelity',reference:fidelityReference,candidate:bad,plan:fidelityPlan});}catch{rejected=true;}
    fidelity={capture,comparison,invalid_rejected:rejected,frame_preserved:before===stable(await rpc({type:'frame'}))};
   }
   const changed=structuredClone(record);changed.task.period_s*=2;
   let rejected=false;try{await rpc({type:'replay',recording:changed});}catch{rejected=true;}
   const recipePreserved=rejected&&stable(await rpc({type:'frame'}))===stable(frames.at(-1));
   const replay=await rpc({type:'replay',recording:record});
   const reset=await rpc({type:'reset',seed});
   return {frames,record,forecasts,motion,fidelity,forecastPreserved,invalidForecastPreserved,preserved,encodingPreserved,recipePreserved,replayExact:stable(replay)===stable(frames.at(-1)),resetExact:stable(reset)===stable(loaded.frame),contract:loaded.metadata.environment_contract,progress,ticks,transitionWall};
  }finally{clearInterval(pulse);worker.terminate();}
 },{data,events:native.recording.input_events??[],frameEncoding,seed,forecastCase,motionCase,fidelityPlan,fidelityReference:fidelityPlan?native:null});
 // Internal rotor speeds have much larger magnitudes than output-joint angles.
 // Use explicit absolute + relative portability budgets, matching the default
 // Newton relative correction scale. Task/physical accuracy gates are separate.
 const absoluteTolerance=1e-7,relativeTolerance=1e-8;
 const differences=[],numericGroups={};let maximum=0,worst='',maximumToleranceFraction=0;
 function numericGroup(path){
  const observation=path.match(/^transition\[\d+\]\.observations\.(\d+)$/);
  if(observation)return `observation:${native.contract.observations[Number(observation[1])].unit}`;
  if(/\.contacts\.\d+\.force_n\./.test(path))return 'contact_force_N';
  if(/\.poses\.\d+\.position_m\./.test(path))return 'link_position_m';
  if(/\.poses\.\d+\.velocity_m_s\./.test(path))return 'link_velocity_m_s';
  return path.replace(/\[\d+\]|\.\d+(?=\.|$)/g,'');
 }
 function compare(a,b,path){
  if(typeof a==='number'&&typeof b==='number'){
   const d=Math.abs(a-b),bound=path.startsWith('motion.')?1e-12+1e-12*Math.max(Math.abs(a),Math.abs(b)):absoluteTolerance+relativeTolerance*Math.max(Math.abs(a),Math.abs(b));
   if(d>maximum){maximum=d;worst=path;}maximumToleranceFraction=Math.max(maximumToleranceFraction,d/bound);
   const group=numericGroups[numericGroup(path)]??={samples:0,maximum_difference:0,maximum_tolerance_fraction:0,failures:0};
   group.samples++;group.maximum_difference=Math.max(group.maximum_difference,d);group.maximum_tolerance_fraction=Math.max(group.maximum_tolerance_fraction,d/bound);
   if(!Number.isFinite(a)||!Number.isFinite(b)||d>bound){differences.push(path);group.failures++;}
   return;
  }
  if(a&&b&&typeof a==='object'&&typeof b==='object'){if(path.startsWith('motion.')&&(Array.isArray(a)!==Array.isArray(b)||JSON.stringify(Object.keys(a).sort())!==JSON.stringify(Object.keys(b).sort())))differences.push(path+'.keys');if(Array.isArray(a)&&a.length!==b.length)differences.push(path+'.length');for(const k of Object.keys(a)){if(k!=='stepping_wall_s')compare(a[k],b[k],path+'.'+k);}return;}
  if(a!==b)differences.push(path);
 }
 assert.equal(native.frames.length,result.frames.length);
 for(let i=0;i<result.frames.length;i++){compare(native.frames[i],result.frames[i],`physics[${i}]`);compare(native.transitions[i],result.frames[i].learning,`transition[${i}]`);}
 compare(native.contract,result.contract,'contract');
 compare(native.recording,result.record.runtime,'recording');
 if(motionCase){
  assert(result.motion.invalidRejected&&result.motion.framePreserved,'motion preparation must reject wrong units and preserve the live environment');
  compare(motionCase.expected,result.motion.materialized,'motion.materialized');
 }
 if(forecastCase){assert.equal(forecastCase.queries.length,result.forecasts.length);forecastCase.queries.forEach((q,i)=>compare(q.prediction,result.forecasts[i],`forecast[${i}]`));}
 const timed=[...result.transitionWall].sort((a,b)=>a-b),wall=timed.reduce((a,b)=>a+b,0);
 const performance={simulated_s:result.frames.at(-1).time_s,transition_wall_s:wall,
  simulation_per_wall_second:result.frames.at(-1).time_s/wall,
  transition_p95_s:timed[Math.ceil(timed.length*.95)-1],
  scope:'Headless browser worker round trips including serialization; excludes loading, replay and rendering. This timing is not rendered active-walking or visible-responsiveness acceptance.'};
 const passed=!differences.length&&result.forecastPreserved&&result.invalidForecastPreserved&&result.preserved&&result.encodingPreserved!==false&&result.recipePreserved&&result.replayExact&&result.resetExact&&result.progress>0&&result.ticks>0;
 const report={passed,preset:presetId,transitions:result.frames.length-1,maximum_native_wasm_difference:maximum,worst,differences:differences.slice(0,20),invalid_action_preserved:result.preserved,changed_task_replay_preserved:result.recipePreserved,replay_exact:result.replayExact,reset_exact:result.resetExact,main_thread_heartbeats:result.ticks,replay_progress_messages:result.progress,
 numeric_tolerance:{absolute:absoluteTolerance,relative:relativeTolerance,maximum_fraction:maximumToleranceFraction},
 scope:'Same task and physical frames through production Rust environment, 1e-7 absolute + 1e-8 relative numeric portability tolerance. Same-host replay/reset remain exact. Not physical accuracy or learned control.'};
 report.performance=performance;
 report.frame_encoding=frameEncoding;report.invalid_encoding_preserved=result.encodingPreserved;
 report.numeric_groups=numericGroups;report.difference_count=differences.length;
 if(overrideEvidence){report.config_override=overrideEvidence;report.scope+=' Uses the explicitly recorded configuration override, not the packaged preset horizon.';}
 report.host={platform:platform(),architecture:arch(),cpu:cpus()[0]?.model,logical_cpus:cpus().length,browser:await browser.version()};
 if(forecastCase) report.controller_forecast={queries:result.forecasts.length,read_only:result.forecastPreserved,invalid_query_preserved:result.invalidForecastPreserved,case:process.env.FORECAST_CASE_PATH,
  model_version:forecastCase.model.version,physics_context:forecastCase.model.recipe.physics_context};
 if(fidelityPlan){
  assert(result.fidelity.invalid_rejected&&result.fidelity.frame_preserved,'fidelity comparison must reject changed context and preserve the environment');
  assert(result.fidelity.comparison.trajectory_within_tolerances&&result.fidelity.comparison.categorical_outcomes_match);
  await writeFile(process.env.FIDELITY_CAPTURE_PATH,JSON.stringify(result.fidelity.capture)+'\n',{flag:'wx'});
  await writeFile(process.env.FIDELITY_REPORT_PATH,JSON.stringify(result.fidelity.comparison)+'\n',{flag:'wx'});
  report.fidelity={capture_path:process.env.FIDELITY_CAPTURE_PATH,report_path:process.env.FIDELITY_REPORT_PATH,invalid_rejected:true,frame_preserved:true};
 }
 if(process.env.BROWSER_CAPTURE_PATH) {
  await writeFile(process.env.BROWSER_CAPTURE_PATH,JSON.stringify({frames:result.frames,recording:result.record,contract:result.contract,forecasts:result.forecasts,motion:result.motion})+'\n',{flag:'wx'});
  report.browser_capture_path=process.env.BROWSER_CAPTURE_PATH;
 }
 if(motionCase)report.motion={native_wasm_materialization_checked:true,absolute_tolerance:1e-12,relative_tolerance:1e-12,invalid_units_rejected:result.motion.invalidRejected,frame_preserved:result.motion.framePreserved};
 await writeFile(reportPath,JSON.stringify(report,null,2));console.log(JSON.stringify(report,null,2));assert(passed);
}finally {await browser?.close();server.kill();}
