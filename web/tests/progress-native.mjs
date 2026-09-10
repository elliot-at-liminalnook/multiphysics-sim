// Check a progress-task extraction against matched legacy native captures.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const [baselinePath, legacyPath, progressPath, reportPath] = process.argv.slice(2);
assert(reportPath, 'usage: progress-native.mjs previous-runtime.json legacy.json progress.json report.json');
const read = path => JSON.parse(fs.readFileSync(path));
const baseline = read(baselinePath), legacy = read(legacyPath), progress = read(progressPath);
for (const capture of [baseline, legacy, progress]) assert(capture.requested_steps_completed && capture.error === null);
const clean = value => Array.isArray(value) ? value.map(clean) : value && typeof value === 'object'
  ? Object.fromEntries(Object.entries(value).filter(([key]) => key !== 'stepping_wall_s').map(([key, item]) => [key, clean(item)])) : value;
assert.deepEqual(clean(baseline.frames), clean(legacy.frames));
assert.deepEqual(clean(baseline.transitions), clean(legacy.transitions));
assert.deepEqual(baseline.recording, legacy.recording);
assert.deepEqual(clean(legacy.frames), clean(progress.frames));
assert.deepEqual(legacy.recording, progress.recording);
assert.equal(legacy.transitions.length, progress.transitions.length);
for (let i = 0; i < legacy.transitions.length; i++) {
  const old = legacy.transitions[i], current = progress.transitions[i];
  assert.equal(old.reward, current.reward);
  assert.equal(old.speed.net_distance_m, current.progress.net_distance_m);
  assert.equal(old.speed.net_speed_m_s, current.progress.net_speed_m_s);
}
const report = {version:1, passed:true, frames:legacy.frames.length,
  simulated_s:legacy.transitions.at(-1).time_s, inputs:[baselinePath, legacyPath, progressPath],
  scope:'Exact native frames, transitions and recording preserved against the previous runtime except stepping_wall_s. New progress task has exact physical frames, runtime recording, rewards, net distances and speeds against legacy task on matched commands. Only the supplied capture duration; no fall-equivalence or sustained-speed qualification.'};
fs.writeFileSync(reportPath, JSON.stringify(report, null, 2)+'\n', {flag:'wx'});
console.log(JSON.stringify(report));
