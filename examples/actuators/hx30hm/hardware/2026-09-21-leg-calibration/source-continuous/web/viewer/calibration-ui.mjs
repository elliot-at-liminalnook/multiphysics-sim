import {ActuatorMotionView} from './actuator-motion-view.mjs';
// Operator interface only. Rust validates commands; the FPGA gates actual drive.
const token=document.querySelector('meta[name="calibration-token"]')?.content;
if(token){
 const style=document.createElement('style');style.textContent=`
 #fixture-panel{position:fixed;right:14px;top:70px;width:360px;max-height:calc(100vh - 90px);overflow:auto;background:#14212a;color:#ecf4f6;border:1px solid #52636b;border-radius:14px;box-shadow:0 12px 60px #0009;padding:18px;z-index:80;font:14px/1.45 system-ui}#fixture-panel h2{margin:0 0 8px;font-size:21px}#fixture-panel h3{font-size:14px;margin:16px 0 8px}#fixture-panel p{margin:8px 0}#fixture-panel button,#fixture-panel select,#fixture-panel input[type=number]{font:inherit;color:inherit;background:#283b47;border:1px solid #6a7d87;border-radius:6px;padding:9px;cursor:pointer}#fixture-panel button:disabled{opacity:.4;cursor:default}#fixture-panel .row{display:flex;gap:8px;margin:8px 0;align-items:center;flex-wrap:wrap}#fixture-panel .small{font-size:12px;color:#b0c1c9}#fixture-stop{position:sticky;top:0;z-index:2;background:#922d33!important;width:100%;font-weight:700}#fixture-position{font:20px ui-monospace,monospace;padding:10px 0}#fixture-status{border-left:3px solid #80d8b6;padding-left:9px}#fixture-bounds{white-space:pre-line;background:#0d1820;padding:10px;border-radius:6px}#fixture-panel label{display:block;margin:10px 0}#fixture-toggle{margin-left:12px;padding:8px;border-radius:6px;background:#2b564b;color:white;border:1px solid #82c6b6}#fixture-panel[hidden]{display:none}@media(max-width:700px){#fixture-panel{right:8px;left:8px;width:auto;top:65px}}`;
 document.head.append(style);
 const toggle=document.createElement('button');toggle.id='fixture-toggle';toggle.textContent='Leg calibration';document.querySelector('header')?.append(toggle);
 const panel=document.createElement('aside');panel.id='fixture-panel';panel.setAttribute('aria-label','Physical leg calibration');panel.innerHTML=`
 <h2>Leg fixture calibration</h2><p class="small">PHYSICAL MOTORS · independent of simulation</p>
 <button id="fixture-stop">STOP · torque off</button>
 <p id="fixture-status" role="status">Connecting to calibration service…</p>
 <div class="row"><button id="fixture-inspect">Read connected motors</button><button id="fixture-close" aria-label="Close calibration panel">×</button></div>
 <label>Motor <select id="fixture-axis"><option value="3">ID 3 · belt / hip</option><option value="2">ID 2 · worm gear</option><option value="1">ID 1 · knee</option></select></label>
 <div id="fixture-position">—</div><div id="fixture-telemetry" class="small">No physical readback yet</div>
 <label><input type="checkbox" id="fixture-supported"> Leg is clamped and supported with torque off; travel is clear and motor power cutoff is within reach.</label>
 <button id="fixture-enable" disabled>Enable this motor for teaching</button>
 <details id="fixture-bounds-editor"><summary id="fixture-bounds-summary">Saved poses / reset bounds</summary>
 <div id="fixture-bounds">No taught limits</div>
 <div class="row"><button id="fixture-lower" disabled>Set lower here</button><button id="fixture-upper" disabled>Set upper here</button><button id="fixture-reference" disabled>Set reference here</button></div>
 <div class="row"><button id="fixture-clear-lower" disabled>Clear lower</button><button id="fixture-clear-upper" disabled>Clear upper</button><button id="fixture-clear-both" disabled>Reset both bounds</button></div>
 <p class="small">Upper/lower name poses of the part, regardless of encoder order. Previous values stay in saved history. Teach with clearance before contact.</p></details>
 <h3>Continuous range sweep</h3>
 <button id="fixture-sweep-start" disabled>Start slow sweep</button>
 <div class="row"><label>Speed (counts/s)<br><input id="fixture-sweep-speed" type="number" required min="0.1" max="500" step="0.1" value="5" style="width:90px"></label>
 <label>PWM ceiling (%)<br><input id="fixture-sweep-pwm" type="number" required min="0" max="100" step="0.1" value="10" style="width:90px"></label></div>
 <div class="row"><button id="fixture-slower">½ speed</button><button id="fixture-faster">2× speed</button></div>
 <label>Turnaround clearance (counts) <input id="fixture-clearance" type="number" required min="4" max="2040" step="1" value="64" style="width:70px"></label>
 <p id="fixture-sweep-readout" class="small">Teach both poses to sweep. Each start begins at 5 counts/s.</p>
 <p class="small">Turns inside the taught poses. Adjust speed and PWM ceiling while running; Stop ends the sweep. Keep this tab active. Loaded-leg tracking is provisional.</p>
 <div id="fixture-motion"></div>
 <details id="fixture-manual"><summary>Manual steps and mounting direction</summary>
 <label>Drive PWM (%) <input id="fixture-drive" type="number" required min="0" max="100" step="0.1" value="10" inputmode="decimal" style="width:100px"></label>
 <label>Step (motor encoder counts) <input id="fixture-step" type="number" required min="1" max="4095" step="1" value="1" style="width:100px"></label>
 <label>Mounting direction <select id="fixture-direction"><option value="1">Encoder + moves toward upper pose</option><option value="-1">Encoder − moves toward upper pose</option></select></label>
 <div class="row"><button id="fixture-minus" disabled>Toward lower</button><button id="fixture-plus" disabled>Toward upper</button></div>
 <p class="small">Click for one step; hold to repeat after each verified stop. Release a hold to stop. Pulsed movement can coast past its target; use lower PWM for fine positioning. Set mounting direction before teaching limits. Four-count working margin; not a verified stopping distance.</p></details>
 <button id="fixture-export">Download calibration JSON</button><p id="fixture-save" class="small"></p>`;
 document.body.append(panel);
 const $=id=>document.getElementById('fixture-'+id),client=crypto.randomUUID();
 let state={},pending=0,sequence=0,stopped=false,applied=0,press=null,holdTimer=null,preview=null,ownedRun=null,heartbeatBusy=false,sweepInput={speed_counts_s:5,drive_pwm:100};
 const motion=new ActuatorMotionView($('motion'));
 const api=async(path,body)=>{
  const r=await fetch('/calibration/'+path,{method:body?'POST':'GET',headers:{'X-Control-Token':token,'X-Client-Id':client,'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,keepalive:['stop','halt'].includes(body?.action)});
  const v=await r.json();if(!r.ok)throw Error(v.error||'Request failed');return v;
 };
 const direction=a=>a.lower!=null&&a.upper!=null?(a.upper<a.lower?-1:1):(a.reverse?-1:1);
 function render(s){
  state=s;const id=Number($('axis').value),t=s.samples?.[id],a=s.calibration?.axes?.[id]??{};
  const running=!!s.sweep?.running;
  const enabled=s.enabled_id===id&&!pending&&!stopped&&!running,holding=!!press?.active;
  $('status').textContent=s.message||s.error||'Ready';
  $('position').textContent=t?`${t.position_raw} counts · ${(t.position_raw*360/4096).toFixed(2)}° motor`:'—';
  $('telemetry').textContent=t?`${t.voltage_v.toFixed(1)} V · ${t.temperature_c} °C · current ${t.current_raw} raw (uncalibrated)`:'No physical readback yet';
  for(const key of ['minus','plus'])$(key).disabled=!(enabled||(holding&&s.enabled_id===id&&!stopped));
  for(const key of ['lower','upper','reference'])$(key).disabled=!enabled||holding;
  $('enable').disabled=running||!!pending||holding||!s.connected||!$('supported').checked;
  $('axis').disabled=!!pending||s.enabled_id!=null;
  $('inspect').disabled=!!pending||s.enabled_id!=null;
  $('direction').value=String(direction(a));$('direction').disabled=!enabled||holding||a.lower!=null||a.upper!=null;
  $('drive').disabled=running||!!pending||holding;$('step').disabled=running||!!pending||holding;
  $('clearance').disabled=running||!!pending;
  $('sweep-start').disabled=!enabled||holding||a.lower==null||a.upper==null;
  for(const key of ['clear-lower','clear-upper','clear-both'])$(key).disabled=!!pending||!s.connected;
  const latest=s.sweep?.motor_id===id?s.sweep.latest:null;
  $('sweep-readout').textContent=latest?`${s.sweep.running?'Running':'Stopped'} · requested ${latest.requested_speed_counts_s.toFixed(1)}, measured ${latest.velocity_counts_s.toFixed(1)} counts/s · error ${(latest.target_raw-latest.position_raw).toFixed(1)} counts · PWM ${(latest.pwm/10).toFixed(1)}% · ${latest.half_cycles} traversals`:'Each start begins at 5 counts/s. Clearance is inside each taught pose.';
  let bounds=`Lower pose: ${a.lower??'not taught'}\nUpper pose: ${a.upper??'not taught'}\nReference: ${a.reference??'not taught'}`;
  if(a.lower!=null&&a.upper!=null){bounds+=`\nWorking encoder range: ${Math.min(a.lower,a.upper)+4} … ${Math.max(a.lower,a.upper)-4}`;if(t)bounds+=`\nPart travel: ${((t.position_raw-a.lower)/(a.upper-a.lower)*100).toFixed(1)}% (lower → upper)`;}
  motion.update({id,axis:a,telemetry:t,jog:s.last_jog,preview,sweep:s.sweep});
  $('bounds-summary').textContent=`Saved poses: lower ${a.lower??'—'} / upper ${a.upper??'—'} · edit/reset`;
  $('bounds').textContent=bounds;$('save').textContent=s.output?'Saved on this Mac: '+s.output:'';
 }
 function clearPress(){clearTimeout(holdTimer);press=null;}
 async function command(action,extra={}){
  const urgent=action==='stop'||action==='halt';if(pending&&!urgent)return false;
  if(action==='stop'||action==='clear'){ownedRun=null;clearPress();stopped=true;}
  if(action==='jog'){
   const id=Number($('axis').value),initial=state.samples?.[id]?.position_raw;
   preview={motor_id:id,start_position_raw:initial,target_position_raw:initial+extra.delta,requested_counts:extra.delta,samples:[{elapsed_ms:0,position_raw:initial}]};
  }
  const seq=++sequence;pending++;render(state);
  $('status').textContent=urgent?'Stopping and verifying…':action==='enable'?'Checking watchdogs at zero drive…':'Working…';
  let ok=false;
  try{
   const next=await api('command',{action,id:Number($('axis').value),sequence:seq,...extra});
   if(action==='enable'){stopped=false;sequence=0;applied=0;state=next;}
   else if(seq>=applied){applied=seq;state=next;}
   if(action==='sweep_start'&&next.sweep?.running&&!stopped)ownedRun=next.sweep.run_id;
   ok=true;
  }catch(e){clearPress();stopped=true;state={...state,message:e.message};}
  finally{if(action==='jog')preview=null;pending--;render(state);}
  return ok;
 }
 function jogArgs(partDirection){
  if(!$('drive').reportValidity()||!$('step').reportValidity())return null;
  const percent=$('drive').valueAsNumber,counts=$('step').valueAsNumber;
  if(!Number.isFinite(percent)||!Number.isInteger(counts))return null;
  const a=state.calibration?.axes?.[Number($('axis').value)]??{};
  return {delta:partDirection*direction(a)*counts,drive_pwm:Math.round(percent*10)};
 }
 async function repeatHeld(p){
  while(press===p&&p.active){
   if(!await command('jog',p.args)||stopped||state.enabled_id!==Number($('axis').value)){clearPress();break;}
   await new Promise(r=>setTimeout(r,120));
  }
 }
 function beginPress(partDirection,pointerId){
  if(press||pending||stopped||state.sweep?.running||state.enabled_id!==Number($('axis').value))return;
  const args=jogArgs(partDirection);if(!args)return;
  const p={args,pointerId,active:false};press=p;
  holdTimer=setTimeout(()=>{if(press===p){p.active=true;repeatHeld(p);}},300);
 }
 function endPress(tap=true){
  const p=press;if(!p)return;clearPress();
  if(p.active)command('halt');else if(tap)command('jog',p.args);
 }
 for(const [key,sign] of [['minus',-1],['plus',1]]){
  const button=$(key);button.style.touchAction='none';button.style.userSelect='none';
  button.addEventListener('pointerdown',e=>{if(e.button!==0)return;e.preventDefault();button.setPointerCapture(e.pointerId);beginPress(sign,e.pointerId)});
  button.addEventListener('pointerup',e=>{if(press?.pointerId===e.pointerId)endPress()});
  button.addEventListener('pointercancel',()=>endPress(false));button.addEventListener('lostpointercapture',()=>endPress(false));
  button.addEventListener('keydown',e=>{if(e.key===' '||e.key==='Enter'){e.preventDefault();if(!e.repeat)beginPress(sign,'key')}});
  button.addEventListener('keyup',e=>{if(e.key===' '||e.key==='Enter'){e.preventDefault();endPress()}});
  // Assistive activation without pointer/key events requests one bounded step.
  button.addEventListener('click',e=>{if(e.detail===0&&!press){const args=jogArgs(sign);if(args)command('jog',args)}});
 }
 $('inspect').onclick=()=>command('inspect');$('enable').onclick=()=>command('enable',{supported:$('supported').checked});$('stop').onclick=()=>command('stop');
 $('sweep-start').onclick=()=>{
  if(!$('sweep-pwm').reportValidity()||!$('clearance').reportValidity())return;
  $('sweep-speed').value='5';sweepInput={speed_counts_s:5,drive_pwm:Math.round($('sweep-pwm').valueAsNumber*10)};
  command('sweep_start',{drive_pwm:Math.round($('sweep-pwm').valueAsNumber*10),clearance_counts:$('clearance').valueAsNumber});
 };
 const commitSweepInput=()=>{if($('sweep-speed').checkValidity()&&$('sweep-pwm').checkValidity())sweepInput={speed_counts_s:$('sweep-speed').valueAsNumber,drive_pwm:Math.round($('sweep-pwm').valueAsNumber*10)}};
 $('sweep-speed').onchange=commitSweepInput;$('sweep-pwm').onchange=commitSweepInput;
 const scaleSpeed=f=>{$('sweep-speed').value=(Math.min(500,Math.max(.1,sweepInput.speed_counts_s*f))).toFixed(1);commitSweepInput()};
 $('slower').onclick=()=>scaleSpeed(.5);$('faster').onclick=()=>scaleSpeed(2);
 for(const boundary of ['lower','upper','both'])$('clear-'+boundary).onclick=()=>command('clear',{boundary});
 async function heartbeat(){
  if(ownedRun!=null&&!heartbeatBusy&&!stopped){
   const run=ownedRun;heartbeatBusy=true;
   try{
    await api('command',{action:'sweep_update',id:Number($('axis').value),run_id:run,sequence:++sequence,...sweepInput});
   }catch(e){if(ownedRun===run){ownedRun=null;command('stop');}}
   finally{heartbeatBusy=false;}
  }
  setTimeout(heartbeat,100);
 }heartbeat();
 $('direction').onchange=()=>command('direction',{reverse:$('direction').value==='-1'});
 for(const boundary of ['lower','upper','reference'])$(boundary).onclick=()=>command('capture',{boundary});
 $('axis').onchange=()=>render(state);$('supported').onchange=()=>render(state);
 function stopForLoss(){const active=press||ownedRun!=null||state.enabled_id!=null;ownedRun=null;clearPress();if(active)command('stop');}
 $('close').onclick=()=>{stopForLoss();panel.hidden=true};toggle.onclick=()=>{panel.hidden=!panel.hidden;if(panel.hidden)stopForLoss()};
 document.addEventListener('keydown',e=>{if(e.key==='Escape'&&state.enabled_id!=null){e.preventDefault();e.stopImmediatePropagation();stopForLoss()}},true);
 window.addEventListener('pagehide',stopForLoss);window.addEventListener('blur',stopForLoss);document.addEventListener('visibilitychange',()=>{if(document.hidden)stopForLoss()});
 $('export').onclick=async()=>{try{const c=await api('export');const u=URL.createObjectURL(new Blob([JSON.stringify(c,null,2)],{type:'application/json'}));const a=document.createElement('a');a.href=u;a.download='leg-calibration.json';a.click();URL.revokeObjectURL(u)}catch(e){$('status').textContent=e.message}};
 async function poll(){const pollingRun=ownedRun;try{const next=await api('status');if(!pending&&ownedRun===pollingRun){if(next.sweep?.run_id===ownedRun&&!next.sweep?.running)ownedRun=null;render(next)}}catch(e){if(ownedRun!=null)stopForLoss();clearPress();stopped=true;$('status').textContent=e.message;for(const k of ['minus','plus','lower','upper','reference'])$(k).disabled=true;}setTimeout(poll,state.sweep?.running?200:600)}poll();
}
