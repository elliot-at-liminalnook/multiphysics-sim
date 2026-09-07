import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [root = 'runs/full-robot/learning/response-diagnosis', output = 'examples/full-robot/response-diagnosis-status.json'] = process.argv.slice(2);
const read = p => JSON.parse(readFileSync(p));
const digest = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const sourcePath = 'runs/full-robot/learning/speed-envelope/1.5x-stop-feedback-1.native.json';
const finePath = 'runs/full-robot/learning/speed-envelope/1.5x-refined-5ms.native.json';
const source = read(sourcePath), fine = read(finePath);
const markers = read('examples/full-robot/foot-markers.json');
const point = (frame, marker) => {
  const pose = frame.poses.find(p => p.name === marker.link);
  assert(pose);
  return pose.position_m.map((v, i) => v + pose.rotation[i].reduce((s, v, j) => s + v * marker.local_point_m[j], 0));
};
function compare(a, b, until = Infinity) {
  for (const c of [a, b]) assert.equal(c.recording.scene.robot.source.cad_sha256, markers.expected_cad_sha256);
  let maximum = 0, maximumTime = 0, frames = 0, lastTime = 0;
  const firstThresholds = {};
  for (let i = 0; i < Math.min(a.frames.length, b.frames.length); i++) {
    const x = a.frames[i], y = b.frames[i];
    assert(Math.abs(x.time_s - y.time_s) < 1e-9);
    if (x.time_s > until + 1e-9) break;
    frames++;
    lastTime = x.time_s;
    let difference = 0;
    for (const marker of markers.markers) {
      const p = point(x, marker), q = point(y, marker);
      difference = Math.max(difference, Math.hypot(...p.map((v, j) => v - q[j])));
    }
    if (difference > maximum) { maximum = difference; maximumTime = x.time_s; }
    for (const threshold of [.0001, .0005, .001, .002]) {
      if (difference > threshold && firstThresholds[threshold] === undefined) firstThresholds[threshold] = x.time_s;
    }
  }
  return {frames, through_s: lastTime, maximum_foot_difference_m: maximum,
    maximum_at_s: maximumTime, first_threshold_crossing_s_by_m: firstThresholds};
}
const frozen = read(`${root}/frozen-20ms.native.json`);
const fixedFine = read(`${root}/frozen-5ms.native.json`);
const compatible = read(`${root}/default-20ms.native.json`);
const stableFrames = c => c.frames.map(f => {const copy = {...f}; delete copy.stepping_wall_s; return copy;});
assert(frozen.completed && compatible.completed);
assert.deepEqual(stableFrames(frozen), stableFrames(source), 'frozen targets must reproduce every source frame');
assert.deepEqual(stableFrames(compatible), stableFrames(source), 'default model must retain every source frame');
const variants = ['dissipation-100-20ms', 'dissipation-100-10ms', 'dissipation-100-5ms', 'dissipation-audit', 'compliant-20ms', 'compliant-5ms'];
const results = variants.map(name => {
  const path = `${root}/${name}.native.json`, c = read(path);
  let acceptance;
  if (c.completed) {
    const p = `${root}/${name}-acceptance/summary.json`, report = read(p);
    assert.equal(report.capture.sha256, digest(path).sha256);
    acceptance = {...digest(p), passed: report.passed,
      swings: report.lifts.length, passing_swings: report.lifts.filter(l => l.passed).length,
      final_body_error_m: report.final_body_error_m, sampled_internal_contacts: report.sampled_internal_contacts};
  } else assert(c.error);
  return {name, ...digest(path), completed: c.completed, through_s: c.frames.at(-1).time_s,
    error: c.error, acceptance};
});
writeFileSync(output, JSON.stringify({
  version: 1, source: digest(sourcePath), fine_feedback_source: digest(finePath),
  generator: digest('examples/full-robot/prepare_response_diagnosis.mjs'),
  frozen_20ms: digest(`${root}/frozen-20ms.native.json`),
  frozen_5ms: {...digest(`${root}/frozen-5ms.native.json`), completed: fixedFine.completed, error: fixedFine.error},
  default_compatibility: {...digest(`${root}/default-20ms.native.json`), exact_physical_and_policy_frames: source.frames.length},
  frozen_target_exact_frames: source.frames.length,
  windows: [1, 2, 5, 10, 11.42].map(until => ({
    requested_through_s: until, frozen_targets: compare(source, fixedFine, until),
    feedback_enabled: compare(source, fine, until),
  })),
  full_feedback_comparison: compare(source, fine), results,
  normal_contact_formula: 'Fn = max(0, k * depth * (1 - alpha * separation_speed)); no force at nonpositive depth. At zero speed the local damping slope is alpha * k * depth (N*s/m).',
  historical_alpha_s_m: .2, trial_alpha_s_m: 100,
  scope: 'Unpromoted controller/contact sensitivity experiments, not calibration or realtime acceptance. Frozen 5 ms and other failed runs cover only their committed prefixes. All comparisons retain actual Rust dynamics; no prescribed physical poses. Wall timings omitted because some independent validation jobs overlapped.',
}, null, 2) + '\n');
console.log(output);
