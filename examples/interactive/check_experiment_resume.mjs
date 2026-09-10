// Compare completed native search journals; solver and score computation remain Rust.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
const [fullDirectory,resumedDirectory,partialLog,cancelLog,fullLog,resumedLog,reportPath]=process.argv.slice(2);
assert(reportPath,'usage: check_experiment_resume full-directory resumed-directory partial-log cancel-log full-log resumed-log fresh-report');
function latest(root){
 const names=fs.readdirSync(root).filter(n=>/^state-\d{20}\.json$/.test(n)).sort();assert(names.length,'missing journal');
 const file=path.join(root,names.at(-1));return {file,value:JSON.parse(fs.readFileSync(file))};
}
function summary(file){const rows=fs.readFileSync(file,'utf8').trim().split('\n');return JSON.parse(rows.at(-1));}
const full=latest(fullDirectory),resumed=latest(resumedDirectory);
assert(isDeepStrictEqual(full.value.journal,resumed.value.journal),'interrupted search changed proposals, physical checkpoints or outcomes');
assert(isDeepStrictEqual(full.value.settings,resumed.value.settings),'selector settings changed');
const a=summary(fullLog),b=summary(resumedLog),partial=summary(partialLog),cancel=summary(cancelLog);
assert(a.pending===null&&b.pending===null,'search is still pending');
assert(isDeepStrictEqual(a.best,b.best),'best observed candidate changed');
assert(cancel.cancelled&&cancel.new_actions===0&&cancel.replayed_actions===0,'cancellation advanced an evaluation');
assert(cancel.revision===partial.revision,'cancelled call changed durable progress');
assert(b.replayed_actions>0,'resume did not reconstruct the interrupted prefix');
assert(full.value.journal.trials.some(t=>t.proposal.method.startsWith('latin_hypercube:')),'no initial design trials');
assert(full.value.journal.trials.some(t=>!['baseline'].includes(t.proposal.method)&&!t.proposal.method.startsWith('latin_hypercube:')),'no Bayesian proposal evaluated');
const report={version:1,passed:true,full_state:full.file,resumed_state:resumed.file,
 trials:full.value.journal.trials.length,all_proposals_checkpoints_and_outcomes_exact:true,cancellation_preserved_revision:true,
 replayed_actions:b.replayed_actions,best:a.best,context_id:full.value.journal.experiment.context_id,
 scope:'Exact same-host interrupted/uninterrupted search equivalence over the declared short task; not global optimality or sustained locomotion qualification.'};
fs.writeFileSync(reportPath,JSON.stringify(report,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify(report));
