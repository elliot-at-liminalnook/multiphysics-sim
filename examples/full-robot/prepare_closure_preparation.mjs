import {readFile,writeFile,mkdir,cp} from 'node:fs/promises';
import {createHash} from 'node:crypto';
const directory='runs/full-robot/learning/closure-preparation';
const source='runs/full-robot/learning/contact-history';
const parent=JSON.parse(await readFile(source+'/manifest.json'));
const hash=b=>createHash('sha256').update(b).digest('hex');
const inputs={};await mkdir(directory,{recursive:true});
for(const name of ['base','refined']){
 const p=`${source}/${name}.config.json`;inputs[p]=hash(await readFile(p));await cp(p,`${directory}/${name}.config.json`);
}
inputs[parent.scene]=hash(await readFile(parent.scene));inputs[import.meta.filename]=hash(await readFile(import.meta.filename));
await writeFile(directory+'/manifest.json',JSON.stringify({
 scene:parent.scene,inputs,previous_runner:'runs/interactive/contact-history/native-runner',
 comparisons:{base:source+'/base.execution.json',refined:source+'/refined.execution.json'},
 scope:'Unchanged configurations/equations: numeric closure queries omit labels, fixed unit scales are prepared once, QR rank diagnostics request SVD values without unused vectors. Require exact full trajectories, events, solve diagnostics and contacts; all original closure and rank checks remain.'
},null,2));console.log('Prepared exact closure-preparation comparisons');
