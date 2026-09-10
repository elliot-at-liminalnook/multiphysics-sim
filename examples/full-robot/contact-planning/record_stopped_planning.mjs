import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {gzipSync,gunzipSync} from 'node:zlib';
const d='examples/full-robot/contact-planning',dir=d+'/speed-objective-evidence-files';
const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
const identity=p=>({path:p,bytes:fs.statSync(p).size,sha256:sha(fs.readFileSync(p))});
const files=[];
function include(directory){for(const n of fs.readdirSync(directory).sort()){
  const p=path.join(directory,n),s=fs.lstatSync(p);assert(!s.isSymbolicLink());
  if(s.isDirectory())include(p);else files.push(p);
}}
for(const root of ['runs/joint-checkpoints/multi-step-speed','runs/mixed-contact-cem-pilot-v2',
  'runs/contact-boundary-replay','runs/contact-order-refinements','runs/contact-order-refined-control'])include(root);
for(const name of ['joint-mesh-command-speed','joint-servo-command-speed','joint-step-speed'])
  for(const suffix of ['log','result.json'])files.push(d+'/'+name+'.'+suffix);
const archives=[];
for(const p of files){const b=fs.readFileSync(p),archive=dir+'/'+sha(b)+'.gz';
  if(!fs.existsSync(archive))fs.writeFileSync(archive,gzipSync(b,{level:9}),{flag:'wx'});
  assert(gunzipSync(fs.readFileSync(archive)).equals(b));archives.push({original:identity(p),archive:identity(archive)});
}
fs.writeFileSync(d+'/stopped-planning-evidence.json',JSON.stringify({archives,
  stop_record:identity(d+'/planning-speed-objective-stopped.json'),code:identity(import.meta.filename),
  scope:'Stopped planner-only searches, corrected boundary replay, and terminal neighborhood/older joint solves. Complete raw checkpoint directories and partial outputs are retained. A partial output is not convergence, family exclusion or physical speed-limit evidence. Gzip restoration was checked byte-for-byte.'},null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({files:archives.length,verified:true}));
