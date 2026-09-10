// Model-based steady-cadence increments, evaluated only by shared Rust mechanics.
import fs from 'node:fs';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
const d='examples/full-robot/speed-ceiling', read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
const catalog=read(`${d}/validation-cases.json`), rows=[];
const batch=process.argv[2]?read(process.argv[2]):{id:'dynamic-load',cases:[.125,.15].map(speed=>({speed,source:`runs/speed-ceiling/validation/smooth-hip0-v${speed*1000}-human-fine`,table_name:`dynamic-v${speed*1000}`,name_prefix:`smooth-dynamic-v${speed*1000}`,scales:[.5,1]}))};
if(!/^[a-z0-9-]+$/.test(batch.id)||!Array.isArray(batch.cases)||!batch.cases.length)throw Error('named nonempty batch required');
const reportPath=`${d}/${batch.id}-trials.json`;
if(fs.existsSync(reportPath))throw Error('refusing overwrite batch report');
const smooth=x=>{x=Math.max(0,Math.min(1,x));return x*x*x*(10+x*(-15+6*x));};
for(const def of batch.cases) {
  const {source,speed}=def;
  if(!Number.isFinite(speed)||speed<=0||![def.table_name,def.name_prefix].every(n=>/^[a-z0-9.-]+$/.test(n))||!Array.isArray(def.scales)||!def.scales.length||def.scales.some(s=>!Number.isFinite(s)||s<0))throw Error('valid named source/speed/scales required');
  const scene=read(`${source}.scene.json`), config=read(`${source}.config.json`), p=scene.controller.parameters;
  const speedChannel=scene.controller.inputs.find(ch=>ch.name==='command.forward_speed');
  if(speedChannel?.upper!==speed||speedChannel?.lower!==-speed)throw Error('load cadence must match source speed');
  const prefix=`runs/speed-ceiling/validation/${def.table_name}`;
  const times=Array.from({length:161},(_,i)=>i*p.period_s/160);
  const support_weights=times.map(time=>{
    const phase=time%p.period_s, u=(phase%(p.period_s/2)-p.swing_start_s)/(p.swing_end_s-p.swing_start_s);
    const share=u>0&&u<1?(u<.25?smooth(u/.25):u>.75?smooth((1-u)/.25):1):0;
    return [0,1,2,3].map(leg=>((phase<p.period_s/2)===(leg===0||leg===2))?1-share:1);
  });
  const tables={};
  for(const sign of [1,-1]) {
    const name=sign===1?'forward':'reverse';
    const recipe={independent_coordinates:config.motors.effective.components.map(m=>m.dof),embedding:config.embedding,
      initial_base_translation_m:config.initial_base_translation_m,support_weights,
      actuators:Object.fromEntries(config.motors.effective.components.map(m=>[m.dof,m.parameters])),
      reference:{trajectory:p.trajectory,sample_times_s:times,phase_rate:sign*speed/p.nominal_speed_m_s,
        phase_acceleration_per_s:0,base_velocity:[sign*speed,0,0,0,0,0],base_acceleration:[0,0,0,0,0,0]}};
    if(def.wrench_allocation)recipe.wrench_allocation=def.wrench_allocation;
    const recipePath=`${prefix}.${name}.load-recipe.json`, tablePath=`${prefix}.${name}.load-table.json`;
    fs.writeFileSync(recipePath,JSON.stringify(recipe)+'\n',{flag:'wx'});
    const out=fs.openSync(tablePath,'wx'),err=fs.openSync(`${prefix}.${name}.load-error.txt`,'wx');
    const run=spawnSync(`${bin}/reference_load_feedforward`,[`${source}.scene.json`,'examples/full-robot/gait-exploration/workspace-markers.json',recipePath],{stdio:['ignore',out,err],timeout:120000});fs.closeSync(out);fs.closeSync(err);
    if(run.status!==0)throw Error(`load analysis failed ${name}`);
    const table=read(tablePath);if(!table.dynamic_reference)throw Error('dynamic reference required');
    if(def.wrench_allocation?.constrained) {
      if(!table.frames.every(f=>f.wrench_allocation?.optimizer?.converged&&f.static_wrench_allocation?.optimizer?.converged&&f.wrench_allocation.unilateral_friction_satisfied&&f.static_wrench_allocation.unilateral_friction_satisfied))throw Error('constrained load table must converge and satisfy all sampled force cones');
    }
    tables[name]={recipe:recipePath,recipe_sha256:hash(recipePath),table:tablePath,table_sha256:hash(tablePath),
      maximum_offset_rad:Math.max(...table.dynamic_increment_offsets_rad.flat().map(Math.abs)),
      maximum_unbalanced_moment_nm:Math.max(...table.frames.map(f=>Math.hypot(...f.unbalanced_base_moment_nm)))};
    if(def.wrench_allocation)Object.assign(tables[name],{
      wrench_allocation:def.wrench_allocation,
      maximum_unbalanced_force_n:Math.max(...table.frames.map(f=>Math.hypot(...f.unbalanced_base_force_n))),
      friction_feasible_frames:table.frames.filter(f=>f.wrench_allocation?.unilateral_friction_satisfied).length,
      sampled_frames:table.frames.length,
      minimum_normal_force_n:Math.min(...table.frames.flatMap(f=>f.support_forces_world_n.map(force=>force[2])))});
    if(def.wrench_allocation?.constrained)Object.assign(tables[name],{
      maximum_optimizer_iterations:Math.max(...table.frames.flatMap(f=>[f.wrench_allocation.optimizer.iterations,f.static_wrench_allocation.optimizer.iterations])),
      maximum_gradient_mapping_norm_n:Math.max(...table.frames.flatMap(f=>[f.wrench_allocation.optimizer.gradient_mapping_norm_n,f.static_wrench_allocation.optimizer.gradient_mapping_norm_n]))});
    tables[name].offsets=table.dynamic_increment_offsets_rad;
    if(tables[name].offsets[0].some((v,j)=>Math.abs(v-tables[name].offsets.at(-1)[j])>1e-9))throw Error('nonperiodic load table');
  }
  for(const scale of def.scales) {
    const name=`${def.name_prefix}-scale${scale}-human-fine`, target=`runs/speed-ceiling/validation/${name}`;
    const next=structuredClone(scene), params=next.controller.parameters;
    params.dynamic_load={forward:tables.forward.offsets,reverse:tables.reverse.offsets,nominal_phase_rate:speed/p.nominal_speed_m_s,
      period_s:p.period_s,scale,maximum_offset_rad:.06};
    const entry=next.controller.sources.entry, old='+p.velocity_lead_s[j]*velocity;';
    if(next.controller.sources.files[entry].split(old).length!==2)throw Error('expected unique target law');
    next.controller.sources.files[entry]=next.controller.sources.files[entry]
      .replace('let reference=trajectory_sample(p.trajectory,state.phase);',`let reference=trajectory_sample(p.trajectory,state.phase);
 let load=0.0;
 let load_active=!state.braking && yaw.abs()<0.000000001 && (state.rate.abs()-p.dynamic_load.nominal_phase_rate).abs()<0.000000001;
 let table=if state.rate>=0.0 {p.dynamic_load.forward}else{p.dynamic_load.reverse};
 let load_cell=state.phase/p.dynamic_load.period_s*(table.len()-1).to_float();
 let load_i=load_cell.floor().to_int();let load_u=load_cell-load_i.to_float();`)
      .replace(old,`+p.velocity_lead_s[j]*velocity;
  if load_active {
   load=p.dynamic_load.scale*(table[load_i][j]*(1.0-load_u)+table[load_i+1][j]*load_u);
   commands[name]+=load.max(-p.dynamic_load.maximum_offset_rad).min(p.dynamic_load.maximum_offset_rad);
  }`);
    const row={name,prefix:target,source,source_scene_sha256:hash(`${source}.scene.json`),kind:'human',family:name,duration_s:20,step_s:config.step_s,command_speed_m_s:speed,
      scale,wrench_allocation:def.wrench_allocation??null,tables:Object.fromEntries(Object.entries(tables).map(([k,{offsets,...v}])=>[k,v])),
      scope:'Signed steady-cadence dynamic torque increment over static allocation at the same reference pose. Shared inverse dynamics, prescribed point-support shares, unbalanced body moment reported. Linear interpolation of 5 ms phase table; gain .5/1, offset capped at .06 rad. Enabled only at the exact nominal cadence with zero commanded yaw and no braking; does not model acceleration/reversal transients. Original physical model and phase/lease/braking policy retained.'};
    for(const [suffix,value] of [['scene',next],['config',config],['actions',read(`${source}.actions.json`)]]) {
      fs.writeFileSync(`${target}.${suffix}.json`,JSON.stringify(value)+'\n',{flag:'wx'});row[`${suffix}_sha256`]=hash(`${target}.${suffix}.json`);
    }
    rows.push(row);catalog.rows.push(row);
  }
}
fs.writeFileSync(reportPath,JSON.stringify({batch,rows},null,2)+'\n',{flag:'wx'});
fs.writeFileSync(`${d}/validation-cases.json`,JSON.stringify(catalog,null,2)+'\n');
