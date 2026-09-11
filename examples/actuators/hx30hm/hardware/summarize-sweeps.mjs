// Offline reporting only; no simulation, actuator commands or serial access.
// Usage: node summarize-sweeps.mjs SESSION_DIRECTORY NEW_REPORT.json
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
const [root, output] = process.argv.slice(2);
if (!root || !output || fs.existsSync(output)) throw Error('Require session directory and new report path');
const inputs = {}, trials = [], runs = [];
const read = p => {
  const b = fs.readFileSync(p);
  inputs[path.relative(root, p)] = crypto.createHash('sha256').update(b).digest('hex');
  return b.toString();
};
const range = a => a.length ? [Math.min(...a), Math.max(...a)] : null;
for (const entry of fs.readdirSync(root, {withFileTypes:true})) {
  if (!entry.isDirectory()) continue;
  const dir = path.join(root, entry.name), csv = path.join(dir, 'sweep.csv');
  if (!fs.existsSync(csv)) continue;
  const run = JSON.parse(read(path.join(dir, 'run.json')));
  const cp = path.join(dir, 'checkpoint.json');
  const outcomes = fs.existsSync(cp) ? JSON.parse(read(cp)).outcomes_this_run : [];
  const done = new Map(outcomes.map(o => [o.trial.index, o]));
  const [header, ...lines] = read(csv).trim().split('\n');
  const keys = header.split(',');
  const groups = new Map();
  for (const line of lines) {
    const v = line.split(',');
    if (v.length !== keys.length) throw Error(`Incomplete CSV row: ${csv}`);
    const r = Object.fromEntries(keys.map((k,i)=>[k,['phase','torque_enabled'].includes(k)?v[i]:Number(v[i])]));
    if (Object.values(r).some(v => typeof v === 'number' && !Number.isFinite(v))) throw Error('Invalid number');
    const key = `${r.trial}:${r.id}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(r);
  }
  for (const rows of groups.values()) {
    const first = rows[0], outcome = done.get(first.trial);
    const pulse = rows.filter(r=>r.phase==='pulse');
    const late = pulse.filter(r=>r.completion_s >= pulse.at(-1).completion_s - 0.08);
    let encoderLateDegS = null;
    if (late.length >= 3 && late.at(-1).request_s-late[0].completion_s >= 0.04) {
      const xy = late.map(r=>[(r.request_s+r.completion_s)/2,r.position_raw*360/4096]);
      const mx=xy.reduce((s,r)=>s+r[0],0)/xy.length, my=xy.reduce((s,r)=>s+r[1],0)/xy.length;
      encoderLateDegS=xy.reduce((s,r)=>s+(r[0]-mx)*(r[1]-my),0)/xy.reduce((s,r)=>s+(r[0]-mx)**2,0);
    }
    trials.push({run:entry.name,trial:first.trial,id:first.id,completed:!!outcome,
      concurrent:outcome ? outcome.trial.ids.length>1 : null,drive_counts:first.drive_counts,
      samples:rows.length,pulse_samples:pulse.length,
      peak_reported_speed_deg_s:Math.max(...rows.map(r=>Math.abs(r.speed_rad_s)*180/Math.PI)),
      late_pulse_encoder_slope_deg_s:encoderLateDegS,
      voltage_v:range(rows.map(r=>r.voltage_v)),temperature_c:range(rows.map(r=>r.temperature_c)),
      current_raw_uncalibrated:range(rows.map(r=>r.current_raw)),
      read_window_s:range(rows.map(r=>r.completion_s-r.request_s)),
      displacement_counts:outcome?.displacement_counts?.[first.id]??null});
  }
  runs.push({name:entry.name,completed:run.completed,failure:run.result?.failure??run.error??null,
    completed_trials:run.result?.completed_trials??done.size,total_trials:run.result?.total_trials??null});
}
const perServo = Object.fromEntries(Array.from({length:9},(_,i)=>i+4).map(id=>{
  const rows=trials.filter(t=>t.id===id&&t.completed), individual=rows.filter(t=>!t.concurrent), concurrent=rows.filter(t=>t.concurrent);
  const best=a=>a.length?a.reduce((p,t)=>p.peak_reported_speed_deg_s>t.peak_reported_speed_deg_s?p:t):null;
  return [id,{completed_trial_observations:rows.length,best_individual:best(individual),best_concurrent:best(concurrent)}];
}));
fs.writeFileSync(output,JSON.stringify({schema_version:1,inputs,runs,per_servo:perServo,trials,
  limitations:['Observed pulse peaks are not certified steady-state or mechanical maximum speed.',
    'Encoder slope uses late pulse samples and host-window midpoints; no device timestamps.',
    'Internal current raw counts are uncalibrated; no loaded torque or supply-current claim.',
    'Failed-trial observations remain in trials, but are excluded from best completed trials.']},null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({runs,per_servo:perServo},null,2));
