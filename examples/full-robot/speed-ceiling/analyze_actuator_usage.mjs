// Measurements from shared-runtime motor readouts, not a second actuator model.
import fs from 'node:fs';import crypto from 'node:crypto';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p));const rows=[];
for(const name of process.argv.slice(2)) {
 const def=read(`${d}/validation-cases.json`).rows.find(r=>r.name===name);if(!def)throw Error(`unknown ${name}`);
 const file=`${def.prefix}.native.json`,c=read(file);if(!c.completed||c.error)throw Error('complete capture required');
 const motors=c.recording.config.motors.effective.components;
 const results=motors.map(m=>({dof:m.dof,no_load_speed_rad_s:m.parameters.no_load_speed,stall_torque_nm:m.parameters.stall_torque,
  positive_power_bound_w:m.parameters.stall_torque*m.parameters.no_load_speed/4,
  maximum_speed_rad_s:0,maximum_torque_nm:0,maximum_positive_power_w:0,maximum_absorbed_power_w:0,above_no_load_samples:0}));
 let peakPower=0;
 for(const f of c.frames){let power=0;f.motor_readings.forEach((m,i)=>{
  const r=results[i],w=m.gear_speed_rad_s,t=m.shaft_torque_nm,p=t*w;
  if(!Number.isFinite(p))throw Error('finite effective motor readings required');
  r.maximum_speed_rad_s=Math.max(r.maximum_speed_rad_s,Math.abs(w));r.maximum_torque_nm=Math.max(r.maximum_torque_nm,Math.abs(t));
  r.maximum_positive_power_w=Math.max(r.maximum_positive_power_w,p);r.maximum_absorbed_power_w=Math.max(r.maximum_absorbed_power_w,-p);
  if(Math.abs(w)>r.no_load_speed_rad_s)r.above_no_load_samples++;power+=Math.max(0,p);
 });peakPower=Math.max(peakPower,power);}
 rows.push({name,recording_samples:c.frames.length,motors:results,maximum_aggregate_positive_power_w:peakPower,
  aggregate_positive_power_bound_w:results.reduce((s,r)=>s+r.positive_power_bound_w,0),
  sampled_power_envelope_passed:results.every(r=>r.maximum_positive_power_w<=r.positive_power_bound_w+1e-9),
  capture_sha256:crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')});
}
const report={rows,scope:'20 ms reporting snapshots of runtime shaft torque and speed. Peaks are lower bounds on continuous peaks; no-load speed is not a hard braking/backdrive limit. Effective CAD-derived model is uncalibrated and omits thermal/electrical limits. No claim of continuous energy balance.'};
fs.writeFileSync(`${d}/actuator-usage.json`,JSON.stringify(report,null,2)+'\n');
console.log(rows.map(r=>({name:r.name,peak_power_w:r.maximum_aggregate_positive_power_w,power_envelope:r.sampled_power_envelope_passed,maximum_shaft_rate_rad_s:Math.max(...r.motors.map(m=>m.maximum_speed_rad_s))})));
