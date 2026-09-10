// Replay one recorded numerical failure with an independently built executable.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
const [source, binary, root] = process.argv.slice(2);
assert(source && binary && root, 'pass observation.json, executable and fresh output directory');
const read = p => JSON.parse(fs.readFileSync(p));
const hash = p => crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const write = (p, v) => fs.writeFileSync(p, JSON.stringify(v, null, 2) + '\n', {flag: 'wx'});
const original = read(source);
assert.equal(original.execution.exit_code, 1);
fs.mkdirSync(root);
const args = [...original.execution.args];
args[5] = root + '/checkpoints';
const code = [import.meta.filename, 'crates/sim-domain-control/src/contact_phase/sequence.rs',
  'crates/sim-runtime/examples/optimize_joint_ipopt_events.rs',
  'crates/sim-runtime/examples/optimize_joint_ipopt.rs'];
write(root + '/launch.json', {source, source_sha256: hash(source), binary, binary_sha256: hash(binary), args,
  inputs: args.slice(0, 5).map(path => ({path, sha256: hash(path)})),
  code: code.map(path => ({path, sha256: hash(path)})),
  scope: 'Identical input recipe and search limits, corrected shared reference sampler. Independent replay, excluded from the older live CEM context.'});
const out = fs.openSync(root + '/result.json', 'wx'), err = fs.openSync(root + '/solver.log', 'wx');
const started = performance.now();
let result;
try {
  result = spawnSync(binary, args, {stdio: ['ignore', out, err],
    env: {...process.env, OMP_NUM_THREADS: '1', VECLIB_MAXIMUM_THREADS: '1'}});
} finally { fs.closeSync(out); fs.closeSync(err); }
write(root + '/execution.json', {exit_code: result.status, signal: result.signal,
  error: result.error?.message ?? null, wall_s: (performance.now() - started) / 1000});
process.exitCode = result.status === 0 ? 0 : 1;
