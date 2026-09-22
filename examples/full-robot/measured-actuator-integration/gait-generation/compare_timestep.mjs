// Numerical report from named observations in completed shared-Rust experiments.
import fs from 'node:fs';
import crypto from 'node:crypto';
const root = 'examples/full-robot/measured-actuator-integration/gait-generation';
const nominalDirectory=process.argv[2]??root;
const refinedDirectory=process.argv[3]??`${root}/baseline-half-step`;
const protocolPath=process.argv[4]??`${root}/numerical-screening-protocol.json`;
const reportPath=process.argv[5]??`${root}/timestep-comparison.json`;
const protocol = JSON.parse(fs.readFileSync(protocolPath));
function load(directory) {
  const file = fs.readdirSync(directory).filter(p => /^state-\d{20}\.json$/.test(p)).sort().at(-1);
  const path = `${directory}/${file}`, bytes = fs.readFileSync(path), stored = JSON.parse(bytes);
  const trial = stored.journal.trials[0], checkpoint = trial?.checkpoint;
  if (!checkpoint || trial.preparation_failure || checkpoint.recording.error || checkpoint.recording.runtime.failure ||
      checkpoint.final_transition.terminated || !checkpoint.final_transition.truncated ||
      checkpoint.recording.runtime.completed_steps !== checkpoint.recording.runtime.config.steps)
    throw Error(`Expected a complete successful baseline checkpoint: ${path}`);
  return {spec: stored.journal.experiment.spec, runtime: stored.journal.experiment.runtime, checkpoint,
    provenance: {path, sha256: crypto.createHash('sha256').update(bytes).digest('hex')}};
}
const nominal = load(`${nominalDirectory}/pilot-journal`), refined = load(`${refinedDirectory}/pilot-journal`);
const same = (a,b,label) => {if(JSON.stringify(a)!==JSON.stringify(b))throw Error(`Changed invariant: ${label}`);};
for (const key of ['scene','task','source_actions','baseline','seed','parameterization','objective'])
  same(nominal.spec[key],refined.spec[key],key);
same(nominal.runtime,refined.runtime,'runtime identity');
const withoutStep = c => {const copy={...c};delete copy.step_s;delete copy.steps;delete copy.report_every;return copy;};
same(withoutStep(nominal.spec.config),withoutStep(refined.spec.config),'all other runtime settings');
for (const [run,step] of [[nominal,protocol.nominal_step_s],[refined,protocol.refined_step_s]]) {
  same(run.spec.config.step_s,step,'declared timestep');
  same(run.spec.config.step_s*run.spec.config.steps,protocol.horizon_s,'horizon');
  same(run.checkpoint.final_transition.time_s,protocol.horizon_s,'endpoint time');
  same(run.spec.seed,protocol.seed,'seed');
}
const a=nominal.checkpoint,b=refined.checkpoint;
const body=nominal.spec.task.speed.body_link;
const pose = c => {const poses=c.frame.poses.filter(p=>p.name===body);if(poses.length!==1)throw Error('Ambiguous body');return poses[0];};
const pa=pose(a),pb=pose(b);
const targets=new Set(nominal.spec.config.motors.target_coordinates);
const joint_differences=nominal.spec.task.observations.flatMap((o,i) =>
  o.source.kind==='coordinate_position' && targets.has(o.source.coordinate)
    ? [{coordinate:o.source.coordinate,absolute_difference_rad:Math.abs(a.final_transition.observations[i]-b.final_transition.observations[i])}] : []);
if(joint_differences.length!==targets.size || new Set(joint_differences.map(j=>j.coordinate)).size!==targets.size)
  throw Error('Named motor observation coverage differs');
const metrics={
  absolute_net_distance_difference_m:Math.abs(a.final_transition.speed.net_distance_m-b.final_transition.speed.net_distance_m),
  endpoint_body_position_difference_m:Math.hypot(...pa.position_m.map((x,i)=>x-pb.position_m[i])),
  endpoint_body_up_z_difference:Math.abs(a.final_transition.speed.body_up_z-b.final_transition.speed.body_up_z),
  endpoint_actuated_joint_difference_rad:Math.max(...joint_differences.map(j=>j.absolute_difference_rad)),
};
const checks=Object.entries(metrics).map(([name,value])=>({name,value,limit:protocol[`maximum_${name}`],passed:Number.isFinite(value)&&value<=protocol[`maximum_${name}`]}));
const report={scope:protocol.scope,nominal:nominal.provenance,refined:refined.provenance,
  checks,joint_differences,passed:checks.every(c=>c.passed),limitations:protocol.interpretation};
fs.writeFileSync(reportPath,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify(report,null,2));
if(!report.passed)process.exitCode=1;
