// Offline statistics of verified controller state and sampled joint sensors.
// Adjacent-slope acceleration is a smooth-curve proxy, not the acceleration of
// the literal piecewise-linear command (which has derivative jumps at knots).
import fs from 'node:fs';import {spawnSync} from 'node:child_process';import crypto from 'node:crypto';
const d='examples/full-robot/speed-ceiling',read=p=>JSON.parse(fs.readFileSync(p)),bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
const report=[];const average=x=>x.reduce((a,b)=>a+b,0)/x.length;
function correlation(a,b){const ma=average(a),mb=average(b);let xy=0,xx=0,yy=0;for(let i=0;i<a.length;i++){const x=a[i]-ma,y=b[i]-mb;xy+=x*y;xx+=x*x;yy+=y*y;}return xx*yy>1e-20?xy/Math.sqrt(xx*yy):null;}
for(const name of process.argv.slice(2)){
 const def=read(`${d}/validation-cases.json`).rows.find(r=>r.name===name);if(!def)throw Error('known validation required');
 const capture=`${def.prefix}.native.json`,c=read(capture),p=c.recording.scene.controller.parameters;
 const replay=spawnSync(`${bin}/replay_policy_state`,[capture],{encoding:'utf8',maxBuffer:32*1024*1024});if(replay.status!==0)throw Error(replay.stderr);
 const states=JSON.parse(replay.stdout),byTime=new Map(c.frames.filter(f=>f.policy).map(f=>[f.policy.time_s,f.policy]));
 const motors=c.recording.config.motors.target_coordinates,markers=c.recording.config.policy.task_observations.markers;
 const n=p.samples.length-1,h=p.period_s/n,stats=motors.map(()=>({error:[],velocity:[],acceleration:[],loaded:[],unloaded:[]}));
 for(const s of states.samples.filter(s=>s.policy_time_s>=1.4&&s.policy_time_s<6)){
  const observation=byTime.get(s.policy_time_s).observations,cell=s.state.phase/h,i=Math.floor(cell),u=cell-i;
  const sample=(index,j)=>p.samples[(index+n)%n][j];
  for(const [j,name]of motors.entries()){
   const leg=Math.floor(j/3),q=sample(i,j)*(1-u)+sample(i+1,j)*u;
   const error=q-observation[name.replace(/^joint\./,'')+'.angle'];
   const velocity=s.state.rate*(sample(i+1,j)-sample(i,j))/h;
   const acc=k=>(sample(k+1,j)-2*sample(k,j)+sample(k-1,j))/(h*h);
   const acceleration=s.state.rate*s.state.rate*(acc(i)*(1-u)+acc(i+1)*u);
   const force=observation[`marker.${markers[leg].id}.floor_force_world.z`];
   if(![error,velocity,acceleration,force].every(Number.isFinite))throw Error('finite matching channels required');
   const out=stats[j];out.error.push(error);out.velocity.push(velocity);out.acceleration.push(acceleration);out[force>=1?'loaded':'unloaded'].push(error);
  }
 }
 report.push({name,capture_sha256:crypto.createHash('sha256').update(fs.readFileSync(capture)).digest('hex'),maximum_command_replay_error_rad:states.maximum_command_error_rad,
  motors:stats.map((s,i)=>({coordinate:motors[i],samples:s.error.length,mean_error_rad:average(s.error),rms_error_rad:Math.sqrt(average(s.error.map(v=>v*v))),
   loaded_mean_error_rad:s.loaded.length?average(s.loaded):null,unloaded_mean_error_rad:s.unloaded.length?average(s.unloaded):null,
   velocity_error_correlation:correlation(s.error,s.velocity),acceleration_proxy_error_correlation:correlation(s.error,s.acceleration)}))});
}
fs.writeFileSync(`${d}/reference-tracking-diagnosis.json`,JSON.stringify({report,scope:'Forward window [1.4,6) s only. Exact Rhai replay aligns reference phase with original policy observations. Correlations are diagnostics, not a fitted controller or proof of causation. Acceleration is a neighboring-slope proxy; continuous inverse feedforward requires a differentiable reference.'},null,2)+'\n');
console.log(report.map(r=>({name:r.name,motors:r.motors.filter(m=>m.coordinate.includes('Worm')).map(m=>({coordinate:m.coordinate,rms_error_rad:m.rms_error_rad,acceleration_correlation:m.acceleration_proxy_error_correlation}))})));
