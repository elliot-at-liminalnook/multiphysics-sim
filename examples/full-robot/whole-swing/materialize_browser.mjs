// Preserve the exact experimental recipe outside ignored run directories.
import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = 'examples/full-robot/whole-swing';
const plan = JSON.parse(readFileSync(`${root}/finish-plan.json`));
const status = JSON.parse(readFileSync(`${root}/finish-status.json`));
const name = 'finish-0.85-20ms', entry = plan.cases.find(c => c.name === name), result = status.cases.find(c => c.name === name);
assert(status.complete && result.completed && result.passed);
for (const key of ['scene', 'config']) {
  const bytes = readFileSync(entry[key]), source = result.sources.find(s => s.path === entry[key]);
  assert.equal(createHash('sha256').update(bytes).digest('hex'), source.sha256);
  writeFileSync(`${root}/browser.${key}.json`, bytes);
}
console.log('Preserved experimental 3.75 mm/s recipe; refined stopping and numerical gates still fail.');
const sustained = JSON.parse(readFileSync(`${root}/sustained-plan.json`));
const sustainedStatus = JSON.parse(readFileSync(`${root}/sustained-status.json`));
const teacher = sustained.cases.find(c => c.name === 'teacher-minute-5ms');
const teacherResult = sustainedStatus.cases.find(c => c.name === teacher.name);
assert(sustainedStatus.complete && teacherResult.completed && !teacherResult.error);
for (const key of ['scene', 'config']) {
  const bytes = readFileSync(teacher[key]), source = teacherResult.sources.find(s => s.path === teacher[key]);
  assert.equal(createHash('sha256').update(bytes).digest('hex'), source.sha256);
  writeFileSync(`${root}/teacher-minute.${key}.json`, bytes);
}
