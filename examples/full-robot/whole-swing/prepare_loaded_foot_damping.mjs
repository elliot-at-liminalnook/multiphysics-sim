import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', output = 'runs/full-robot/learning/loaded-foot-damping';
assert(!existsSync(output)); mkdirSync(output, {recursive:true});
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const scenePath = `${root}/settled-integral-candidate.scene.json`, scene = read(scenePath);
const configPath = `${root}/direct-support-minute.config.json`;
const archivePath = `${root}/settled-integral-regression-recipes.json`, archive = read(archivePath);
for (const s of [archive.scene,archive.task]) assert.equal(source(s.path).sha256,s.sha256);
const original = archive.cases.find(c=>c.name==='minute-1.25ms');
const priorPath = `${root}/direct-support-status.json`, prior = read(priorPath).cases.find(c=>c.name==='direct-minute');
const baseline = prior.sources.find(s=>s.path.endsWith('.native.json'));
assert.equal(source(baseline.path).sha256,baseline.sha256);
const task = read(archive.task.path), config = read(configPath), stride = Math.round(task.period_s/config.step_s);
let held=scene.controller.inputs.map(c=>c.initial), next=0;
const actions=Array.from({length:config.steps/stride},(_,i)=>{
  if(original.input_events[next]?.at_step===i*stride) held=original.input_events[next++].values;
  return [...held];
});
assert.equal(next,original.input_events.length);
assert.equal(createHash('sha256').update(JSON.stringify(actions)).digest('hex'),original.actions_json_sha256);
const cases = [['position-only',null],['damping-50ms',0.05],['damping-200ms',0.2],['damping-500ms',0.5]].map(([name,gain])=>{
  const c=structuredClone(config);
  assert(!c.policy.point_feedback.floor_velocity_damping);
  if(gain!==null)c.policy.point_feedback.floor_velocity_damping={velocity_damping_s:gain,full_support_force_n:1};
  const configPath=`${output}/${name}.config.json`, actionsPath=`${output}/${name}.actions.json`;
  writeFileSync(configPath,JSON.stringify(c)+'\n'); writeFileSync(actionsPath,JSON.stringify(actions)+'\n');
  return {name,scene:scenePath,config:configPath,actions:actionsPath,task:archive.task.path,
    duration_s:c.steps*c.step_s,step_s:c.step_s,seed:original.seed,velocity_damping_s:gain};
});
const plan={version:1,cases,baseline,maximum_contact_motion_to_body_advance_ratio:0.05,
  sources:[`${root}/LOADED-FOOT-DAMPING-PLAN.md`,scenePath,configPath,archivePath,priorPath,archive.task.path,
    ...cases.flatMap(c=>[c.config,c.actions]), 'crates/sim-domain-control/src/load_damping.rs',
    'crates/sim-runtime/src/point_feedback.rs','crates/sim-domain-robot/src/articulated.rs',
    'crates/sim-domain-control/tests/load_damping.rs','crates/sim-domain-robot/tests/contact_velocity.rs',
    'crates/sim-runtime/tests/point_feedback.rs','crates/sim-script/tests/load_damping.rs',
    'examples/interactive/load-weighted-foot-damping.md','examples/interactive/analyze_floor_contact_motion.mjs',
    'examples/interactive/analyze_contact_phases.mjs','examples/interactive/recorded_contact_motion.mjs',
    'runs/load-damping-kernel-tests-retry.log','runs/load-damping-runtime-tests.log','runs/load-damping-contact-tests.log',
    'runs/load-damping-native-build.log','target/release/examples/run_environment','target/release/examples/evaluate_lift',import.meta.filename].map(source),
  scope:'Four frozen native development minutes. Identical robot, physics and direct-transfer controller except optional loaded horizontal contact-velocity damping. Task and contact thresholds unchanged; no held-out, terrain, browser or timestep qualification.'};
for(const path of [`${output}/plan.json`,`${root}/loaded-foot-damping-plan.json`])writeFileSync(path,JSON.stringify(plan,null,2)+'\n');
console.log(`Prepared ${cases.length} loaded-foot damping cases.`);
