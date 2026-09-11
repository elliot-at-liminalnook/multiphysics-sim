// Offline measurement analysis; never opens serial or simulates a motor.
// node summarize-rotation.mjs RUN_DIRECTORY NEW_REPORT.json
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
const [root, output] = process.argv.slice(2);
if (!root || !output || fs.existsSync(output)) throw Error('Require run and new report path');
const inputs = {};
function read(name) {
  const b = fs.readFileSync(path.join(root, name));
  inputs[name] = crypto.createHash('sha256').update(b).digest('hex');
  return b.toString();
}
const run = JSON.parse(read('run.json'));
const checkpoint = fs.existsSync(path.join(root, 'checkpoint.json')) ? JSON.parse(read('checkpoint.json')) : {outcomes_this_run: []};
const [header, ...lines] = read('sweep.csv').trim().split('\n');
const keys = header.split(',');
const rows = lines.filter(Boolean).map(l => {
  const v = l.split(',');
  if (v.length !== keys.length) throw Error('Incomplete CSV');
  const row = Object.fromEntries(keys.map((k, i) => [k, ['phase', 'torque_enabled'].includes(k) ? v[i] : Number(v[i])]));
  if (Object.values(row).some(v => typeof v === 'number' && !Number.isFinite(v))) throw Error('Non-finite data');
  return row;
});
const range = a => a.length ? [Math.min(...a), Math.max(...a)] : null;
const mean = a => a.length ? a.reduce((s, v) => s + v, 0) / a.length : null;
function slope(a) {
  if (a.length < 5 || a.at(-1).request_s - a[0].completion_s < 0.15) return null;
  const x = a.map(r => (r.request_s + r.completion_s) / 2);
  const y = a.map(r => r.position_unwrapped_counts * 360 / 4096);
  const mx = mean(x), my = mean(y);
  return y.reduce((s, v, i) => s + (v - my) * (x[i] - mx), 0) / x.reduce((s, v) => s + (v - mx) ** 2, 0);
}
const trials = [];
for (const o of checkpoint.outcomes_this_run) {
  for (const id of o.trial.ids) {
    const a = rows.filter(r => r.trial === o.trial.index && r.id === id);
    const excitation = a.filter(r => r.phase === 'pulse' || r.phase === 'excitation');
    const finalDrive = excitation.at(-1)?.drive_counts ?? o.trial.drive;
    // Summarize the final uninterrupted constant-command portion of a ramp.
    let beginning = excitation.length;
    while (beginning > 0 && excitation[beginning - 1].drive_counts === finalDrive) beginning--;
    const p = excitation.slice(beginning);
    const end = p.at(-1)?.completion_s;
    const late = p.filter(r => r.completion_s >= end - 0.5);
    const previous = p.filter(r => r.completion_s >= end - 1 && r.completion_s < end - 0.5);
    const lateSlope = slope(late), previousSlope = slope(previous);
    const change = lateSlope !== null && previousSlope !== null && Math.abs(lateSlope) > 5 ? Math.abs(lateSlope - previousSlope) / Math.abs(lateSlope) : null;
    trials.push({trial: o.trial.index, id, drive_counts: finalDrive, label: o.trial.label ?? null,
      concurrent: o.trial.ids.length > 1, displacement_counts: o.displacement_counts[id],
      turns: o.displacement_counts[id] / 4096, pulse_samples: p.length,
      observed_pulse_span_s: p.length ? p.at(-1).request_s - p[0].completion_s : null,
      constant_command_encoder_speed_deg_s: slope(p),
      peak_reported_speed_deg_s: Math.max(...a.map(r => Math.abs(r.speed_rad_s) * 180 / Math.PI)),
      late_encoder_speed_deg_s: lateSlope, previous_encoder_speed_deg_s: previousSlope,
      late_reported_speed_deg_s: mean(late.map(r => r.speed_rad_s * 180 / Math.PI)),
      fractional_speed_change: change, plateau_within_5_percent: change === null ? null : change <= 0.05,
      sampled_voltage_v: range(a.map(r => r.voltage_v)), sampled_temperature_c: range(a.map(r => r.temperature_c)),
      stationary_verified: o.stationary_verified});
  }
}
const report = {schema_version: 1, run: path.basename(root), completed: run.completed,
  failure: run.result?.failure ?? run.error ?? null, completed_trials: checkpoint.outcomes_this_run.length,
  plan: run.plan, inputs, trials,
  limitations: ['Plateau compares encoder slopes in the last two half-second windows; it is a finite-duration repeatability criterion, not proof of a physical maximum.',
    'Sensor timestamps are host request/reply windows. Current and connector voltage are not independently calibrated.',
    'Displacement ends at the last stationary rest sample. Only fully completed trials are summarized.']};
fs.writeFileSync(output, JSON.stringify(report, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify({run: report.run, completed: report.completed, failure: report.failure, trials: trials.length,
  maximum_observed_deg_s: Math.max(0, ...trials.map(t => t.peak_reported_speed_deg_s)),
  full_drive: trials.filter(t => Math.abs(t.drive_counts) === 1000)}, null, 2));
