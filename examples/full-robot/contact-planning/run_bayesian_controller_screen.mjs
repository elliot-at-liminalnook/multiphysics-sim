// Experiment orchestration only. The selector, controller and physics run in Rust;
// existing reducers measure completed captures and exact replay audits.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {isDeepStrictEqual} from 'node:util';
import {measureSpeedRun} from './measure_speed_run.mjs';
const [specPath]=process.argv.slice(2);assert(specPath,'pass experiment spec.json');
const spec=JSON.parse(fs.readFileSync(specPath));
const profile=spec.evaluation_profile??'screen8';
assert(['screen8','human20','speed20'].includes(profile));
const speedOnly=profile==='speed20',replayBaseline=spec.baseline_replay??true;
const root=spec.output_directory;fs.mkdirSync(root); // Exclusive run ownership.
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v)+'\n',{flag:'wx'});
const scene=read(spec.source_prefix+'.scene.json'),config=read(spec.source_prefix+'.config.json');
const actions=read(spec.source_prefix+'.actions.json');
const sourceCapture=replayBaseline?read(spec.source_prefix+'.native.json'):null;
const speedIndex=scene.controller.inputs.findIndex(i=>i.name==='command.forward_speed');
const gainIndex=scene.controller.inputs.findIndex(i=>i.name==='command.tracking_gain');
assert(speedIndex>=0&&gainIndex>=0&&scene.duration_s===(profile==='screen8'?8:20)&&config.step_s===.000625);
assert.equal(spec.problem.parameters.length,3);
assert(isDeepStrictEqual(spec.problem.parameters.map(p=>p.name),['command_speed','tracking_gain','velocity_lead_factor']));
assert.equal(spec.problem.constraints.length,speedOnly?1:profile==='screen8'?8:9);
const inputs=[spec.source_prefix+'.scene.json',spec.source_prefix+'.config.json',spec.source_prefix+'.actions.json',...(replayBaseline?[spec.source_prefix+'.native.json']:[]),spec.task];
const code=['examples/full-robot/contact-planning/analyze_controller_screen.mjs','examples/full-robot/speed-ceiling/analyze_validation.mjs','examples/full-robot/contact-planning/audit_contact_controller.mjs','examples/interactive/recorded_contact_motion.mjs','examples/full-robot/contact-planning/measure_speed_run.mjs',import.meta.filename];
const context={version:1,inputs:inputs.map(path=>({path,sha256:hash(path)})),code:code.map(path=>({path,sha256:hash(path)})),binaries:[spec.selector,...['run_environment','replay_policy_state','evaluate_lift'].map(n=>spec.runtime_examples+'/'+n)].map(path=>({path,sha256:hash(path)})),problem:{...spec.problem,context_id:undefined},seed:0,scope:'Same detailed runtime, CAD, physical limits, reference motion and eight-second gate definitions. Three explicit controller/task parameters vary; sampled geometry only, no qualification beyond this screen.'};
context.evaluation_profile=profile;
context.baseline_replay=replayBaseline;
context.scope='Same detailed runtime, CAD, physical limits, reference motion and declared screen8/human20 gates. Three explicit policy/task parameters vary. Sampled geometry, no qualification beyond the selected schedule.';
if(speedOnly)context.scope='Continuous travel: maximize net chassis distance/full elapsed trial duration, with falling as the sole acceptance constraint. Same CAD and actuator physics. Initial parameter search windows are not physical limits; no slip, lift, tracking, steering or stop rejection.';
const contextId=crypto.createHash('sha256').update(JSON.stringify(context)).digest('hex');
const problem={...spec.problem,context_id:contextId};write(root+'/context.json',{...context,context_id:contextId});
write(root+'/spec.json',spec);
function run(name,binary,args,expected){
 const out=fs.openSync(root+'/'+name+'.stdout.log','wx'),err=fs.openSync(root+'/'+name+'.stderr.log','wx');
 const r=spawnSync(binary,args,{env:{...process.env,SIM_EXAMPLES:spec.runtime_examples,RAYON_NUM_THREADS:'2',EGOBOX_LOG:'off',VECLIB_MAXIMUM_THREADS:'1',OMP_NUM_THREADS:'1'},stdio:['ignore',out,err]});
 fs.closeSync(out);fs.closeSync(err);assert(!r.error,`${name} launch failed`);
 write(root+'/'+name+'.execution.json',{binary,args,exit_code:r.status,signal:r.signal});
 if(expected!==undefined)assert.equal(r.status,expected,`${name} failed; inspect logs`);
 return r.status;
}
let evaluation=0;
function evaluate(values,method){
 assert(values.every((v,i)=>Number.isFinite(v)&&v>=problem.parameters[i].bounds[0]&&v<=problem.parameters[i].bounds[1]));
 const name='evaluation-'+String(evaluation++).padStart(3,'0'),prefix=root+'/'+name;
 const [speed,gain,lead]=values;
 const trialScene=structuredClone(scene),trialActions=structuredClone(actions);
 const speedChannel=trialScene.controller.inputs[speedIndex],gainChannel=trialScene.controller.inputs[gainIndex];
 speedChannel.lower=-problem.parameters[0].bounds[1];speedChannel.upper=problem.parameters[0].bounds[1];
 gainChannel.lower=problem.parameters[1].bounds[0];gainChannel.upper=problem.parameters[1].bounds[1];gainChannel.initial=gain;
 trialScene.controller.parameters.velocity_lead_s=scene.controller.parameters.velocity_lead_s.map(v=>v*lead);
 for(const row of trialActions){row[speedIndex]=Math.sign(row[speedIndex])*speed;row[gainIndex]=gain;}
 // Only declared policy inputs/parameters change. The complete physical config
 // is copied verbatim; target bounds, actuator values and contact are preserved.
 const restored=structuredClone(trialScene);restored.controller.inputs=scene.controller.inputs;
 restored.controller.parameters.velocity_lead_s=scene.controller.parameters.velocity_lead_s;
 assert(isDeepStrictEqual(restored,scene),'unexpected scene change');
 write(prefix+'.scene.json',trialScene);fs.copyFileSync(spec.source_prefix+'.config.json',prefix+'.config.json',fs.constants.COPYFILE_EXCL);write(prefix+'.actions.json',trialActions);
 const nativeOut=fs.openSync(prefix+'.native.json','wx'),nativeErr=fs.openSync(prefix+'.native.log','wx');
 const start=performance.now();
 const native=spawnSync(spec.runtime_examples+'/run_environment',[prefix+'.scene.json',prefix+'.config.json',spec.task,prefix+'.actions.json'],{stdio:['ignore',nativeOut,nativeErr],env:{...process.env,VECLIB_MAXIMUM_THREADS:'1',OMP_NUM_THREADS:'1'}});
 fs.closeSync(nativeOut);fs.closeSync(nativeErr);assert(!native.error,'runtime launch failure');
 const capture=read(prefix+'.native.json');
 if(method==='baseline-replay'){
  const physical=frames=>frames.map(frame=>{const copy={...frame};delete copy.stepping_wall_s;return copy;});
  assert(isDeepStrictEqual(physical(capture.frames),physical(sourceCapture.frames)),'baseline non-timing frames changed after exposing experimental policy-input bounds');
 }
 let outcome,metrics=null;
 if(speedOnly&&capture.error===null){
  metrics=measureSpeedRun(capture);
  write(prefix+'.summary.json',metrics);
  outcome=metrics.completed||metrics.fallen
   ?{status:'complete',objective:-metrics.speed_m_s,residuals:[metrics.fallen?1:-1]}
   :{status:'failed',reason:'incomplete episode without a measured fall'};
 }else if(!capture.completed||capture.error!==null){
  outcome={status:'failed',reason:capture.error??'episode did not complete'};
 }else{
  assert.equal(native.status,0);assert.equal(capture.recording.seed,0);
  let measured;
  if(profile==='screen8') {
   run(name+'-measure','node',['examples/full-robot/contact-planning/analyze_controller_screen.mjs',prefix,prefix+'.summary.json',String(speed)],0);
   measured=read(prefix+'.summary.json');
  }else{
   write(prefix+'.cases.json',{rows:[{name,kind:'human',family:name,prefix,step_s:config.step_s,
    command_speed_m_s:speed,travel_heading_offset_rad:scene.controller.parameters.travel_heading_offset_rad??0}]});
   run(name+'-measure','node',['examples/full-robot/speed-ceiling/analyze_validation.mjs',prefix+'.cases.json',prefix+'.summary.json'],0);
   const row=read(prefix+'.summary.json').rows[0];assert(row&&row.completed);
   measured={...row,segments:row.windows.map(w=>({...w,speed_along_heading_m_s:w.speed_m_s})),
    stops:row.stops.map(s=>({...s,settled_after_s:s.permanently_below_1mm_s_at===null?null:s.permanently_below_1mm_s_at-s.request_or_packet_loss_s}))};
  }
  write(prefix+'.clearance.spec.json',{prefix,windows_s:profile==='screen8'?[[.8,3],[4.2,6.4]]:[[1.4,9.8],[11,15.8]]});
  const audit=run(name+'-audit','node',['examples/full-robot/contact-planning/audit_contact_controller.mjs',prefix+'.clearance.spec.json',prefix+'.clearance.summary.json']);
  if(audit!==0){outcome={status:'failed',reason:'clearance/replay evaluation failed; inspect retained audit logs'};}
  else{
   const clearance=read(prefix+'.clearance.summary.json');assert.equal(clearance.maximum_command_error_rad,0);
   const speeds=measured.segments.map(s=>s.speed_along_heading_m_s);
   const residuals=[
    measured.maximum_slip_ratio/.05-1,
    Math.max(...speeds.map(v=>Math.abs(v/speed-1)))/.05-1,
    Math.max(...measured.segments.map(s=>Math.abs(s.direction_error_rad)))/.1-1,
    measured.maximum_tilt_rad/.1-1,
    // Null denotes an observed failed settling gate, not a fabricated time.
    Math.max(...measured.stops.map(s=>s.settled_after_s===null?1:s.settled_after_s/(.8+1e-9)-1)),
    Math.max(...measured.stops.map(s=>s.late_drift_m))/.003-1,
    1-clearance.passed_foot_clearances/clearance.planned_foot_clearances,
    clearance.inter_link_geometry_audit.maximum_penetration_m/.0001,
   ];
   if(profile==='human20')residuals.push(Math.abs(measured.turn_rad-.24)/.06-1);
   assert(residuals.every(Number.isFinite)&&speeds.every(Number.isFinite));
   outcome={status:'complete',objective:-Math.min(...speeds),residuals};
   metrics={speeds_m_s:speeds,slip:measured.maximum_slip_ratio,control:measured.passed_control_checks,planned_lifts:clearance.planned_foot_clearances,passed_lifts:clearance.passed_foot_clearances,overlap_m:clearance.inter_link_geometry_audit.maximum_penetration_m};
   if(profile==='human20')metrics.turn_rad=measured.turn_rad;
   assert.equal(residuals.every(v=>v<=0),measured.passed_control_checks&&measured.passed_contact_quality&&clearance.passed_foot_clearances===clearance.planned_foot_clearances&&clearance.inter_link_geometry_audit.maximum_penetration_m===0,'selector gates differ from independent screen');
  }
 }
 const observation={context_id:contextId,values,outcome,evidence:prefix+'.evaluation.json'};
 write(observation.evidence,{observation,method,metrics,wall_s:(performance.now()-start)/1000,native_exit_code:native.status,capture_sha256:hash(prefix+'.native.json')});
 console.log(JSON.stringify({evaluation:name,method,values,outcome,metrics}));
 return observation;
}
function design(name,count,seed){
 const request=root+'/'+name+'.design-request.json',output=root+'/'+name+'.design.json';
 write(request,{problem,count,seed});run(name+'-design',spec.selector,['--design',request,output],0);
 return read(output).values;
}
const common=[evaluate(spec.baseline_values,replayBaseline?'baseline-replay':'source-parameters-new-task')];
for(const values of spec.seed_values??[])common.push(evaluate(values,'recorded-search-seed-new-task'));
for(const values of design('initial',spec.initial_design_count,spec.seed))common.push(evaluate(values,'initial-latin-hypercube'));
write(root+'/initial-observations.json',common);
const adaptive=[...common];
for(let i=0;i<spec.comparison_evaluations;i++){
 const request=root+`/adaptive-${i}.request.json`,proposal=root+`/adaptive-${i}.proposal.json`;
 write(request,{problem,observations:adaptive,config:{seed:spec.seed+i,acquisition_starts:8,maximum_training_rows:256}});
 run('adaptive-'+i,spec.selector,[request,proposal],0);
 adaptive.push(evaluate(read(proposal).proposal.values,'constrained-log-expected-improvement'));
 write(root+`/adaptive-${i}.observations.json`,adaptive);
}
const control=[...common];
for(const values of design('control',spec.comparison_evaluations,spec.seed+1000))control.push(evaluate(values,'latin-hypercube-control'));
const best=observations=>observations.filter(o=>o.outcome.status==='complete'&&o.outcome.residuals.every(v=>v<=0)).sort((a,b)=>a.outcome.objective-b.outcome.objective)[0]??null;
write(root+'/result.json',{problem,evaluation_profile:profile,common,adaptive,control,best_adaptive:best(adaptive),best_control:best(control),scope:'One seeded matched-count comparison on a fixed motion family and declared schedule. Full cost includes selection and runtime/audit time. Sampled schedule gates are not complete timestep, sustained, browser or hardware qualification; no global speed claim.'});
