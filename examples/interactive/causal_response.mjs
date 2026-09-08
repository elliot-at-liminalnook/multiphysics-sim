// Measurements of paired recorded trajectories; no physics or controller code.
import assert from 'node:assert/strict';

export function firstSustainedResponse(samples, {start_s,end_s,threshold,hold_s}) {
  assert([start_s,end_s,threshold,hold_s].every(Number.isFinite));
  assert(start_s>=0&&end_s>start_s&&threshold>0&&hold_s>0);
  assert(samples.every((s,i)=>Number.isFinite(s.time_s)&&Number.isFinite(s.value)
    &&(!i||s.time_s>samples[i-1].time_s)));
  for(let i=0;i<samples.length;i++) {
    const first=samples[i];
    if(first.time_s<start_s||first.time_s>end_s||first.value<threshold)continue;
    let last=i;
    while(last+1<samples.length&&samples[last+1].time_s<=end_s&&samples[last+1].value>=threshold)last++;
    const confirm=samples.findIndex((s,j)=>j>=i&&j<=last&&s.time_s-first.time_s>=hold_s-1e-10);
    if(confirm>=0)return {index:i,time_s:first.time_s,latency_s:first.time_s-start_s,
      previous_sample_time_s:i?samples[i-1].time_s:null,confirmation_time_s:samples[confirm].time_s,
      last_contiguous_time_s:samples[last].time_s,value:first.value};
    i=last;
  }
  return null;
}

export function pairedBodySignal(commanded,counterfactual,{body_link,metric,direction,start_s}) {
  assert(commanded.length===counterfactual.length&&['translation','yaw'].includes(metric)&&[1,-1].includes(direction));
  const pose=f=>{const p=f.poses.find(p=>p.name===body_link);assert(p);return p;};
  const initial=commanded.find(f=>Math.abs(f.time_s-start_s)<1e-10);assert(initial,'branch must be on the sample grid');
  const yaw=p=>Math.atan2(p.rotation[1][0],p.rotation[0][0]);
  const heading=yaw(pose(initial)),axis=[Math.cos(heading),Math.sin(heading)];
  return commanded.map((a,i)=>{
    const b=counterfactual[i];assert.equal(a.time_s,b.time_s);
    const p=pose(a),q=pose(b),delta=p.position_m.map((v,j)=>v-q.position_m[j]);
    const dyaw=Math.atan2(Math.sin(yaw(p)-yaw(q)),Math.cos(yaw(p)-yaw(q)));
    return {time_s:a.time_s,value:direction*(metric==='yaw'?dyaw:delta[0]*axis[0]+delta[1]*axis[1]),
      horizontal_difference_m:Math.hypot(delta[0],delta[1]),yaw_difference_rad:dyaw};
  });
}

export function mapResponseToBrowser(response,timeline,commandStage,samples,threshold) {
  const command=timeline.commands.find(c=>c.stage===commandStage);assert(command);
  if(!response)return null;
  const receive=timeline.received.find(f=>f.time_s===response.time_s);assert(receive);
  const values=new Map(samples.map(s=>[s.time_s,s.value]));
  const drawn=timeline.drawn.find(f=>f.time_s>=response.time_s&&f.time_s<=response.last_contiguous_time_s
    &&values.get(f.time_s)>=threshold&&f.submitted_at_ms>=command.issued_at_ms);
  const received_s=(receive.received_at_ms-command.issued_at_ms)/1000;assert(received_s>=0);
  if(drawn)assert(drawn.submitted_at_ms>=receive.received_at_ms);
  return {received_wall_latency_s:received_s,drawn_wall_latency_s:drawn?(drawn.submitted_at_ms-command.issued_at_ms)/1000:null,
    drawn_physics_time_s:drawn?.time_s??null,drawn_frame:drawn?.frame??null,
    commanded_native_signal_at_draw:drawn?values.get(drawn.time_s):null};
}
