// Author robot-specific binding recipes; all materialization and execution is Rust.
// Input specs are durable, previously checked captures restored from the experiment manifest.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
const [inputDirectory, outputDirectory] = process.argv.slice(2);
assert(outputDirectory, 'usage: prepare_coordinated_motion input-spec-directory fresh-output-directory');
fs.mkdirSync(outputDirectory); // Refuse to overwrite a previous acceptance run.
const constant = value => ({source:'constant', value});
const factor = {source:'parameter', name:'time_factor'};
const scaled = (value, power) => ({source:'scaled', value:constant(value), factor, power});
const checked = pointer => ({source:'controller_parameter', pointer, kind:'Time'});
const write = (name, value) => fs.writeFileSync(path.join(outputDirectory, name+'.json'), JSON.stringify(value)+'\n', {flag:'wx'});
for (const robot of ['quadruped', 'wheeled']) {
  const spec = JSON.parse(fs.readFileSync(path.join(inputDirectory, robot+'-spec.json')));
  const original = structuredClone(spec);
  const p = spec.scene.controller.parameters;
  const recipe = spec.parameterization;
  recipe.space.parameters.push({name:'time_factor', kind:'Dimensionless', bounds:[0.5,1.5]});
  recipe.scalars = [];
  recipe.checks = [];
  function bind(pointer, kind, power) {
    const reference = pointer.slice(1).split('/').reduce((v,k)=>v[k], p);
    assert(Number.isFinite(reference), 'existing numeric reference required: '+pointer);
    recipe.scalars.push({pointer, kind, reference, value:scaled(reference, power)});
  }
  function command(name) {
    const input = spec.scene.controller.inputs.find(c=>c.name===name);
    assert(input, 'missing command '+name);
    recipe.commands.push({input:name, kind:input.kind, scale:scaled(1,-1), center:constant(0), offset:constant(0)});
  }
  if (robot === 'quadruped') {
    recipe.trajectories[0].template.transforms.push({operation:'time_scale', factor});
    for (const name of ['period_s','initial_phase_s','phase_offset_s','reversal_settle_s']) bind('/'+name,'Time',1);
    bind('/motion/period_s','Time',1);
    p.motion.body.keyframes.forEach((_,i)=>bind(`/motion/body/keyframes/${i}/time_s`,'Time',1));
    p.pause_windows_s.forEach((window,i)=>window.forEach((_,j)=>bind(`/pause_windows_s/${i}/${j}`,'Time',1)));
    p.velocity_lead_s.forEach((_,i)=>bind(`/velocity_lead_s/${i}`,'Time',1));
    bind('/nominal_speed_m_s','LinearVelocity',-1);
    for (const name of ['acceleration_m_s2','deceleration_m_s2']) bind('/'+name,'LinearAcceleration',-2);
    for (const name of ['command.forward_speed','command.lateral_speed','command.yaw_rate']) command(name);
    const pointers = ['/motion/period_s',`/motion/body/keyframes/${p.motion.body.keyframes.length-1}/time_s`,
      `/trajectory/keyframes/${p.trajectory.keyframes.length-1}/time_s`];
    recipe.checks = pointers.map((pointer,i)=>({name:'cycle_clock_'+i,left:checked('/period_s'),right:checked(pointer),tolerance:0}));
  } else {
    // This controller has no gait cycle. Keep the integrator timestep fixed;
    // independently demonstrate scalar actuation through initial target angles.
    for (const name of ['initial_left','initial_right']) bind('/'+name,'Angle',1);
    for (const input of spec.scene.controller.inputs) command(input.name);
    recipe.checks = [{name:'integration_clock',left:checked('/period_s'),right:{source:'scene_period'},tolerance:0}];
  }
  spec.baseline.time_factor = 1;
  write(robot+'-identity-spec', spec);
  spec.baseline.time_factor = 0.8;
  write(robot+'-changed-spec', spec);
  write(robot+'-original-spec', original);
}
write('scope', {time_factor:0.8, quad_reference_rate_factor:1.25, quad_reference_acceleration_factor:1.5625,
  unchanged:'CAD robot, world, physics configuration, episode horizon, sampling clock and protocol lease',
  meaning:'Reference coordination, not physical dynamic similarity. Gravity, contact and actuator physics remain active. Short sensitivity and replay acceptance, not sustained locomotion or a speed record.'});
