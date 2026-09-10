// Experiment recipes only. Shared Rust performs IK, geometry and simulation.
import fs from 'node:fs';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
const d='examples/full-robot/speed-ceiling',run='runs/speed-ceiling/clearance',base='examples/full-robot/fast-wasd';
fs.mkdirSync(run,{recursive:true});
const read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
const cases=process.argv[2]?read(process.argv[2]):[...([12,16,20].flatMap(lift=>[.5,1].map(gain=>({hip:0,speed:.1,lift,gain})))),
 {hip:0,speed:.1,lift:8,gain:1},{hip:30,speed:.125,lift:16,gain:1},
 {hip:30,speed:.125,lift:20,gain:1},{hip:45,speed:.1,lift:16,gain:1}];
const rows=fs.existsSync(`${d}/clearance-trials.json`)?read(`${d}/clearance-trials.json`).rows:[];
const smoothReturn=(u,a)=>{
 u=Math.max(0,Math.min(1,u));
 if(u<a){let x=u/a;return a*(x*x*x-.5*x*x*x*x)/(1-a);}
 if(u>1-a)return 1-smoothReturn(1-u,a);
 return (u-a/2)/(1-a);
};
for(const def of cases){
 const name=def.name??`hip${def.hip}-v${def.speed*1000}-lift${def.lift}-gain${def.gain}`,prefix=`${run}/${name}`;
 if(fs.existsSync(`${prefix}.planning.json`))throw Error(`refusing to overwrite ${prefix}`);
 const p=read(`${base}/landing-100.planning.json`);
 const referenceSpeed=def.reference_speed??def.speed;
 if(!Number.isFinite(referenceSpeed)||referenceSpeed<=0)throw Error('positive reference speed required');
 const lifts=def.lift_by_leg_mm??[def.lift,def.lift,def.lift,def.lift];
 if(lifts.length!==4||lifts.some(v=>!Number.isFinite(v)||v<=0))throw Error('four positive lift heights required');
 for(let leg=0;leg<4;leg++)p.initial_coordinates[3*leg]=def.hip*Math.PI/180;
 if(def.foot_deg!==undefined){
  const inspected=read(`${d}/posture-inspection.json`).rows;
  const old=inspected.find(r=>r.id===`hip${def.hip}-foot-60`),next=inspected.find(r=>r.id===`hip${def.hip}-foot${def.foot_deg}`);
  if(!old||!next||next.error||next.sampled_penetrations.length)throw Error('missing valid posture inspection');
  const meanZ=r=>r.markers.reduce((s,m)=>s+m.position_world_m[2],0)/r.markers.length;
  p.initial_base_translation_m[2]+=meanZ(old)-meanZ(next);
  for(let leg=0;leg<4;leg++)p.initial_coordinates[3*leg+2]=def.foot_deg*Math.PI/180;
 }
 for(const k of p.displacements_world_m.keyframes)for(let leg=0;leg<4;leg++){
  k.values[3*leg]*=referenceSpeed/.1;k.values[3*leg+2]*=lifts[leg]/8;
 }
 for(const k of p.base_displacements_world_m.keyframes)k.values[0]*=referenceSpeed/.1;
 if(def.return_start_s!==undefined){
  if(!(def.return_start_s>=.04&&def.return_end_s<=.36&&def.return_end_s>def.return_start_s&&def.return_ramp_fraction>0&&def.return_ramp_fraction<.5))throw Error('invalid return timing');
  for(const k of p.displacements_world_m.keyframes){
   const elapsed=Math.max(0,Math.min(4.8,k.time_s-.4)),transfer=Math.min(11,Math.floor((elapsed+1e-9)/.4));
   const u=(elapsed-transfer*.4-def.return_start_s)/(def.return_end_s-def.return_start_s),active=transfer%2===0?[0,2]:[1,3];
   for(let leg=0;leg<4;leg++){
    const group=leg===0||leg===2?0:1,completed=Math.floor(transfer/2)+(group<transfer%2?1:0);
    k.values[3*leg]=referenceSpeed*.8*(completed+(active.includes(leg)?smoothReturn(u,def.return_ramp_fraction):0));
   }
  }
 }
 if(def.foot_offsets_world_m!==undefined){
  const offsets=def.foot_offsets_world_m;
  if(offsets.length!==4||offsets.some(v=>v.length!==3||v.some(x=>!Number.isFinite(x))))throw Error('four finite world foot offsets required');
  for(const k of p.displacements_world_m.keyframes){
   const u=Math.max(0,Math.min(1,k.time_s/.4)),blend=u*u*u*(10+u*(-15+6*u));
   for(let leg=0;leg<4;leg++)for(let axis=0;axis<3;axis++)k.values[3*leg+axis]+=blend*offsets[leg][axis];
  }
 }
 fs.writeFileSync(`${prefix}.planning.json`,JSON.stringify(p)+'\n');
 const o=fs.openSync(`${prefix}.plan.json`,'wx'),e=fs.openSync(`${prefix}.plan-error.txt`,'wx');
 const sourceScene=def.source_scene??`${base}/refined.scene.json`;
 const r=spawnSync(`${bin}/plan_marker_motion`,[sourceScene,'examples/full-robot/gait-exploration/workspace-markers.json',`${prefix}.planning.json`],{stdio:['ignore',o,e],timeout:60000});
 fs.closeSync(o);fs.closeSync(e);
 const row={name,...def,prefix,source_scene:sourceScene,source_scene_sha256:sha(sourceScene),planning_exit:r.status,planning_error:r.error?.message??null,planning_sha256:sha(`${prefix}.planning.json`)};rows.push(row);
 console.log(row.name,row.planning_exit);
 if(r.status===0){
  const plan=read(`${prefix}.plan.json`),scene=read(`${base}/braked-5ms.scene.json`),c=read(`${base}/braked-5ms.config.json`);
  scene.robot=read(sourceScene).robot;
  const samples=plan.trajectory.keyframes.filter(k=>k.time_s>=.4-1e-9&&k.time_s<=1.2+1e-9).map(k=>k.values);
  if(samples.length!==41||samples[0].some((v,i)=>Math.abs(v-samples.at(-1)[i])>1e-7))throw Error('nonperiodic cycle');
  Object.assign(scene.controller.parameters,{samples,nominal_speed_m_s:referenceSpeed});
  if(def.hold_lookahead_s!==undefined){
   if(!Number.isFinite(def.hold_lookahead_s)||def.hold_lookahead_s<0)throw Error('nonnegative hold lookahead required');
   scene.controller.parameters.velocity_lead_s=c.motors.effective.components.map(m=>m.parameters.damping/m.parameters.stiffness+def.hold_lookahead_s);
  }
  scene.duration_s=6;
  for(const ch of scene.controller.inputs){
   if(ch.name==='command.forward_speed'){ch.lower=-def.speed;ch.upper=def.speed;ch.initial=0;}
   if(ch.name==='command.tracking_gain')ch.lower=ch.upper=ch.initial=def.gain;
  }
  c.initial_coordinates=samples[0];c.initial_base_translation_m=p.initial_base_translation_m;c.motors.servos.forEach((s,i)=>s.target_rad=samples[0][i]);
  c.step_s=.005;c.steps=1200;c.report_every=4;
  const actions=Array.from({length:300},(_,i)=>scene.controller.inputs.map(ch=>ch.name==='command.forward_speed'?(i>=20&&i<260?def.speed:0):ch.name==='command.packet_sequence'?i+1:ch.initial));
  for(const [suffix,value] of [['scene',scene],['config',c],['actions',actions]]){
   fs.writeFileSync(`${prefix}.${suffix}.json`,JSON.stringify(value)+'\n');row[`${suffix}_sha256`]=sha(`${prefix}.${suffix}.json`);
  }
 }
 fs.writeFileSync(`${d}/clearance-trials.json`,JSON.stringify({version:1,scope:'Development screens: 6 s command-driven Rust runtime, unchanged CAD/actuators; 5 ms numerical profile. Clearance and outer joint tracking gain vary. Performance qualification is separate.',rows},null,2)+'\n');
}
