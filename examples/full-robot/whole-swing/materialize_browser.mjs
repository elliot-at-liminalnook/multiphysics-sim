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
const turnPlan = JSON.parse(readFileSync(`${root}/turn-support-plan.json`));
const turnStatus = JSON.parse(readFileSync(`${root}/turn-support-status.json`));
const turn = turnPlan.cases.find(c => c.name === 'student-turn-20ms-support-24mm');
const turnResult = turnStatus.cases.find(c => c.name === turn.name);
assert(turnStatus.complete && turnResult.completed && turnResult.passed);
for (const key of ['scene', 'config']) {
  const bytes = readFileSync(turn[key]), source = turnResult.sources.find(s => s.path === turn[key]);
  assert.equal(createHash('sha256').update(bytes).digest('hex'), source.sha256);
  writeFileSync(`${root}/turn-browser.${key}.json`, bytes);
}
for (const [study, name, prefix] of [
  ['combined', 'combined-minute-2.5ms', 'combined-minute'],
  ['broyden', 'student-turn-20ms-broyden', 'broyden-turn'],
  ['minute-refinement', 'combined-minute-1.25ms', 'reference-minute'],
]) {
  const plan = JSON.parse(readFileSync(`${root}/${study}-plan.json`));
  const status = JSON.parse(readFileSync(`${root}/${study}-status.json`));
  const entry = plan.cases.find(c => c.name === name), result = status.cases.find(c => c.name === name);
  assert(status.complete && result.completed && result.passed);
  for (const key of ['scene', 'config']) {
    const bytes = readFileSync(entry[key]), source = result.sources.find(s => s.path === entry[key]);
    assert.equal(createHash('sha256').update(bytes).digest('hex'), source.sha256);
    writeFileSync(`${root}/${prefix}.${key}.json`, bytes);
  }
}
