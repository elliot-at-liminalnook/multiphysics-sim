import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {gzipSync,gunzipSync} from 'node:zlib';
const root='examples/full-robot/contact-planning/',dir=root+'mixed-search-evidence-files';
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=b=>crypto.createHash('sha256').update(b).digest('hex');
const evidence=p=>({path:p,bytes:fs.statSync(p).size,sha256:hash(fs.readFileSync(p))});
const validation=read(root+'diagonal81-validation.summary.json');
assert.equal(validation.rows.length,4);assert(validation.rows.every(r=>r.completed));
const trial='runs/bayesian-diagonal68-pilot/evaluation-025';
const record=read(trial+'.evaluation.json');assert(record.observation.outcome.residuals.every(v=>v<=0));
const prefixes=[...validation.rows.map(r=>r.prefix),trial,'runs/contact-planning/diagonal68-human-0p625ms'];
const archives=[];
for(const prefix of prefixes)for(const name of fs.readdirSync(path.dirname(prefix)).filter(n=>n.startsWith(path.basename(prefix)+'.')).sort()) {
 const p=path.join(path.dirname(prefix),name);assert(fs.statSync(p).isFile());
 const bytes=fs.readFileSync(p),id=hash(bytes),archive=dir+'/'+id+'.gz';
 if(!fs.existsSync(archive))fs.writeFileSync(archive,gzipSync(bytes,{level:9}),{flag:'wx'});
 assert(gunzipSync(fs.readFileSync(archive)).equals(bytes));archives.push({original:evidence(p),archive:evidence(archive)});
}
fs.writeFileSync(root+'mixed-validation-extension.json',JSON.stringify({version:1,archives,validation,trial:record,
 code:evidence(import.meta.filename),diagnostic:read(root+'diagonal-contact-windows.result.json'),
 scope:'Additional complete captures, inputs and audit results for rejected 81-command cases, sampled control winner 025 and the 68-command phase comparison. The 78-command validation remains live and is not claimed complete.'},null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({files:archives.length,verified:true}));
