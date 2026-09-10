// Reuse existing guarded shared solver options; preserve physical/time tolerances.
import fs from 'node:fs';import crypto from 'node:crypto';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const source='runs/speed-ceiling/validation/flat125-human-5ms';
const variants=[['broyden',{broyden_updates:true},{}],['tangent-broyden',{broyden_updates:true},{linearized_jacobian_probes:true,linearized_probe_relative_step:1e-5,reuse_exact_probe_base:true}]];
const rows=[];
for(const [name,newton,implicit] of variants){
 const prefix=`runs/speed-ceiling/performance/flat125-${name}`;if(fs.existsSync(`${prefix}.config.json`))throw Error('refusing overwrite solver trial');
 const c=read(`${source}.config.json`);Object.assign(c.implicit.newton,newton);Object.assign(c.implicit,implicit);
 fs.writeFileSync(`${prefix}.config.json`,JSON.stringify(c)+'\n');rows.push({name,prefix,source,config_sha256:sha(`${prefix}.config.json`),source_config_sha256:sha(`${source}.config.json`),newton_overrides:newton,implicit_overrides:implicit});
}
fs.writeFileSync(`${d}/solver-trials.json`,JSON.stringify({rows,scope:'Same physical scene, inputs, 5 ms timestep and Newton/contact tolerances. Existing guarded correction-matrix approximations retain exact residual/endpoint checks. Compare complete native frames and production browser parity before any performance claim.'},null,2)+'\n');
