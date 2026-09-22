// Collect immutable receipts. Missing checks never count as a pass.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
const [directory, output] = process.argv.slice(2);
assert(output, 'Usage: qualify_contact_candidate.mjs CANDIDATE_DIRECTORY NEW_REPORT');
const hash = p => crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const required = [
  ['10s/geometry-report.json', 'passes_all_finalist_checks'],
  ['10s/direction-report.json', 'pass'],
  ['long/geometry-report.json', 'passes_all_finalist_checks'],
  ['long/direction-report.json', 'pass'],
  ['half-step/check.json', 'pass'],
  ['replay/check.json', 'pass'],
  ['stop/check.json', 'pass'],
  ['stop/geometry-report.json', 'passes_all_finalist_checks'],
  ['reverse/check.json', 'pass'],
  // Returning toward the start must not be judged by the forward net-travel gate.
  // Reverse distance is independently required by reverse/check.json.
  ['reverse/geometry-report.json', ['completed', 'not_fallen', 'upright', 'command_bounds', 'tracking', 'finalist_horizon', 'foot_lift', 'sampled_collision']],
];
const receipts = required.map(([name, field]) => {
  const path = `${directory}/${name}`;
  if (!fs.existsSync(path)) return {path, available: false, pass: false};
  const r = JSON.parse(fs.readFileSync(path));
  const inputs = r.inputs ?? [r.capture, r.protocol, r.geometry].filter(Boolean);
  for (const input of inputs) assert.equal(hash(input.path), input.sha256, `Stale receipt input: ${input.path}`);
  return {path, sha256: hash(path), available: true,
    required_fields: field, pass: Array.isArray(field) ? field.every(k => r.checks[k] === true) : r[field] === true};
});
// Qualification must concern the same physical model, controller and seed.
const base = JSON.parse(fs.readFileSync(`${directory}/10s/capture.json`));
const identity = c => {
  const scene = structuredClone(c.recording.scene), config = structuredClone(c.recording.config);
  delete scene.duration_s; delete config.steps; delete config.step_s;
  return {scene, config, runtime: c.recording.runtime_identity, seed: c.recording.seed, task: c.task};
};
for (const name of ['long', 'half-step', 'stop', 'reverse', 'replay']) {
  const path = `${directory}/${name}/capture.json`;
  if (fs.existsSync(path) && fs.statSync(path).size > 0) {
    const c = JSON.parse(fs.readFileSync(path));
    assert.deepEqual(identity(c), identity(base), `Candidate identity differs: ${name}`);
    assert.equal(c.recording.config.step_s, base.recording.config.step_s / (name === 'half-step' ? 2 : 1));
  }
}
const report = {scope: 'Flat-ground simulated gait qualification for the recorded provisional CAD motor model. No browser, hardware, battery, terrain or disturbance qualification.',
  directory, cad_sha256: base.recording.scene.robot.source.cad_sha256, runtime_identity: base.recording.runtime_identity,
  seed: base.recording.seed, receipts, complete: receipts.every(r => r.available),
  qualified_in_simulation: receipts.every(r => r.pass), hardware_qualified: false, browser_promoted: false};
fs.writeFileSync(output, JSON.stringify(report, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify(report));
