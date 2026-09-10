// Artifact transport only: explicit same-settings native/WASM portability plan.
// Physical accuracy budgets must be authored separately for profile experiments.
import fs from 'node:fs';
import {createHash} from 'node:crypto';
import {cpus,platform,arch} from 'node:os';
import assert from 'node:assert/strict';
const [reference,candidate,nativeExecutable,wasmExecutable,sourceReference,output]=process.argv.slice(2);
assert(output,'usage: prepare_fidelity_parity_plan native-capture browser-capture native-executable wasm-executable source-reference fresh-plan');
const hash=p=>'sha256:'+createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const host=`${platform()}/${arch()}/${cpus()[0]?.model}`;
const plan={version:1,changes:[],absolute_tolerances:Object.fromEntries(['rad','rad/s','m','m/s','m/s²','1','s','N'].map(unit=>[unit,1e-7])),
 reference:{capture_reference:`${reference}#${hash(reference)}`,source_reference:sourceReference,executable_reference:`${nativeExecutable}#${hash(nativeExecutable)}`,
 host_reference:host,timing_scope:'run_environment stepping loop including endpoint observation/capture; excludes construction and final serialization'},
 candidate:{capture_reference:candidate,source_reference:sourceReference,executable_reference:`${wasmExecutable}#${hash(wasmExecutable)}`,
 host_reference:`headless Chromium worker on ${host}; browser version recorded in browser report`,timing_scope:'worker transition round trips including serialization; excludes construction, replay, forecasting, comparison and rendering'}};
fs.writeFileSync(output,JSON.stringify(plan,null,2)+'\n',{flag:'wx'});
