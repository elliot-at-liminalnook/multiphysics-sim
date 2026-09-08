import {test} from 'node:test';
import assert from 'node:assert/strict';
import {firstSustainedResponse,pairedBodySignal,mapResponseToBrowser} from './causal_response.mjs';
const budget={start_s:0.1,end_s:0.5,threshold:0.0001,hold_s:0.1};
test('sampled detection rejects transient and wrong-way motion and reports no response honestly',()=>{
  const samples=Array.from({length:26},(_,i)=>({time_s:i*0.02,value:i===7?0.001:i>=12?0.0002:-0.001}));
  const r=firstSustainedResponse(samples,budget);
  assert.equal(r.time_s,0.24);assert.equal(r.confirmation_time_s,0.34);
  assert(Math.abs(r.latency_s-0.14)<1e-12);
  assert.equal(firstSustainedResponse(samples,{...budget,end_s:0.3}),null);
  assert.equal(firstSustainedResponse(samples.map(s=>({...s,value:-1})),budget),null);
  const timeline={commands:[{stage:0,issued_at_ms:1000}],
    received:[{time_s:0.24,received_at_ms:1250}],
    drawn:[{frame:5,time_s:0.22,submitted_at_ms:1260},{frame:6,time_s:0.26,submitted_at_ms:1290}]};
  const mapped=mapResponseToBrowser(r,timeline,0,samples,budget.threshold);
  assert.equal(mapped.received_wall_latency_s,0.25);assert.equal(mapped.drawn_wall_latency_s,0.29);
  assert.equal(mapped.drawn_physics_time_s,0.26);
});
test('body-forward projection and yaw wrap use actual poses, independent of policy references',()=>{
  const frame=(time,x,y,yaw)=>({time_s:time,poses:[{name:'body',position_m:[x,y,0],
    rotation:[[Math.cos(yaw),-Math.sin(yaw),0],[Math.sin(yaw),Math.cos(yaw),0],[0,0,1]]}]});
  const a=[frame(0,0,0,Math.PI/2),frame(0.02,0,0.001,Math.PI/2)];
  const b=[a[0],frame(0.02,0,0,Math.PI/2)];
  const p={body_link:'body',metric:'translation',direction:1,start_s:0};
  assert.equal(pairedBodySignal(a,b,p)[1].value,0.001);
  a[1]=frame(0.02,0,0,-Math.PI+0.001);b[1]=frame(0.02,0,0,Math.PI-0.001);
  assert(Math.abs(pairedBodySignal(a,b,{...p,metric:'yaw'})[1].value-0.002)<1e-12);
});
