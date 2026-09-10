// Refit immutable short-run observations with the shared Rust heading-trend
// predictor. These estimated objectives must never be mixed with measured ones.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
const [sourceRoot,root,predictor]=process.argv.slice(2);
assert(sourceRoot&&root&&predictor,'usage: rescore_planar_speed_screen source-root new-root predictor');
const read=p=>JSON.parse(fs.readFileSync(p)),write=(p,v)=>fs.writeFileSync(p,JSON.stringify(v)+'\n',{flag:'wx'});
const pin=path=>({path,sha256:crypto.createHash('sha256').update(fs.readFileSync(path)).digest('hex')});
const source=read(sourceRoot+'/result.json'),spec=read(sourceRoot+'/spec.json'),experiment=read(spec.source_experiment);
assert.equal(source.version,1);assert(source.rows.length>=1);
fs.mkdirSync(root);
const inputs={version:1,files:[sourceRoot+'/result.json',sourceRoot+'/inputs.json',sourceRoot+'/spec.json',spec.source_experiment,predictor,import.meta.filename].map(pin),scope:'Rescores exactly the completed prefix observations; no new simulation or future ground truth. Objective is an estimated300s net speed, not a measured outcome. Numerical and fall failures stay failed. This separate context must not be combined with full-horizon physical scores.'};
write(root+'/inputs.json',inputs);
const context_id=crypto.createHash('sha256').update(JSON.stringify(inputs)).digest('hex');
const problem={...experiment.problem,context_id,objective_name:'negative predicted full-horizon net speed from a completed physical prefix',constraints:[{name:'sampled fall during observed prefix',unit:'1',scale:1}]};
const observations=[],rows=[];
for(const old of source.rows){
 const name='evaluation-'+String(old.ordinal).padStart(3,'0'),path=root+'/'+name,sourcePath=sourceRoot+'/'+name;
 fs.mkdirSync(path);
 if(old.status==='failed'){
   write(path+'/summary.json',{...old,source_summary:pin(sourcePath+'/summary.json')});rows.push(old);
   observations.push({context_id,values:old.values,outcome:{status:'failed',reason:old.error},evidence:path+'/summary.json'});continue;
 }
 assert.equal(old.status,'predicted');const fits=[];
 for(let i=0;i<spec.fit_windows.length;i++){
   const sourceRequest=sourcePath+`/fit-${i}.request.json`,request=read(sourceRequest);
   assert(request.queries.every(q=>q.observed_position_m===undefined),'screen requests must not contain future measurements');
   request.heading_trend=true;write(path+`/fit-${i}.request.json`,request);
   const output=path+`/fit-${i}.result.json`,r=spawnSync(predictor,[path+`/fit-${i}.request.json`,output],{encoding:'utf8'});
   write(path+`/fit-${i}.execution.json`,{exit_code:r.status,signal:r.signal,error:r.error?.message??null,stderr:r.stderr});assert.equal(r.status,0,r.stderr);
   fits.push(read(output));
 }
 const speeds=fits.map(f=>f.forecasts[0].predicted_net_speed_m_s),predicted_speed_m_s=Math.max(...speeds);
 const row={...old,predicted_speed_m_s,fit_window_speeds_m_s:speeds,prior_predicted_speed_m_s:old.predicted_speed_m_s,prior_window_speeds_m_s:old.fit_window_speeds_m_s,physical_full_horizon_qualified:false,source_summary:pin(sourcePath+'/summary.json'),scope:'Heading trend, same past poses and same optimistic maximum across fit windows. Estimate for experiment selection only; not a speed qualification or calibrated confidence bound.'};
 write(path+'/summary.json',row);rows.push(row);
 observations.push({context_id,values:old.values,outcome:{status:'complete',objective:-predicted_speed_m_s,residuals:[-1]},evidence:path+'/summary.json'});
}
write(root+'/result.json',{version:1,problem,observations,rows,ranked:rows.filter(r=>r.status==='predicted').sort((a,b)=>b.predicted_speed_m_s-a.predicted_speed_m_s),scope:inputs.scope});
console.log(JSON.stringify({rows:rows.length,completed_prefixes:observations.filter(r=>r.outcome.status==='complete').length,parameters:problem.parameters.length}));
