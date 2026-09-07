import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
const root='examples/full-robot/browser-precision',read=p=>JSON.parse(readFileSync(p));
const original=read(`${root}/validation-initial-status.json`),refined=read(`${root}/refined-validation-status.json`);
assert(original.complete&&refined.complete);
assert.equal(refined.cases.length,1);assert.equal(refined.cases[0].name,'refined-minute');assert(refined.passed);
const previous=original.cases.find(c=>c.name==='refined-minute');
assert(!previous.passed&&previous.error.includes('step 0: cached mechanical iteration limit'));
const corrected=read(`${root}/guarded-refined-minute.config.json`),invalid=read(`${root}/guarded-refined-minute.invalid.config.json`);
const comparison=structuredClone(corrected);comparison.implicit.restart_failed_reused_mechanics=invalid.implicit.restart_failed_reused_mechanics;
if(!Object.hasOwn(invalid.implicit,'restart_failed_reused_mechanics'))delete comparison.implicit.restart_failed_reused_mechanics;
assert(isDeepStrictEqual(comparison,invalid),'corrected recipe may only enable fresh restart');assert(corrected.implicit.restart_failed_reused_mechanics);
const cases=original.cases.map(c=>c.name==='refined-minute'?refined.cases[0]:c);
assert(cases.every(c=>c.passed));
writeFileSync(`${root}/validation-status.json`,JSON.stringify({version:1,complete:true,passed:true,cases,
 prior_invalid_recipe:{path:`${root}/validation-initial-status.json`,sha256:createHash('sha256').update(readFileSync(`${root}/validation-initial-status.json`)).digest('hex')},
 scope:'Five independent executions including corrected refined timestep, two reserved yaw probes and a forward/turn/reverse/stop schedule. The initial invalid refined recipe remains archived. No acceptance threshold changed; this is not broad terrain or hardware-transfer acceptance.'},null,2)+'\n');
