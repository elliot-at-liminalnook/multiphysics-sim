import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
const d='examples/full-robot/contact-planning/';
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const equal=(a,b,label)=>assert(isDeepStrictEqual(a,b),label);
const names=['belt-three15-v230','belt-three15-xreach45-v230',
 'diagonal-hold230','diagonal-hold250','diagonal-hold280',
 'xhip-clearance10-v230','xhip-lift-v230','xhip-lift-v230-fine'];
const result=[];
for(const name of names){
 const spec=read(d+name+'-trial.json'),s=read(d+name+'.summary.json'),audit=read(d+name+'-clearance.summary.json');
 for(const f of [...spec.sources,...spec.files])assert.equal(hash(f.path),f.sha256);
 const c=read(spec.prefix+'.native.json');
 assert(c.completed&&c.error===null&&c.frames.length===401&&c.frames.at(-1).time_s===8);
 assert.equal(hash(spec.prefix+'.native.json'),s.capture_sha256);
 assert.equal(s.command_speed_m_s,spec.command_speed_m_s);
 assert.equal(audit.maximum_command_error_rad,0);
 const speed=s.command_speed_m_s,base='runs/contact-planning/diagonal-cadence'+Math.round(speed*1000);
 const scene=read(spec.prefix+'.scene.json'),parentScene=read(base+'.scene.json');
 const config=read(spec.prefix+'.config.json'),parentConfig=read(base+'.config.json');
 equal(scene.robot,parentScene.robot,'CAD and robot physics preserved');
 const a=structuredClone(scene),b=structuredClone(parentScene);delete a.controller;delete b.controller;
 equal(a,b,'world, scene timing and all non-controller fields preserved');
 equal(scene.controller.sources,parentScene.controller.sources,'shared controller code unchanged');
 equal(scene.controller.inputs,parentScene.controller.inputs,'same command limits and inputs');
 equal(read(spec.prefix+'.actions.json'),read(base+'.actions.json'),'same complete command schedule');
 if(name.startsWith('diagonal-hold')){
  const restored=structuredClone(scene);
  restored.controller.parameters.velocity_lead_s=parentScene.controller.parameters.velocity_lead_s;
  equal(restored,parentScene,'only hold lead changes');
  equal(config,parentConfig,'hold experiment physical config unchanged');
  equal(scene.controller.parameters.velocity_lead_s,
   parentScene.controller.parameters.velocity_lead_s.map(v=>v+scene.period_s/2),'analytic half-hold lead');
 }else{
  const restored=structuredClone(config);restored.initial_coordinates=parentConfig.initial_coordinates;
  restored.motors.servos.forEach((servo,i)=>{servo.target_rad=parentConfig.motors.servos[i].target_rad;});
  if(name.endsWith('-fine')){
   assert.equal(config.step_s,parentConfig.step_s/2);assert.equal(config.steps,parentConfig.steps*2);
   assert.equal(config.report_every,parentConfig.report_every*2);
   for(const key of ['step_s','steps','report_every'])restored[key]=parentConfig[key];
  }
  equal(restored,parentConfig,'path experiment only changes initial joint coordinates/targets in physical config');
 }
 result.push({name,command_speed_m_s:speed,physics_step_s:config.step_s,speeds_m_s:s.segments.map(s=>s.speed_along_heading_m_s),
  slip:s.maximum_slip_ratio,control_passed:s.passed_control_checks,
  passed_lifts:audit.passed_foot_clearances,planned_lifts:audit.planned_foot_clearances,
  maximum_overlap_m:audit.inter_link_geometry_audit.maximum_penetration_m,
  overlapping_poses:audit.inter_link_geometry_audit.penetrating_samples,
  failed_lift_links:[...new Set(audit.failed_foot_clearances.map(f=>f.requirements.swing_link))]});
}
const pairs=read(d+'belt15-parent-geometry-pairs.json'),compact=read(d+'belt15-parent-geometry-default.json');
const pairSummary={};
for(const frame of pairs.frames){
 const maximum=Math.max(0,...frame.inter_link_penetrations.map(p=>p.penetration_m));
 assert.equal(maximum,frame.maximum_inter_link_penetration_m);
 for(const p of frame.inter_link_penetrations){
  const key=[pairs.link_names[p.link],pairs.link_names[p.other]].sort().join(' / ');
  pairSummary[key]??={samples:0,maximum_overlap_m:0};
  pairSummary[key].samples++;pairSummary[key].maximum_overlap_m=Math.max(pairSummary[key].maximum_overlap_m,p.penetration_m);
 }
 delete frame.inter_link_penetrations;
}
delete pairs.link_names;equal(pairs,compact,'optional pair report preserves common physical output');
const liftRecipe=read(d+'xhip-lift-reference.recipe.json'),liftParent=read(d+'xhip-clearance10-reference.recipe.json');
const liftPreparation=read(d+'xhip-lift-preparation.json');
assert.equal(liftRecipe.motion.feet[1].swing_offset_world_m[2],liftPreparation.new_lift_m);
liftRecipe.motion.feet[1].swing_offset_world_m[2]=liftParent.motion.feet[1].swing_offset_world_m[2];
equal(liftRecipe,liftParent,'only failed-foot lift changes in final plan');
const coarse=read('runs/contact-planning/xhip-lift-v230.native.json');
const fine=read('runs/contact-planning/xhip-lift-v230-fine.native.json');
let maximumPathDifference=0;
for(let i=0;i<coarse.frames.length;i++){
 const a=coarse.frames[i],b=fine.frames[i];assert.equal(a.time_s,b.time_s);
 const body=f=>f.poses.find(p=>p.name.includes('Chassis')).position_m;
 maximumPathDifference=Math.max(maximumPathDifference,Math.hypot(...body(a).map((v,j)=>v-body(b)[j])));
}
console.log(JSON.stringify({results:result,parent_overlap_pairs:pairSummary,
 timestep_comparison:{maximum_chassis_position_difference_m:maximumPathDifference,coarse_s:.000625,fine_s:.0003125},
 scope:'Matched detailed eight-second runtime measurements, exact controller replay and sampled CAD geometry, plus one timestep refinement of the final candidate. Path and hold changes preserve physical definitions and gates. No sustained, continuous-time, steering, browser, sim-to-real or physical maximum-speed certificate.'},null,2));
