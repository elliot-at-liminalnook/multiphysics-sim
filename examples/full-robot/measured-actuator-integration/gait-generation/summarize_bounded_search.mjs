// Read committed search state and independently qualified capture reports.
import fs from 'node:fs';
import crypto from 'node:crypto';
const [directory,out,...reportPaths]=process.argv.slice(2);
if(!out)throw Error('summarize_bounded_search.mjs JOURNAL_DIRECTORY NEW_REPORT [CAPTURE_REPORTS...]');
const read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const revision=fs.readdirSync(directory).filter(n=>/^state-\d{20}\.json$/.test(n)).sort().at(-1);
const path=directory+'/'+revision,s=read(path),j=s.journal;
const trials=j.trials.map(t=>{
 const c=t.checkpoint,transition=c?.final_transition;
 const complete=!!c&&!c.recording.error&&!c.recording.runtime.failure&&!transition.terminated&&transition.truncated&&transition.completed_steps===j.experiment.spec.config.steps;
 return {id:t.proposal.id,method:t.proposal.method,values:t.proposal.values,complete,
  preparation_failure:t.preparation_failure,termination_reasons:transition?.termination_reasons,
  net_speed_m_s:transition?.speed.net_speed_m_s??null,eligible_short_screen_score:complete?transition.speed.net_speed_m_s:null};
});
const captured=reportPaths.map(path=>{const r=read(path);return{path,sha256:sha(path),horizon_s:r.horizon_s,net_speed_m_s:r.net_speed_m_s,
 worst_motor_rms_degrees:Math.max(...r.motors.map(m=>m.rms_tracking_degrees)),checks:r.checks,passes_all_finalist_checks:r.passes_all_finalist_checks};});
const report={simulation_only:true,scope:'Short search ranking is not promotion. Full tracking/geometry, numerical, stop, and replay checks remain separate.',
 journal:{path,sha256:sha(path),revision:s.revision},context_id:j.experiment.context_id,runtime:j.experiment.runtime,
 seed:j.experiment.spec.seed,completed_trials:trials.filter(t=>t.complete).length,trials,captured,
 fastest_short_screen:trials.filter(t=>t.complete).sort((a,b)=>b.eligible_short_screen_score-a.eligible_short_screen_score)[0]??null,
 promoted_candidate:null};
fs.writeFileSync(out,JSON.stringify(report,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({completed_trials:report.completed_trials,fastest_short_speed:report.fastest_short_screen?.net_speed_m_s,captured:captured.map(c=>({path:c.path,pass:c.passes_all_finalist_checks}))}));
