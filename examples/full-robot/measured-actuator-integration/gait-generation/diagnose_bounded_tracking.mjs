// Diagnostic summaries only; all measurements come from saved Rust captures.
import fs from 'node:fs';
import crypto from 'node:crypto';
const [capturePath,out]=process.argv.slice(2);
if(!out)throw Error('diagnose_bounded_tracking.mjs CAPTURE NEW_REPORT');
const c=JSON.parse(fs.readFileSync(capturePath)),obs=c.task.observations,q=180/Math.PI;
const motors=c.recording.config.motors.target_coordinates.map((coordinate,k)=>{
 const i=obs.findIndex(o=>o.source.kind==='coordinate_position'&&o.source.coordinate===coordinate);
 const fi=obs.findIndex(o=>o.name===`foot.${Math.floor(k/3)}.force.z`);
 if(i<0||fi<0)throw Error('Missing physical joint or foot force observation');
 const rows=c.frames.map((f,j)=>({time:f.time_s,error:(f.servo_targets_rad[k]-c.transitions[j].observations[i])*q,force:c.transitions[j].observations[fi]})).filter(r=>r.time>=1);
 const mean=rows.reduce((s,r)=>s+r.error,0)/rows.length,sd=Math.sqrt(rows.reduce((s,r)=>s+(r.error-mean)**2,0)/rows.length);
 const subset=rows=>({samples:rows.length,rms_degrees:rows.length?Math.sqrt(rows.reduce((s,r)=>s+r.error*r.error,0)/rows.length):null});
 return{coordinate,bias_degrees:mean,variation_rms_degrees:sd,loaded:subset(rows.filter(r=>r.force>1)),unloaded:subset(rows.filter(r=>r.force<=1))};
});
fs.writeFileSync(out,JSON.stringify({scope:'Read-only diagnostic; loaded means modeled foot force > 1 N, not a real measurement.',capture_sha256:crypto.createHash('sha256').update(fs.readFileSync(capturePath)).digest('hex'),motors},null,2)+'\n',{flag:'wx'});
