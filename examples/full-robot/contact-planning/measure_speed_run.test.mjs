import test from 'node:test';
import assert from 'node:assert/strict';
import {measureSpeedRun} from './measure_speed_run.mjs';
function capture(){
  const name='Robot | Chassis and hip mounts';
  return {error:null,completed:true,recording:{scene:{duration_s:20,robot:{world:{floor_z:0},links:[{name,collision:{hull:[[0,0,-.1]]}}]}}},
    frames:[0,10,20].map(time_s=>({time_s,contacts:[],poses:[{name,position_m:[.12*time_s,.16*time_s,1],rotation:[[1,0,0],[0,1,0],[0,0,1]]}]}))};
}
test('net displacement includes acceleration time and ignores traveled loops',()=>{
  const c=capture();assert.equal(measureSpeedRun(c).speed_m_s,.2);
  c.frames[1].poses[0].position_m[0]=100;
  assert.equal(measureSpeedRun(c).speed_m_s,.2);
  assert.equal(measureSpeedRun(c).fallen,false);
});
test('leaning is allowed; ground contact and overturning are falls',()=>{
  const c=capture();c.frames[1].poses[0].rotation=[[.5,0,Math.sqrt(.75)],[0,1,0],[-Math.sqrt(.75),0,.5]];
  assert.equal(measureSpeedRun(c).fallen,false);
  c.frames[1].poses[0].rotation=[[0,0,1],[0,1,0],[-1,0,0]];
  assert.equal(measureSpeedRun(c).fallen,true);
  const b=capture();b.frames[1].poses[0].position_m[2]=.1;
  assert.equal(measureSpeedRun(b).fallen,true);
  const f=capture();f.frames[1].contacts=[{link:0,other:null,force_n:[0,0,1]}];
  assert.equal(measureSpeedRun(f).fallen,true);
});
test('early fall retains measured displacement and full requested duration',()=>{
  const c=capture();c.frames.pop();c.completed=false;c.frames[1].poses[0].rotation=[[-.1,0,Math.sqrt(.99)],[0,1,0],[-Math.sqrt(.99),0,-.1]];
  const m=measureSpeedRun(c);assert.equal(m.fallen,true);assert.equal(m.elapsed_s,10);assert.equal(m.speed_m_s,.1);
});
