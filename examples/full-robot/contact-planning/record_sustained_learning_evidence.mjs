// Preserve completed experiments; never include an active run's open outputs.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {gzipSync,gunzipSync} from 'node:zlib';
const d='examples/full-robot/contact-planning',dir=d+'/speed-objective-evidence-files';
const read=p=>JSON.parse(fs.readFileSync(p)),sha=b=>crypto.createHash('sha256').update(b).digest('hex');
const identity=p=>({path:p,bytes:fs.statSync(p).size,sha256:sha(fs.readFileSync(p))});
const files=[];
function include(root){for(const n of fs.readdirSync(root).sort()){
  const p=path.join(root,n),s=fs.lstatSync(p);assert(!s.isSymbolicLink());
  if(s.isDirectory())include(p);else files.push(p);
}}
function prefix(p){for(const n of fs.readdirSync(path.dirname(p)))if(n.startsWith(path.basename(p)+'.'))files.push(path.join(path.dirname(p),n));}
const comparisons=[];
for(const root of ['runs/bayesian-speed-only-diagonal21','runs/bayesian-speed-only-fast-short-trial']){
  const r=read(root+'/result.json');assert(r.adaptive.length===25&&r.control.length===25);
  comparisons.push({root,adaptive:r.best_adaptive,control:r.best_control,
    scope:'One seed, matched trial counts and shared initial observations; not a statistical significance claim or exhaustive search.'});
  include(root);
}
for(const root of ['runs/speed-only-462-validation','runs/speed-only-485-validation']){
  assert(read(root+'/summary.json').rows.every(r=>r.execution.exit_code===0));include(root);
}
for(const root of ['runs/speed-462-self-contact-smoke','runs/speed-462-self-contact20']){
  assert(read(root+'/summary.json').completed);include(root);
}
assert(read('runs/speed-neural-aggregation-v2/evaluation/student.summary.json').completed);
include('runs/speed-neural-aggregation-v2');
assert.equal(read('runs/speed-neural-residual-448/summary.json').first_physical_difference,null);
include('runs/speed-neural-residual-448');include('runs/speed-neural-residual-462');
prefix('runs/contact-planning/speed448-bounded60');
for(const id of ['011','015'])prefix('runs/bayesian-speed-only-bounded-diagonal21/evaluation-'+id);
for(const id of ['000','001','002'])prefix('runs/bayesian-speed-only-fast-expanded/evaluation-'+id);
for(const id of ['000','001']){
  const p='runs/bayesian-sustained60-fast/evaluation-'+id;assert(read(p+'.evaluation.json').observation.outcome.status==='complete');prefix(p);
}
for(const root of ['runs/bayesian-speed-only-bounded-diagonal21','runs/bayesian-speed-only-fast-expanded','runs/bayesian-sustained60-fast']){
  files.push(root+'/context.json',root+'/spec.json');
}
for(const name of ['speed-neural-aggregation-v2.summary.json','speed-neural-observation-neighbors.json','sustained-net-progress-diagnostics.json'])files.push(d+'/'+name);
const archives=[];
for(const p of [...new Set(files)].sort()){
  const b=fs.readFileSync(p),original={path:p,bytes:b.length,sha256:sha(b)},archive=dir+'/'+original.sha256+'.gz';
  if(!fs.existsSync(archive))fs.writeFileSync(archive,gzipSync(b,{level:6}),{flag:'wx'});
  assert(gunzipSync(fs.readFileSync(archive)).equals(b));
  assert.equal(identity(p).sha256,original.sha256,'source changed during archival');
  archives.push({original,archive:identity(archive)});
}
fs.writeFileSync(d+'/sustained-learning-evidence.json',JSON.stringify({archives,comparisons,code:identity(import.meta.filename),
  scope:'Complete original speed20 comparisons, 0.462/0.485 longer/finer evidence, inter-link-contact comparisons, failed data-aggregation student, zero-residual equivalence, expanded-window seeds and completed sustained60 baseline/first trial. Active later trials and active contact60 output are excluded. The 0.462 residual input is an unexecuted preparation. Every gzip restores original bytes.'},null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({files:archives.length,verified:true}));
