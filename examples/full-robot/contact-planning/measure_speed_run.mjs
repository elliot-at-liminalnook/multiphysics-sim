// Reduce recorded Rust physics only; no controller or dynamics implementation.
import assert from 'node:assert/strict';
export function measureSpeedRun(capture) {
  assert(capture.error===null && capture.frames.length>=2);
  const scene=capture.recording.scene, frames=capture.frames;
  const index=scene.robot.links.findIndex(l=>l.name==='Robot | Chassis and hip mounts');
  assert(index>=0);
  const link=scene.robot.links[index],hull=link.collision.hull,floor=scene.robot.world.floor_z;
  assert(hull.length>0&&Number.isFinite(floor));
  const body=f=>{const b=f.poses.find(p=>p.name===link.name);assert(b);return b;};
  let minimumUp=Infinity,minimumClearance=Infinity,groundContact=false;
  for(const f of frames){
    const b=body(f),up=b.rotation[2][2];
    assert(Number.isFinite(up)&&b.position_m.every(Number.isFinite));
    minimumUp=Math.min(minimumUp,up);
    for(const p of hull){
      const z=b.position_m[2]+b.rotation[2].reduce((sum,r,i)=>sum+r*p[i],0);
      minimumClearance=Math.min(minimumClearance,z-floor);
    }
    groundContact ||= f.contacts.some(c=>c.link===index&&c.other===null&&c.force_n[2]>0);
  }
  const a=body(frames[0]),b=body(frames.at(-1));
  const displacement=b.position_m.slice(0,2).map((v,i)=>v-a.position_m[i]);
  const elapsed=frames.at(-1).time_s-frames[0].time_s;
  assert(elapsed>0);
  const fallen=minimumUp<=0||minimumClearance<=0||groundContact;
  // Use the full trial duration even for an early fall. No distance is invented
  // for unexecuted time, and a fallen trial cannot win feasible speed selection.
  const duration=scene.duration_s;
  assert(duration>0&&elapsed<=duration+1e-8);
  return {net_distance_m:Math.hypot(...displacement),displacement_xy_m:displacement,
    elapsed_s:elapsed,requested_duration_s:duration,
    speed_m_s:Math.hypot(...displacement)/duration,
    minimum_chassis_up_z:minimumUp,minimum_chassis_ground_clearance_m:minimumClearance,
    chassis_floor_contact:groundContact,fallen,completed:capture.completed,
    scope:'Net horizontal chassis displacement divided by the full trial duration. Falling means chassis ground contact or overturning. CAD hull clearance and orientation are sampled at recorded frames; no slip, tracking, lift, steering or stop threshold.'};
}
