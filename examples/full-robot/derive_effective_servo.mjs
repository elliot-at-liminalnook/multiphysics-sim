// Explicit browser-profile experiment. Physics remains in the shared Rust runtime.
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const directory=process.argv[2]||'runs/full-robot/learning/effective-servo';
const step=Number(process.argv[3]||0.001);
const period=Number(process.argv[4]||0.002);
assert(step>0&&period>=step&&Math.abs(period/step-Math.round(period/step))<1e-9);
const source='examples/full-robot/teacher-baseline';
const read=p=>JSON.parse(readFileSync(p));
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const scene=read(`${source}/scene.json`),config=read(`${source}/config.json`);
const task=read('examples/full-robot/teacher-environment.json');
const components=config.motors.target_coordinates.map(dof=>{
  const matches=scene.robot.motors.filter(m=>`joint.${m.joint}`===dof);
  assert.equal(matches.length,1,`exactly one CAD actuator required for ${dof}`);
  const m=matches[0];assert.equal(m.firmware.output,'voltage');assert.equal(m.gear_ratio,1);
  // Firmware gains are volts/rad and volts/(rad/s). Kt/R maps volts to Nm.
  // This is a local static approximation, not identification of loaded response.
  const gain=m.electrical.torque_constant/m.electrical.resistance;
  return {dof,parameters:{stiffness:m.firmware.kp*gain,damping:m.firmware.kd*gain,
    stall_torque:m.stall_torque,no_load_speed:m.no_load_speed}};
});
delete config.motors.events;
config.audit_contact_steps=false;
// Browser-only numerical acceptance. Keep the detailed reference unchanged.
// A 1e-10 raw mechanical residual threshold reaches platform rounding noise.
config.implicit.newton.absolute_tolerance=1e-8;
const horizon=config.steps*config.step_s;
config.step_s=step;
config.steps=Math.round(horizon/step);
config.report_every=Math.max(1,Math.round(0.02/step));
scene.period_s=period;
config.motion_gate.clock.period_s=period;
// Gate events must lie on the selected controller grid. Record this adaptation.
for(const key of ['guard_start_s','guard_end_s','qualification_s','maximum_pause_s'])
  config.motion_gate.clock[key]=Math.round(config.motion_gate.clock[key]/period)*period;
config.motors.effective={version:1,
  assumption_reference:'examples/full-robot/effective-servo.md; uncalibrated static gain derivation from versioned CAD export',components};
// The effective model has no electrical state; do not invent current observations.
task.observations=task.observations.filter(o=>o.source.kind!=='motor_current');
mkdirSync(directory,{recursive:true});
for(const [name,value] of Object.entries({scene,config,task}))
  writeFileSync(`${directory}/${name}.json`,JSON.stringify(value,null,name==='scene'?0:2)+'\n');
writeFileSync(`${directory}/derivation.json`,JSON.stringify({version:1,
  source_cad_sha256:scene.robot.source.cad_sha256,
  inputs:Object.fromEntries([`${source}/scene.json`,`${source}/config.json`,
    'examples/full-robot/teacher-environment.json','examples/full-robot/derive_effective_servo.mjs'].map(p=>[p,hash(p)])),
  approximations:['Static bounded PD torque with linear motoring torque-speed envelope',
    'No electrical, thermal, internal rotor, firmware timing, quantization, backlash or gearbox compliance states',
    'Effective damping is not an identified gearbox friction/efficiency model'],
  integration:{step_s:step,controller_period_s:period,motion_clock:config.motion_gate.clock,
    newton:config.implicit.newton},
  preserved:['Source robot definition','All mechanism bodies and closure constraints','Gravity and contact','Reference controller source'],
  components},null,2)+'\n');
console.log(`Wrote explicit effective-servo profile to ${directory}`);
