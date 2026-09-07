// Author a retimed experiment using the existing Rust planner and runtime.
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const inputs={};
const hash=b=>createHash('sha256').update(b).digest('hex');
async function read(p){const b=await readFile(p);inputs[p]=hash(b);return JSON.parse(b);}
const recipe=await read('examples/full-robot/forward-rate-experiment.json'),scale=recipe.time_scale,out=recipe.output_directory;
assert(Number.isFinite(scale)&&scale>0);
const scene=await read(recipe.source_directory+'/scene.json'),original=await read(recipe.source_directory+'/config.json');
const planner=await read(recipe.planner_source),requirements=await read(recipe.lift_requirements_source);
const scaleTrajectory=p=>{for(const k of p.keyframes)k.time_s*=scale;};
scaleTrajectory(planner.displacements_world_m);if(planner.base_displacements_world_m)scaleTrajectory(planner.base_displacements_world_m);
planner.sample_period_s*=scale;
for(const r of planner.support_requirements){r.start_s*=scale;r.end_s*=scale;}
requirements.start_s*=scale;requirements.end_s*=scale;
const config=structuredClone(original),clock=config.motion_gate.clock,oldDuration=clock.duration_s;
for(const key of ['duration_s','guard_start_s','guard_end_s'])clock[key]*=scale;
config.steps=Math.round(original.steps+(clock.duration_s-oldDuration)/config.step_s);
const times=Array.from({length:Math.round(clock.duration_s/clock.period_s)+1},(_,i)=>i*clock.period_s);
await mkdir(out,{recursive:true});const outputs={};
async function write(name,data){const b=JSON.stringify(data);await writeFile(out+'/'+name,b);outputs[name]=hash(b);}
await write('scene.json',scene);await write('planner.json',planner);await write('lift-requirements.json',requirements);await write('inspection-times.json',times);
const binary='target/release/examples/plan_marker_motion';inputs[binary]=hash(await readFile(binary));
const planResult=spawnSync(binary,[out+'/scene.json','examples/full-robot/foot-markers.json',out+'/planner.json',out+'/inspection-times.json'],{encoding:'utf8',maxBuffer:64*1024*1024});
assert.equal(planResult.status,0,planResult.stderr);const plan=JSON.parse(planResult.stdout);assert(plan.completed);
assert.deepEqual(plan.source,scene.robot.source);assert.deepEqual(plan.independent_coordinates,config.motors.target_coordinates);
assert.equal(plan.trajectory.keyframes.length,original.motors.target_trajectory.keyframes.length);
let maximumKnotDifference=0;
plan.trajectory.keyframes.forEach((k,i)=>{const old=original.motors.target_trajectory.keyframes[i];assert(Math.abs(k.time_s-old.time_s*scale)<1e-10);k.values.forEach((v,j)=>maximumKnotDifference=Math.max(maximumKnotDifference,Math.abs(v-old.values[j])));});
assert(maximumKnotDifference<1e-10,'retiming changed the geometric command path');
config.motors.target_trajectory=plan.trajectory;
const refined=structuredClone(config);refined.step_s/=2;refined.steps*=2;refined.report_every*=2;
const coarse=structuredClone(config);coarse.step_s=recipe.coarse_physics_step_s;
assert(Number.isFinite(coarse.step_s)&&coarse.step_s>0);
coarse.steps=Math.round(config.steps*config.step_s/coarse.step_s);
coarse.report_every=Math.round(config.report_every*config.step_s/coarse.step_s);
assert(Math.abs(coarse.steps*coarse.step_s-config.steps*config.step_s)<1e-10);
assert(Math.abs(coarse.report_every*coarse.step_s-config.report_every*config.step_s)<1e-10);
await write('plan.json',plan);await write('config.json',config);await write('refined.config.json',refined);await write('coarse.config.json',coarse);
inputs['examples/full-robot/foot-markers.json']=hash(await readFile('examples/full-robot/foot-markers.json'));
inputs['examples/full-robot/prepare_forward_rate.mjs']=hash(await readFile('examples/full-robot/prepare_forward_rate.mjs'));
await writeFile(out+'/manifest.json',JSON.stringify({recipe,inputs,outputs,maximum_motor_knot_difference_rad:maximumKnotDifference},null,2));
console.log(`Prepared ${scale}x duration experiment with ${plan.frames.length} inspected poses; maximum command-knot difference ${maximumKnotDifference} rad`);
