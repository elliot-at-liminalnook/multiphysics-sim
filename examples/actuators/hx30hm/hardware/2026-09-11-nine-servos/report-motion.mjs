// Summarize recorded hardware measurements; this script contains no controller or physics model.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.dirname(fileURLToPath(import.meta.url));
const read = p => JSON.parse(fs.readFileSync(p, 'utf8'));
const format = (x, digits = 1) => Number.isFinite(x) ? x.toFixed(digits) : '—';
const names = process.argv.slice(2);
if (!names.length) throw new Error('Pass analyzed run directory names.');
const runs = names.map(name => ({
  name,
  run: read(path.join(root, name, 'run.json')),
  analysis: read(path.join(root, name, 'analysis.json')),
  supply: fs.existsSync(path.join(root, name, 'supply-observation.json'))
    ? read(path.join(root, name, 'supply-observation.json')) : {},
}));
for (const r of runs) {
  const txns = fs.readFileSync(path.join(root, r.name, 'transactions.jsonl'), 'utf8')
    .trim().split('\n').filter(Boolean).map(JSON.parse);
  const packetValid = p => p.length >= 6 && p[0] === 255 && p[1] === 255
    && p.length === p[3] + 4 && (p.slice(2).reduce((a, b) => a + b, 0) & 255) === 255;
  r.transactions = {
    count: txns.length,
    invalid_tx_packets: txns.filter(t => !packetValid(t.tx)).length,
    invalid_replies: txns.filter(t => !t.no_reply_expected
      && (!packetValid(t.rx) || t.rx[2] !== t.id || t.rx[4] !== 0 || t.decode_error)).length,
    broadcasts_without_ack: txns.filter(t => t.no_reply_expected).length,
    invalid_time_windows: txns.filter(t => t.completion_host_ns < t.request_host_ns).length,
  };
  fs.writeFileSync(path.join(root, r.name, 'transaction-validation.json'),
    JSON.stringify(r.transactions, null, 2) + '\n');
}

const lines = [
  '# HX-30HM measured motion through the FPGA bridge',
  '',
  'Nine servos, IDs 4–12. Position mode, free output shafts, secured housings as reported by the user. These are warm-bench motion measurements, not calibrated torque, absolute accuracy, or loaded endurance tests.',
  '',
  'The WANPTEK DPS3010U settings are user reports. The original approximately 0.45 A observation was consumption: the user corrected the indicator report from CC to CV. It does not establish current limiting. The interrupted 1 A run includes an unsynchronized change to 13 V and is excluded from constant-voltage comparisons. Subsequent 12.6 V runs have their own preflight and supply records.',
  '',
  '## Run outcomes',
  '',
  '| Run | Motion | Completed | Samples | Servo voltage range V | Maximum reported °C | Fault samples |',
  '|---|---|---|---:|---|---:|---:|',
];
for (const { name, run, analysis: a } of runs) {
  const p = a.per_servo.filter(x => x.samples > 0);
  lines.push(`| [${name}](${name}/run.json) | ${run.plan?.concurrent ? 'All nine together' : 'One at a time'} | ${run.completed ? 'Yes' : 'No'} | ${a.samples} | ${format(Math.min(...p.map(x => x.voltage_min)))}–${format(Math.max(...p.map(x => x.voltage_max)))} | ${Math.max(...p.map(x => x.temperature_max))} | ${p.reduce((n, x) => n + x.fault_samples, 0)} |`);
}
lines.push('', '## Long-travel speed by servo', '',
  'Each cell is positive / negative speed in degrees per second during the ±90° stage (180° between opposite targets), using the median across eligible movements. A dash means fewer than five samples in the fitted interval. These are measured mid-travel speeds, not proof of an absolute actuator ceiling.', '',
  `| ID | ${runs.map(r => r.name).join(' | ')} |`,
  `|---|${runs.map(() => '---').join('|')}|`);
for (let id = 4; id <= 12; id++) {
  lines.push(`| ${id} | ${runs.map(({ analysis: a }) => {
    const s = a.per_servo.find(p => p.id === id)?.by_stage.find(s => s.name === 'long-ceiling-repeat');
    return s ? `${format(s.positive_midtravel_speed_deg_s)} / ${format(s.negative_midtravel_speed_deg_s)}` : '—';
  }).join(' | ')} |`);
}
for (const { name, run, analysis: a, supply, transactions: t } of runs) {
  lines.push('', `## ${name}`, '',
    `Supply record: [supply-observation.json](${name}/supply-observation.json). ${supply.comparison_validity ?? ''}`.trim(),
    '', `Result: ${run.completed ? 'all planned stages completed' : run.result?.failure ?? 'incomplete'}.`);
  lines.push('', `Packet audit: ${t.count} transactions, ${t.invalid_tx_packets} invalid requests, ${t.invalid_replies} invalid/error replies, ${t.invalid_time_windows} invalid timestamp windows; ${t.broadcasts_without_ack} broadcasts correctly expect no acknowledgment.`);
  if (run.result?.recovery?.length) lines.push('', `Recovery: ${run.result.recovery.map(r => `ID ${r.id} hold ${r.hold_confirmed ? 'verified' : 'unconfirmed'}`).join('; ')}.`);
  const final = Object.values(run.result?.final_states ?? {});
  if (final.length) lines.push('',
    `Final readback: ${final.filter(s => s.telemetry?.speed_raw === 0).length}/${final.length} stationary; ${final.filter(s => s.original_RAM_restored).length}/${final.length} original position/time/speed RAM fields restored; ${final.filter(s => s.registers_0_through_39_unchanged).length}/${final.length} configuration blocks unchanged.`);
  const sustained = a.segments.filter(s => s.name === 'sustained-concurrent-ceiling');
  if (sustained.length) lines.push('',
    `Sustained concurrent stage: ${sustained.length} recorded servo movement segments; maximum reported temperature ${Math.max(...sustained.map(s => s.temperature_max))}°C; minimum reported voltage ${format(Math.min(...sustained.map(s => s.voltage_min)))} V. Planned duration excludes setup and readback overhead.`);
}
lines.push('', '## Measurement method and limits', '',
  '- Position-to-time linear regression over 20–80% of commanded travel, using host transaction midpoints. Per-direction summaries require at least five samples. Original raw packet bytes and request/reply times are retained.',
  '- Commands increase from ±5° at 26.4°/s to ±90° with a 527.3°/s speed command. The command value is a requested limit, not measured shaft speed. Settling gates use ±12 encoder counts (1.055°).',
  '- Individual motion polls the active servo. Concurrent motion uses one 35-byte position SYNC WRITE for all nine, followed by sequential telemetry reads. Simultaneous command transmission does not prove simultaneous sensor sampling.',
  '- Host timestamps include USB and FPGA buffering. Internal sensor update age is unknown. Sub-millisecond latency, peak acceleration, and peak transient supply current are not identified.',
  '- Voltage and temperature are uncalibrated onboard readings. Current register values retain their raw units; provisional 1 mA/count values are not independent supply-current measurements.',
  '- Temperature cutoff 60°C, reported voltage range 9–12.6 V, status faults, travel margins, and settling checks gate escalation. STOP requests hold the active servos at their latest measured positions. No nonvolatile configuration or torque-limit changes are made.',
  '- A known load or torque instrument is still needed for torque-speed curves, friction, backlash under load, output inertia, and thermal model identification. These data support a provisional no-load response model with the voltage and temperature conditions preserved.',
  '', '## Reproduce the analysis', '', '```sh',
  `node examples/actuators/hx30hm/hardware/2026-09-11-nine-servos/analyze-motion.mjs ${names.join(' ')}`,
  `node examples/actuators/hx30hm/hardware/2026-09-11-nine-servos/report-motion.mjs ${names.join(' ')}`,
  '```', '');
fs.writeFileSync(path.join(root, 'motion-report.md'), lines.join('\n'));
console.log('Wrote motion-report.md');
