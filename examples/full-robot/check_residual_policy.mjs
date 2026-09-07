// Verify neutral actions preserve physics and probe observations reach endpoints.
import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [baselinePath, zeroPath, probePath, output] = process.argv.slice(2);
assert(output, 'usage: baseline-capture zero-capture probe-capture report.json');
const read = p => JSON.parse(readFileSync(p));
const digest = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const baseline = read(baselinePath), zero = read(zeroPath), probe = read(probePath);
const spec = read('examples/full-robot/browser-residual-policy/learning.json');
const task = read('examples/full-robot/browser-residual-policy/task.json');
for (const c of [zero, probe]) {
  assert(c.completed && !c.error);
  assert.deepEqual(c.task, task, 'capture must use the current teacher task');
  assert.deepEqual(c.recording.scene.robot, baseline.recording.scene.robot);
  assert.deepEqual(c.recording.scene.options, baseline.recording.scene.options);
}
assert.deepEqual(zero.recording.config, baseline.recording.config);
assert.equal(zero.frames.length, baseline.frames.length);
for (let i = 0; i < baseline.frames.length; i++) {
  const a = baseline.frames[i], b = zero.frames[i];
  assert.deepEqual(b.policy_inputs.slice(0, 6), a.policy_inputs);
  assert(b.policy_inputs.slice(6).every(v => v === 0));
  for (const [key, value] of Object.entries(a)) {
    if (['stepping_wall_s', 'policy_inputs'].includes(key)) continue;
    if (key === 'policy') {
      for (const [name, value] of Object.entries(a.policy ?? {})) {
        if (name === 'observations') {
          for (const [n, v] of Object.entries(value)) assert.equal(b.policy.observations[n], v);
        } else assert.deepEqual(b.policy[name], value);
      }
    } else assert.deepEqual(b[key], value, `physical frame ${i}.${key}`);
  }
}
const exercised = new Set();
for (const c of [zero, probe]) {
  for (let f = 0; f < c.frames.length; f++) {
    const frame = c.frames[f], transition = c.transitions[f];
    assert.equal(frame.time_s, transition.time_s);
    assert.equal(transition.reward, transition.reward_terms.reduce((sum, t) => sum + t.value, 0));
    for (const [i, binding] of spec.policy_action_bindings.entries()) {
      const action = c.contract.actions.findIndex(a => a.name === binding.input);
      const observation = c.contract.observations.findIndex(o => o.name === `motor.${i}.residual`);
      const torque = c.contract.observations.findIndex(o => o.name === `motor.${i}.torque`);
      assert(action >= 0 && observation >= 0 && torque >= 0);
      assert.equal(c.contract.observations[observation].unit, 'rad');
      assert.equal(c.contract.observations[torque].unit, 'N·m');
      assert.equal(transition.observations[observation], frame.policy_inputs[action]);
      assert.equal(transition.observations[torque], frame.motor_readings[i].shaft_torque_nm);
      if (c === probe && frame.policy_inputs[action] !== 0) exercised.add(binding.input);
    }
  }
}
assert.equal(exercised.size, spec.policy_action_bindings.length);
writeFileSync(output, JSON.stringify({
  version: 1, passed: true, baseline: digest(baselinePath), zero: digest(zeroPath), probe: digest(probePath),
  exact_original_physical_frames: zero.frames.length, zero_simulated_s: zero.frames.at(-1).time_s,
  observed_motor_residuals: exercised.size, teacher_observation_count: probe.contract.observations.length,
  zero_total_reward: zero.transitions.reduce((s, t) => s + t.reward, 0),
  probe_total_reward: probe.transitions.reduce((s, t) => s + t.reward, 0),
  scope: 'No trained policy. Neutral residuals preserve physical frames and original controller values exactly; added task scores/observations are deliberately different. All twelve residual inputs and torque observations are checked at committed endpoints. Walking, geometry and support acceptance are separate reports.',
}, null, 2) + '\n');
console.log(output);
