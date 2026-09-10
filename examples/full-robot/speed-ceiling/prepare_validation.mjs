import fs from 'node:fs';import crypto from 'node:crypto';
const d='examples/full-robot/speed-ceiling',base='examples/full-robot/fast-wasd',run='runs/speed-ceiling/validation';
const read=p=>JSON.parse(fs.readFileSync(p)),sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const spec=process.argv[2]&&!Number.isFinite(Number(process.argv[2]))?read(process.argv[2]):null;
const speed=spec?spec.speed:(process.argv[2]?Number(process.argv[2]):.1);
if(!Number.isFinite(speed)||speed<=0)throw Error('positive speed required');
const source=spec?.source??`runs/speed-ceiling/clearance/short-stroke-cadence-${speed*1000}`,rows=fs.existsSync(`${d}/validation-cases.json`)?read(`${d}/validation-cases.json`).rows:[];fs.mkdirSync(run,{recursive:true});
for(const [baseName,kind,duration,step,actionsSource,implicitOverrides] of spec?.profiles??[
 ['human-5ms','human',20,.005,'braked-5ms'],['human-1p25ms','human',20,.00125,'braked-5ms'],
 ['human-0p625ms','human',20,.000625,'braked-5ms'],['sustained-5ms','sustained',60,.005,'sustained'],
 ['dropout-5ms','dropout',12,.005,'dropout']]){
 if(process.argv.length>3&&!process.argv.slice(3).includes(baseName))continue;
 const name=spec?`${spec.id}-${baseName}`:speed===.1?baseName:`v${speed*1000}-${baseName}`;
 const prefix=`${run}/${name}`;if(fs.existsSync(`${prefix}.scene.json`))throw Error(`refusing overwrite ${name}`);
 const scene=read(`${source}.scene.json`),c=read(`${source}.config.json`);
 scene.duration_s=duration;c.step_s=step;c.steps=Math.round(duration/step);c.report_every=Math.round(.02/step);
 if(implicitOverrides){const {newton,...other}=implicitOverrides;Object.assign(c.implicit,other);if(newton)Object.assign(c.implicit.newton,newton);}
 const index=scene.controller.inputs.findIndex(ch=>ch.name==='command.forward_speed');
 const actions=read(`${base}/${actionsSource}.actions.json`).map(row=>{row[index]*=speed/.065;return row;});
 if(actions.length!==duration/.02)throw Error('action duration mismatch');
 const row={name,kind,prefix,duration_s:duration,step_s:step,command_speed_m_s:speed,source,actions_source:`${base}/${actionsSource}.actions.json`,actions_source_sha256:sha(`${base}/${actionsSource}.actions.json`)};
 if(spec)row.family=spec.id;
 if(implicitOverrides)row.numerical_overrides=implicitOverrides;
 for(const [suffix,value] of [['scene',scene],['config',c],['actions',actions]]){fs.writeFileSync(`${prefix}.${suffix}.json`,JSON.stringify(value)+'\n');row[`${suffix}_sha256`]=sha(`${prefix}.${suffix}.json`);}
 rows.push(row);
}
fs.writeFileSync(`${d}/validation-cases.json`,JSON.stringify({version:1,rows},null,2)+'\n');
