import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root='runs/full-robot/learning/student-disturbances',definition='examples/full-robot/student-disturbances',browser='runs/interactive/student-disturbances';
const read=p=>JSON.parse(readFileSync(p));
const hash=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const basePath='runs/full-robot/learning/student-distillation/multi-train.native.json',base=read(basePath);
const body='Robot | Chassis and hip mounts';
function compare(a,b){
 assert.equal(a.frames.length,b.frames.length);
 let maximumBody=0,bodyTime=0,maximumLink=0,linkTime=0,linkName='',maximumCoordinate=0;
 for(let i=0;i<a.frames.length;i++){
  const x=a.frames[i],y=b.frames[i];assert.equal(x.time_s,y.time_s);
  for(let j=0;j<x.poses.length;j++){
   const p=x.poses[j],q=y.poses[j];assert.equal(p.name,q.name);
   const d=Math.hypot(...p.position_m.map((v,k)=>v-q.position_m[k]));
   if(d>maximumLink){maximumLink=d;linkName=p.name;linkTime=x.time_s;}
   if(p.name===body&&d>maximumBody){maximumBody=d;bodyTime=x.time_s;}
  }
  for(let j=0;j<x.joint_positions.length;j++)maximumCoordinate=Math.max(maximumCoordinate,Math.abs(x.joint_positions[j]-y.joint_positions[j]));
 }
 return {maximum_body_displacement_m:maximumBody,body_time_s:bodyTime,maximum_link_com_displacement_m:maximumLink,link_time_s:linkTime,link:linkName,maximum_coordinate_difference:maximumCoordinate,
  scope:'Matched 50 Hz samples. Link COM and generalized-coordinate differences; not contact-point error, between-sample agreement or hardware accuracy.'};
}
const cases={};
for(const name of ['lateral','reverse-lateral','stronger-lateral','forward','stress-5','stress-15','refined','unforced-refined','refined-5ms']){
 const path=`${root}/${name}.native.json`,run=read(path),config=read(`${definition}/${name}.config.json`);
 assert(run.completed&&!run.error);assert.deepEqual(run.recording.config,config);
 assert.deepEqual(run.recording.scene.robot,base.recording.scene.robot);
 assert.deepEqual(run.recording.input_events.map(e=>({time_s:e.at_step*config.step_s,values:e.values})),base.recording.input_events.map(e=>({time_s:e.at_step*base.recording.config.step_s,values:e.values})));
 const s=read(`${root}/${name}-acceptance/summary.json`),schedule=config.world_loads;
 const impulse=[0,0,0],expected=[0,0,0];
 let prefixExact;
 if(schedule){
  for(const p of schedule.pulses)for(let j=0;j<3;j++)expected[j]+=p.force_world_n[j]*p.duration_s;
  for(let i=0;i<run.frames.length-1;i++)for(let j=0;j<3;j++)impulse[j]+=run.frames[i].environment_load.force_world_n[j]*run.task.period_s;
  for(let j=0;j<3;j++)assert(Math.abs(impulse[j]-expected[j])<1e-12);
  if(config.step_s===base.recording.config.step_s){
   const start=Math.min(...schedule.pulses.map(p=>p.start_s));
   for(let i=0;run.frames[i]?.time_s<=start;i++){
    const {environment_load,stepping_wall_s,...a}=run.frames[i];
    const {stepping_wall_s:unused,...b}=base.frames[i];assert.deepEqual(a,b,`${name} changed physics before push`);
   }
   prefixExact=true;
  }
 }
 cases[name]={config:hash(`${definition}/${name}.config.json`),capture:hash(path),acceptance:hash(`${root}/${name}-acceptance/summary.json`),
  completed:run.completed,accepted:s.passed,swings:s.lifts.length,qualified_swings:s.lifts.filter(l=>l.passed).length,failed_swings:s.lifts.filter(l=>!l.passed),
  final_body_error_m:s.final_body_error_m,maximum_body_tilt_rad:s.maximum_body_tilt_rad,final_yaw_error_rad:s.final_yaw_error_rad,
  sampled_internal_contacts:s.sampled_internal_contacts,inter_link_geometry_audit:s.inter_link_geometry_audit,
  impulse_world_ns:impulse,prefix_exact:prefixExact,nominal_comparison:compare(run,base),total_reward:run.transitions.reduce((n,t)=>n+t.reward,0)};
}
const refinements={coarse_to_10ms:compare(read(`${root}/stronger-lateral.native.json`),read(`${root}/refined.native.json`)),
 '10ms_to_5ms':compare(read(`${root}/refined.native.json`),read(`${root}/refined-5ms.native.json`)),
 push_effect_at_10ms:compare(read(`${root}/refined.native.json`),read(`${root}/unforced-refined.native.json`))};
const parity=read(`${browser}/lateral-parity.json`),viewer=read(`${browser}/viewer-report.json`);
assert(parity.passed&&viewer.passed);
const rewardAudit={failed_stress_15_reward:cases['stress-15'].total_reward,accepted_lateral_reward:cases.lateral.total_reward,
 failed_run_scores_higher:cases['stress-15'].total_reward>cases.lateral.total_reward,
 explanation:'Current reward emphasizes survival, motor tracking/effort and upright posture; it omits executed step qualification and body position relative to the walking reference. Do not use this score alone to promote a robust walking policy.'};
const timing=read(`${browser}/live-performance.json`),record=read(`${browser}/live-performance.recording.json`);
assert(timing.completed);assert.deepEqual(record.runtime.input_events,read(`${root}/lateral.native.json`).recording.input_events);
const bundle=read(`${browser}/viewer/build-manifest.json`);
for(const [path,sha] of Object.entries(bundle.inputs))assert.equal(hash(path).sha256,sha,`stale browser input ${path}`);
const report={version:1,stage:'bounded physical disturbance development evaluation',training_performed:false,
 sources:['crates/sim-domain-robot/src/world_load.rs','crates/sim-runtime/src/embedded.rs','crates/sim-domain-robot/tests/world_load.rs','crates/sim-runtime/tests/world_load.rs','crates/sim-domain-robot/tests/embedded_step.rs','web/viewer/viewer.js','web/tests/viewer.mjs','examples/full-robot/summarize_student_disturbances.mjs'].map(hash),
 baseline:hash(basePath),manifest:read(`${definition}/manifest.json`),cases,refinements,reward_audit:rewardAudit,native_wasm:parity,viewer,
 rendered:{...timing,performance:{...timing.performance,breakdown:undefined}},browser_bundle:hash(`${browser}/viewer/build-manifest.json`),
 tests:['schedule-tests','library-tests','runtime-tests','workspace-check','wasm-build'].map(n=>hash(`${root}/${n}.log`)),
 scope:'Live force scheduling, physical impulse checks and native/WASM replay are verified. Observed development challenges, not independent final robustness validation. Strong pushes and timestep refinement expose task failures; no controller promotion, weakened gate or hardware-transfer claim.'};
writeFileSync('examples/full-robot/student-disturbances-status.json',JSON.stringify(report,null,2)+'\n');console.log(Object.fromEntries(Object.entries(cases).map(([n,c])=>[n,{accepted:c.accepted,qualified:c.qualified_swings,body_mm:c.final_body_error_m*1000}])));console.log(refinements);
