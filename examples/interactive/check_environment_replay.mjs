// Native CLI replay must use the same validation and prefix semantics as WASM.
import {readFileSync, writeFileSync, mkdtempSync, rmSync} from 'node:fs';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import {execFileSync, spawnSync} from 'node:child_process';
import assert from 'node:assert/strict';
const executable = process.argv[2] ?? 'target/release/examples/run_environment';
const dir = mkdtempSync(join(tmpdir(), 'environment-replay-'));
try {
  const scene = JSON.parse(readFileSync('examples/interactive/pendulum.scene.json'));
  const actions = Array.from({length: 20}, (_, i) => scene.controller.inputs.map(c =>
    c.name === 'command.position' && i >= 5 ? c.lower : c.initial));
  const actionsPath = join(dir, 'actions.json'); writeFileSync(actionsPath, JSON.stringify(actions));
  const original = JSON.parse(execFileSync(executable, ['examples/interactive/pendulum.scene.json',
    'examples/interactive/pendulum.policy.json', 'examples/interactive/pendulum.environment.json', actionsPath], {maxBuffer: 32 * 1024 * 1024}));
  assert(original.completed);
  const episode = {version: 1, kind: 'sampled_environment_recording', task: original.task,
    runtime: original.recording, error: null};
  const path = join(dir, 'episode.json');
  const stable = f => { f = structuredClone(f); delete f.stepping_wall_s; return f; };
  const replay = r => { writeFileSync(path, JSON.stringify(r)); return JSON.parse(execFileSync(executable, ['--replay', path], {maxBuffer: 32 * 1024 * 1024})); };
  const full = replay(episode);
  assert(full.completed); assert.deepEqual(full.frames.map(stable), original.frames.map(stable));
  assert.deepEqual(full.transitions, original.transitions);
  const partial = structuredClone(episode), midpoint = Math.floor(original.frames.length / 2);
  partial.runtime.completed_steps = original.frames[midpoint].completed_steps;
  partial.runtime.input_events = (partial.runtime.input_events ?? []).filter(e => e.at_step < partial.runtime.completed_steps);
  const prefix = replay(partial);
  assert(prefix.requested_steps_completed); assert.equal(prefix.completed, false);
  assert.deepEqual(prefix.frames.map(stable), original.frames.slice(0, midpoint + 1).map(stable));
  const bad = structuredClone(episode); bad.runtime.input_events = [{at_step: 1, values: []}];
  writeFileSync(path, JSON.stringify(bad));
  const failed = spawnSync(executable, ['--replay', path], {encoding: 'utf8'});
  assert.notEqual(failed.status, 0); assert.match(failed.stderr, /invalid environment replay action schedule/);
  console.log('Environment CLI: exact full/prefix replay; invalid action timing rejected.');
} finally { rmSync(dir, {recursive: true, force: true}); }
