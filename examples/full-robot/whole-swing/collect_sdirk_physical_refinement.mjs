import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', run = 'runs/full-robot/learning/sdirk-physical-refinement';
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const verify = s => assert.equal(source(s.path).sha256, s.sha256, s.path);
const planPath = `${root}/sdirk-physical-refinement-plan.json`, plan = read(planPath);
const statusPath = `${root}/sdirk-physical-refinement-status.json`, status = read(statusPath);
const integrityPath = `${root}/sdirk-physical-refinement-integrity.json`, integrity = read(integrityPath);
assert(status.complete && integrity.passed);
for (const report of [plan, status, integrity]) report.sources.forEach(verify);
const cases = status.cases.map(c => {
  assert(c.completed && !c.error); c.sources.forEach(verify);
  const capture = c.sources.find(s => s.path.endsWith('.native.json'));
  const path = `${root}/sdirk-physical-refinement-${c.name}-metrics.json`;
  execFileSync(process.execPath, ['examples/interactive/analyze_walking_capture.mjs', capture.path, path], {stdio: 'pipe'});
  return {name: c.name, task_passed: c.passed, acceptance: c.acceptance,
    native_compute: read(path).native_compute, metrics: source(path), capture};
});
const be125 = 'runs/full-robot/learning/whole-combined/combined-turn-1.25ms.native.json';
const be0625 = `${run}/teacher-be-0.625ms.native.json`;
const s10 = `${run}/teacher-sdirk2-10ms.native.json`, s5 = `${run}/teacher-sdirk2-5ms.native.json`;
const s20 = 'runs/full-robot/learning/sdirk-physical-guess/teacher-sdirk2-20ms.native.json';
const old5 = 'runs/full-robot/learning/sdirk-steering/teacher-sdirk2-5ms.native.json';
const definitions = [
  ['be-reference-refinement', be125, be0625, ['--timestep-reference']],
  ['sdirk-refinement', s10, s5, ['--timestep-reference']],
  ['sdirk10-be125', s10, be125, ['--timestep-reference', '--sdirk2']],
  ['sdirk5-be125', s5, be125, ['--timestep-reference', '--sdirk2']],
  ['sdirk10-be0625', s10, be0625, ['--timestep-reference', '--sdirk2']],
  ['sdirk5-be0625', s5, be0625, ['--timestep-reference', '--sdirk2']],
  ['sdirk20-be0625', s20, be0625, ['--timestep-reference', '--sdirk2']],
  ['sdirk5-guess-difference', old5, s5, []],
];
const comparisons = definitions.map(([name, a, b, flags]) => {
  const path = `${root}/sdirk-physical-refinement-${name}.json`;
  execFileSync(process.execPath, ['examples/full-robot/compare_mechanical_reuse.mjs', a, b, path, ...flags], {stdio: 'pipe'});
  const r = read(path), foot = r.metrics.foot_marker_position_m.maximum, body = r.metrics.body_position_m.maximum;
  return {name, baseline: r.baseline, candidate: r.candidate, source: source(path),
    maximum_foot_difference_m: foot, maximum_body_difference_m: body,
    foot_passed: foot <= .001, body_passed: body <= .0005, passed: foot <= .001 && body <= .0005};
});
writeFileSync(`${root}/sdirk-physical-refinement-summary.json`, JSON.stringify({version: 1, cases, comparisons,
  budgets: {maximum_foot_difference_m: .001, maximum_body_difference_m: .0005}, browser_promoted: false,
  sources: [planPath, statusPath, integrityPath, import.meta.filename,
    'examples/interactive/analyze_walking_capture.mjs', 'examples/interactive/capture_outcome.mjs',
    'examples/full-robot/compare_mechanical_reuse.mjs'].map(source),
  scope: 'All predeclared trajectory comparisons are retained, including both finite backward-Euler references; neither is asserted to be ground truth. Task gates and native compute are separate. These 24-second development cases do not establish sustained-minute, held-out, terrain, browser or hardware qualification.'}, null, 2) + '\n');
console.log(JSON.stringify({cases: cases.map(c => ({name: c.name, task_passed: c.task_passed, native_rate: c.native_compute.simulation_per_wall})),
  comparisons: comparisons.map(({name, maximum_foot_difference_m: foot, maximum_body_difference_m: body, passed}) => ({name, foot, body, passed}))}, null, 2));
