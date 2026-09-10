// Prepare explicit held-command windows through the ordinary Rhai controller.
// This changes only policy requests and duration, never robot/world physics.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
const [specPath, root] = process.argv.slice(2);
if (!specPath || !root) throw new Error('usage: prepare_command_hold_probe spec.json fresh-output-directory');
const read = p => JSON.parse(fs.readFileSync(p));
const spec = read(specPath), scene = read(spec.scene), config = read(spec.config), task = read(spec.task);
const duration = spec.duration_s, period = scene.period_s;
if (!(duration > 0) || Math.abs(duration / period - Math.round(duration / period)) > 1e-8
    || !Array.isArray(spec.windows_s) || !spec.windows_s.length) throw new Error('invalid probe clock/windows');
let previousEnd = 0;
for (const [start, end] of spec.windows_s) {
  if (![start, end].every(Number.isFinite) || start < previousEnd || end <= start || end > duration
      || [start, end].some(t => Math.abs(t / period - Math.round(t / period)) > 1e-8)) throw new Error('invalid or overlapping held-command windows');
  previousEnd = end;
}
const actions = read(spec.actions).slice(0, Math.round(duration / period));
if (actions.length !== Math.round(duration / period)) throw new Error('insufficient recorded action schedule');
if (config.policy?.neural_exploration) throw new Error('held-command probe requires deterministic output');
const last = config.policy?.neural_residual?.layers.at(-1);
if (last && [...last.weights.flat(), ...last.biases].some(v => v !== 0)) throw new Error('held-command probe requires zero neural corrections');
const entry = scene.controller.sources.entry, source = scene.controller.sources.files[entry];
if ((source.match(/\bfn\s+control\s*\(/g) ?? []).length !== 1 || source.includes('held_probe_baseline_control')) throw new Error('expected one unwrapped entry control function');
scene.controller.sources.files[entry] = source.replace(/\bfn\s+control\s*\(/, 'fn held_probe_baseline_control(') + `
// Explicit diagnostic windows. Baseline state/lease continue on simulation time.
fn control(t, sensors, commands, state) {
    let response = held_probe_baseline_control(t, sensors, commands, state);
    let p = parameters_except(["trajectory", "static_feedforward", "dynamic_feedforward", "velocity_feedforward"]);
    let active = -1; let index = 0;
    for window in p.forecast_probe_windows_s {
        if t >= window[0] && t < window[1] { active = index; }
        index += 1;
    }
    if !response.state.contains("held_probe_window") { response.state.held_probe_window = -1; }
    if active >= 0 {
        if response.state.held_probe_window != active { response.state.held_probe_commands = response.commands; }
        response.commands = response.state.held_probe_commands;
    }
    response.state.held_probe_window = active;
    response
}
`;
if ('forecast_probe_windows_s' in scene.controller.parameters) throw new Error('probe parameters already present');
scene.controller.parameters.forecast_probe_windows_s = spec.windows_s;
scene.duration_s = duration; config.steps = Math.round(duration / config.step_s);
if (Math.abs(config.steps * config.step_s - duration) > 1e-8) throw new Error('physics clock mismatch');
fs.mkdirSync(root);
const write = (name, value) => fs.writeFileSync(path.join(root, name), JSON.stringify(value) + '\n', {flag: 'wx'});
write('scene.json', scene); write('config.json', config); write('task.json', task); write('actions.json', actions);
write('inputs.json', {specification: spec, files: [specPath, spec.scene, spec.config, spec.task, spec.actions].map(p => ({path: p, sha256: crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex')})),
  scope: 'Diagnostic policy holds its current bounded actuator targets in explicit windows; baseline state and command lease continue. Original recorded input packets are retained. No speed benefit or predictive accuracy is assumed.'});
