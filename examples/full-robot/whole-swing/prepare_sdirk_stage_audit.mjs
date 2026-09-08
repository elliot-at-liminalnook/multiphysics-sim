import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', out = 'runs/full-robot/learning/sdirk-stage-audit';
assert(!existsSync(out), 'stage-audit output already exists'); mkdirSync(out, {recursive: true});
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const prior = read(`${root}/sdirk-steering-plan.json`), status = read(`${root}/sdirk-steering-status.json`);
const cases = [20, 5].map(ms => {
  const originalName = `teacher-sdirk2-${ms}ms`, base = prior.cases.find(c => c.name === originalName);
  const original = status.cases.find(c => c.name === originalName); assert(base && original);
  const previous = original.sources.find(s => s.path.endsWith('.native.json')); assert(previous);
  for (const s of original.sources) assert.equal(source(s.path).sha256, s.sha256, s.path);
  const config = read(base.config); assert(config.implicit.newton_audit_window_s == null);
  config.implicit.newton_audit_window_s = [1.25, 1.34];
  const name = `teacher-${ms}ms-audited`, configPath = `${out}/${name}.config.json`;
  writeFileSync(configPath, JSON.stringify(config) + '\n');
  return {...base, name, config: configPath, previous_capture: previous,
    profile: `${out}/${name}.profile.json`};
});
const plan = {version: 1, cases,
  sources: [`${root}/SDIRK2-STAGE-AUDIT.md`, `${root}/sdirk-steering-plan.json`,
    `${root}/sdirk-steering-status.json`, 'target/release/examples/run_environment',
    'crates/sim-domain-robot/src/articulated/embedding/implicit.rs',
    'crates/sim-domain-robot/tests/embedded_step.rs', 'runs/sdirk-endpoint-audit-tests.log',
    'runs/sdirk-endpoint-audit-build.log', import.meta.filename,
    ...cases.flatMap(c => [c.scene, c.config, c.task, c.actions, c.previous_capture.path])].map(source),
  scope: 'Original 24-second teacher 20/5 ms development recipes, seed 0. Only observational Newton audit window added. Preserve previous states exactly; instrumented timing is not performance evidence.'};
writeFileSync(`${out}/plan.json`, JSON.stringify(plan, null, 2) + '\n');
writeFileSync(`${root}/sdirk-stage-plan.json`, JSON.stringify(plan, null, 2) + '\n');
