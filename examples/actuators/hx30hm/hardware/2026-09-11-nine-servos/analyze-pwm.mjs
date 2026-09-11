import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root = path.dirname(fileURLToPath(import.meta.url));
const read = p => JSON.parse(fs.readFileSync(p, 'utf8'));
const result = [];
for (const name of process.argv.slice(2)) {
  const dir = path.join(root, name), run = read(path.join(dir, 'run.json'));
  const preflight = read(path.join(dir, 'preflight.json'));
  const lines = fs.readFileSync(path.join(dir, 'pwm.csv'), 'utf8').trim().split('\n');
  const keys = lines.shift().split(',');
  const rows = lines.map(line => Object.fromEntries(line.split(',').map((v, i) => [keys[i], keys[i] === 'phase' ? v : +v])));
  const segments = [];
  for (const id of run.ids) {
    let start = preflight.servos[id].telemetry.position_raw;
    for (let stage = 0; stage < run.plan.drive_counts.length; stage++) {
      const all = rows.filter(r => r.id === id && r.stage === stage);
      if (!all.length) continue;
      const pulse = all.filter(r => r.phase === 'pulse');
      const end = all.at(-1);
      segments.push({ id, stage, drive_raw: run.plan.drive_counts[stage],
        displacement_including_coast_deg: (end.position_raw - start) * 360 / 4096,
        peak_reported_speed_during_pulse_deg_s: Math.max(...pulse.map(r => Math.abs(r.speed_rad_s) * 180 / Math.PI)),
        final_speed_raw: end.speed_raw, samples: all.length,
        voltage_min_v: Math.min(...all.map(r => r.voltage_v)),
        temperature_max_c: Math.max(...all.map(r => r.temperature_c)),
        fault_samples: all.filter(r => r.status !== 0).length });
      start = end.position_raw;
    }
  }
  const summary = { name, completed: run.completed, failure: run.result?.failure,
    samples: rows.length, segments, final_states: run.result?.final_states,
    method: 'Positive PWM pulses, 150 ms requested duration. Displacement includes subsequent coast during zero drive. Peak speed is the largest observed internal speed-register reading, not a calibrated peak or steady-state measurement. Host-timed UART commands; not an FPGA watchdog.' };
  fs.writeFileSync(path.join(dir, 'analysis.json'), JSON.stringify(summary, null, 2) + '\n');
  result.push(summary);
}
const lines = ['# HX-30HM PWM commissioning', '',
  'PWM control was explicitly authorized by the user after position-mode measurements. Each servo was tested separately at 25, 50, 100, 150, and 200 drive counts out of 1000, for 150 ms per pulse with 800 ms zero-drive intervals. Only positive-direction encoding was exercised.', '',
  '| ID | Run complete | Total displacement ° | Peak reported speed °/s | Minimum reported V | Maximum reported °C | Final mode / PWM / torque enable |',
  '|---|---|---:|---:|---:|---:|---|'];
for (const r of result) {
  for (const [id, final] of Object.entries(r.final_states ?? {})) {
    const s = r.segments.filter(s => s.id === +id);
    lines.push(`| ${id} | ${r.completed ? 'Yes' : 'No'} | ${s.reduce((a, x) => a + x.displacement_including_coast_deg, 0).toFixed(2)} | ${Math.max(...s.map(x => x.peak_reported_speed_during_pulse_deg_s)).toFixed(1)} | ${Math.min(...s.map(x => x.voltage_min_v)).toFixed(1)} | ${Math.max(...s.map(x => x.temperature_max_c))} | ${JSON.stringify(final.mode)} / ${JSON.stringify(final.pwm)} / ${JSON.stringify(final.torque_enable)} |`);
  }
}
lines.push('', 'Mode 2 is PWM. A final PWM readback of [0,0] and torque-enable [0] means output drive is disabled. Mode writes use one byte at 0x21 and the NVS bank is relocked; the selected mode remains configured after the test. Exact preflight and final register blocks are retained.', '',
  'The Rust loop sent serial duty commands through the existing FPGA release bridge and monitored encoder travel, voltage, temperature, and fault status. The drive command is persistent until replaced; software deadlines and cleanup are not an independent hardware watchdog. Before fast closed-loop or unattended PWM operation, an FPGA watchdog and deterministic feedback scheduling remain to be implemented and validated.', '',
  'These short pulses establish mode switching, motion response, and zero-drive stopping. They do not establish maximum PWM-mode speed, calibrated torque/current control, reverse-drive encoding, or all-nine simultaneous PWM performance. The prior all-nine position test stopped on a voltage collapse at the last confirmed 1 A supply limit; a 2 A setting has been requested for further concurrent tests.', '',
  'Raw measurements:', '', ...result.map(r => `- [${r.name}](${r.name}/run.json), [analysis](${r.name}/analysis.json), [supply conditions](${r.name}/supply-observation.json)`), '');
fs.writeFileSync(path.join(root, 'pwm-report.md'), lines.join('\n'));
console.log(lines.join('\n'));
