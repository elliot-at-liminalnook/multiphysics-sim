// Prepare a boolean solver experiment from an exact leaderboard recipe.
// Physics/controller/world inputs stay pinned; Rust validates the solver option.
import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const descriptorPath=process.argv[2];assert(descriptorPath,'usage: prepare_solver_pair.mjs study.json');
const read=p=>JSON.parse(readFileSync(p)),write=(p,v)=>writeFileSync(p,JSON.stringify(v)+'\n');
const source=path=>({path,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')});
const study=read(descriptorPath);assert.equal(study.version,1);
assert(/^[a-z0-9-]+$/.test(study.id));assert(/^(implicit|embedding)(\.[a-z_]+)+$/.test(study.boolean_solver_option));
const catalogPath='web/leaderboard/evaluations.json',entry=read(catalogPath).entries.find(e=>e.id===study.recipe_id);assert(entry);
for(const key of ['scene','config','task'])assert.equal(source(entry.load[key].path).sha256,entry.load[key].sha256);
const scene=read(entry.load.scene.path),base=read(entry.load.config.path),task=read(entry.load.task.path);
const keys=study.boolean_solver_option.split('.'),field=keys.pop();
const parent=c=>keys.reduce((v,k)=>{assert(v&&typeof v[k]==='object'&&!Array.isArray(v[k]));return v[k];},c);
assert(parent(base)[field]===undefined||parent(base)[field]===false,'reference must have the solver option off');
mkdirSync(study.output,{recursive:true});
const actions=`${study.output}/tested.actions.json`;
execFileSync(process.execPath,['web/leaderboard/materialize_actions.mjs',study.recipe_id,actions]);
const cases=[];
for(const enabled of [false,true]){
  const name=`${study.id}-${enabled?'enabled':'reference'}`,config=structuredClone(base),paths={};
  assert(!existsSync(`${study.output}/${name}.native.json`),'refusing to alter an executed experiment');
  if(enabled)parent(config)[field]=true;
  for(const [kind,value] of Object.entries({scene,config,actions:read(actions)})){
    paths[kind]=`${study.output}/${name}.${kind}.json`;write(paths[kind],value);
  }
  cases.push({name,...paths,task:entry.load.task.path,seed:entry.load.seed,duration_s:config.steps*config.step_s,step_s:config.step_s});
}
assert.equal(task.period_s/base.step_s,Math.round(task.period_s/base.step_s));
const plan={version:1,cases,sources:[descriptorPath,study.plan_document,'examples/interactive/prepare_solver_pair.mjs',catalogPath,
  'web/leaderboard/materialize_actions.mjs',...['scene','config','task'].map(k=>entry.load[k].path),...study.sources].map(source),scope:study.scope};
write(`${study.output}/plan.json`,plan);write(study.plan_output,plan);console.log({id:study.id,cases:cases.length});
