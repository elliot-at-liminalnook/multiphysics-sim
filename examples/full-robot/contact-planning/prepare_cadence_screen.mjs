// Experiment assembly only; the existing Rust runtime and Rhai policy own execution.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
const [source,name,speedArg,stepArg]=process.argv.slice(2),speed=Number(speedArg);
assert(source&&/^[a-z0-9-]+$/.test(name)&&Number.isFinite(speed)&&speed>0,
 'usage: prepare_cadence_screen.mjs source-prefix fresh-name command-speed');
const dir='examples/full-robot/contact-planning/',prefix='runs/contact-planning/'+name;
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const scene=read(source+'.scene.json'),config=read(source+'.config.json'),actions=read(source+'.actions.json');
const original=structuredClone(scene),index=scene.controller.inputs.findIndex(i=>i.name==='command.forward_speed');
assert(index>=0&&scene.duration_s===8&&actions.length*scene.period_s===8);
assert(config.step_s===.000625,'use the detailed baseline for this cadence screen');
const nominal=scene.controller.parameters.nominal_speed_m_s;
if(stepArg!==undefined){
 const step=Number(stepArg);
 assert(Number.isFinite(step)&&step>0&&step<=config.step_s&&
  Number.isInteger(scene.duration_s/step)&&Number.isInteger(scene.period_s/step),
  'refinement must exactly divide episode and controller periods');
 config.step_s=step;config.steps=scene.duration_s/step;config.report_every=scene.period_s/step;
}
scene.controller.inputs[index].lower=-speed;scene.controller.inputs[index].upper=speed;
for(const a of actions){assert(a[index]===0||Math.abs(a[index])===nominal);a[index]=Math.sign(a[index])*speed;}
const restored=structuredClone(scene);restored.controller.inputs[index]=original.controller.inputs[index];
assert(isDeepStrictEqual(restored,original),'only the commanded speed range may change');
const files=['scene','config','actions'].map(kind=>prefix+'.'+kind+'.json');
const specPath=dir+name+'-trial.json';
assert([...files,specPath].every(p=>!fs.existsSync(p)),'fresh output paths required');
for(const [i,value] of [scene,config,actions].entries())fs.writeFileSync(files[i],JSON.stringify(value)+'\n',{flag:'wx'});
fs.writeFileSync(specPath,JSON.stringify({prefix,source,command_speed_m_s:speed,
 reference_nominal_speed_m_s:nominal,clock_rate:speed/nominal,physics_step_s:config.step_s,
 steady_reference_velocity_scale:speed/nominal,steady_reference_acceleration_scale:(speed/nominal)**2,
 windows_s:[[.8,3],[4.2,6.4]],task:'examples/full-robot/fast-wasd/task.json',
 sources:['scene','config','actions'].map(kind=>({path:source+'.'+kind+'.json',sha256:hash(source+'.'+kind+'.json')})),
 files:files.map(path=>({path,sha256:hash(path)})),
 scope:'Direct closed-loop speed experiment. Motion-command bounds and scheduled speed change, with optional explicit timestep refinement; CAD, world, actuator limits, joint bounds, controller code, reference and solver settings are preserved. Existing odd/even feedforward scales with clock rate; off-nominal loads and transitions remain approximate. Inherited failed reference audits remain explicit. No collision/slip or sustained qualification is implied.'
},null,2)+'\n',{flag:'wx'});
console.log({prefix,speed,clock_rate:speed/nominal});
