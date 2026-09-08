// Reconstruct inputs from versioned artifacts, without an existing runs/ tree.
import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';import assert from 'node:assert/strict';
const [output]=process.argv.slice(2);assert(output&&!existsSync(output),'supply a fresh output directory');
const read=p=>JSON.parse(readFileSync(p));
const archive=read('examples/full-robot/whole-swing/feedback-ablation-recipes.json');
for(const s of [archive.scene,archive.config,archive.task])assert.equal(createHash('sha256').update(readFileSync(s.path)).digest('hex'),s.sha256,s.path);
mkdirSync(output,{recursive:true});const cases=[];
for(const c of archive.cases){
  const scene=read(archive.scene.path),config=read(archive.config.path),task=read(archive.task.path);
  for(const change of c.input_schema_changes){
    const input=scene.controller.inputs.find(i=>i.name===change.name);assert(input&&change.field==='lower');
    assert.equal(input.lower,change.before);input.lower=change.after;
  }
  const stride=Math.round(task.period_s/config.step_s);let held=scene.controller.inputs.map(i=>i.initial),next=0;
  const actions=Array.from({length:config.steps/stride},(_,i)=>{
    if(c.input_events[next]?.at_step===i*stride)held=c.input_events[next++].values;
    return [...held];
  });
  assert.equal(next,c.input_events.length);
  assert.equal(createHash('sha256').update(JSON.stringify(actions)).digest('hex'),c.actions_json_sha256);
  const paths={scene:`${output}/${c.name}.scene.json`,config:`${output}/${c.name}.config.json`,
    task:`${output}/${c.name}.task.json`,actions:`${output}/${c.name}.actions.json`};
  for(const [key,value] of Object.entries({scene,config,task,actions}))writeFileSync(paths[key],JSON.stringify(value)+'\n');
  cases.push({name:c.name,seed:c.seed,...paths});
}
writeFileSync(`${output}/recipes.json`,JSON.stringify({version:1,cases,
  scope:'Reconstructed controller-input experiments. Run through the shared native or browser environment; reconstruction alone does not establish task or contact acceptance.'},null,2)+'\n');
console.log(`Reconstructed ${cases.length} complete recipes without prior run captures.`);
