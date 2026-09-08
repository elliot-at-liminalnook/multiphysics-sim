// Strict parsed-value equality, including signed zero, outside wall clocks.
import {readFileSync,writeFileSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
import assert from 'node:assert/strict';
const [studyPath,output,previousPath]=process.argv.slice(2);assert(output,'usage: verify_solver_pair_identity study.json report.json [previous-reference]');
const read=p=>JSON.parse(readFileSync(p)),source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const study=read(studyPath),plan=read(study.plan_output),paths=plan.cases.map(c=>`${study.output}/${c.name}.native.json`);
const native=paths.map(read);assert(native.every(c=>c.completed&&!c.error));
const frames=c=>c.frames.map(({stepping_wall_s,...f})=>f);
const cleanRecord=c=>{c=structuredClone(c.recording);const keys=study.boolean_solver_option.split('.'),field=keys.pop();let p=c.config;for(const key of keys)p=p[key];delete p[field];return c;};
const checks=[];
function verify(a,b,label,optionDifference=false){
  const passed=isDeepStrictEqual(frames(a),frames(b))&&isDeepStrictEqual(a.transitions,b.transitions)
    &&isDeepStrictEqual(a.task,b.task)&&isDeepStrictEqual(a.contract,b.contract)
    &&isDeepStrictEqual(optionDifference?cleanRecord(a):a.recording,optionDifference?cleanRecord(b):b.recording);
  checks.push({label,passed,frames:a.frames.length,transitions:a.transitions.length});
}
verify(native[0],native[1],'declared solver option off/on',true);
const sources=[studyPath,study.plan_output,...paths,import.meta.filename];
for(const [i,c] of plan.cases.entries()){
  const path=`${study.output}/${c.name}.profiled.native.json`;
  if(existsSync(path)){verify(native[i],read(path),`${c.name}: profiling off/on`);sources.push(path);}
}
if(previousPath){verify(native[0],read(previousPath),'default-off previous reference');sources.push(previousPath);}
const passed=checks.every(c=>c.passed);
writeFileSync(output,JSON.stringify({version:1,passed,checks,sources:sources.map(source),
  scope:'Strict equality of every parsed physical frame (except its top-level stepping clock), task transition, task, contract and recording. Signed zero is distinguished. Only the explicitly declared solver option is ignored in the off/on recording pair; no numerical tolerance is used.'},null,2)+'\n');
console.log({passed,checks});assert(passed);
