// Kinematic measurements only: no dynamics, contact law or state advancement.
import assert from 'node:assert/strict';
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
export function recordedContactMotion(frame, index, link) {
  const pose = frame.poses.find(p => p.name === link); assert(pose);
  const contacts = frame.contacts.filter(c => c.link === index && c.other == null);
  let force = 0, weightedVelocity = [0, 0], pointSpeed = 0, shearPower = 0, shear = [0, 0];
  for (const c of contacts) {
    const spin = cross(pose.angular_velocity_rad_s, c.point_m.map((v, j) => v - pose.position_m[j]));
    const v = pose.velocity_m_s.map((x, j) => x + spin[j]);
    const normal = c.force_n[2]; assert(normal >= 0);
    force += normal; pointSpeed += normal * Math.hypot(v[0], v[1]);
    for (let j = 0; j < 2; j++) { weightedVelocity[j] += normal * v[j]; shear[j] += c.force_n[j]; }
    shearPower += c.force_n[0] * v[0] + c.force_n[1] * v[1];
  }
  if (force > 0) { weightedVelocity = weightedVelocity.map(v => v / force); pointSpeed /= force; }
  const result = {time_s: frame.time_s, force, velocity: weightedVelocity,
    speed: Math.hypot(...weightedVelocity), pointSpeed, shearPower, shear_ratio: force > 0 ? Math.hypot(...shear) / force : 0};
  assert([result.force, ...result.velocity, result.speed, result.pointSpeed, result.shearPower, result.shear_ratio].every(Number.isFinite));
  return result;
}
