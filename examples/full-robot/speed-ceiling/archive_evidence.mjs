// Snapshot only after experiment processes finish. Verify every extracted file.
import fs from 'node:fs';import path from 'node:path';import os from 'node:os';
import crypto from 'node:crypto';import {spawnSync} from 'node:child_process';
const d=process.argv[5]??'examples/full-robot/speed-ceiling',root=process.argv[4]??'runs/speed-ceiling',version=process.argv[2]??'v1';
if(!/^v[0-9]+$/.test(version))throw Error('snapshot version must be v<number>');
const stem=`evidence-${version}`;
fs.mkdirSync(d,{recursive:true});
if(fs.existsSync(`${d}/${stem}-index.json`))throw Error('refusing to overwrite an evidence snapshot');
const hash=async p=>{const h=crypto.createHash('sha256');for await(const chunk of fs.createReadStream(p))h.update(chunk);return h.digest('hex');};
const files=[];
function walk(dir){for(const name of fs.readdirSync(dir).sort()){
 const p=path.join(dir,name),s=fs.lstatSync(p);if(s.isSymbolicLink())throw Error(`unexpected symlink ${p}`);
 if(s.isDirectory())walk(p);else if(s.isFile())files.push(path.relative(root,p));else throw Error(`unexpected entry ${p}`);
}}
walk(root);
console.log({snapshot_files:files.length,root});
const snapshotEntries=[];for(const file of files)snapshotEntries.push({file,size_bytes:fs.statSync(path.join(root,file)).size,sha256:await hash(path.join(root,file))});
const basePath=process.argv[3]==='-'?undefined:process.argv[3],base=basePath?JSON.parse(fs.readFileSync(basePath)):null;
if(base&&base.root!==root)throw Error('incremental base root mismatch');
const previous=new Map((base?.snapshot_files??base?.files??[]).map(e=>[e.file,e]));
const entries=snapshotEntries.filter(e=>previous.get(e.file)?.sha256!==e.sha256);
const archivedFiles=entries.map(e=>e.file),deletedFiles=[...previous.keys()].filter(file=>!files.includes(file));
// An overlay with deletions needs explicit removal semantics; do not silently
// leave stale files behind in a restored experimental baseline.
if(deletedFiles.length)throw Error('incremental snapshot with deletions is unsupported; make a full snapshot');
if(!entries.length)throw Error('no changed evidence to archive');
const scratch=fs.mkdtempSync(path.join(os.tmpdir(),'speed-ceiling-evidence-')),archive=path.join(scratch,'evidence.tar.gz');
const run=(args)=>{const r=spawnSync('tar',args,{stdio:'inherit',env:{...process.env,COPYFILE_DISABLE:'1'}});if(r.status!==0)throw Error(`tar failed ${r.status}`);};
run(['-czf',archive,'-C',root,...archivedFiles]);
const archiveSha=await hash(archive),parts=[],chunkSize=45*1024*1024;
const input=fs.openSync(archive,'r'),buffer=Buffer.alloc(chunkSize);let index=0,position=0;
while(true){let n=0;while(n<chunkSize){const count=fs.readSync(input,buffer,n,chunkSize-n,position+n);if(count===0)break;n+=count;}if(!n)break;
 const file=`${stem}.part-${String(index++).padStart(2,'0')}`;
 fs.writeFileSync(`${d}/${file}`,buffer.subarray(0,n),{flag:'wx'});parts.push({file,size_bytes:n,sha256:await hash(`${d}/${file}`)});position+=n;console.log(file,n);
}fs.closeSync(input);
const extracted=path.join(scratch,'verify');fs.mkdirSync(extracted);run(['-xzf',archive,'-C',extracted]);
for(const entry of entries){const p=path.join(extracted,entry.file);if(fs.statSync(p).size!==entry.size_bytes||await hash(p)!==entry.sha256)throw Error(`extraction mismatch ${entry.file}`);}
// Confirm no source changed during compression/verification.
for(const entry of snapshotEntries)if(await hash(path.join(root,entry.file))!==entry.sha256)throw Error(`source changed during snapshot ${entry.file}`);
fs.writeFileSync(`${d}/${stem}-index.json`,JSON.stringify({version:2,root,archive_sha256:archiveSha,archive_size_bytes:position,parts,files:entries,snapshot_files:snapshotEntries,
 base:base?{index:basePath,index_sha256:await hash(basePath),restore_first:base.restore}:null,
 verification:{extracted_files_verified:entries.length,source_files_rechecked:snapshotEntries.length},
 restore:`${base?`${base.restore}; `:''}cat ${d}/${stem}.part-* > /tmp/${stem}.tar.gz; mkdir -p ${root}; tar -xzf /tmp/${stem}.tar.gz -C ${root}`},null,2)+'\n');
fs.rmSync(scratch,{recursive:true});console.log({files:entries.length,bytes:position,archive_sha256:archiveSha});
