import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {gzipSync,gunzipSync} from 'node:zlib';
import {execFileSync} from 'node:child_process';
const root='examples/full-robot/contact-planning/';
const read=p=>JSON.parse(fs.readFileSync(p));
const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
const evidence=p=>({path:p,bytes:fs.statSync(p).size,sha256:sha(fs.readFileSync(p))});
const write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v,null,2)+'\n',{flag:'wx'});
const files=[];
function include(dir,predicate=()=>true){for(const n of fs.readdirSync(dir).sort()) {
  const p=path.join(dir,n);if(!predicate(n))continue;
  const s=fs.lstatSync(p);assert(!s.isSymbolicLink());if(s.isDirectory())include(p);else files.push(p);
}}
// The faulty launch has an authoritative terminal result. All measurements
// are absent; no syntax/configuration failure becomes a physical observation.
const failed=read('runs/mixed-contact-cem-pilot/result.json');
assert.equal(failed.observations.length,24);
assert(failed.observations.every(o=>o.outcome.status==='failed'));
include('runs/mixed-contact-cem-pilot');
for(const id of ['000','009','018']) {
  const dir='runs/bayesian-diagonal68-pilot',prefix='evaluation-'+id;
  const e=read(dir+'/'+prefix+'.evaluation.json');
  assert.equal(e.capture_sha256,sha(fs.readFileSync(dir+'/'+prefix+'.native.json')));
  include(dir,n=>n.startsWith(prefix+'.')||n.startsWith(prefix+'-'));
}
// The complete 77-command validation includes all native captures, exact
// policy replay and geometry/lift reports, including its rejected cases.
const validation=read(root+'diagonal77-validation.summary.json');
assert.equal(validation.rows.length,4);
for(const row of validation.rows) {
  assert.equal(row.capture_sha256,sha(fs.readFileSync(row.prefix+'.native.json')));
  include(path.dirname(row.prefix),n=>n.startsWith(path.basename(row.prefix)+'.'));
}
const archiveDir=root+'mixed-search-evidence-files';fs.mkdirSync(archiveDir);
const seen=new Map(),archives=[];
for(const p of files) {
  const bytes=fs.readFileSync(p),id=sha(bytes);let archive=seen.get(id);
  if(!archive){archive=archiveDir+'/'+id+'.gz';fs.writeFileSync(archive,gzipSync(bytes,{level:9}),{flag:'wx'});
    assert(gunzipSync(fs.readFileSync(archive)).equals(bytes));seen.set(id,archive);}
  archives.push({original:evidence(p),archive:evidence(archive)});
}
const code=['crates/sim-solve/src/mixed_cem.rs','crates/sim-solve/src/bayesian.rs','crates/sim-solve/src/lib.rs',
 'crates/sim-solve/Cargo.toml','crates/sim-solve/examples/mixed_cem.rs',
 'crates/sim-runtime/src/contact_planning/joint.rs','crates/sim-runtime/src/contact_planning/joint_orders.rs',
 'crates/sim-runtime/examples/set_joint_initializer.rs',
 'examples/full-robot/speed-ceiling/analyze_validation.mjs',
 ...['run_mixed_contact_search.mjs','run_mixed_contact_search-v1.mjs','prepare_controller_validation.mjs',
 'run_controller_validation.mjs','record_mixed_search.mjs'].map(n=>root+n)];
const inputs=['mixed-contact-cem-pilot.spec.json','mixed-contact-cem-pilot-v2.spec.json',
 'mixed-cem-edge006-initial.recipe.json','bayesian-diagonal68-pilot.spec.json',
 'diagonal77-validation-settings.json','diagonal77-validation-cases.json',
 'diagonal81-validation-settings.json','diagonal81-validation-cases.json'].map(n=>root+n);
const logs=['mixed-cem-tests.log','mixed-cem-build.log','mixed-cem-initializer-tests.log',
 'mixed-cem-initializer-build.log','mixed-contact-cem-pilot.log','mixed-contact-cem-pilot.error.log',
 'diagonal77-validation.log','diagonal77-validation.error.log'].map(n=>root+n);
const live=['runs/bayesian-diagonal68-pilot/context.json','runs/mixed-contact-cem-pilot-v2/context.json',
 root+'diagonal81-validation.launch.json'].map(p=>({path:p,content:read(p),identity:evidence(p)}));
const candidates=['009','018'].map(id=>read('runs/bayesian-diagonal68-pilot/evaluation-'+id+'.evaluation.json'));
write(root+'mixed-search-evidence.json',{version:1,recorded_at:new Date().toISOString(),
 base_commit:execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim(),code:code.map(evidence),
 inputs:inputs.map(evidence),logs:logs.map(evidence),archives,candidates,validation,
 verification:{solver_tests:41,planner_tests:15,failed_launch_observations:24,
  baseline_replay:'Physical frames match the diagonal68 source exactly; only stepping wall-time telemetry excluded.'},
 active_launch_contexts:live,scene_archive_index:'examples/full-robot/speed-ceiling/evidence-v8-index.json',
 scope:'Complete failed mixed-search launch, selected complete short-screen trials and complete 77-command validation archived. Other live search/81-command validation outputs are not archived as terminal evidence. Short-screen progress does not establish a qualified gait, contact-family exhaustion or physical speed maximum.'});
console.log(JSON.stringify({files:archives.length,unique_archives:seen.size,
 compressed_bytes:[...seen.values()].reduce((n,p)=>n+fs.statSync(p).size,0),verified:true}));
