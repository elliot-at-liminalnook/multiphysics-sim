import fs from 'node:fs';
import assert from 'node:assert/strict';

const root = import.meta.dirname;
const read = name => JSON.parse(fs.readFileSync(`${root}/${name}.json`));
function exactPhysicalReplay(left, right) {
  assert(left.completed && right.completed && !left.error && !right.error);
  assert.equal(left.frames.length, right.frames.length);
  const fields = Object.keys(left.frames[0]).filter(key => key !== 'stepping_wall_s');
  for (let i = 0; i < left.frames.length; i++) {
    assert.deepEqual(Object.keys(left.frames[i]), Object.keys(right.frames[i]));
    for (const field of fields) {
      assert.deepEqual(left.frames[i][field], right.frames[i][field], `frame ${i}: ${field}`);
    }
  }
  assert.deepEqual(left.transitions, right.transitions);
  return {frames: left.frames.length, exactly_equal_fields: fields, transitions_exact: true,
    before_wall_s: left.wall_s, after_wall_s: right.wall_s};
}
for (const [name, left, right] of [
  ['numeric/default-parity', 'numeric/before.native', 'numeric/candidate.native'],
  ['numeric/profile-parity', 'numeric/candidate.native', 'numeric/candidate.profiled'],
  ['profile-parity', 'candidate.native', 'candidate.profiled'],
]) {
  const result = exactPhysicalReplay(read(left), read(right));
  fs.writeFileSync(`${root}/${name}.json`, JSON.stringify(result, null, 2) + '\n');
  console.log(`${name}: ${result.frames} frames exactly equal`);
}
