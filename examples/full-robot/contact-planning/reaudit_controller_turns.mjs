import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
const [specPath]=process.argv.slice(2);assert(specPath);
const read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v,null,2)+'\n',{flag:'wx'});
const spec=read(specPath);fs.mkdirSync(spec.output_root);
const rows=[];
write(spec.output_root+'/launch.json',{spec,code_sha256:hash(import.meta.filename),audit_code_sha256:hash('examples/full-robot/contact-planning/audit_contact_controller.mjs')});
for(const casesPath of spec.cases)for(const row of read(casesPath).rows.filter(r=>r.kind==='human')) {
 const prefix=spec.output_root+'/'+row.name;
 write(prefix+'.spec.json',{prefix:row.prefix,audit_prefix:prefix,windows_s:spec.windows_s});
 const out=fs.openSync(prefix+'.stdout.json','wx'),err=fs.openSync(prefix+'.stderr.log','wx');
 const started=performance.now();let r;
 try{r=spawnSync('node',['examples/full-robot/contact-planning/audit_contact_controller.mjs',prefix+'.spec.json',prefix+'.summary.json'],{stdio:['ignore',out,err]});}
 finally{fs.closeSync(out);fs.closeSync(err);}
 write(prefix+'.execution.json',{exit_code:r.status,signal:r.signal,error:r.error?.message??null,wall_s:(performance.now()-started)/1000});
 assert.equal(r.status,0,'re-audit failed; preserve logs');
 const summary=read(prefix+'.summary.json');
 rows.push({name:row.name,summary_path:prefix+'.summary.json',summary_sha256:hash(prefix+'.summary.json'),summary});
 console.error(JSON.stringify({name:row.name,passed:summary.passed_foot_clearances,planned:summary.planned_foot_clearances}));
}
write(spec.result,{spec,rows,scope:'Corrected historical human lift windows include turning. Original captures and earlier audit outputs preserved. Control/slip results are unchanged; these reports supersede earlier human lift counts.'});
