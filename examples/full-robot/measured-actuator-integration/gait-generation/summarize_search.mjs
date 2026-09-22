// Read-only presentation of committed Rust journal outcomes; no simulation.
import fs from 'node:fs';
import crypto from 'node:crypto';
const root = process.argv[2] ?? 'examples/full-robot/measured-actuator-integration/gait-generation';
const directory = `${root}/pilot-journal`;
const latest = fs.readdirSync(directory).filter(n => /^state-\d{20}\.json$/.test(n)).sort().at(-1);
if (!latest) throw Error('No committed journal revision.');
const bytes = fs.readFileSync(`${directory}/${latest}`), state = JSON.parse(bytes);
const log = fs.readFileSync(`${root}/search.log`, 'utf8').trim().split('\n').flatMap(line => {
  try { return [JSON.parse(line)]; } catch { return []; }
});
const spec = state.journal.experiment.spec;
function endpointDiagnostics(checkpoint) {
  const frame = checkpoint.frame;
  const powered = Boolean(spec.config.motors.power);
  if (powered !== Boolean(frame.power)) throw Error('Missing or unexpected power-network observation');
  const motors = spec.config.motors.target_coordinates.map((coordinate,i) => {
    const index=spec.task.observations.findIndex(o=>o.source.kind==='coordinate_position'&&o.source.coordinate===coordinate);
    if(index<0)throw Error(`No named position observation: ${coordinate}`);
    const angle=checkpoint.final_transition.observations[index], target=frame.servo_targets_rad[i];
    let voltage=spec.config.motors.servos[i].supply_voltage_v;
    if(powered){
      const ids=spec.scene.robot.motors.filter(m=>`joint.${m.joint}`===coordinate).map(m=>m.id);
      if(ids.length!==1)throw Error('Ambiguous powered motor');
      const branches=spec.scene.robot.actuator_profiles.power.branches.filter(b=>b.motors.includes(ids[0]));
      if(branches.length!==1)throw Error('Ambiguous power branch');
      const readings=frame.power.branches.filter(b=>b.id===branches[0].id);
      if(readings.length!==1||!Number.isFinite(readings[0].voltage_v))throw Error('Missing branch voltage');
      voltage=readings[0].voltage_v;
    }
    return {coordinate,angle_rad:angle,target_rad:target,tracking_error_deg:(target-angle)*180/Math.PI,
      supply_voltage_v:voltage,
      duty:frame.servo_commands[i],motor_current_a:frame.motor_readings[i].current_a,
      driver_supply_current_a:frame.driver_readings[i].supply_current_a};
  });
  return {
    scope:`Endpoint-only diagnostics, not peak or time-averaged values. Electrical values are uncalibrated predictions under ${powered?'the CAD battery/wiring model':'imposed supplies'}.`,
    motors,
    maximum_absolute_tracking_error_deg:Math.max(...motors.map(m=>Math.abs(m.tracking_error_deg))),
    predicted_driver_supply_current_a_sum:motors.reduce((sum,m)=>sum+m.driver_supply_current_a,0),
    predicted_driver_supply_power_w_sum:motors.reduce((sum,m)=>sum+m.driver_supply_current_a*m.supply_voltage_v,0),
    predicted_power_network:frame.power ?? null,
  };
}
const report = {
  scope: 'Gait experiment with provisional motor/electrical parameters; not hardware validation. Only committed checkpoints below; progress may be newer. Interpret speed over the stated horizon and inspect the declared supply scenario.',
  journal: latest,
  journal_sha256: crypto.createHash('sha256').update(bytes).digest('hex'),
  context_id: state.journal.experiment.context_id,
  horizon_s: state.journal.experiment.spec.config.steps * state.journal.experiment.spec.config.step_s,
  latest_progress: log.at(-1) ?? null,
  trials: state.journal.trials.map(t => ({
    id: t.proposal.id, method: t.proposal.method, values: t.proposal.values,
    preparation_failure: t.preparation_failure,
    checkpoint: t.checkpoint ? {
      time_s: t.checkpoint.final_transition.time_s,
      terminated: t.checkpoint.final_transition.terminated,
      truncated: t.checkpoint.final_transition.truncated,
      termination_reasons: t.checkpoint.final_transition.termination_reasons,
      error: t.checkpoint.recording.error,
      physics_failure: t.checkpoint.recording.runtime.failure,
      speed: t.checkpoint.final_transition.speed,
      endpoint_diagnostics: endpointDiagnostics(t.checkpoint),
    } : null,
  })),
};
fs.writeFileSync(`${root}/results.json`, JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify(report, null, 2));
