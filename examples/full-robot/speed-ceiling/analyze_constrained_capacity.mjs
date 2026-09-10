// Re-evaluate preserved load recipes to expose the shared servo torque envelope.
import fs from 'node:fs';import assert from 'node:assert/strict';import crypto from 'node:crypto';import {spawnSync} from 'node:child_process';
const d='examples/full-robot/speed-ceiling',root='runs/speed-ceiling/constrained-capacity';fs.mkdirSync(root,{recursive:true});
const read=p=>JSON.parse(fs.readFileSync(p)),hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples',rows=[];
for(const r of read(`${d}/constrained-load-trials.json`).rows.filter(r=>r.scale===1))for(const sign of ['forward','reverse']) {
  const t=r.tables[sign],outPath=`${root}/${r.name}-${sign}.json`,out=fs.openSync(outPath,'wx'),err=fs.openSync(`${outPath}.error.log`,'wx');
  const result=spawnSync(`${bin}/reference_load_feedforward`,[`${r.source}.scene.json`,'examples/full-robot/gait-exploration/workspace-markers.json',t.recipe],{stdio:['ignore',out,err],timeout:120000});fs.closeSync(out);fs.closeSync(err);assert.equal(result.status,0);
  const next=read(outPath),old=read(t.table);assert.deepEqual(next.target_offsets_rad,old.target_offsets_rad);assert.deepEqual(next.dynamic_increment_offsets_rad,old.dynamic_increment_offsets_rad);
  for(let i=0;i<next.frames.length;i++)for(const key of Object.keys(old.frames[i]))assert.deepEqual(next.frames[i][key],old.frames[i][key]);
  const violations=[];let minimum=Infinity;
  for(let i=0;i<next.frames.length;i++){const f=next.frames[i];for(let j=0;j<f.torque_capacity_margin_nm.length;j++) {const margin=f.torque_capacity_margin_nm[j];minimum=Math.min(minimum,margin);if(margin< -1e-9)violations.push({sample:i,coordinate:next.coordinate_names[j],required_torque_nm:f.motor_torques_nm[j],reference_speed_rad_s:f.reduced_velocity[j+6],capacity_nm:f.signed_torque_capacity_nm[j],margin_nm:margin});}}
  rows.push({name:r.name,sign,source_recipe_sha256:hash(t.recipe),source_table_sha256:hash(t.table),capacity_report_sha256:hash(outPath),frames:next.frames.length,
    minimum_margin_nm:minimum,violating_samples:violations.length,violating_frames:new Set(violations.map(v=>v.sample)).size,worst:violations.toSorted((a,b)=>a.margin_nm-b.margin_nm).slice(0,12),
    counts_by_coordinate:violations.reduce((a,v)=>(a[v.coordinate]=(a[v.coordinate]??0)+1,a),{})});
}
fs.writeFileSync(`${d}/constrained-capacity-summary.json`,JSON.stringify({rows,scope:'Required torques under the prescribed constrained point-force allocation, compared with the exact shared effective-servo torque_capacity at signed reference speeds. Negative margin rules out that specific torque/velocity/force allocation at that sample; it does not prove no alternative support allocation, body motion or trajectory works. The allocator does not yet constrain motor torques, and nonzero wrench residuals remain. Not actual measured motor saturation or a global speed ceiling. All prior load outputs reproduce exactly.'},null,2)+'\n',{flag:'wx'});
console.log(rows.map(({name,sign,minimum_margin_nm,violating_samples,violating_frames,counts_by_coordinate})=>({name,sign,minimum_margin_nm,violating_samples,violating_frames,counts_by_coordinate})));
