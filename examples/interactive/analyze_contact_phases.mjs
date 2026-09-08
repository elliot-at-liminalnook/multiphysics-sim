import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {recordedContactMotion} from './recorded_contact_motion.mjs';
const [capturePath, output, previousContactReport] = process.argv.slice(2);
assert(capturePath && output && previousContactReport);
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const r = read(capturePath), previous = read(previousContactReport);
assert(r.completed && r.error == null && r.recording.scene.robot.world.terrain == null);
assert.equal(source(capturePath).sha256, previous.capture.sha256);
const markers = r.recording.config.policy.point_feedback.markers;
const feet = markers.map((marker, j) => {
  const index = r.recording.scene.robot.links.findIndex(l => l.name === marker.link); assert(index >= 0);
  const samples = r.frames.map(f => recordedContactMotion(f, index, marker.link));
  const groups = {}; let total = 0;
  for (let i = 1; i < samples.length; i++) {
    const a = samples[i - 1], b = samples[i], dt = b.time_s - a.time_s;
    const policy = r.frames[i].policy, reference = policy.step_reference.reference;
    assert(Math.abs(policy.time_s - a.time_s) < 1e-8, 'pair the interval with its actual held controller sample');
    if (a.force < 1 || b.force < 1) continue;
    const role = reference.foot === j ? 'selected_transfer_foot' : 'other_foot';
    const key = `${reference.phase}/${role}`;
    const group = groups[key] ??= {phase: reference.phase, role, loaded_s: 0, contact_motion_m: 0,
      shear_dissipation_j: 0, maximum_shear_to_normal_ratio: 0, normal_impulse_ns: 0};
    const motion = dt * (a.speed + b.speed) / 2;
    group.loaded_s += dt; group.contact_motion_m += motion; total += motion;
    group.shear_dissipation_j += dt * (Math.max(0, -a.shearPower) + Math.max(0, -b.shearPower)) / 2;
    group.maximum_shear_to_normal_ratio = Math.max(group.maximum_shear_to_normal_ratio, a.shear_ratio, b.shear_ratio);
    group.normal_impulse_ns += dt * (a.force + b.force) / 2;
  }
  assert.equal(total, previous.feet.find(f => f.id === marker.id).integrated_load_weighted_tangential_speed_m,
    'shared diagnostic must preserve the previous full contact integral exactly');
  return {id: marker.id, groups: Object.values(groups), total_contact_motion_m: total};
});
const phases = {};
for (const foot of feet) for (const g of foot.groups) {
  const p = phases[g.phase] ??= {contact_motion_m: 0, selected_foot_motion_m: 0, other_foot_motion_m: 0, shear_dissipation_j: 0};
  p.contact_motion_m += g.contact_motion_m;
  p[g.role === 'selected_transfer_foot' ? 'selected_foot_motion_m' : 'other_foot_motion_m'] += g.contact_motion_m;
  p.shear_dissipation_j += g.shear_dissipation_j;
}
const report = {version: 1, feet, phases, previous_contact_integrals_exact: true,
  sources: [capturePath, previousContactReport, import.meta.filename, 'examples/interactive/recorded_contact_motion.mjs'].map(source),
  scope: 'Read-only phase attribution using the controller held over each physical interval. Selected transfer foot does not mean unloaded; all contributions require both endpoints to carry at least 1 N. Across-foot sums count each contact separately. The shared kernel exactly preserves prior per-foot integrals. No new physics, acceptance threshold or causal attribution from phase correlation.'};
writeFileSync(output, JSON.stringify(report, null, 2) + '\n'); console.log(phases);
