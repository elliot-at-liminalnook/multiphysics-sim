// Reuse a search candidate with a declared horizon and command schedule.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
const [searchPath,schedulePath,valuesPath,out,mode='forward']=process.argv.slice(2);
if(!out)throw Error('prepare_bounded_validation.mjs SEARCH_SPEC FULL_SCHEDULE_SPEC VALUES_JSON NEW_DIRECTORY [forward|half-step|stop]');
assert(['forward','half-step','stop'].includes(mode));
const read=p=>JSON.parse(fs.readFileSync(p));const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const spec=read(searchPath),schedule=read(schedulePath);spec.baseline=read(valuesPath);
assert.equal(spec.scene.period_s,schedule.scene.period_s);assert.equal(spec.scene.robot.source.cad_sha256,schedule.scene.robot.source.cad_sha256);
const horizon=10,dt=spec.scene.period_s;
spec.source_actions=schedule.source_actions.slice(0,Math.round(horizon/dt));assert.equal(spec.source_actions.length,horizon/dt);
if(mode==='half-step')spec.config.step_s/=2;
spec.config.steps=Math.round(horizon/spec.config.step_s);spec.scene.duration_s=horizon;
if(mode==='stop'){
 const names=spec.scene.controller.inputs.map(a=>a.name);const channels=['command.forward_speed','command.lateral_speed','command.yaw_rate'].map(n=>names.indexOf(n));assert(channels.every(i=>i>=0));
 for(let i=Math.round(7/dt);i<spec.source_actions.length;i++)for(const k of channels)spec.source_actions[i][k]=0;
}
fs.mkdirSync(out);fs.writeFileSync(out+'/spec.json',JSON.stringify(spec)+'\n',{flag:'wx'});
fs.writeFileSync(out+'/preparation.json',JSON.stringify({mode,horizon_s:horizon,seed:spec.seed,values:spec.baseline,inputs:[searchPath,schedulePath,valuesPath].map(path=>({path,sha256:sha(path)})),script_sha256:sha(new URL(import.meta.url))},null,2)+'\n',{flag:'wx'});
