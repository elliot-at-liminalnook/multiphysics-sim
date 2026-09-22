// Present sampled geometry from the shared Rust audit; no collision math here.
import fs from 'node:fs';
import crypto from 'node:crypto';
const root=process.argv[2]??'examples/full-robot/measured-actuator-integration/gait-generation/selected-10s';
const capturePath=process.argv[3]??`${root}/replay-capture.json`;
const auditPath=`${root}/geometry-audit.json`;
const read=p=>JSON.parse(fs.readFileSync(p));
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const g=read(auditPath),c=read(capturePath);
if(g.frames.length!==c.frames.length||g.frames.some((f,i)=>f.time_s!==c.frames[i].time_s)||
   g.source.cad_sha256!==c.recording.scene.robot.source.cad_sha256)throw Error('Audit/capture identity or sample grid mismatch');
const names=c.recording.config.policy.task_observations.markers.map(m=>m.link);
if(new Set(names).size!==names.length)throw Error('Ambiguous foot links');
const feet=names.map(link=>{
  const h=g.frames.map(f=>f.floor_clearances.find(x=>x.link===link)?.minimum_clearance_m);
  if(h.some(v=>!Number.isFinite(v)))throw Error(`Missing foot geometry: ${link}`);
  return {link,minimum_clearance_m:Math.min(...h),maximum_clearance_m:Math.max(...h),
    samples_above_5mm:h.filter(v=>v>0.005).length,samples:h.length};
});
let worst=null;
for(const f of g.frames)for(const p of f.inter_link_penetrations)
  if(!worst||p.penetration_m>worst.penetration_m)
    worst={time_s:f.time_s,link:g.link_names[p.link],other:g.link_names[p.other],penetration_m:p.penetration_m};
const result={
  scope:'Shared Rust compiled-geometry audit at saved poses only. Five-millimetre lift counts are descriptive, not an acceptance threshold. No continuous collision or hardware-clearance certificate.',
  capture:{path:capturePath,sha256:sha(capturePath)},audit:{path:auditPath,sha256:sha(auditPath),recorded_capture_blake3:g.capture_blake3},
  completed:c.completed,frame_count:g.frames.length,sample_period_s:c.task.period_s,
  feet,worst_sampled_overlap:worst,
};
fs.writeFileSync(`${root}/geometry-summary.json`,JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(result,null,2));
