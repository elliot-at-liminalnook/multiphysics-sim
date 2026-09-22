import fs from 'node:fs';
import assert from 'node:assert/strict';
const root = import.meta.dirname;
const read = name => JSON.parse(fs.readFileSync(`${root}/${name}.json`));
for (const [output, left, right] of [
  ['default-parity', 'before.native', 'candidate.native'],
  ['profile-parity', 'candidate.native', 'candidate.profiled'],
  ['analytic/profile-parity', 'analytic/candidate.native', 'analytic/candidate.profiled'],
]) {
  const a = read(left), b = read(right);
  assert(a.completed && b.completed && !a.error && !b.error);
  assert.equal(a.frames.length, b.frames.length);
  const fields = Object.keys(a.frames[0]).filter(key => key !== 'stepping_wall_s');
  for (let i = 0; i < a.frames.length; i++) {
    assert.deepEqual(Object.keys(a.frames[i]), Object.keys(b.frames[i]));
    for (const field of fields) assert.deepEqual(a.frames[i][field], b.frames[i][field], `frame ${i}: ${field}`);
  }
  assert.deepEqual(a.transitions, b.transitions);
  fs.writeFileSync(`${root}/${output}.json`, JSON.stringify({frames: a.frames.length,
    exactly_equal_fields: fields, transitions_exact: true}, null, 2) + '\n');
  console.log(output, `${a.frames.length} frames and all transitions exactly equal`);
}
