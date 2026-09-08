import {test} from 'node:test';
import assert from 'node:assert/strict';
import {captureOutcome} from './capture_outcome.mjs';

const terminated = () => ({completed: false, error: null,
  frames: [{time_s: 0, completed_steps: 0}, {time_s: .02, completed_steps: 2}],
  transitions: [{terminated: false, truncated: false},
    {terminated: true, truncated: false, time_s: .02, completed_steps: 2,
      termination_reasons: ['upright outside bounds']}]});

test('preserves runtime failures and verified task termination as separate outcomes', () => {
  assert.equal(captureOutcome({completed: true, error: null}).kind, 'completed');
  assert.equal(captureOutcome({completed: false, error: 'Newton failed'}).kind, 'runtime_error');
  assert.deepEqual(captureOutcome(terminated()), {kind: 'task_termination', error: null,
    termination_reasons: ['upright outside bounds'], time_s: .02, completed_steps: 2});
});

test('rejects unexplained, truncated, mismatched or continued partial captures', () => {
  assert.throws(() => captureOutcome({completed: false, error: null}));
  assert.throws(() => captureOutcome({completed: true, error: 'failed'}));
  for (const mutate of [
    c => { c.transitions.at(-1).terminated = false; },
    c => { c.transitions.at(-1).truncated = true; },
    c => { c.transitions.at(-1).termination_reasons = []; },
    c => { c.transitions.at(-1).termination_reasons = ['']; },
    c => { c.frames.pop(); },
    c => { c.frames.at(-1).time_s = .01; },
    c => { c.frames.at(-1).completed_steps = 1; },
    c => { c.transitions[0].terminated = true; },
  ]) { const c = terminated(); mutate(c); assert.throws(() => captureOutcome(c)); }
});
