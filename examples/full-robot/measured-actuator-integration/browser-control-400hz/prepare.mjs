// Explicit experimental profile over the shared CAD-derived Rust environment.
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root=path.resolve(import.meta.dirname,'../../../..'),out=import.meta.dirname;
const read=p=>JSON.parse(fs.readFileSync(path.join(root,p)));
const hash=p=>createHash('sha256').update(fs.readFileSync(path.join(root,p))).digest('hex');
const parent='examples/full-robot/measured-actuator-integration/gait-generation/full-authority-10s/recording.json';
const record=read(parent),scene=record.runtime.scene,config=record.runtime.config,task=record.task;
const originals=structuredClone(scene.robot.actuator_profiles),base=Object.values(originals.families)[0];
const identity=JSON.parse(fs.readFileSync(path.join(out,'controller-identity.json')));
assert.equal(hash(identity.path),identity.sha256,'Controller changed: repeat Rust/RTL checks and regenerate the explicit implementation identity.');
const evidence={version:1,simulation_only:true,parent:{path:parent,sha256:hash(parent)},previous_overrides:scene.robot.source.experimental_overrides,
  control_hz:400,measurement_hz:200,outer_policy_hz:50,latency_assumption_s:0.00125,controller_identity:identity,
  mapping:'Hip, worm and foot roles reuse fitted candidates 10, 11 and 12 on every leg. This is an experimental template assignment, not identification of twelve physical motors.',
  limitations:['All candidate fits remain provisional and failed the historical closed-loop calibration threshold.','Motor feedback latency is assumed, not measured internal sensor age.','Three-motor UART timing does not qualify a twelve-motor physical bus.','Full-scale PWM and imposed 11.1 V are simulation settings, not a commissioned hardware mode.'],models:[]};
const profiles={version:originals.version,bindings:structuredClone(originals.bindings),families:{}};
for(const id of [10,11,12]){
  const source=`examples/full-robot/measured-actuator-integration/controller-tracking-full-drive-simulation/id${id}-gait-nominal-selected-1000.json`;
  const fit=read(source).experiment.model,family=structuredClone(base),key=`hx30hm-fit-${id}-400hz`;
  family.description=`Provisional motor ${id} bench-fit candidate with a simulated 400 Hz FPGA integer controller`;
  family.evidence.fit={path:source,sha256:hash(source),scope:'Exact parameter set used in the 54-case cadence study; not an accepted hardware calibration.'};
  for(const [name,parameter] of Object.entries(family.motor)){assert(name in fit.motor,name);parameter.value=fit.motor[name];parameter.evidence='fit';parameter.provenance='derived';}
  for(const [name,parameter] of Object.entries(family.driver)){assert(name in fit.bridge,name);parameter.value=fit.bridge[name];parameter.evidence='fit';parameter.provenance='derived';}
  family.controller.period={value:0.0025,unit:'s',provenance:'derived',uncertainty:null,evidence:'browser_400hz'};
  family.controller.latency={value:0.00125,unit:'s',provenance:'estimated',uncertainty:null,evidence:'browser_400hz'};
  family.controller.evidence='browser_400hz';
  family.controller.implementation_blake3=identity.blake3;
  family.evidence.controller.sha256=identity.sha256;
  family.limitations=[...evidence.limitations];
  profiles.families[key]=family;evidence.models.push({id,family:key,path:source,sha256:hash(source)});
}
const receipt='examples/full-robot/measured-actuator-integration/browser-control-400hz/overrides.json';
fs.writeFileSync(path.join(root,receipt),JSON.stringify(evidence,null,2)+'\n');
for(const f of Object.values(profiles.families))f.evidence.browser_400hz={path:receipt,sha256:hash(receipt),scope:'Explicit experimental cadence and latency; preserve source CAD and prior qualification.'};
for(let i=0;i<scene.robot.motors.length;i++){
  const motor=scene.robot.motors[i],id=motor.name.includes('Hip swing')?10:motor.name.includes('Worm drive')?11:motor.name.includes('Foot slide')?12:null;
  assert(id, motor.name);profiles.bindings[motor.id].family=`hx30hm-fit-${id}-400hz`;
}
scene.robot.actuator_profiles=profiles;scene.robot.source.experimental_overrides={path:receipt,sha256:hash(receipt)};
// Keep the authored 50 Hz motion policy separate from the 400 Hz motor loop.
config.steps=384000;config.report_every=32; // 60 s; 5 ms native measurement interval.
assert.equal(config.step_s,0.00015625);assert.equal(scene.period_s,0.02);assert.equal(task.period_s,0.02);
scene.duration_s=60;scene.options.analytic_motor_jacobian=true;
for(const c of scene.controller.inputs)if(['command.forward_speed','command.lateral_speed','command.yaw_rate','command.packet_sequence'].includes(c.name))c.initial=0;
for(const [name,data] of [['scene.json',scene],['config.json',config],['task.json',task]])fs.writeFileSync(path.join(out,name),JSON.stringify(data)+'\n');
console.log(JSON.stringify({motors:scene.robot.motors.length,models:evidence.models,control_hz:400,native_measurement_hz:200,outer_policy_hz:50}));
