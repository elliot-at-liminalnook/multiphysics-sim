// Select an existing Rust-audited pose for read-only CAD inspection.
// No kinematics, pose interpolation, distance calculation or dynamics here.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const [scenePath,auditPath,outputPath]=process.argv.slice(2);
assert(scenePath&&auditPath&&outputPath,'usage: scene audit output');
const scene=JSON.parse(fs.readFileSync(scenePath)),audit=JSON.parse(fs.readFileSync(auditPath));
assert.deepEqual(audit.link_names,scene.robot.links.map(l=>l.name),'exported link order differs');
const frame=audit.geometry.reduce((a,b)=>a.maximum_inter_link_penetration_m>=b.maximum_inter_link_penetration_m?a:b);
assert(frame.maximum_inter_link_penetration_m>0,'no overlap witness to inspect');
assert.deepEqual(frame.poses.map(p=>p.name),audit.link_names,'complete authoritative poses required');
const contacts=frame.inter_link_penetrations;
assert.equal(Math.max(...contacts.map(p=>p.penetration_m)),frame.maximum_inter_link_penetration_m);
const identity=path=>{const b=fs.readFileSync(path);return {path,bytes:b.length,sha256:crypto.createHash('sha256').update(b).digest('hex')};};
const output={source:scene.robot.source,completed:true,error:null,
  kind:'planned_geometry_probe',
  provenance:{scene:identity(scenePath),audit:identity(auditPath),converter:identity(import.meta.filename)},
  frames:[{time_s:frame.time_s,poses:frame.poses,internal_contacts:contacts}],
  scope:'Completed export of one planned geometry sample from the shared Rust embedding. This is not a simulated runtime rollout, controller capture, speed measurement or collision-free path claim. Poses and contact witnesses are copied without interpolation or recomputation.'};
fs.writeFileSync(outputPath,JSON.stringify(output,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({output:outputPath,time_s:frame.time_s,maximum_overlap_m:frame.maximum_inter_link_penetration_m,pairs:contacts.map(c=>[audit.link_names[c.link],audit.link_names[c.other]])}));
