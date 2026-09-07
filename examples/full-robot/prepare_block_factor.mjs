import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
const inputs={};
async function read(p){const b=await readFile(p);inputs[p]=createHash('sha256').update(b).digest('hex');return JSON.parse(b);}
const recipe=await read('examples/full-robot/block-factor-experiment.json');
await read(recipe.scene);const baseline=await read(recipe.config);baseline.profile_solver=true;
const candidate=structuredClone(baseline);candidate.embedding.block_dependent_factorization=true;
const viewer=structuredClone(candidate);viewer.profile_solver=false;
const refined=structuredClone(candidate);refined.step_s/=2;refined.steps*=2;refined.report_every*=2;
await mkdir(recipe.output_directory,{recursive:true});const outputs={};
for(const [name,value] of [['reference.config.json',baseline],['config.json',candidate],['viewer.config.json',viewer],['refined.config.json',refined]]){
 const b=JSON.stringify(value);outputs[name]=createHash('sha256').update(b).digest('hex');await writeFile(recipe.output_directory+'/'+name,b);
}
inputs['examples/full-robot/prepare_block_factor.mjs']=createHash('sha256').update(await readFile('examples/full-robot/prepare_block_factor.mjs')).digest('hex');
await writeFile(recipe.output_directory+'/manifest.json',JSON.stringify({recipe,inputs,outputs},null,2));
console.log('Prepared exact block-factor and whole-matrix comparison configs.');
