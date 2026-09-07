// Author references, then ask the shared Rust planner to solve the CAD mechanism.
// This script never integrates physics or changes the robot's physical definition.
import {readFileSync,writeFileSync,mkdirSync,openSync,closeSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const out=process.argv[2]||'runs/full-robot/learning/crawl';
const read=p=>JSON.parse(readFileSync(p)),hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const recipe=process.argv[3]?read(process.argv[3]):{};
const source='examples/full-robot/browser-effective-servo';
const scene=read(`${source}/scene.json`),config=read(`${source}/config.json`),task=read(`${source}/task.json`);
const planner=read('examples/full-robot/single-foot-marker-motion-forward-10mm.json');
const markers=read('examples/full-robot/foot-markers.json');
// Extend the previous single-foot search envelope for multi-foot body shifts.
// This is a provisional controller envelope; authored CAD limits and collision
// inspection remain authoritative and the planner still rejects violations.
planner.independent_coordinates.forEach((name,i)=>{
 if(name.includes('Hip servo output')){
  planner.bounds[i]={lower:-0.2,upper:0.2,max_step:0.01};
  config.policy.target_bounds_rad[name]=[-0.2,0.2];
 }
 if(name.includes('Worm servo output')){
  const center=planner.initial_coordinates[i];
  planner.bounds[i]={lower:center-0.5,upper:center+0.5,max_step:0.02};
  config.policy.target_bounds_rad[name]=[center-0.5,center+0.5];
 }
});
const advance=recipe.advance_m??0.01,lift=recipe.lift_m??0.005,shift=0.016,order=[0,2,1,3];
const xShift=recipe.positive_x_shift_m??0.018,crouch=recipe.positive_x_crouch_m??0.003;
const flankAdvance=recipe.flank_advance_m??0.02;
const cycles=recipe.cycles??1;
assert(Number.isInteger(cycles)&&cycles>=1&&cycles<=20,'cycles must be an integer in 1..20');
assert([advance,lift,xShift].every(x=>Number.isFinite(x)&&x>0)&&Number.isFinite(crouch)&&crouch>=0);
assert(Number.isFinite(flankAdvance)&&flankAdvance>0);
assert(Object.keys(recipe).every(k=>['advance_m','lift_m','positive_x_shift_m','positive_x_crouch_m','flank_advance_m','cycles'].includes(k)));
const feet=Array(12).fill(0);let body=[0,0,0],time=0;
const footKeys=[],bodyKeys=[],phases=[];
const knot=()=>{footKeys.push({time_s:time,values:[...feet]});bodyKeys.push({time_s:time,values:[...body]});};
const next=dt=>{time=Math.round((time+dt)*100)/100;knot();};
knot();next(0.2);
// The imported assembly's four legs point -Y, +X, +Y, -X respectively.
// These are robot-specific reference directions, not a generic physics rule.
// CAD-derived initial COM is about +13.3 mm in X; the +X swing needs
// a larger negative-X transfer. These are provisional planning parameters.
const away=[[0,1],[-xShift/shift,0],[0,-1],[0.009/shift,0]];
for(let cycle=0;cycle<cycles;cycle++)for(const [n,leg] of order.entries()){
 const start=time;
 body=[advance*(cycle+n/4)+shift*away[leg][0],shift*away[leg][1],leg===1?-crouch:0];next(0.5);
 const liftStart=time;
 feet[3*leg]+=cycle===0&&(leg===0||leg===2)?flankAdvance:advance;feet[3*leg+2]=lift;next(0.4);
 const peak=time;
 feet[3*leg+2]=0;next(0.4);
 const landed=time;
 body=[advance*(cycle+(n+1)/4),0,0];next(0.5);next(0.2);
 phases.push({marker:markers.markers[leg].id,leg,cycle,start_s:start,lift_start_s:liftStart,peak_s:peak,land_s:landed,end_s:time});
}
planner.embedding=config.embedding;
planner.sample_period_s=0.02;
planner.maximum_interpolation_error_m=0.0001;
planner.displacements_world_m={interpolation:'quintic_rest_to_rest',keyframes:footKeys};
planner.base_displacements_world_m={interpolation:'quintic_rest_to_rest',keyframes:bodyKeys};
planner.support_requirements=phases.map(p=>({start_s:p.lift_start_s,end_s:p.land_s,
 marker_ids:markers.markers.filter((_,i)=>i!==p.leg).map(m=>m.id),minimum_forces_n:[1,1,1]}));
mkdirSync(out,{recursive:true});
const write=(name,value)=>writeFileSync(`${out}/${name}`,JSON.stringify(value)+'\n');
write('scene.json',scene);write('planner.json',planner);write('phases.json',phases);
const binary='target/release/examples/plan_marker_motion';
const fd=openSync(`${out}/plan.json`,'w');
const result=spawnSync(binary,[`${out}/scene.json`,'examples/full-robot/foot-markers.json',`${out}/planner.json`],{stdio:['ignore',fd,'inherit'],timeout:180000});closeSync(fd);
assert.equal(result.status,0,`crawl reference failed geometric/support checks: ${result.error||''}`);
const plan=read(`${out}/plan.json`);assert(plan.completed);assert.deepEqual(plan.source,scene.robot.source);
config.motors.target_trajectory=plan.trajectory;
// First diagnose all four loaded swings on an explicit fixed schedule. A
// multi-contact phase guard is a subsequent controller requirement, not implied.
delete config.motion_gate;
config.steps=Math.round((time+0.4)/config.step_s);
config.report_every=1;
const origin=config.policy.body_feedback.position_world_m.keyframes[0].values;
config.policy.body_feedback.position_world_m={interpolation:'quintic_rest_to_rest',keyframes:bodyKeys.map(k=>({time_s:k.time_s,values:k.values.map((x,i)=>x+origin[i])}))};
config.policy.point_feedback.markers=markers.markers;
config.policy.point_feedback.position_world_m={interpolation:'quintic_rest_to_rest',keyframes:footKeys.map(k=>({time_s:k.time_s,values:k.values.map((x,i)=>x+plan.frames[0].marker_positions_world_m[Math.floor(i/3)][i%3])}))};
config.policy.point_feedback.activation={interpolation:'linear',keyframes:[{time_s:0,values:[1,1,1,1]}]};
task.termination_bounds=[{observation:'body.position.z',lower:-0.04,upper:0.02}];
write('config.json',config);write('task.json',task);
const refined=structuredClone(config);refined.step_s/=2;refined.steps*=2;refined.report_every*=2;
// The 10 ms first-liftoff solve exceeds the 40-iteration budget. Permit
// more work in this validation profile, retaining both convergence tolerances.
refined.implicit.newton.max_iterations=80;
write('refined.config.json',refined);
for(const p of phases)write(`lift-${p.leg}${p.cycle?`-cycle-${p.cycle}`:''}.json`,{start_s:p.lift_start_s,end_s:p.land_s,
 maximum_sample_gap_s:.020001,qualifying_duration_s:.2,swing_link:markers.markers[p.leg].link,
 minimum_clearance_m:.001,maximum_swing_force_n:.1,
 minimum_support_forces_n:Object.fromEntries(markers.markers.filter((_,i)=>i!==p.leg).map(m=>[m.link,1]))});
write('manifest.json',{version:1,description:'Open-loop four-foot crawl reference with Rhai body/point feedback and effective actuators; not accepted walking or teleoperation',
 source_cad_sha256:scene.robot.source.cad_sha256,parameters:{advance_m:advance,flank_advance_m:flankAdvance,lift_m:lift,support_shift_m:shift,positive_x_shift_m:xShift,positive_x_crouch_m:crouch,negative_x_shift_m:0.009,cycles,order},
 inputs:Object.fromEntries([`${source}/scene.json`,`${source}/config.json`,`${source}/task.json`,binary,'examples/full-robot/prepare_crawl.mjs','examples/full-robot/foot-markers.json'].map(p=>[p,hash(p)])),
 maximum_planned_marker_error_m:plan.maximum_marker_error_m,phases});
console.log(JSON.stringify({prepared:out,duration_s:time,inspected_poses:plan.frames.length,maximum_marker_error_m:plan.maximum_marker_error_m}));
