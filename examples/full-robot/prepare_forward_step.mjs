// Compose a captured reference experiment. Physics/control remain in Rust.
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const out='runs/full-robot/learning/forward-10mm',inputs={};
async function read(p){const b=await readFile(p);inputs[p]=createHash('sha256').update(b).digest('hex');return JSON.parse(b);}
const scene=await read('runs/full-robot/learning/landing-checkpoint/scene.json');
const config=await read('runs/full-robot/learning/landing-checkpoint/config.json');
const plan=await read(out+'/plan.json');
await read('examples/full-robot/single-foot-marker-motion-forward-10mm.json');
inputs['examples/full-robot/prepare_forward_step.mjs']=createHash('sha256').update(await readFile('examples/full-robot/prepare_forward_step.mjs')).digest('hex');
assert(plan.completed);assert.deepEqual(plan.source,scene.robot.source);
assert.deepEqual(plan.independent_coordinates,config.motors.target_coordinates);
assert.deepEqual(plan.initial_coordinates,config.initial_coordinates);
config.motors.target_trajectory=plan.trajectory;
const refined=structuredClone(config);refined.step_s/=2;refined.steps*=2;refined.report_every*=2;
const clock=config.motion_gate.clock;
const inspectionTimes=Array.from({length:Math.round(clock.duration_s/clock.period_s)+1},(_,i)=>i*clock.period_s);
await mkdir(out,{recursive:true});const outputs={};
for(const [name,data]of [['scene.json',scene],['config.json',config],['refined.config.json',refined],['inspection-times.json',inspectionTimes]]){const b=JSON.stringify(data);await writeFile(out+'/'+name,b);outputs[name]=createHash('sha256').update(b).digest('hex');}
await writeFile(out+'/manifest.json',JSON.stringify({inputs,outputs,scope:'One foot moves 10 mm in export-world +X and remains there; same provisional mechanics, gain-0.5 policy and support checkpoint. Not a completed walking stride or calibrated controller.'},null,2));
console.log('Prepared forward placement motor experiment');
