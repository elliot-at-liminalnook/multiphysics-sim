import fs from 'node:fs';import {spawnSync} from 'node:child_process';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p));
const status=fs.existsSync(`${d}/validation-status.json`)?read(`${d}/validation-status.json`):[];
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
for(const {name,prefix,duration_s} of read(`${d}/validation-cases.json`).rows){
 if(status.some(r=>r.name===name))continue;
 const o=fs.openSync(`${prefix}.native.json`,'wx'),e=fs.openSync(`${prefix}.error.txt`,'wx');
 const r=spawnSync(`${bin}/run_environment`,[`${prefix}.scene.json`,`${prefix}.config.json`,'examples/full-robot/fast-wasd/task.json',`${prefix}.actions.json`],{stdio:['ignore',o,e],timeout:Math.max(240000,(duration_s??20)*10000)});fs.closeSync(o);fs.closeSync(e);
 status.push({name,prefix,exit:r.status,error:r.error?.message??null});console.log(status.at(-1));
 fs.writeFileSync(`${d}/validation-status.json`,JSON.stringify(status,null,2)+'\n');if(r.error)break;
}
