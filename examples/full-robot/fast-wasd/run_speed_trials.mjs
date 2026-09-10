import fs from 'node:fs';import {spawnSync} from 'node:child_process';
const d='examples/full-robot/fast-wasd',status=fs.existsSync(`${d}/speed-status.json`)?JSON.parse(fs.readFileSync(`${d}/speed-status.json`)):[];
for(const {name} of JSON.parse(fs.readFileSync(`${d}/speed-trials.json`)).definitions){
 if(status.some(s=>s.name===name))continue;
 const o=fs.openSync(`${d}/${name}.native.json`,'wx'),e=fs.openSync(`${d}/${name}.error.txt`,'wx');
 const r=spawnSync('/Users/elliot/physics-simulator/target/gait-exploration/release/examples/run_environment',[`${d}/${name}.scene.json`,`${d}/${name}.config.json`,`${d}/task.json`,`${d}/${name}.actions.json`],{stdio:['ignore',o,e],timeout:90000});fs.closeSync(o);fs.closeSync(e);
 status.push({name,exit:r.status,error:r.error?.message});fs.writeFileSync(`${d}/speed-status.json`,JSON.stringify(status,null,2)+'\n');console.log(status.at(-1));if(r.error)break;
}
