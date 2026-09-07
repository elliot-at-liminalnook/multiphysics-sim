// Keep unsuccessful trials visible alongside passing short runs.
import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [directory = 'runs/full-robot/learning/speed-envelope', output = `${directory}/summary.json`] = process.argv.slice(2);
const read = p => JSON.parse(readFileSync(p));
const digest = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const manifest = read(`${directory}/manifest.json`);
const results = manifest.cases.map(experiment => {
  const path = `${directory}/${experiment.name}.native.json`, capture = read(path);
  assert(capture.frames.length > 0 && typeof capture.completed === 'boolean', path);
  assert(capture.completed || capture.error, `Incomplete run must explain failure: ${path}`);
  const last = capture.frames.at(-1);
  let acceptance;
  if (capture.completed) {
    const report = `${directory}/${experiment.name}-acceptance/summary.json`, a = read(report);
    assert.equal(a.capture.sha256, digest(path).sha256, `Stale acceptance report: ${report}`);
    acceptance = {
      ...digest(report), passed: a.passed, swings: a.lifts.length,
      passing_swings: a.lifts.filter(l => l.passed).length,
      minimum_qualifying_span_s: Math.min(...a.lifts.map(l => l.longest_qualifying_span_s)),
      final_body_error_m: a.final_body_error_m,
      final_yaw_error_rad: a.final_yaw_error_rad,
      maximum_body_tilt_rad: a.maximum_body_tilt_rad,
      sampled_internal_contacts: a.sampled_internal_contacts,
      body_advance_world_m: a.body_advance_world_m,
    };
  }
  return {
    ...experiment, capture: digest(path), completed: capture.completed,
    last_committed_time_s: last.time_s, last_phase: last.policy?.step_reference?.reference?.phase,
    error: capture.error ?? null, acceptance,
  };
});
writeFileSync(output, JSON.stringify({
  version: 1, manifest, results,
  scope: 'Controller development on the explicit terrain-contact profile. Existing sampled clearance/support, stop and overlap gates are unchanged. Passing a short run does not establish sustained walking, timestep accuracy, realtime performance or hardware feasibility. Timings deliberately omitted: some independent validation/build jobs overlapped.',
}, null, 2) + '\n');
console.log(output);
