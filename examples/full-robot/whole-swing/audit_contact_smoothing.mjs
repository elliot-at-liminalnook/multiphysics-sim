// Each contact law is a distinct fidelity profile. Keep the ordinary suite
// audit's strict identical-options rule intact and audit each profile separately.
import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', run = 'runs/full-robot/learning/contact-smoothing';
const read = path => JSON.parse(readFileSync(path));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const planPath = `${root}/contact-smoothing-plan.json`, plan = read(planPath);
const statusPath = `${root}/contact-smoothing-status.json`, status = read(statusPath);
assert(status.complete); assert.equal(status.cases.length, plan.cases.length);
let robot, fixedOptions;
const profiles = plan.cases.map((c, i) => {
  const result = status.cases[i]; assert.equal(c.name, result.name);
  const p = `${run}/${c.name}.audit-plan.json`, s = `${run}/${c.name}.audit-status.json`;
  const output = `${root}/contact-smoothing-${c.name}-integrity.json`;
  writeFileSync(p, JSON.stringify({...plan, cases: [c], sources: [...plan.sources, source(planPath)]}) + '\n');
  writeFileSync(s, JSON.stringify({...status, cases: [result]}) + '\n');
  execFileSync(process.execPath, ['examples/interactive/audit_walking_suite.mjs', p, s, output], {stdio: 'pipe'});
  const audit = read(output); assert(audit.passed);
  const captured = result.sources.find(s => s.path.endsWith('.native.json'));
  const scene = read(captured.path).recording.scene;
  const options = structuredClone(scene.options);
  assert.equal(options.floor_friction.kind, 'regularized_coulomb');
  assert.equal(options.floor_friction.slip_speed_m_s, c.slip_speed_m_s);
  delete options.floor_friction.slip_speed_m_s;
  if (robot) { assert.deepEqual(scene.robot, robot); assert.deepEqual(options, fixedOptions); }
  else { robot = scene.robot; fixedOptions = options; }
  return {name: c.name, slip_speed_m_s: c.slip_speed_m_s, source: source(output)};
});
writeFileSync(`${root}/contact-smoothing-integrity.json`, JSON.stringify({version: 1, passed: true, profiles,
  identical_parsed_robot: true, identical_physics_options_except: ['floor_friction.slip_speed_m_s'],
  sources: [planPath, statusPath, import.meta.filename, 'examples/interactive/audit_walking_suite.mjs',
    ...profiles.map(p => p.source.path)].map(source),
  scope: 'Per-profile authored recipe, seed, actions and acceptance audits retain the standard same-options rule. Across profiles only the predeclared smoothing speed differs; this is explicitly a contact-model sensitivity study, not identical physics or timestep convergence.'}, null, 2) + '\n');
console.log({passed: true, profiles: profiles.length});
