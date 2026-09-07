// Acceptance orchestration; contact/clearance evaluation remains in shared Rust.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [directory,capturePath,output]=process.argv.slice(2);
assert(directory&&capturePath&&output,'usage: check_crawl.mjs recipe-directory capture.json output-directory');
const read=p=>JSON.parse(readFileSync(p));
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const capture=read(capturePath),phases=read(`${directory}/phases.json`),config=read(`${directory}/config.json`);
assert.equal(capture.completed,true);assert.equal(capture.error,null);
assert(Math.abs(capture.frames.at(-1).time_s-config.steps*config.step_s)<1e-9);
// Source fields must match the typed recording; defaults added by Rust are fine.
function contains(actual,expected,path='config'){
 if(expected&&typeof expected==='object'){
  assert(actual&&typeof actual==='object',path);
  if(Array.isArray(expected))assert.equal(actual.length,expected.length,path);
  for(const [k,v]of Object.entries(expected))contains(actual[k],v,`${path}.${k}`);
 }else assert.deepEqual(actual,expected,path);
}
contains(capture.recording.config,config);
contains(capture.task,read(`${directory}/task.json`),'task');
mkdirSync(output,{recursive:true});
const lifts=[];
for(const phase of phases){
 const name=`lift-${phase.leg}${phase.cycle?`-cycle-${phase.cycle}`:''}`;
 const text=execFileSync('target/release/examples/evaluate_lift',[`${directory}/scene.json`,capturePath,`${directory}/${name}.json`],{encoding:'utf8',maxBuffer:32*1024*1024});
 writeFileSync(`${output}/${name}.json`,text);
 const result=JSON.parse(text);
 lifts.push({leg:phase.leg,cycle:phase.cycle??0,...result.report});
}
const body=f=>f.poses.find(p=>p.name==='Robot | Chassis and hip mounts');
const scene=read(`${directory}/scene.json`),markers=config.policy.point_feedback.markers;
const landings=phases.map(phase=>{
 const f=capture.frames.find(f=>Math.abs(f.time_s-phase.end_s)<1e-9);assert(f,'missing end-of-phase sample');
 const target=config.policy.point_feedback.position_world_m.keyframes.find(k=>Math.abs(k.time_s-phase.end_s)<1e-9);assert(target);
 const feet=markers.map((m,index)=>{
  const pose=f.poses.find(p=>p.name===m.link);assert(pose);
  const point=pose.position_m.map((x,i)=>x+pose.rotation[i].reduce((s,r,j)=>s+r*m.local_point_m[j],0));
  const force=f.contacts.filter(c=>c.other==null&&scene.robot.links[c.link].name===m.link).reduce((s,c)=>s+c.force_n[2],0);
  return {marker:m.id,error_m:Math.hypot(...point.map((v,i)=>v-target.values[3*index+i])),upward_force_n:force};
 });
 return {leg:phase.leg,cycle:phase.cycle??0,time_s:f.time_s,feet};
});
const first=body(capture.frames[0]),last=body(capture.frames.at(-1));assert(first&&last);
let internalContacts=0,maxTilt=0;
for(const f of capture.frames){
 internalContacts+=f.contacts.filter(c=>c.other!=null).length;
 const p=body(f);assert(p);maxTilt=Math.max(maxTilt,Math.acos(Math.max(-1,Math.min(1,p.rotation[2][2]))));
}
const requestedAdvance=config.policy.body_feedback.position_world_m.keyframes.at(-1).values[0]-config.policy.body_feedback.position_world_m.keyframes[0].values[0];
const advance=last.position_m[0]-first.position_m[0];
// Provisional flat-floor commissioning budgets, not hardware accuracy bounds.
const budgets={maximum_body_advance_error_m:.001,maximum_body_tilt_rad:.01,maximum_sampled_internal_contacts:0,maximum_settled_foot_error_m:.001,minimum_settled_foot_force_n:1};
const passed=lifts.every(l=>l.passed)&&landings.every(l=>l.feet.every(f=>f.error_m<=budgets.maximum_settled_foot_error_m&&f.upward_force_n>=budgets.minimum_settled_foot_force_n))&&Math.abs(advance-requestedAdvance)<=budgets.maximum_body_advance_error_m&&maxTilt<=budgets.maximum_body_tilt_rad&&internalContacts===0;
const report={version:1,passed,capture:{path:capturePath,sha256:hash(capturePath)},source_cad_sha256:capture.recording.scene.robot.source.cad_sha256,
 simulated_s:capture.frames.at(-1).time_s,lifts,landings,body_advance_m:advance,requested_body_advance_m:requestedAdvance,maximum_body_tilt_rad:maxTilt,sampled_internal_contacts:internalContacts,budgets,
 scope:'Fixed reference on one flat floor, ideal feedback, effective servos. Samples must show simultaneous swing clearance/unloading and three loaded supporting feet. No between-sample guarantee, live steering, disturbance robustness or hardware validation.'};
writeFileSync(`${output}/summary.json`,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify(report,null,2));assert(passed,'crawl task acceptance failed; see summary');
