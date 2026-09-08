import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', out = 'runs/full-robot/learning/settled-integral-numeric';
assert(!existsSync(out), 'numeric retry output already exists'); mkdirSync(out, {recursive: true});
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const oldPath = `${root}/settled-integral-plan.json`, old = read(oldPath);
const inputs = old.cases.flatMap(c => [c.scene, c.config, c.actions, c.task]);
for (const path of inputs) {
  const pinned = old.sources.find(s => s.path === path); assert(pinned);
  assert.equal(source(path).sha256, pinned.sha256, path);
}
const plan = {...old,
  sources: [oldPath, `${root}/SETTLED-INTEGRAL-NUMERIC-RETRY.md`,
    `${root}/settled-integral-binding-failure.json`,
    'crates/sim-domain-control/src/angle_integral.rs', 'crates/sim-script/src/lib.rs',
    'crates/sim-script/tests/angle_integral.rs', 'runs/angle-integral-numeric-tests.log',
    'runs/angle-integral-numeric-build.log', 'target/release/examples/run_environment',
    'target/release/examples/evaluate_lift', import.meta.filename, ...inputs, old.previous_capture.path].map(source),
  scope: `${old.scope} Exact input files retried after the integer-to-float Rhai parameter conversion repair; previous zero-step binding failures remain retained.`};
for (const path of [`${out}/plan.json`, `${root}/settled-integral-numeric-plan.json`]) writeFileSync(path, JSON.stringify(plan, null, 2) + '\n');
