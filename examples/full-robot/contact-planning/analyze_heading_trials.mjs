import fs from 'node:fs';
import crypto from 'node:crypto';
const d='examples/full-robot/contact-planning',read=p=>JSON.parse(fs.readFileSync(p));
const sources=[],rows=[];
for(const trial of read(d+'/heading-trials.json').rows) {
 const result=d+'/'+trial.name+'-audit.result.json',p=read(trial.recipe),r=read(result).report;
 sources.push(trial.recipe,result);
 const frames=r.frames.filter(f=>f.clock.phase_rate===1&&f.clock.phase_acceleration_per_s===0);
 const hips=p.robot.independent_coordinates.flatMap((name,i)=>name.endsWith('Hip servo output')?[i]:[]).map(i=>{
  const q=frames.map(f=>f.coordinates[i]);
  return {joint:p.robot.independent_coordinates[i],range_degrees:(Math.max(...q)-Math.min(...q))*180/Math.PI,
   peak_speed_rad_s:Math.max(...frames.map(f=>Math.abs(f.reduced_velocity[6+i]))),
   minimum_torque_margin_nm:Math.min(...r.frames.map(f=>f.torque_capacity_margin_nm[i]))};
 });
 rows.push({name:trial.name,speed_m_s:r.speed_m_s,sampled_feasible:r.sampled_feasible,
  maximum_force_error_n:r.maximum_force_error_n,maximum_moment_error_nm:r.maximum_moment_error_nm,
  minimum_torque_margin_nm:r.minimum_torque_margin_nm,maximum_penetration_m:r.maximum_penetration_m,hips});
}
sources.push(import.meta.filename,d+'/heading-trials.json');
fs.writeFileSync(d+'/heading-audit.summary.json',JSON.stringify({rows,
 sources:sources.map(path=>({path,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')})),
 scope:'Same unretuned warm-start body and foot schedule at three travel headings relative to CAD chassis. Forward and reverse torque gates both active. Hip range is commanded kinematic range; no live gait or heading-specific maximum speed claim.'},null,2)+'\n');
