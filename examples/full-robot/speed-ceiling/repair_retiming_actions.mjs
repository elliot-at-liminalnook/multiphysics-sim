// Correct only floating-point overshoot of explicitly declared input bounds.
// Executed rejections remain immutable; unexecuted inputs retain their original.
import fs from 'node:fs';import assert from 'node:assert/strict';import crypto from 'node:crypto';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p));
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const catalog=read(`${d}/validation-cases.json`),changes=[];
for(const blend of ['0p5','0p8'])for(const dynamic of [false,true]) {
  const name=dynamic?`redistributed-dynamic-${blend}-v175-scale1-human-fine`:`redistributed-${blend}-v175-human-fine`;
  const old=catalog.rows.find(r=>r.name===name);assert(old);
  const prefix=old.prefix,scene=read(`${prefix}.scene.json`),actions=read(`${prefix}.actions.json`);
  const index=scene.controller.inputs.findIndex(c=>c.name==='command.forward_speed'),channel=scene.controller.inputs[index];
  let maximum=0,count=0;
  for(const action of actions) {
    const value=action[index],next=Math.max(channel.lower,Math.min(channel.upper,value));
    assert(Math.abs(next-value)<=1e-15,'only roundoff may be repaired');
    maximum=Math.max(maximum,Math.abs(next-value));if(next!==value)count++;action[index]=next;
    assert(action.every((v,i)=>Number.isFinite(v)&&v>=scene.controller.inputs[i].lower&&v<=scene.controller.inputs[i].upper));
  }
  assert(count>0);const before=hash(`${prefix}.actions.json`);
  let row;
  if(dynamic) {
    assert(!fs.existsSync(`${prefix}.native.json`),'unexecuted input correction only');
    fs.copyFileSync(`${prefix}.actions.json`,`${prefix}.actions-before-roundoff.json`,fs.constants.COPYFILE_EXCL);
    row=old;
  } else {
    const rejected=read(`${prefix}.native.json`);assert(!rejected.completed&&rejected.error==='policy inputs must match declared count, units and bounds');
    row={...old,name:name.replace('-human-fine','-bounded-inputs-human-fine'),prefix:prefix.replace('-human-fine','-bounded-inputs-human-fine'),rejected_input_trial:name};
    for(const suffix of ['scene','config'])fs.copyFileSync(`${prefix}.${suffix}.json`,`${row.prefix}.${suffix}.json`,fs.constants.COPYFILE_EXCL);
    catalog.rows.push(row);
  }
  fs.writeFileSync(`${row.prefix}.actions.json`,JSON.stringify(actions)+'\n',dynamic?{}:{flag:'wx'});
  row.actions_sha256=hash(`${row.prefix}.actions.json`);
  row.input_roundoff_correction={source_actions_sha256:before,changed_values:count,maximum_change_m_s:maximum,scope:'Exact declared speed endpoints; no physical/controller change. Rejected executed source remains preserved.'};
  changes.push({name:row.name,actions_path:`${row.prefix}.actions.json`,actions_sha256:row.actions_sha256,...row.input_roundoff_correction});
}
fs.writeFileSync(`${d}/validation-cases.json`,JSON.stringify(catalog,null,2)+'\n');
fs.writeFileSync(`${d}/retiming-input-corrections.json`,JSON.stringify({changes},null,2)+'\n',{flag:'wx'});
