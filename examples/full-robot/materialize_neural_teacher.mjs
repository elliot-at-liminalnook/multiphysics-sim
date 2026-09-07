// Install a completed search artifact into explicit evaluation/browser recipes.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import assert from 'node:assert/strict';
const search=process.argv[2]||'runs/full-robot/learning/neural-teacher/search';
const root='examples/full-robot/neural-teacher';
const read=p=>JSON.parse(readFileSync(p));
assert.deepEqual(read(`${search}/experiment.json`),read(`${root}/experiment.json`),'search must match the versioned training recipe');
const result=read(`${search}/search.json`),config=read(`${search}/config.json`);
assert.deepEqual(config.policy.neural_residual,result.policy);
assert(result.best_score>=result.initial_score);
mkdirSync(`${root}/trials`,{recursive:true});
for(let i=0;i<=result.trials.length;i++){
 const name=`evaluation-${String(i).padStart(4,'0')}.json`;
 const trial=read(`${search}/${name}`);
 assert.equal(trial.score,i===0?result.initial_score:result.trials[i-1].score);
 writeFileSync(`${root}/trials/${name}`,JSON.stringify(trial)+'\n');
}
const write=(name,v)=>writeFileSync(`${root}/${name}.json`,JSON.stringify(v)+'\n');
write('policy',result.policy);write('search',result);write('short.config',config);
const long=structuredClone(config);long.steps=Math.round(60/long.step_s);
// Explicit browser adaptation: allow convergence work beyond the 40-iteration
// budget used in short training. Equations and acceptance tolerances unchanged.
long.implicit.newton.max_iterations=80;write('config',long);
for(const [name,c] of [['refined.config',structuredClone(config)],['initial.refined.config',read(`${root}/initial.config.json`)]]){
 c.step_s/=2;c.steps*=2;c.report_every*=2;write(name,c);
}
console.log(root);
