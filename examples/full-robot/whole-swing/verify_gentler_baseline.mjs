import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing', read = p => JSON.parse(readFileSync(p));
const source = path => ({path, sha256: createHash('sha256').update(readFileSync(path)).digest('hex')});
const oldStatus = `${root}/shift-duration-status.json`, status = `${root}/gentler-contact-status.json`;
const previous = read(oldStatus).cases.find(c => c.name === 'shift-return-2x');
const repeated = read(status).cases.find(c => c.name === 'slip-1mm-s');
const captures = [previous, repeated].map(c => c.sources.find(s => s.path.endsWith('.native.json')));
for (const s of captures) assert.equal(source(s.path).sha256, s.sha256, s.path);
const [a, b] = captures.map(s => read(s.path));
assert(a.completed && b.completed);
assert.equal(a.frames.length, b.frames.length);
const physical = ({stepping_wall_s, ...frame}) => frame;
for (let i = 0; i < a.frames.length; i++) {
  assert(isDeepStrictEqual(physical(a.frames[i]), physical(b.frames[i])), `physical frame ${i} differs`);
}
const fields = ['transitions', 'recording', 'contract', 'task'];
for (const field of fields) assert(isDeepStrictEqual(a[field], b[field]), `${field} differs`);
writeFileSync(`${root}/gentler-contact-baseline-parity.json`, JSON.stringify({version: 1, passed: true,
  compared: ['frames excluding stepping_wall_s', ...fields], frames: b.frames.length,
  transitions: b.transitions.length, baseline: captures[0], repeated: captures[1],
  sources: [oldStatus, status, import.meta.filename].map(source),
  scope: 'Exact repeated physical trajectory and authored recording for the original 1 mm/s contact profile. Wall times are excluded. No changed-contact-law or timestep equivalence claim.'}, null, 2) + '\n');
console.log({exact: true, frames: b.frames.length, transitions: b.transitions.length});
