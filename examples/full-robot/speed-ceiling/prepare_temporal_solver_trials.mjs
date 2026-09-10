import fs from 'node:fs';import crypto from 'node:crypto';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const source='runs/speed-ceiling/validation/flat125-human-5ms',catalog=read(`${d}/validation-cases.json`);
const refine=process.argv[2]==='--refine';
if(process.argv[2]&&!refine)throw Error('usage: prepare_temporal_solver_trials.mjs [--refine]');
const rows=refine?read(`${d}/temporal-solver-trials.json`).rows:[];
for(const [method,step] of (refine?[['sdirk2',.0025],['sdirk2',.00125]]:[['be',.01],['sdirk2',.005],['sdirk2',.01],['sdirk2',.02]])){
 const name=`flat125-${method}-${String(step*1000).replace('.','p')}ms-tangent`,prefix=`runs/speed-ceiling/validation/${name}`;
 if(fs.existsSync(`${prefix}.config.json`))throw Error('refusing overwrite temporal trial');
 const c=read('runs/speed-ceiling/performance/flat125-tangent-broyden.config.json');
 c.step_s=step;c.steps=Math.round(20/step);c.report_every=Math.round(.02/step);if(method==='sdirk2')c.implicit.sdirk2=true;
 const row={name,kind:'human',family:'flat125',prefix,duration_s:20,step_s:step,command_speed_m_s:.125,source,
  solver_method:method,scope:'Explicit numerical profile: existing guarded tangent/Broyden solver and declared integration step/method. CAD, contact, actuators, controller, inputs and convergence tolerances unchanged.'};
 for(const [suffix,value]of[['scene',read(`${source}.scene.json`)],['config',c],['actions',read(`${source}.actions.json`)]] ){
  fs.writeFileSync(`${prefix}.${suffix}.json`,JSON.stringify(value)+'\n');row[`${suffix}_sha256`]=sha(`${prefix}.${suffix}.json`);
 }
 rows.push(row);catalog.rows.push(row);
}
fs.writeFileSync(`${d}/temporal-solver-trials.json`,JSON.stringify({rows,accuracy_reference:'flat125-human-0p3125ms',scope:'Evaluate full trajectories against the finest completed backward-Euler run, in addition to the standard family comparison. Contact behavior and failures remain explicit; performance acceptance is a separate rendered browser test.'},null,2)+'\n');
fs.writeFileSync(`${d}/validation-cases.json`,JSON.stringify(catalog,null,2)+'\n');
