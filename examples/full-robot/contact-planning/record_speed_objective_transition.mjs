import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {gzipSync,gunzipSync} from 'node:zlib';
const root='examples/full-robot/contact-planning/',dir=root+'speed-objective-evidence-files';
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=b=>crypto.createHash('sha256').update(b).digest('hex');
const identity=p=>({path:p,bytes:fs.statSync(p).size,sha256:hash(fs.readFileSync(p))});
const files=[];
function include(directory,predicate=()=>true){
  for(const n of fs.readdirSync(directory).sort()){
    if(!predicate(n))continue;
    const p=path.join(directory,n),stat=fs.lstatSync(p);assert(!stat.isSymbolicLink());
    if(stat.isDirectory())include(p);else files.push(p);
  }
}
// This run was intentionally terminated after the user's criterion change.
// Its partially executed trial remains partial evidence, never a zero score.
const stopped=read(root+'bayesian-human20-slip5-stopped.json');
assert.equal(stopped.completed_evaluations.length,14);
include('runs/bayesian-human20-pilot-v2');
const validations=['human20-lhs70','human20-lhs90'].map(id=>read(root+id+'-validation.summary.json'));
for(const v of validations){
  assert.equal(v.rows.length,4);assert(v.rows.every(r=>r.completed));
  for(const r of v.rows){
    assert.equal(identity(r.prefix+'.native.json').sha256,r.capture_sha256);
    include(path.dirname(r.prefix),n=>n.startsWith(path.basename(r.prefix)+'.'));
  }
}
const source='runs/contact-planning/diagonal21-screen',prepared='runs/contact-planning/speed-discovery-diagonal21';
const scene=read(prepared+'.scene.json'),original=read(source+'.scene.json');
scene.duration_s=original.duration_s;assert.deepEqual(scene,original);
const cfg=read(prepared+'.config.json'),oldCfg=read(source+'.config.json');
cfg.steps=oldCfg.steps;assert.deepEqual(cfg,oldCfg);
include(path.dirname(prepared),n=>n.startsWith(path.basename(prepared)+'.'));
files.push(root+'speed-discovery-metric-tests.log',root+'bayesian-human20-pilot-v2.log',root+'bayesian-human20-pilot-v2.error.log');
fs.mkdirSync(dir);
const archives=[];
for(const p of files){
  const b=fs.readFileSync(p),archive=dir+'/'+hash(b)+'.gz';
  if(!fs.existsSync(archive))fs.writeFileSync(archive,gzipSync(b,{level:9}),{flag:'wx'});
  assert(gunzipSync(fs.readFileSync(archive)).equals(b));
  archives.push({original:identity(p),archive:identity(archive)});
}
fs.writeFileSync(root+'speed-objective-evidence.json',JSON.stringify({archives,stopped,validations,
  new_context:read('runs/bayesian-speed-only-diagonal21/context.json'),
  code:identity(import.meta.filename),
  verification:'Every archived file round-trips byte-for-byte. New source scene/config differ from diagonal21 only in duration and step count.',
  restore:'Gunzip archive to original path and verify SHA-256. CAD baseline and full scene provenance are retained by speed-ceiling/evidence-v8-index.json and the existing contact-planning evidence chain.',
  scope:'Complete obsolete slip-constrained trials, canceled partial trial, completed 70/90 validation and new speed-only input artifacts. Legacy pass/fail fields are historical diagnostics, not current speed acceptance. The speed-only search and corrected boundary planning replay are still running.'},null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({files:archives.length,unique:fs.readdirSync(dir).length,verified:true}));
