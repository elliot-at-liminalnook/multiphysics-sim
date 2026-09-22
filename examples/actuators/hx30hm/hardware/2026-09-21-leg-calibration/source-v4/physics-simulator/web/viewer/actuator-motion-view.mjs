// Read-only measured-motion view. No kinematics, plant model, or actuator commands.
export function motionComparison(axis, jog) {
 const sign=axis.lower!=null&&axis.upper!=null?(axis.upper<axis.lower?-1:1):(axis.reverse?-1:1);
 if(!jog)return {sign,agreement:'No measured step yet'};
 const actual=jog.actual_counts;
 return {sign,requested:jog.requested_counts*sign,actual:actual==null?null:actual*sign,
  agreement:actual==null?'Awaiting measured reply':actual===0?'No measured movement':Math.sign(actual)===Math.sign(jog.requested_counts)?'Measured direction matches command':'DIRECTION MISMATCH — check mapping'};
}
export class ActuatorMotionView {
 constructor(container){
  this.root=container;this.receipts=new Map();
  container.innerHTML='<h3>Command vs real motion</h3><div data-motion-direction></div><svg data-motion-chart viewBox="0 0 320 160" role="img" aria-label="Requested and measured encoder motion" style="width:100%;background:#0d1820;border-radius:8px;margin-top:8px"></svg><div data-motion-agreement role="status"></div><p class="small">Blue: requested · green: encoder readback. Updates after each step; gaps between samples are not measured. Part direction uses your mounting setting. The 3D robot is not yet bound to these encoder readings.</p>';
 }
 update({id,axis,telemetry,jog,preview}){
  if(jog?.motor_id!=null)this.receipts.set(jog.motor_id,jog);
  const record=preview?.motor_id===id?preview:this.receipts.get(id),c=motionComparison(axis,record);
  this.root.querySelector('[data-motion-direction]').textContent=`Toward upper ↑ = encoder ${c.sign>0?'+':'−'} · toward lower ↓ = encoder ${c.sign>0?'−':'+'}`;
  const status=this.root.querySelector('[data-motion-agreement]');
  status.textContent=c.agreement+(record?` · requested ${c.requested>0?'↑':'↓'} ${Math.abs(c.requested)} counts${c.actual==null?'':` · measured ${c.actual>0?'↑':c.actual<0?'↓':'—'} ${Math.abs(c.actual)} counts`}`:'');
  status.style.color=c.agreement.startsWith('DIRECTION')?'#ff9191':'#b0e8d3';
  const svg=this.root.querySelector('svg');svg.replaceChildren();
  const add=(tag,attrs,text)=>{const e=document.createElementNS('http://www.w3.org/2000/svg',tag);for(const [k,v]of Object.entries(attrs))e.setAttribute(k,String(v));if(text!=null)e.textContent=text;svg.append(e);return e};
  const label=(x,y,t,color='#b0c1c9')=>add('text',{x,y,fill:color,'font-size':11},t);
  if(!record){label(16,60,telemetry?`Encoder: ${telemetry.position_raw} counts`:'Waiting for encoder readback');label(16,82,'Take one step to compare command and motion.');return;}
  const points=record.samples??[],origin=record.start_position_raw;
  const values=[0,record.requested_counts,...points.map(p=>p.position_raw-origin)],lo=Math.min(...values),hi=Math.max(...values),span=Math.max(1,hi-lo),margin=span*.15;
  const end=Math.max(1,...points.map(p=>p.elapsed_ms)),x=t=>38+266*t/end,y=v=>127-100*(v-lo+margin)/(span+2*margin);
  add('line',{x1:38,x2:304,y1:y(0),y2:y(0),stroke:'#50616c'});
  add('line',{x1:38,x2:304,y1:y(record.requested_counts),y2:y(record.requested_counts),stroke:'#79aaff','stroke-width':2,'stroke-dasharray':'6 4'});
  if(points.length){add('polyline',{points:points.map(p=>`${x(p.elapsed_ms)},${y(p.position_raw-origin)}`).join(' '),fill:'none',stroke:'#71e3ba','stroke-width':2});for(const p of points)add('circle',{cx:x(p.elapsed_ms),cy:y(p.position_raw-origin),r:3,fill:'#71e3ba'});}
  label(8,16,'Motor encoder change (counts)');label(5,y(hi)+4,String(hi));label(5,y(lo)+4,String(lo));label(38,149,'0 ms');label(247,149,`${end} ms`);
 }
}
