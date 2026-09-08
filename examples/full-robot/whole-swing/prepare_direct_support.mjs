import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', output = 'runs/full-robot/learning/direct-support';
assert(!existsSync(output)); mkdirSync(output, {recursive: true});
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const scenePath = `${root}/settled-integral-candidate.scene.json`, scene = read(scenePath);
const archivePath = `${root}/settled-integral-regression-recipes.json`, archive = read(archivePath);
for (const s of [archive.scene, archive.task]) assert.equal(source(s.path).sha256, s.sha256, s.path);
const priorStatusPath = `${root}/settled-integral-regression-status.json`, prior = read(priorStatusPath);
const task = read(archive.task.path);
const cases = [
  ['default-steering', 'steering-1.25ms', false],
  ['direct-minute', 'minute-1.25ms', true],
  ['direct-steering', 'steering-1.25ms', true],
].map(([name, originalName, direct]) => {
  const original = archive.cases.find(c => c.name === originalName); assert(original);
  const c = structuredClone(original.config), seq = c.policy.step_reference.sequence;
  if (direct) { seq.direct_support_transfer = true; seq.phase_durations_s = [.58, .38, .38, .02, .02]; }
  assert(Math.abs(seq.phase_durations_s.reduce((s, d) => s + d, 0) - 1.38) < 1e-12);
  const stride = Math.round(task.period_s / c.step_s);
  let held = scene.controller.inputs.map(c => c.initial), next = 0;
  const actions = Array.from({length: c.steps / stride}, (_, i) => {
    if (original.input_events[next]?.at_step === i * stride) held = original.input_events[next++].values;
    return [...held];
  });
  assert.equal(next, original.input_events.length);
  assert.equal(createHash('sha256').update(JSON.stringify(actions)).digest('hex'), original.actions_json_sha256);
  const config = `${output}/${name}.config.json`, actionPath = `${output}/${name}.actions.json`;
  writeFileSync(config, JSON.stringify(c) + '\n'); writeFileSync(actionPath, JSON.stringify(actions) + '\n');
  const baseline = prior.cases.find(c => c.name === originalName).sources.find(s => s.path.endsWith('.native.json'));
  assert.equal(source(baseline.path).sha256, baseline.sha256);
  return {name, scene: scenePath, config, actions: actionPath, task: archive.task.path,
    duration_s: c.steps * c.step_s, step_s: c.step_s, seed: original.seed, direct_support_transfer: direct, baseline};
});
const plan = {version: 1, cases, maximum_contact_motion_to_body_advance_ratio: .05,
  sources: [`${root}/DIRECT-SUPPORT-PLAN.md`, `${root}/shift-duration-summary.json`, scenePath, archivePath,
    priorStatusPath, archive.task.path, ...cases.flatMap(c => [c.config, c.actions]),
    'crates/sim-domain-control/src/stepping.rs', 'crates/sim-domain-control/tests/stepping.rs',
    'examples/interactive/direct-support-transfer.md', 'examples/interactive/analyze_floor_contact_motion.mjs',
    'examples/interactive/analyze_contact_phases.mjs', 'examples/interactive/recorded_contact_motion.mjs',
    'runs/direct-support-final-tests.log', 'runs/direct-support-runtime-tests.log', 'runs/direct-support-native-build.log',
    'target/release/examples/run_environment', 'target/release/examples/evaluate_lift', import.meta.filename].map(source),
  scope: 'Default identity and two direct-transfer development cases. Identical physical model and controller gains, explicit new shared sequence path. Original task gates plus the prospective contact-motion screen; no browser, timestep, held-out or terrain qualification.'};
for (const path of [`${output}/plan.json`, `${root}/direct-support-plan.json`]) writeFileSync(path, JSON.stringify(plan, null, 2) + '\n');
console.log(`Prepared ${cases.length} direct-support study cases.`);
