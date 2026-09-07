// Add controller actions and teacher observations to the accepted slow crawl.
// Physical CAD data and the shared Rust dynamics are unchanged.
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const output = process.argv[2] || 'examples/full-robot/browser-residual-policy';
const read = p => JSON.parse(readFileSync(p));
const scenePath = 'examples/full-robot/browser-terrain-contact/scene.json';
const configPath = 'examples/full-robot/browser-reversal/config.json';
const taskPath = 'examples/full-robot/browser-reversal/task.json';
const scene = read(scenePath), config = read(configPath), task = read(taskPath);
const coordinates = config.motors.target_coordinates;
assert.equal(coordinates.length, 12);
const bindings = coordinates.map(coordinate => {
  const joint = coordinate.replace(/^joint\./, '');
  return {coordinate, target: `${joint}.target`, input: `residual.${joint}`, scale_rad: .01};
});
scene.controller.parameters.residual_input_by_target = Object.fromEntries(bindings.map(b => [b.target, b.input]));
const old = scene.controller.sources.files[scene.controller.sources.entry];
const needle = '+ sensors["command.point_gain"] * sensors[joint + ".point_correction"];';
assert(old.includes(needle));
scene.controller.sources = {entry: 'residual-crawl.rhai', files: {
  'residual-crawl.rhai': old.replace(needle, `${needle}
        // Residual is an angle command, still subject to software/CAD bounds.
        // Preserve the original arithmetic exactly when no correction is requested.
        let residual = sensors[p.residual_input_by_target[name]];
        if residual != 0.0 { commands[name] += residual; }`),
}};
// These baseline gains are fixed; the learner controls motor residuals.
for (const c of scene.controller.inputs.slice(0, 3)) c.lower = c.upper = c.initial;
for (const b of bindings) scene.controller.inputs.push({name: b.input, kind: 'Angle', lower: -b.scale_rad, upper: b.scale_rad, initial: 0});
const add = (name, source) => task.observations.push({name, source});
const reward = (name, observation, scale, weight) => task.rewards.push({name, observation, target: {kind: 'constant', value: 0}, scale, weight_per_s: weight});
for (const [i, b] of bindings.entries()) {
  assert.equal(`joint.${scene.robot.motors[i].joint}`, b.coordinate);
  assert.equal(config.motors.effective.components[i].dof, b.coordinate);
  add(`motor.${i}.residual`, {kind: 'controller_input', name: b.input});
  add(`motor.${i}.torque`, {kind: 'motor_torque', motor: scene.robot.motors[i].name});
  reward(`motor.${i}.residual_size`, `motor.${i}.residual`, b.scale_rad, .001);
  reward(`motor.${i}.effort`, `motor.${i}.torque`, config.motors.effective.components[i].parameters.stall_torque, .002);
}
const body = config.policy.body_feedback.reference_link;
for (const [i, marker] of config.policy.task_observations.markers.entries()) {
  for (const axis of ['x', 'y', 'z']) add(`foot.${i}.force.${axis}`, {kind: 'floor_force', link: marker.link, axis});
}
for (const axis of ['x', 'y', 'z']) {
  add(`body.local_velocity.${axis}`, {kind: 'body_local_velocity', link: body, axis});
  add(`body.angular_velocity.${axis}`, {kind: 'body_angular_velocity', link: body, axis});
  // World vertical expressed in the body frame: R^T * world_Z.
  // This gives the teacher an ideal gravity direction, not a measured IMU.
  add(`body.up.${axis}`, {kind: 'body_axis', link: body, body_axis: axis, world_axis: 'z'});
}
for (const name of config.policy.step_reference.command_channels) add(name, {kind: 'controller_input', name});
reward('body.upright.x', 'body.up.x', .05, .5);
reward('body.upright.y', 'body.up.y', .05, .5);
task.termination_bounds.push({observation: 'body.up.z', lower: .95, upper: 1.000000001});
task.survival_reward_per_s = 1;
task.termination_penalty = 10;
assert.equal(new Set(task.observations.map(o => o.name)).size, task.observations.length);
mkdirSync(output, {recursive: true});
const write = (name, value) => writeFileSync(`${output}/${name}.json`, JSON.stringify(value) + '\n');
write('scene', scene); write('config', config); write('task', task);
const short = structuredClone(config); short.steps = Math.round(24 / short.step_s); write('short.config', short);
for (const name of ['forward-reverse', 'sustained']) {
  const actions = read(`examples/full-robot/browser-reversal/${name}.actions.json`).map(a => [...a, ...bindings.map(() => 0)]);
  write(`${name}.actions`, actions);
  if (name === 'forward-reverse') {
    const probe = structuredClone(actions);
    for (let i = 0; i < bindings.length; i++) {
      for (let k = 0; k < 2; k++) probe[20 + i * 3 + k][6 + i] = (i % 2 ? -1 : 1) * .001;
    }
    write('probe.actions', probe);
  }
}
write('learning', {version: 1, policy_action_bindings: bindings,
  command_input_names: config.policy.step_reference.command_channels,
  fixed_inputs: Object.fromEntries(scene.controller.inputs.slice(0, 3).map(c => [c.name, c.initial])),
  scope: 'Teacher residual-action commissioning, not a trained policy. Actions adjust motor angle targets after baseline feedback and retain every command bound. Ideal observations remain privileged. Reward supports initial reference tracking and uprightness; it is not a validated terrain-walking objective. Motion commands condition the task and must not be chosen by the residual learner.'});
const paths = [scenePath, configPath, taskPath, 'examples/full-robot/prepare_residual_policy.mjs',
  ...['forward-reverse', 'sustained'].map(n => `examples/full-robot/browser-reversal/${n}.actions.json`)];
write('manifest', {version: 1, source_cad_sha256: scene.robot.source.cad_sha256,
  inputs: Object.fromEntries(paths.map(p => [p, createHash('sha256').update(readFileSync(p)).digest('hex')])),
  residual_count: bindings.length, residual_unit: 'rad', residual_bound_rad: .01,
  scope: 'Same slow-crawl physical definition, limits and numerical tolerances; explicit Rhai controller inputs and teacher task changes only. Zero residuals must reproduce the accepted baseline.'});
console.log(output);
