// Package observed Rust poses for the existing viewer; never synthesize motion.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const [capturePath, output, id, label, annotationPath] = process.argv.slice(2);
assert(label && /^[a-z0-9-]+$/.test(id), 'Usage: CAPTURE VIEWER_DIRECTORY ID LABEL');
const bytes = fs.readFileSync(capturePath), c = JSON.parse(bytes);
assert.equal(c.kind, 'sampled_environment_capture');
assert(c.completed && !c.error && !c.recording.failure);
const robot = c.recording.scene.robot;
assert(c.frames.length > 1 && c.frames.every(f => !f.error));
const follow = c.task.speed?.body_link;
const centers = c.frames.map(f => f.poses.find(p => p.name === follow)?.position_m).filter(Boolean);
const span = centers.length ? Math.max(...[0, 1].map(i => Math.max(...centers.map(p => p[i])) - Math.min(...centers.map(p => p[i])))) : 0;
const data = {version: 1, kind: 'recorded_physics_view', source: robot.source,
  capture_sha256: crypto.createHash('sha256').update(bytes).digest('hex'),
  simulated_s: c.frames.at(-1).time_s, stepping_wall_s: c.wall_s,
  coordinate_names: c.metadata.coordinate_names, joint_indices: c.metadata.joint_indices,
  follow_link: follow, view_grid_size_m: Math.max(4, span * 2 + 3),
  robot: {source: robot.source, world: robot.world, links: robot.links.map(l => ({name: l.name, com: l.com, collision: l.collision}))},
  frames: c.frames.map(f => ({time_s: f.time_s, poses: f.poses, joint_positions: f.joint_positions,
    servo_targets_rad: f.servo_targets_rad, contacts: f.contacts, done: f.done})),
  scope: 'Recorded detailed Rust simulation, played at recorded simulation time. No live controller, physics or hardware commands.'};
for (const f of data.frames) {
  assert.equal(f.servo_targets_rad.length, data.coordinate_names.length);
  assert(data.joint_indices.every(i => Number.isFinite(f.joint_positions[i])));
}
const catalogPath = path.join(output, 'catalog.json');
const catalog = JSON.parse(fs.readFileSync(catalogPath));
const annotation = annotationPath ? JSON.parse(fs.readFileSync(annotationPath)) : {};
assert(!catalog.presets.some(p => p.id === id), 'Preset already exists');
const asset = `data/${id}.json`, serialized = JSON.stringify(data);
fs.writeFileSync(path.join(output, asset), serialized, {flag: 'wx'});
catalog.presets.push({id, label, mode: 'recorded', path: asset,
  asset_sha256: crypto.createHash('sha256').update(serialized).digest('hex'),
  description: annotation.description || 'Play, scrub, orbit, or slow down this recorded physics experiment.',
  readiness: annotation.readiness || 'Recorded simulation · see the source experiment for validation and calibration status',
  evidence: annotation.evidence || 'This view replays saved Rust physical poses and motor readings. WASD does not alter the recording. No hardware commands are sent.'});
fs.writeFileSync(catalogPath + '.tmp', JSON.stringify(catalog, null, 2) + '\n', {flag: 'wx'});
fs.renameSync(catalogPath + '.tmp', catalogPath);
console.log(JSON.stringify({id, asset, frames: data.frames.length, capture_sha256: data.capture_sha256}));
