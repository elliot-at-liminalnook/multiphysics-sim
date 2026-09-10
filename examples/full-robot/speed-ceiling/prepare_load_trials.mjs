// Static support policy and tau/K feedforward; all inverse mechanics run in Rust.
import fs from 'node:fs';import crypto from 'node:crypto';import {spawnSync} from 'node:child_process';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p)),catalog=read(`${d}/clearance-trials.json`);
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const smooth=u=>{u=Math.max(0,Math.min(1,u));return u*u*u*(10+u*(-15+6*u));};
for(const source of process.argv.slice(2))for(const scale of [.5,1]){
 const def=catalog.rows.find(r=>r.name===source);if(!def||def.planning_exit!==0)throw Error(`missing compiled source ${source}`);
 const name=`${source}-load${scale}`,prefix=`runs/speed-ceiling/clearance/${name}`;
 if(catalog.rows.some(r=>r.name===name)||fs.existsSync(`${prefix}.scene.json`))throw Error(`refusing overwrite ${name}`);
 const scene=read(`${def.prefix}.scene.json`),c=read(`${def.prefix}.config.json`),samples=scene.controller.parameters.samples;
 const support_weights=samples.map((_,i)=>{
  const phase=i*.02,u=(phase%.4-.04)/.32,active=(phase%0.8)<.4?[0,2]:[1,3];
  const liftFraction=u>0&&u<1?(u<.25?smooth(u/.25):u>.75?smooth((1-u)/.25):1):0;
  return [0,1,2,3].map(leg=>active.includes(leg)?1-liftFraction:1);
 });
 const recipe={independent_coordinates:c.motors.effective.components.map(m=>m.dof),embedding:c.embedding,
  initial_base_translation_m:c.initial_base_translation_m,samples,support_weights,
  actuators:Object.fromEntries(c.motors.effective.components.map(m=>[m.dof,m.parameters]))};
 fs.writeFileSync(`${prefix}.load-recipe.json`,JSON.stringify(recipe)+'\n');
 const o=fs.openSync(`${prefix}.load-table.json`,'wx'),e=fs.openSync(`${prefix}.load-error.txt`,'wx');
 const r=spawnSync(`${bin}/reference_load_feedforward`,[`${def.prefix}.scene.json`,'examples/full-robot/gait-exploration/workspace-markers.json',`${prefix}.load-recipe.json`],{stdio:['ignore',o,e],timeout:60000});fs.closeSync(o);fs.closeSync(e);
 if(r.status!==0)throw Error(`load compilation failed ${name}`);
 const offsets=read(`${prefix}.load-table.json`).target_offsets_rad;
 if(offsets[0].some((v,i)=>Math.abs(v-offsets.at(-1)[i])>1e-7))throw Error('nonperiodic feedforward');
 Object.assign(scene.controller.parameters,{static_load_offsets:offsets,static_load_scale:scale});
 const files=scene.controller.sources.files,key=scene.controller.sources.entry,needle='+p.velocity_lead_s[j]*velocity;';
 if(!files[key].includes(needle))throw Error('unknown command law');
 files[key]=files[key].replace(needle,'+p.velocity_lead_s[j]*velocity+p.static_load_scale*(p.static_load_offsets[i][j]+blend*(p.static_load_offsets[i+1][j]-p.static_load_offsets[i][j]));');
 const row={...def,name,prefix,source,load_scale:scale,method:'Shared Rust static gravity/contact-load feedforward as tau/K servo target offsets',source_plan:`${def.prefix}.plan.json`,load_recipe_sha256:sha(`${prefix}.load-recipe.json`),load_table_sha256:sha(`${prefix}.load-table.json`)};
 for(const [suffix,value] of [['scene',scene],['config',c],['actions',read(`${def.prefix}.actions.json`)]]){
  fs.writeFileSync(`${prefix}.${suffix}.json`,JSON.stringify(value)+'\n');row[`${suffix}_sha256`]=sha(`${prefix}.${suffix}.json`);
 }
 catalog.rows.push(row);fs.writeFileSync(`${d}/clearance-trials.json`,JSON.stringify(catalog,null,2)+'\n');console.log(name);
}
