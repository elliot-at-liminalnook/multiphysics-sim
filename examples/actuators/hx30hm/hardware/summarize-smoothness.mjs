// Descriptive analysis of measured encoder trajectories, not an acoustic test
// or a motor simulation. node summarize-smoothness.mjs RUN NEW_OUTPUT.json
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
const [root, output] = process.argv.slice(2);
if (!root || !output || fs.existsSync(output)) throw Error('Require run and new output');
const inputs = {};
function read(name) {
  const b = fs.readFileSync(path.join(root, name));
  inputs[name] = crypto.createHash('sha256').update(b).digest('hex');
  return b.toString();
}
const run = JSON.parse(read('run.json'));
if (!run.completed) throw Error('Require completed run');
const cp = JSON.parse(read('checkpoint.json'));
const [header, ...lines] = read('sweep.csv').trim().split('\n');
const h = header.split(',');
const rows = lines.map(l => {
  const v = l.split(',');
  if (v.length !== h.length) throw Error('Partial row');
  return Object.fromEntries(h.map((k, i) => [k, ['phase', 'torque_enabled'].includes(k) ? v[i] : Number(v[i])]));
});
const mean = a => a.reduce((s, v) => s + v, 0) / a.length;
const sd = a => Math.sqrt(mean(a.map(v => (v - mean(a)) ** 2)));
const quantile = (a, q) => [...a].sort((a, b) => a - b)[Math.floor((a.length - 1) * q)];
const trials = [];
for (const o of cp.outcomes_this_run) {
  for (const id of o.trial.ids) {
    const a = rows.filter(r => r.trial === o.trial.index && r.id === id && r.phase === 'excitation' && Math.abs(r.drive_counts) === 1000)
      .map(r => ({...r, t: (r.request_s + r.completion_s) / 2, angle: r.position_unwrapped_counts * 360 / 4096}));
    if (a.length < 20) continue;
    const samples = [];
    for (const r of a) {
      if (r.t < a[0].t + 0.5 || r.t > a.at(-1).t - 0.045) continue;
      const w = a.filter(s => Math.abs(s.t - r.t) <= 0.045);
      if (w.length < 5 || w.at(-1).t - w[0].t < 0.06) continue;
      const mt = mean(w.map(s => s.t)), my = mean(w.map(s => s.angle));
      const speed = Math.abs(w.reduce((s, p) => s + (p.t - mt) * (p.angle - my), 0) / w.reduce((s, p) => s + (p.t - mt) ** 2, 0));
      samples.push({time_s: r.t, angle_deg: r.angle, speed_deg_s: speed, voltage_v: r.voltage_v});
    }
    if (!samples.length) continue;
    const speeds = samples.map(s => s.speed_deg_s);
    const bins = Array.from({length: 12}, (_, i) => {
      const b = samples.filter(s => Math.floor(((s.angle_deg % 360 + 360) % 360) / 30) === i);
      const cycles = [...new Set(b.map(s => Math.floor(s.angle_deg / 360)))].map(c => {
        const v = b.filter(s => Math.floor(s.angle_deg / 360) === c);
        return {cycle: c, samples: v.length, mean_speed_deg_s: mean(v.map(s => s.speed_deg_s))};
      });
      return {start_angle_deg: i * 30, samples: b.length, cycles};
    });
    trials.push({id, drive_counts: o.trial.drive, mean_speed_deg_s: mean(speeds),
      smoothed_speed_sd_deg_s: sd(speeds), smoothed_speed_cv_percent: 100 * sd(speeds) / mean(speeds),
      p05_speed_deg_s: quantile(speeds, 0.05), p95_speed_deg_s: quantile(speeds, 0.95),
      bins, samples});
  }
}
const report = {schema_version: 1, run: path.basename(root), user_quiet_reference_id: 12, inputs,
  method: 'Absolute encoder slope from centered 90 ms windows with at least five samples spanning 60 ms. Exclude the first 0.5 seconds. SD/CV and percentiles are descriptive; overlapping windows are not independent observations. Angle bins are 30 degrees in the output encoder frame.',
  limitations: ['No microphone, vibration sensor, calibrated external encoder, or mechanical-load measurement.',
    'Only about two output revolutions per direction; insufficient evidence to identify a repeatable defect or separate motor, gear, supply, and measurement effects.',
    'Fast gear-mesh/PWM noise can be audible without a large change in output speed. Do not interpret this as an acoustic ranking.'], trials};
fs.writeFileSync(output, JSON.stringify(report, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify(trials.map(({id, drive_counts, mean_speed_deg_s, smoothed_speed_cv_percent}) => ({id, drive_counts, mean_speed_deg_s, smoothed_speed_cv_percent})), null, 2));
