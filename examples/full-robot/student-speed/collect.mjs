// Preserve every planned outcome, including early failures. Large captures are
// reproducible from the versioned generator and CAD-derived source artifacts.
import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual as equal} from 'node:util';
import assert from 'node:assert/strict';

const root = 'examples/full-robot/student-speed';
const run = process.argv[2] ?? 'runs/full-robot/learning/student-speed';
const read = p => JSON.parse(readFileSync(p));
const hash = p => createHash('sha256').update(readFileSync(p)).digest('hex');
const source = path => ({path, sha256: hash(path)});
const plan = read(`${run}/plan.json`);
for (const s of plan.sources) assert.equal(hash(s.path), s.sha256, `changed source: ${s.path}`);
const task = read(plan.task), reports = [];
let physicalScene;
for (const c of plan.cases) {
  const capturePath = `${run}/${c.name}.native.json`, capture = read(capturePath);
  const config = read(c.config), scene = read(c.scene), actions = read(c.actions);
  assert(equal(capture.recording.config, config), `${c.name}: recorded configuration differs`);
  assert(equal(capture.task, task), `${c.name}: recorded task differs`);
  assert(equal(capture.recording.scene.controller, scene.controller), `${c.name}: controller differs`);
  assert.equal(capture.recording.seed, 0);
  const normalized = structuredClone(capture.recording.scene);
  delete normalized.controller.inputs;
  if (physicalScene) assert(equal(normalized, physicalScene), `${c.name}: physical model differs`);
  else physicalScene = normalized;
  assert.equal(normalized.robot.source.cad_sha256, scene.robot.source.cad_sha256);
  const events = [];
  for (let i = 0; i < actions.length; i++) {
    if (i === 0 || !equal(actions[i], actions[i - 1])) {
      events.push({at_step: Math.round(i * task.period_s / config.step_s), values: actions[i]});
    }
  }
  const recordedEvents = capture.recording.input_events;
  assert(recordedEvents.length > 0);
  assert(equal(recordedEvents, events.slice(0, recordedEvents.length)), `${c.name}: command sequence differs`);
  let acceptance = null;
  const sources = [c.scene, c.config, c.actions, capturePath].map(source);
  if (capture.completed) {
    assert.equal(capture.error, null);
    assert.equal(capture.transitions[0].time_s, 0);
    assert.equal(capture.transitions[0].completed_steps, 0);
    assert.equal(capture.transitions.length, actions.length + 1, 'reset transition plus every action');
    assert.equal(recordedEvents.length, events.length);
    const acceptancePath = `${run}/${c.name}-acceptance/summary.json`;
    acceptance = read(acceptancePath);
    assert.equal(acceptance.capture.sha256, hash(capturePath));
    sources.push(source(acceptancePath));
  } else assert(capture.error, `${c.name}: unsuccessful capture must explain failure`);
  reports.push({
    name: c.name, requested_speed_m_s: c.requested_speed_m_s,
    completed: capture.completed, error: capture.error,
    simulated_s: capture.frames.at(-1).time_s,
    passed: acceptance?.passed ?? false,
    acceptance: acceptance && {
      passed: acceptance.passed, swings: acceptance.lifts.length,
      failed_swings: acceptance.lifts.filter(l => !l.passed),
      minimum_qualified_s: Math.min(...acceptance.lifts.map(l => l.longest_qualifying_span_s)),
      final_body_error_m: acceptance.final_body_error_m,
      final_yaw_error_rad: acceptance.final_yaw_error_rad,
      maximum_body_tilt_rad: acceptance.maximum_body_tilt_rad,
      sampled_internal_contacts: acceptance.sampled_internal_contacts,
      body_advance_world_m: acceptance.body_advance_world_m,
      budgets: acceptance.budgets,
    }, sources,
  });
}
const result = {
  version: 1, complete: reports.length === plan.cases.length, promoted: false,
  cases: reports, sources: [...plan.sources, source(`${run}/plan.json`), source('target/release/examples/run_environment')],
  scope: 'Development gait screening with the retained learned student. Every planned capture and every completed capture’s independent acceptance report required. Same parsed physical scene across cases; source CAD provenance retained. Parallel execution means wall times are not performance benchmarks. No browser, hardware, sustained or held-out acceptance implied.',
};
writeFileSync(`${root}/status.json`, JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify(reports.map(r => ({name: r.name, passed: r.passed, error: r.error}))));
