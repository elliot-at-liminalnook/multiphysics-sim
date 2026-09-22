// Verify a native environment replay against its committed search checkpoint.
import fs from 'node:fs';
import crypto from 'node:crypto';
const root=process.argv[2]??'examples/full-robot/measured-actuator-integration/gait-generation/selected-10s';
const directory=`${root}/pilot-journal`;
const file=fs.readdirSync(directory).filter(p=>/^state-\d{20}\.json$/.test(p)).sort().at(-1);
const journalPath=`${directory}/${file}`,capturePath=process.argv[3]??`${root}/replay-capture.json`;
const reportPath=process.argv[4]??`${root}/replay-verification.json`;
const read=p=>JSON.parse(fs.readFileSync(p));
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const checkpoint=read(journalPath).journal.trials[0].checkpoint;
const capture=read(capturePath);
if(!checkpoint||!capture.frames?.length||!capture.transitions?.length)throw Error('Missing completed evidence');
const frame=structuredClone(capture.frames.at(-1));delete frame.stepping_wall_s;
// Match the runtime's harmless signed-zero normalization. No numeric tolerance.
function canonical(value){
  if(typeof value==='number'){
    if(!Number.isFinite(value))throw Error('Nonfinite replay value');
    return value===0?0:value;
  }
  if(Array.isArray(value))return value.map(canonical);
  if(value&&typeof value==='object')return Object.fromEntries(Object.keys(value).sort().map(k=>[k,canonical(value[k])]));
  return value;
}
const digest=value=>crypto.createHash('sha256').update(JSON.stringify(canonical(value))).digest('hex');
const checks={
  completed:capture.completed===true&&capture.requested_steps_completed===true&&capture.error===null,
  source_complete:checkpoint.final_transition.truncated===true&&!checkpoint.final_transition.terminated&&
    !checkpoint.recording.error&&!checkpoint.recording.runtime.failure,
  final_frame_exact:digest(frame)===digest(checkpoint.frame),
  final_transition_exact:digest(capture.transitions.at(-1))===digest(checkpoint.final_transition),
  runtime_recording_exact:digest(capture.recording)===digest(checkpoint.recording.runtime),
  task_exact:digest(capture.task)===digest(checkpoint.recording.task),
};
const report={
  scope:`Full ${capture.transitions.at(-1).time_s} s environment replay, exact endpoint frame/transition and recording comparison. Host wall-time field is excluded; signed zeros normalize to zero. No floating-point tolerance.`,
  journal:{path:journalPath,sha256:sha(journalPath)},capture:{path:capturePath,sha256:sha(capturePath)},
  checks,passed:Object.values(checks).every(Boolean),
  simulated_s:capture.transitions.at(-1).time_s,reported_step_and_capture_wall_s:capture.wall_s,
  transition_count:capture.transitions.length,
  limitations:'Exact replay proves reproducibility for this run, not model calibration, convergence at another timestep or sustained 300-second walking.',
};
fs.writeFileSync(reportPath,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify(report,null,2));
if(!report.passed)process.exitCode=1;
