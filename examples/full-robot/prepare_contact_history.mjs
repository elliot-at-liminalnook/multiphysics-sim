import {readFile,writeFile,mkdir,cp} from 'node:fs/promises';
import {createHash} from 'node:crypto';
const root='runs/full-robot/learning/contact-history';
const source='runs/full-robot/learning/endpoint-correction';
const inputs={};
const hash=b=>createHash('sha256').update(b).digest('hex');
await mkdir(root,{recursive:true});
const manifest=JSON.parse(await readFile(source+'/manifest.json'));
for(const name of ['base','refined']){
 const p=`${source}/${name}.config.json`;inputs[p]=hash(await readFile(p));
 await cp(p,`${root}/${name}.config.json`);
}
inputs[manifest.scene]=hash(await readFile(manifest.scene));
inputs[import.meta.filename]=hash(await readFile(import.meta.filename));
await writeFile(root+'/manifest.json',JSON.stringify({
 scene:manifest.scene,inputs,
 comparisons:{base:source+'/base.execution.json',refined:source+'/refined.execution.json'},
 previous_runner:'runs/interactive/endpoint-correction/native-runner',
 scope:'Same configurations and equations. Contact-history queries omit inverse dynamics; memoryless contact uses the same independent decay law without geometry. Require exact sampled trajectories, accepted contact traces, events and subdivisions before promotion. No new walking/controller claim.'
},null,2));
console.log('Prepared exact contact-history comparisons');
