import fs from 'node:fs';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
const dir=import.meta.dirname,read=name=>JSON.parse(fs.readFileSync(`${dir}/${name}.json`));
const protocol=read('protocol');
function exact(a,b) {
 assert(a.completed&&b.completed&&!a.error&&!b.error);assert.equal(a.frames.length,b.frames.length);
 const fields=Object.keys(a.frames[0]).filter(k=>k!=='stepping_wall_s');
 for(let i=0;i<a.frames.length;i++) {assert.deepEqual(Object.keys(a.frames[i]),Object.keys(b.frames[i]));for(const key of fields)assert.deepEqual(a.frames[i][key],b.frames[i][key],`frame ${i}: ${key}`);}
 assert.deepEqual(a.transitions,b.transitions);return {frames:a.frames.length,exact_fields:fields,all_task_transitions_exact:true};
}
const baseline=read('candidate.native');
const report={scope:'Ordered native single-run measurements of the same three-second command schedule. No repeated-machine estimate or timestep/hardware fidelity claim; original gates are unchanged.',default_parity:exact(read('../direct-dense-products/restored.native'),baseline),runs:{}};
for(const folder of ['',...Object.keys(protocol.variants)]) {
 const prefix=folder?`${folder}/candidate`:'candidate';
 if(!fs.existsSync(`${dir}/${prefix}.profile.json`)) {report.runs[folder]={incomplete:true};continue;}
 const p=read(`${prefix}.profile`),capture=read(`${prefix}.native`),parity=exact(capture,read(`${prefix}.profiled`));
 const stats={};for(const row of p.motor_trial_statistics)for(const [key,value]of Object.entries(row))stats[key]=key.startsWith('maximum')?Math.max(stats[key]??0,value):(stats[key]??0)+value;
 const result=spawnSync(process.execPath,[`${dir}/../qualify.mjs`,`outer-secant-only/${prefix}`,`outer-secant-only/candidate`],{encoding:'utf8'});assert.equal(result.status,0,result.stderr);
 const qualification=read(`${prefix}.qualification`),inputs=folder?protocol.variants[folder].inputs:protocol.inputs;
 for(const [name,hash]of Object.entries(inputs))assert.equal(createHash('sha256').update(fs.readFileSync(`${dir}/${folder?folder+'/':''}${name}`)).digest('hex'),hash);
 assert.equal(read(`${prefix}.binary`).sha256,protocol.binary_candidate_sha256);
 report.runs[folder||'default']={qualification,profile_parity:parity,statistics:stats,buckets:p.buckets.filter(b=>b.calls>0),controller_exact:Object.fromEntries(['servo_states','servo_commands'].map(key=>[key,baseline.frames.every((frame,i)=>isDeepStrictEqual(frame[key],capture.frames[i][key]))]))};
 console.log(folder||'default',JSON.stringify({wall_s:capture.wall_s,speedup:qualification.speedup,accepted:qualification.accepted_as_solver_optimization,max_error:qualification.max_error}));
}
assert.deepEqual(read('../jacobian-use-limit/candidate.profile').motor_trial_statistics,read('candidate.profile').motor_trial_statistics);
assert.deepEqual(read('../jacobian-use-limit/candidate.profile').accepted_intervals,read('candidate.profile').accepted_intervals);
report.default_solver_diagnostics_exact=true;
fs.writeFileSync(`${dir}/analysis.json`,JSON.stringify(report,null,2)+'\n');
