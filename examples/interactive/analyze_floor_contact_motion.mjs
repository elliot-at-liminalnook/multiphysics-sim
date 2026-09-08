// Read-only diagnostic from recorded rigid-body velocities and contact forces.
import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {captureOutcome} from './capture_outcome.mjs';
const [capturePath, output] = process.argv.slice(2);
assert(capturePath && output, 'usage: analyze_floor_contact_motion.mjs capture.json report.json');
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const r = JSON.parse(readFileSync(capturePath)), outcome = captureOutcome(r);
assert(r.completed, 'Full completed capture required for this diagnostic.');
assert(r.recording.scene.robot.world.terrain == null, 'This diagnostic assumes the recorded stationary horizontal floor.');
const markers = r.recording.config.policy.point_feedback.markers;
const links = r.recording.scene.robot.links;
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const minimumForce = 1;
const feet = markers.map(marker => {
  const index = links.findIndex(l => l.name === marker.link); assert(index >= 0);
  const samples = r.frames.map(frame => {
    const pose = frame.poses.find(p => p.name === marker.link); assert(pose);
    const contacts = frame.contacts.filter(c => c.link === index && c.other == null);
    let force = 0, weightedVelocity = [0, 0], pointSpeed = 0, shearPower = 0;
    for (const c of contacts) {
      const spin = cross(pose.angular_velocity_rad_s, c.point_m.map((v, j) => v - pose.position_m[j]));
      const v = pose.velocity_m_s.map((x, j) => x + spin[j]);
      const normal = c.force_n[2]; assert(normal >= 0);
      force += normal; pointSpeed += normal * Math.hypot(v[0], v[1]);
      for (let j = 0; j < 2; j++) weightedVelocity[j] += normal * v[j];
      shearPower += c.force_n[0] * v[0] + c.force_n[1] * v[1];
    }
    if (force > 0) { weightedVelocity = weightedVelocity.map(v => v / force); pointSpeed /= force; }
    return {time_s: frame.time_s, force, velocity: weightedVelocity,
      speed: Math.hypot(...weightedVelocity), pointSpeed, shearPower};
  });
  let duration = 0, path = 0, pointPath = 0, work = 0, suppliedWork = 0, drift = [0, 0];
  for (let i = 1; i < samples.length; i++) {
    const a = samples[i - 1], b = samples[i], dt = b.time_s - a.time_s;
    assert(dt > 0);
    if (a.force < minimumForce || b.force < minimumForce) continue;
    duration += dt; path += dt * (a.speed + b.speed) / 2;
    pointPath += dt * (a.pointSpeed + b.pointSpeed) / 2;
    for (let j = 0; j < 2; j++) drift[j] += dt * (a.velocity[j] + b.velocity[j]) / 2;
    work += dt * (Math.max(0, -a.shearPower) + Math.max(0, -b.shearPower)) / 2;
    suppliedWork += dt * (Math.max(0, a.shearPower) + Math.max(0, b.shearPower)) / 2;
  }
  const loaded = samples.filter(s => s.force >= minimumForce);
  assert(loaded.length > 0);
  return {id: marker.id, link: marker.link, sampled_loaded_duration_s: duration,
    integrated_load_weighted_tangential_speed_m: path,
    integrated_load_weighted_point_speed_m: pointPath,
    integrated_load_weighted_velocity_world_m: drift,
    maximum_load_weighted_tangential_speed_m_s: Math.max(...loaded.map(s => s.speed)),
    sampled_translational_shear_dissipation_j: work,
    sampled_translational_shear_supplied_work_j: suppliedWork};
});
writeFileSync(output, JSON.stringify({version: 1, outcome, feet,
  capture: source(capturePath), sources: [import.meta.filename, 'examples/interactive/capture_outcome.mjs'].map(source),
  conventions: {minimum_endpoint_floor_force_n: minimumForce,
    kinematics: 'Recorded COM velocity plus angular velocity cross the world contact-point offset. Floor is stationary and horizontal.',
    integration: 'Trapezoidal 50 Hz samples over intervals with both endpoints loaded. Velocities are weighted by recorded normal forces, not by tracking a fixed contact point.'},
  scope: 'Diagnostic of material velocities at sampled floor contacts, separate from geometric foot-marker travel. Translation of the load-weighted patch and pointwise rotation are both reported. Contact birth/death, changing weights, between-sample peaks and independent torsional friction work are unresolved. No new acceptance threshold or slip-free/hardware claim.'}, null, 2) + '\n');
console.log(feet.map(f => ({id: f.id, contact_motion_mm: f.integrated_load_weighted_tangential_speed_m * 1000,
  point_motion_mm: f.integrated_load_weighted_point_speed_m * 1000,
  shear_dissipation_j: f.sampled_translational_shear_dissipation_j})));
