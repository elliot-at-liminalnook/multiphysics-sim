import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const directory='runs/full-robot/learning/fast-motor',inputs={},outputs={};
async function read(path){const b=await readFile(path);inputs[path]=createHash('sha256').update(b).digest('hex');return JSON.parse(b);}
const scene=await read('runs/full-robot/learning/point-feedback/scene.json');
const source=await read('runs/full-robot/learning/sample-reuse/final-refresh.base.config.json');
assert(!scene.options.motor_dynamics || scene.options.motor_dynamics==='detailed');
assert(source.implicit.newton.refresh_before_iteration_limit);
const config=structuredClone(source);
config.step_s=0.001;config.steps=2800;config.report_every=10;
await mkdir(directory,{recursive:true});
for(const mode of ['quasistatic_winding','quasistatic_rotor','quasistatic']){
 const candidate=structuredClone(scene);candidate.options.motor_dynamics=mode;
 const b=JSON.stringify(candidate);await writeFile(`${directory}/${mode}.scene.json`,b);
 outputs[`${mode}.scene.json`]=createHash('sha256').update(b).digest('hex');
}
for(const [name,value] of [['config.json',config],['refined.config.json',{...config,step_s:0.0005,steps:5600,report_every:20}]]){
 const b=JSON.stringify(value);await writeFile(directory+'/'+name,b);outputs[name]=createHash('sha256').update(b).digest('hex');
}
inputs['examples/full-robot/prepare_fast_motor.mjs']=createHash('sha256').update(await readFile(import.meta.filename)).digest('hex');
await writeFile(directory+'/manifest.json',JSON.stringify({
 scope:'Physical reduction experiment, not numerical equivalence. The robot CAD, masses, linkage geometry, transmissions, firmware, delays, driver limits, friction, compliance, backlash law, world and controller are unchanged. Explicit modes omit winding storage, internal rotor/gear inertia, or both. Initial and event algebraic consistency and loaded reversals need validation. No training or hardware acceptance is implied.',
 inputs,outputs,
 preliminary_screen:{maximum_foot_difference_m:0.0005,rms_foot_difference_m:0.0002,maximum_motor_angle_difference_rad:0.005,
   maximum_relative_per_foot_impulse_difference:0.01,absolute_impulse_floor_ns:0.001,
   sampled_supported_lift_required:true,
   scope:'Provisional simulation-only screening limits, chosen before the robot run. Current spikes, electrical energy/heat, contact-event timing and timestep sensitivity must additionally be evaluated before any promotion.'}
},null,2));
console.log('Prepared explicit motor reductions at 1 ms and 0.5 ms; physical source values preserved.');
