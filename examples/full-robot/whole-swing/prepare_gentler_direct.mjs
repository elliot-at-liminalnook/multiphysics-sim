import {readFileSync, writeFileSync, existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing';
const output = process.argv[2] ?? 'runs/full-robot/learning/gentler-direct';
assert(!existsSync(output), 'refusing to overwrite an experiment');
const read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
// Reuse the frozen versioned reconstruction, not an ignored baseline recipe.
const helper = `${root}/prepare_gentler_contact.mjs`;
execFileSync(process.execPath, [helper, `${output}/reference-recipes`], {stdio: 'pipe'});
const referencePlanPath = `${output}/reference-recipes/plan.json`, reference = read(referencePlanPath);
const statusPath = `${root}/gentler-contact-status.json`, previous = read(statusPath);
assert(previous.complete);
const cases = reference.cases.map((c, i) => {
  const config = read(c.config), seq = config.policy.step_reference.sequence;
  assert.equal(seq.direct_support_transfer, undefined);
  const period = seq.phase_durations_s.reduce((s, d) => s + d, 0);
  seq.direct_support_transfer = true;
  seq.phase_durations_s = [1.10, .38, .38, .02, .02];
  assert(Math.abs(seq.phase_durations_s.reduce((s, d) => s + d, 0) - period) < 1e-15);
  const name = `direct-slip-${Number((c.slip_speed_m_s * 1000).toPrecision(8))}mm-s`;
  const path = `${output}/${name}.config.json`;
  writeFileSync(path, JSON.stringify(config) + '\n');
  assert.equal(previous.cases[i].name, c.name);
  return {...c, name, config: path, direct_support_transfer: true,
    phase_durations_s: seq.phase_durations_s,
    baseline_name: c.name, baseline: previous.cases[i].sources.find(s => s.path.endsWith('.native.json'))};
});
const plan = {version: 1, cases, maximum_contact_motion_to_body_advance_ratio: .05,
  sources: [`${root}/GENTLER-DIRECT-PLAN.md`, helper, referencePlanPath, statusPath,
    `${root}/gentler-contact-summary.json`, ...reference.sources.map(s => s.path),
    ...cases.map(c => c.config), 'crates/sim-domain-control/src/stepping.rs',
    'examples/interactive/direct-support-transfer.md', import.meta.filename].filter((p, i, a) => a.indexOf(p) === i).map(source),
  scope: 'Three matched contact fidelity profiles with a gentler direct support path and unchanged nominal stride/period. Only direct_support_transfer and phase_durations_s change within each matched baseline. Original task and 5% contact screen retained; no timestep, held-out, terrain or browser qualification.'};
writeFileSync(`${output}/plan.json`, JSON.stringify(plan, null, 2) + '\n');
if (!process.argv[2]) writeFileSync(`${root}/gentler-direct-plan.json`, JSON.stringify(plan, null, 2) + '\n');
console.log(cases.map(c => ({name: c.name, phases: c.phase_durations_s, speed_mm_s: c.forward_speed_m_s * 1000})));
