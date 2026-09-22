// Aggregate saved Rust observations only; no simulation or contact model here.
import fs from 'node:fs';
import crypto from 'node:crypto';

const capturePath = process.argv[2], outputPath = process.argv[3];
if (!capturePath || !outputPath) throw Error('Usage: summarize_trace.mjs capture.json report.json');
const sha = path => crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex');
const c = JSON.parse(fs.readFileSync(capturePath));
const {frames, transitions, task} = c;
if (!c.completed || !c.requested_steps_completed || c.error || frames.length < 2 ||
    frames.length !== transitions.length || transitions.at(-1).terminated ||
    !transitions.at(-1).truncated) throw Error('A complete nonterminated capture is required');
for (let i = 0; i < frames.length; i++) {
  if (frames[i].time_s !== transitions[i].time_s || frames[i].error ||
      (i && Math.abs(frames[i].time_s - frames[i-1].time_s - task.period_s) > 1e-9))
    throw Error('Missing or mismatched task samples');
}
const start = frames[0].time_s, end = frames.at(-1).time_s, duration = end - start;
const finite = value => { if (!Number.isFinite(value)) throw Error('Nonfinite/missing observation'); return value; };
const index = predicate => {
  const matches = task.observations.flatMap((o, i) => predicate(o.source) ? [i] : []);
  if (matches.length !== 1) throw Error('Missing or ambiguous named observation');
  return matches[0];
};
const mean = values => values.reduce((a,b) => a+b,0) / values.length;
const body = task.speed.body_link;
const x = index(s => s.kind === 'body_position' && s.link === body && s.axis === 'x');
const y = index(s => s.kind === 'body_position' && s.link === body && s.axis === 'y');
const points = transitions.map(t => [finite(t.observations[x]), finite(t.observations[y])]);
const distance = (a,b) => Math.hypot(a[0]-b[0], a[1]-b[1]);
const legs = points.slice(1).map((p,i) => distance(points[i],p));
const path = legs.reduce((a,b) => a+b,0), net = distance(points[0],points.at(-1));
if (Math.abs(net - transitions.at(-1).speed.net_distance_m) > 1e-9)
  throw Error('Named body positions disagree with native net-distance metric');
const windows = [];
const width = Math.round(2/task.period_s);
if (Math.abs(width * task.period_s - 2) > 1e-9) throw Error('Two-second windows require an aligned task grid');
for (let a = 0; a < points.length-1; a += width) {
  const b = Math.min(a+width, points.length-1), seconds = frames[b].time_s-frames[a].time_s;
  windows.push({start_s:frames[a].time_s,end_s:frames[b].time_s,
    net_speed_m_s:distance(points[a],points[b])/seconds,
    sampled_path_speed_m_s:legs.slice(a,b).reduce((v,d)=>v+d,0)/seconds,
    radial_progress_speed_m_s:(distance(points[0],points[b])-distance(points[0],points[a]))/seconds});
}
const cfg = c.recording.config.motors, robot = c.recording.scene.robot;
if (cfg.controller !== 'cad_fixed_pd') throw Error('This report requires the explicit CAD fixed-PD scenario');
const powered=Boolean(cfg.power);
if(frames.some(f=>Boolean(f.power)!==powered))throw Error('Missing or unexpected power-network readings');
const motors = cfg.target_coordinates.map((coordinate,i) => {
  const j = index(s => s.kind === 'coordinate_position' && s.coordinate === coordinate);
  const bindingMotors = robot.motors.filter(m => `joint.${m.joint}` === coordinate);
  if (bindingMotors.length !== 1) throw Error('Ambiguous CAD actuator identity');
  const motor = bindingMotors[0], binding = robot.actuator_profiles.bindings[motor.id];
  const limit = robot.actuator_profiles.families[binding.family].controller.gains.limit / 1000;
  if (!(limit > 0 && limit <= 1)) throw Error('Invalid declared duty limit');
  const errors = frames.slice(1).map((f,k) =>
    (finite(f.servo_targets_rad[i]) - finite(transitions[k+1].observations[j])) * 180/Math.PI);
  const duties = frames.slice(1).map(f => finite(f.servo_commands[i]));
  const currents = frames.slice(1).map(f => finite(f.motor_readings[i].current_a));
  if (duties.some(d => Math.abs(d) > limit+1e-12)) throw Error('Duty exceeds declared CAD limit');
  return {coordinate,cad_motor_id:motor.id,duty_limit:limit,
    sampled_tracking_rms_deg:Math.sqrt(mean(errors.map(e=>e*e))),
    sampled_tracking_peak_abs_deg:Math.max(...errors.map(Math.abs)),
    sampled_at_duty_limit_fraction:mean(duties.map(d=>Number(Math.abs(d)>=limit-1e-12))),
    predicted_sampled_winding_current_peak_abs_a:Math.max(...currents.map(Math.abs))};
});
// Imposed terminal voltages are explicit configuration, never inferred from duty.
const amps = frames.map(f => powered ? finite(f.power.current_a) :
  f.driver_readings.reduce((s,d)=>s+finite(d.supply_current_a),0));
const watts = frames.map(f => powered ? finite(f.power.power_w) :
  f.driver_readings.reduce((s,d,i)=>s+finite(d.supply_current_a)*finite(cfg.servos[i].supply_voltage_v),0));
let joules = 0;
for (let i=1;i<watts.length;i++) joules += (watts[i]+watts[i-1])/2 * (frames[i].time_s-frames[i-1].time_s);
const result = {
  scope:'Descriptive aggregates of saved Rust task samples; provisional simulation, no acceptance threshold or hardware rating.',
  capture:{path:capturePath,sha256:sha(capturePath)},script_sha256:sha(new URL(import.meta.url)),
  duration_s:duration,sample_period_s:task.period_s,frame_count:frames.length,
  motion:{net_distance_m:net,net_speed_m_s:net/duration,sampled_path_length_m:path,
    sampled_path_speed_m_s:path/duration,net_to_sampled_path_ratio:path>0?net/path:null,windows},
  motors,
  electrical:{boundary:powered?'CAD battery and wiring; predicted sag and depletion':'imposed supplies; no simulated sag or battery depletion in this capture',
    imposed_supply_voltage_v:powered?null:cfg.servos.map(s=>s.supply_voltage_v),
    predicted_sampled_total_terminal_current_peak_a:Math.max(...amps),
    predicted_sampled_total_terminal_power_peak_w:Math.max(...watts),
    predicted_terminal_energy_trapezoid_j:joules,predicted_mean_terminal_power_w:joules/duration},
  limitations:[
    'Path is the sum of XY chords between task samples, not continuous distance or commanded-direction accuracy.',
    'Tracking and saturation statistics use equally spaced post-step samples, exclude the initial sample and can miss between-sample peaks.',
    'Joint errors are target minus actual unwrapped coordinate position; no modulo-angle reduction.',
    'Electrical estimates are uncalibrated; trapezoidal energy is a sampled diagnostic, not the native integrated battery-energy state.',
    'Net progress can fall when a robot turns or backtracks; path speed alone can reward ineffective motion.'
  ]
};
if(powered){
  const powers=frames.map(f=>f.power);
  const energy=finite(powers.at(-1).terminal_energy_j)-finite(powers[0].terminal_energy_j);
  result.electrical.predicted_native_terminal_energy_j=energy;
  result.electrical.predicted_native_mean_terminal_power_w=energy/duration;
  result.electrical.predicted_pack_voltage_range_v=[Math.min(...powers.map(p=>finite(p.voltage_v))),Math.max(...powers.map(p=>finite(p.voltage_v)))];
  result.electrical.predicted_soc_start_end=[finite(powers[0].state_of_charge),finite(powers.at(-1).state_of_charge)];
  result.electrical.branches=robot.actuator_profiles.power.branches.map(b=>{
    const values=powers.map(p=>{
      const v=p.branches.filter(x=>x.id===b.id);if(v.length!==1)throw Error('Missing/ambiguous power branch');return v[0];
    });
    return {id:b.id,motors:b.motors,
      predicted_voltage_range_v:[Math.min(...values.map(v=>finite(v.voltage_v))),Math.max(...values.map(v=>finite(v.voltage_v)))],
      predicted_current_peak_abs_a:Math.max(...values.map(v=>Math.abs(finite(v.current_a)))),
      predicted_wiring_loss_peak_w:Math.max(...values.map(v=>finite(v.wiring_loss_w)))};
  });
}
fs.writeFileSync(outputPath,JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify({duration_s:duration,motion:result.motion,electrical:result.electrical},null,2));
