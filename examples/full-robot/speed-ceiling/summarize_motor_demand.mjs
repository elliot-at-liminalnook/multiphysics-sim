// Statistics of recorded actuator measurements; no alternate dynamics model.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p));
const definitions=read(`${d}/validation-cases.json`).rows,rows=[];
for(const name of process.argv.slice(2)) {
  const definition=definitions.find(r=>r.name===name);assert(definition,'known capture required');
  const path=`${definition.prefix}.native.json`,capture=read(path);assert(capture.completed&&!capture.error);
  const motors=capture.recording.config.motors.effective.components;
  assert.deepEqual(capture.metadata.coordinate_names,motors.map(m=>m.dof),'recorded motor order must match parameters');
  const frames=capture.frames.filter(f=>f.time_s>=1.4&&f.time_s<=6);
  assert(frames.length>2);assert(frames.every(f=>f.motor_readings.length===motors.length));
  const maximumPositivePower=Math.max(...frames.map(f=>f.motor_readings.reduce((sum,r)=>sum+Math.max(0,r.shaft_torque_nm*r.gear_speed_rad_s),0)));
  const result=motors.map((motor,index)=>{
    const samples=frames.map(f=>({time:f.time_s,torque:f.motor_readings[index].shaft_torque_nm,speed:f.motor_readings[index].gear_speed_rad_s}));
    assert(samples.every(s=>[s.time,s.torque,s.speed].every(Number.isFinite)));
    const maximumSpeed=Math.max(...samples.map(s=>Math.abs(s.speed))),maximumTorque=Math.max(...samples.map(s=>Math.abs(s.torque)));
    let positiveWork=0;
    for(let i=1;i<samples.length;i++){const a=samples[i-1],b=samples[i];assert(b.time>a.time);positiveWork+=(b.time-a.time)*(Math.max(0,a.torque*a.speed)+Math.max(0,b.torque*b.speed))/2;}
    return {coordinate:motor.dof,maximum_absolute_speed_rad_s:maximumSpeed,maximum_absolute_torque_nm:maximumTorque,
      speed_fraction_of_declared_no_load:maximumSpeed/motor.parameters.no_load_speed,
      torque_fraction_of_declared_stall:maximumTorque/motor.parameters.stall_torque,
      approximate_positive_work_j:positiveWork,approximate_mean_positive_power_w:positiveWork/(samples.at(-1).time-samples[0].time)};
  });
  rows.push({name,source_capture_sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex'),
    window_s:[frames[0].time_s,frames.at(-1).time_s],samples:frames.length,maximum_sampled_total_positive_power_w:maximumPositivePower,motors:result});
}
assert(rows.length,'capture names required');
fs.writeFileSync(`${d}/front-motor-demand.json`,JSON.stringify({rows,scope:'Recorded effective-servo torque and speed at 20 ms reporting points, forward window only. Positive work uses trapezoidal integration of those samples, not solver-stage energy. Individual peaks need not coincide; unobserved peaks are not bounded. No-load speed is not a hard backdrive limit, and stall torque is not simultaneously available at no-load speed. Ratios do not establish torque-speed feasibility, thermal limits or a global physical speed ceiling.'},null,2)+'\n');
console.log(rows.map(r=>({name:r.name,peak_total_positive_power_w:r.maximum_sampled_total_positive_power_w,motors:r.motors.filter(m=>m.speed_fraction_of_declared_no_load>.8)})));
