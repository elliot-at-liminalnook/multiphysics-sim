// Apply the existing shared Rust capability analysis to each exact new cycle.
import fs from 'node:fs';import crypto from 'node:crypto';import {spawnSync} from 'node:child_process';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
for(const name of process.argv.slice(2)){
 const def=read(`${d}/clearance-trials.json`).rows.find(r=>r.name===name);if(!def||def.planning_exit!==0)throw Error('compiled trial required');
 const scene=read(`${def.prefix}.scene.json`),recipe=read(`${d}/capability-recipe.json`),p=scene.controller.parameters;
 recipe.inspection.samples=[{id:name,coordinates:p.samples[0]}];
 recipe.reference_cycle={period_s:p.period_s,stride_m:p.period_s*p.nominal_speed_m_s,samples:p.samples};
 recipe.provenance={scene_sha256:sha(`${def.prefix}.scene.json`),plan_sha256:sha(`${def.prefix}.plan.json`),scope:'Exact planned cycle, conditional no-load shaft-rate screen. Static point support allocation is a separate approximation.'};
 const output=`${def.prefix}.capability.json`,input=`${def.prefix}.capability-recipe.json`;
 if(fs.existsSync(output))throw Error('refusing overwrite capability output');
 fs.writeFileSync(input,JSON.stringify(recipe,null,2)+'\n');
 const r=spawnSync(`${bin}/analyze_motion_capability`,[`${def.prefix}.scene.json`,'examples/full-robot/gait-exploration/workspace-markers.json',input],{encoding:'utf8',maxBuffer:32*1024*1024});
 if(r.status!==0)throw Error(r.stderr);fs.writeFileSync(output,r.stdout);console.log(name);
}
