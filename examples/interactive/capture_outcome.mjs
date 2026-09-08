// Classify retained benchmark outcomes; a partial capture is never success.
import assert from 'node:assert/strict';

export function captureOutcome(c) {
  assert.equal(typeof c.completed, 'boolean', 'missing episode completion flag');
  if (c.completed) {
    assert(c.error == null, 'completed episode carries an error');
    return {kind: 'completed', error: null, termination_reasons: []};
  }
  if (typeof c.error === 'string' && c.error.trim()) {
    return {kind: 'runtime_error', error: c.error, termination_reasons: []};
  }
  assert(c.error == null, 'invalid episode error');
  const t = c.transitions?.at(-1), f = c.frames?.at(-1);
  assert(t?.terminated === true && t.truncated === false,
    'incomplete capture needs a runtime error or recorded task termination');
  assert(Array.isArray(t.termination_reasons) && t.termination_reasons.length > 0 &&
    t.termination_reasons.every(r => typeof r === 'string' && r.trim()),
  'task termination needs nonempty reasons');
  assert(f && c.transitions.length === c.frames.length, 'termination frame missing');
  assert(Number.isFinite(t.time_s) && t.time_s >= 0 && t.time_s === f.time_s,
    'termination time does not match final physics frame');
  assert(Number.isSafeInteger(t.completed_steps) && t.completed_steps >= 0 &&
    t.completed_steps === f.completed_steps, 'termination step does not match final physics frame');
  assert(c.transitions.slice(0, -1).every(t => t.terminated === false && t.truncated === false),
    'capture continued after an earlier terminal transition');
  return {kind: 'task_termination', error: null, termination_reasons: [...t.termination_reasons],
    time_s: t.time_s, completed_steps: t.completed_steps};
}
