// Cross-check batched audits against the single-window CLI, including rejection.
import {readFileSync,writeFileSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
const [capture,acceptance]=process.argv.slice(2);assert(capture&&acceptance);
const read=p=>JSON.parse(readFileSync(p)),base=read(`${acceptance}/lifts.requirements.json`);
const windows=[base[0],base.at(-1),{...base[0],minimum_clearance_m:100}];
const invoke=(input,name)=>{const p=`${acceptance}/${name}.json`;writeFileSync(p,JSON.stringify(input));return JSON.parse(execFileSync('target/release/examples/evaluate_lift',[`${acceptance}/recorded.scene.json`,capture,p],{encoding:'utf8',maxBuffer:128*1024*1024}));};
const batch=invoke(windows,'batch-equivalence-requirements');
for(let i=0;i<windows.length;i++)assert.deepEqual(batch.reports[i].report,invoke(windows[i],`single-equivalence-${i}`).report);
assert.equal(batch.reports[2].report.passed,false);
const result={passed:true,windows:windows.length,rejected_clearance_case:true,scope:'Batched and single-window CLI reports are identical for distinct and repeated swing links, including a deliberately impossible clearance requirement.'};
writeFileSync(`${acceptance}/batch-equivalence.json`,JSON.stringify(result,null,2));console.log(JSON.stringify(result));
