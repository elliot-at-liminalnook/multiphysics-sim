import fs from 'node:fs';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import crypto from 'node:crypto';
import {gzipSync,gunzipSync} from 'node:zlib';
const d='examples/full-robot/contact-planning/',prefix=d+'joint-conic-patterns';
const rawPath=prefix+'.result.jsonl',zipPath=rawPath+'.gz';
const bytes=fs.existsSync(rawPath)?fs.readFileSync(rawPath):gunzipSync(fs.readFileSync(zipPath));
const rows=bytes.toString().trim().split('\n').map(JSON.parse),batch=JSON.parse(fs.readFileSync(prefix+'.batch.json'));
assert.equal(rows.length,batch.starts.length);assert.equal(new Set(rows.map(r=>r.id)).size,rows.length);
const starts=new Map(batch.starts.map(s=>[s.id,s]));
for(const row of rows){assert(starts.has(row.id));if(row.result?.candidate)assert(isDeepStrictEqual(row.result.candidate.motion,starts.get(row.id).candidate.motion),'Conic screen changed fixed motion');if(row.result?.independent_affine_error!=null)assert(row.result.independent_affine_error<1e-8);}
const trial=rows.filter(r=>!r.id.startsWith('control')),valid=trial.filter(r=>r.result?.physical),ranked=[...valid].sort((a,b)=>a.result.physical.maximum_inequality-b.result.physical.maximum_inequality);
const statuses={};for(const r of trial){const key=r.error?'error':r.result.conic.status;statuses[key]=(statuses[key]??0)+1;}
const identity=path=>{const b=fs.readFileSync(path);return {path,bytes:b.length,sha256:crypto.createHash('sha256').update(b).digest('hex')};};
if(!fs.existsSync(zipPath))fs.writeFileSync(zipPath,gzipSync(bytes,{level:9}),{flag:'wx'});assert(gunzipSync(fs.readFileSync(zipPath)).equals(bytes));
const output={cases:rows.length,experimental_cases:trial.length,errors:trial.filter(r=>r.error).map(r=>({id:r.id,error:r.error})),missing_primal_candidates:trial.filter(r=>r.result&&!r.result.physical).length,fast_sampled_feasible:valid.filter(r=>r.result.physical.sampled_feasible).map(r=>r.id),statuses,maximum_independent_affine_error:Math.max(...rows.map(r=>r.result?.independent_affine_error??0)),best_by_maximum_physical_inequality:ranked.slice(0,12).map(r=>({id:r.id,conic:r.result.conic,physical:r.result.physical})),controls:rows.filter(r=>r.id.startsWith('control')).map(r=>({id:r.id,conic:r.result.conic,physical:r.result.physical})),input:identity(prefix+'.batch.json'),compressed_output:identity(zipPath),uncompressed_output:{bytes:bytes.length,sha256:crypto.createHash('sha256').update(bytes).digest('hex')},scope:'Completed fixed-motion contact-pattern initialization screen at 0.211710 m/s. A failed starting motion does not exclude its optimized gait family. No new runtime gait or physical speed maximum.'};
fs.writeFileSync(prefix+'.summary.json',JSON.stringify(output,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({cases:output.cases,statuses,fast_feasible:output.fast_sampled_feasible.length,errors:output.errors.length,best:output.best_by_maximum_physical_inequality[0],compressed_bytes:output.compressed_output.bytes}));
