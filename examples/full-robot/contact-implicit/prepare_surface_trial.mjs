// Configuration/provenance only; shared Rust provides surface geometry and audits.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/contact-implicit/';
const read = file => JSON.parse(fs.readFileSync(root + file));
const sha = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const source = read('diagonal25-refinement.result.json');
const audit = read('diagonal25-refinement.audit.json');
const recipe = read('diagonal25-scaled.recipe.json');
assert.deepEqual(audit.planning, source.stages.at(-1).result.report);
// Select the first existing audited knot without sampled self-overlap. Its joint
// pose alone is reused; no moving trajectory or contact sequence is inherited.
const frames = audit.geometry.filter((_, index) => index % 5 === 0);
const index = frames.findIndex(frame => frame.maximum_inter_link_penetration_m === 0);
assert(index >= 0, 'a sampled collision-free initial pose is required');
const q = source.positions[index].slice();
const minimumClearance = Math.min(...frames[index].floor_clearances.map(p => p.minimum_clearance_m));
const initialClearance = .00002;
const translationZ = initialClearance - minimumClearance;
q[2] += translationZ; // Explicit pose change from the Rust geometry measurement.
const count = source.positions.length, n = q.length, speed = .25;
recipe.initial_positions = Array.from({length: count}, () => q.slice());
recipe.config.initial_velocity = Array(n).fill(0);
recipe.config.position_reference = Array.from({length: count}, (_, k) => {
  const p = q.slice();
  p[0] += speed / Math.sqrt(2) * k * recipe.config.step_s;
  p[1] += speed / Math.sqrt(2) * k * recipe.config.step_s;
  return p;
});
recipe.config.velocity_reference = [speed / Math.sqrt(2), speed / Math.sqrt(2), ...Array(n - 2).fill(0)];
recipe.bounds = recipe.config.position_reference.slice(1).map(p => p.map((v, i) => i < 6
  ? {lower: v - (i < 2 ? .12 : i === 2 ? .06 : .3), upper: v + (i < 2 ? .12 : i === 2 ? .06 : .3)}
  : recipe.bounds[0][i]));
for (let k = 1; k < count; k++) for (let i = 0; i < n; i++)
  assert(q[i] >= recipe.bounds[k-1][i].lower && q[i] <= recipe.bounds[k-1][i].upper);
recipe.stiffness_schedule_n_m = [2000, 10000, 50000, 200000];
recipe.smoothing_schedule_m = [.003, .001, .0001, .00001];
recipe.hessian_scaling_exponent = .25;
recipe.provenance = {
  source: root + 'diagonal25-refinement.result.json', source_sha256: sha(root + 'diagonal25-refinement.result.json'),
  geometry_audit: root + 'diagonal25-refinement.audit.json', audit_sha256: sha(root + 'diagonal25-refinement.audit.json'),
  surface_markers: root + 'surface-markers.json', surface_markers_sha256: sha(root + 'surface-markers.json'),
  scene: 'runs/speed-ceiling/validation/constrained-front165-scale1-human-fine.scene.json',
  scene_sha256: sha('runs/speed-ceiling/validation/constrained-front165-scale1-human-fine.scene.json'),
  source_pose_knot: index, initial_base_translation_z_m: translationZ,
  intended_initial_minimum_sampled_floor_clearance_m: initialClearance,
  target_speed_m_s: speed,
  initial_condition: 'Stationary repeated pose, zero initial velocity. First audited knot with no sampled interlink overlap; base translated explicitly using measured floor clearance. No gait timing, joint trajectory or contact schedule retained.',
  contact_model: 'Every compiled CAD foot surface sample, using the exact shared runtime sampling locations. Stiffness is per sample as in the runtime. Smooth IDTO contact and local regularized friction remain explicit planning approximations; no bristle patch state or runtime contact-law equivalence claimed.',
  limitations: 'Initial and final geometry must be independently audited. Finite horizon, no periodicity or terminal viability guarantee, no detailed runtime execution, no physical speed maximum.'
};
for (const [name, value] of [['surface25.recipe.json', recipe], ['surface25-initial.positions.json', {positions: recipe.initial_positions}]])
  fs.writeFileSync(root + name, JSON.stringify(value, null, 2) + '\n', {flag:'wx'});
console.log({source_pose_knot: index, base_shift_m: translationZ, intervals: count-1,
  contact_samples: read('surface-markers.json').markers.length, target_speed_m_s:speed});
