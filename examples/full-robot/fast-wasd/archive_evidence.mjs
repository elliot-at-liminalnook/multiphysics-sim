import fs from 'node:fs';import crypto from 'node:crypto';import {spawnSync} from 'node:child_process';
const d='examples/full-robot/fast-wasd',sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const files=fs.readdirSync(d).filter(n=>!n.startsWith('evidence.')&&!n.endsWith('.tar.gz')&&fs.statSync(`${d}/${n}`).isFile()&&fs.statSync(`${d}/${n}`).size>262144).sort();
const index=files.map(path=>({path,bytes:fs.statSync(`${d}/${path}`).size,sha256:sha(`${d}/${path}`)}));
fs.writeFileSync(`${d}/evidence-index.json`,JSON.stringify({version:1,files:index,scope:'Restore every listed file from the versioned archive parts. Raw large files remain locally for review but are ignored individually.'},null,2)+'\n');
fs.writeFileSync(`${d}/archive-members.txt`,files.join('\n')+'\n');
const archive=`${d}/evidence.tar.gz`;let r=spawnSync('tar',['-czf',archive,'-C',d,'-T',`${d}/archive-members.txt`],{stdio:'inherit'});if(r.status!==0)throw Error('archive failed');
const data=fs.readFileSync(archive),parts=[],size=45*1024*1024;
for(let at=0,i=0;at<data.length;at+=size,i++){
 const path=`evidence.part-${String(i).padStart(2,'0')}`;fs.writeFileSync(`${d}/${path}`,data.subarray(at,at+size));parts.push({path,bytes:fs.statSync(`${d}/${path}`).size,sha256:sha(`${d}/${path}`)});
}
fs.writeFileSync(`${d}/archive-sha256.json`,JSON.stringify({archive:{bytes:data.length,sha256:sha(archive)},parts},null,2)+'\n');
fs.writeFileSync(`${d}/.gitignore`,['/evidence.tar.gz',...files.map(f=>'/'+f)].join('\n')+'\n');
r=spawnSync('tar',['-tzf',archive],{encoding:'utf8'});if(r.status!==0||r.stdout.trim().split('\n').sort().join('\n')!==files.join('\n'))throw Error('archive member verification failed');
const restored=fs.mkdtempSync('runs/fast-wasd/archive-check-');
r=spawnSync('tar',['-xzf',archive,'-C',restored],{stdio:'inherit'});if(r.status!==0)throw Error('restore failed');
for(const file of index)if(sha(`${restored}/${file.path}`)!==file.sha256)throw Error(`restored hash mismatch: ${file.path}`);
fs.rmSync(restored,{recursive:true});
fs.writeFileSync(`${d}/archive-verification.json`,JSON.stringify({restored_files_verified:index.length,archive_sha256:sha(archive)},null,2)+'\n');
console.log({archived_files:files.length,archive_bytes:data.length,parts:parts.length});
