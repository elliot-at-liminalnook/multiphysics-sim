import fs from 'node:fs';import {spawnSync} from 'node:child_process';
const d='examples/full-robot/speed-ceiling',bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
for(const {name,prefix,source} of JSON.parse(fs.readFileSync(`${d}/solver-trials.json`)).rows){
 const o=fs.openSync(`${prefix}.native.json`,'wx'),e=fs.openSync(`${prefix}.error.txt`,'wx');
 const r=spawnSync(`${bin}/run_environment`,[`${source}.scene.json`,`${prefix}.config.json`,'examples/full-robot/fast-wasd/task.json',`${source}.actions.json`,'--profile',`${prefix}.profile.json`],{stdio:['ignore',o,e],timeout:240000});fs.closeSync(o);fs.closeSync(e);
 console.log({name,exit:r.status,error:r.error?.message??null});if(r.status!==0)throw Error('solver experiment failed; retain incomplete capture');
}
