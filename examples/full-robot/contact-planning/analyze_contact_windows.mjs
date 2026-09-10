// Attribute the existing loaded-material slip metric to command phases.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {recordedContactMotion} from '../../interactive/recorded_contact_motion.mjs';
const [specPath,output]=process.argv.slice(2);assert(specPath&&output);
const read=p=>JSON.parse(fs.readFileSync(p));
const spec=read(specPath),rows=[];
for(const prefix of spec.prefixes) {
  const file=prefix+'.native.json',bytes=fs.readFileSync(file),capture=JSON.parse(bytes);
  assert(capture.completed);
  const frames=capture.frames,body=f=>f.poses.find(p=>p.name.includes('Chassis'));
  const windows=spec.windows.map(w=>({...w,body_path_m:0,feet:{}}));
  const markers=capture.recording.config.policy.task_observations.markers;
  const feet=markers.map(m=>({link:m.link,index:capture.recording.scene.robot.links.findIndex(l=>l.name===m.link),loaded_path_m:0}));
  let previous=feet.map(f=>recordedContactMotion(frames[0],f.index,f.link)),bodyPath=0;
  for(let i=1;i<frames.length;i++) {
    const a=frames[i-1],b=frames[i],mid=(a.time_s+b.time_s)/2;
    const matches=windows.filter(w=>mid>=w.start_s&&mid<w.end_s);assert.equal(matches.length,1);
    const w=matches[0],distance=Math.hypot(...body(b).position_m.slice(0,2).map((x,j)=>x-body(a).position_m[j]));
    bodyPath+=distance;w.body_path_m+=distance;
    const current=feet.map(f=>recordedContactMotion(b,f.index,f.link));
    for(let j=0;j<feet.length;j++) {
      const increment=previous[j].force>=1&&current[j].force>=1?(b.time_s-a.time_s)*(previous[j].speed+current[j].speed)/2:0;
      feet[j].loaded_path_m+=increment;w.feet[feet[j].link]=(w.feet[feet[j].link]??0)+increment;
    }
    previous=current;
  }
  for(const f of feet)assert(Math.abs(windows.reduce((s,w)=>s+w.feet[f.link],0)-f.loaded_path_m)<1e-12);
  const worst=feet.reduce((a,b)=>a.loaded_path_m>b.loaded_path_m?a:b);
  rows.push({prefix,capture_sha256:crypto.createHash('sha256').update(bytes).digest('hex'),body_path_m:bodyPath,
    worst_foot:worst.link,slip_ratio:worst.loaded_path_m/bodyPath,
    windows:windows.map(w=>({...w,worst_foot_fraction_of_loaded_path:w.feet[worst.link]/worst.loaded_path_m}))});
}
fs.writeFileSync(output,JSON.stringify({spec,rows,scope:'Partition of the existing loaded-material path metric. Fractions identify when the worst foot slips; they do not prove causation or alter acceptance thresholds.'},null,2)+'\n',{flag:'wx'});
