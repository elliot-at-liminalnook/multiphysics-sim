import {ActuatorMotionView} from './actuator-motion-view.mjs';
// UI supplies intent only; shared Rust feedback control owns motion and holding.
const token=document.querySelector('meta[name="calibration-token"]')?.content;
if(token){
 const style=document.createElement('style');style.textContent=`
 #fixture-panel{position:fixed;right:14px;top:66px;width:390px;max-height:calc(100vh - 88px);overflow:auto;background:#12232b;color:#edf6f5;border:1px solid #536c74;border-radius:16px;box-shadow:0 12px 60px #0009;padding:18px;z-index:80;font:14px/1.45 system-ui}#fixture-panel h2{margin:0;font-size:20px;white-space:nowrap}#fixture-panel p{margin:8px 0}#fixture-panel button,#fixture-panel input,#fixture-panel select{font:inherit;accent-color:#85dbc4}#fixture-panel button,#fixture-panel input[type=number],#fixture-panel select{color:inherit;background:#283e48;border:1px solid #6c838c;border-radius:8px;padding:10px;cursor:pointer}#fixture-panel button:disabled{opacity:.4;cursor:default}#fixture-panel .row{display:flex;gap:8px;margin:10px 0;align-items:center}#fixture-panel .row>*{flex:1;min-width:0}#fixture-panel .small{font-size:12px;color:#b5c9cf}#fixture-panel .motor[aria-pressed=true]{background:#285e50;border-color:#9febd4}#fixture-stop{background:#943c43!important;font-weight:750}#fixture-panel .top{position:sticky;top:-18px;background:#12232b;z-index:2;padding:10px 0}#fixture-panel .jog{font-size:19px;padding:15px;touch-action:none;user-select:none}#fixture-panel .jog[data-held=true]{background:#347363}#fixture-status{min-height:40px;border-left:3px solid #85dbc4;padding-left:10px}#fixture-panel label{display:block;margin:10px 0}#fixture-panel input[type=range]{width:100%}#fixture-panel kbd{font:inherit;border:1px solid #abc4ca;border-radius:5px;padding:1px 6px;margin-right:5px}#fixture-panel .pose{font-size:12px;display:block;color:#aaddcd}#fixture-position{font-size:27px;font-variant-numeric:tabular-nums;text-align:center}#fixture-dial{width:100%;height:68px}#fixture-panel details{margin-top:14px;border-top:1px solid #405761;padding-top:10px}#fixture-panel summary{cursor:pointer}#fixture-toggle{margin-left:12px;padding:8px;background:#285e50;color:white;border:1px solid #82c6b6;border-radius:6px}#fixture-panel[hidden]{display:none}@media(max-width:700px){#fixture-panel{left:8px;right:8px;width:auto}}`;
 document.head.append(style);
 const toggle=document.createElement('button');toggle.id='fixture-toggle';toggle.textContent='Leg calibration';document.querySelector('header')?.append(toggle);
 const panel=document.createElement('aside');panel.id='fixture-panel';panel.setAttribute('aria-label','Physical leg calibration');panel.innerHTML=`
 <div class="top"><div class="row"><h2>Leg calibration</h2><button id="fixture-stop"><kbd>Z</kbd> Stop</button><button id="fixture-close" style="flex:0" aria-label="Close calibration">×</button></div></div>
 <p class="small">Select a motor. Hold to move. Release to hold its pose.</p>
 <div class="row" role="group" aria-label="Choose motor"><button class="motor" data-id="1" aria-pressed="false">Knee <span class="small">1</span></button><button class="motor" data-id="2" aria-pressed="false">Worm <span class="small">2</span></button><button class="motor" data-id="3" aria-pressed="false">Belt <span class="small">3</span></button></div>
 <p id="fixture-status" role="status">Choose the motor you want to calibrate.</p>
 <div class="row"><button class="jog" id="fixture-plus" disabled><kbd>Q</kbd> Upper ↑</button><button class="jog" id="fixture-minus" disabled><kbd>A</kbd> Lower ↓</button></div>
 <label for="fixture-speed">Movement speed <span id="fixture-speed-value" class="small"></span></label><input id="fixture-speed" aria-label="Movement speed" type="range" min="0" max="100" value="0"><div class="row small"><span>Slow crawl</span><span style="text-align:right">Faster</span></div>
 <div class="row"><button id="fixture-lower" disabled>Save lower here<span id="fixture-lower-pose" class="pose">Not taught</span></button><button id="fixture-upper" disabled>Save upper here<span id="fixture-upper-pose" class="pose">Not taught</span></button></div>
 <p id="fixture-capture-message" class="small"></p>
 <div class="row"><button id="fixture-sweep" disabled>Try saved range</button><button id="fixture-reset" disabled>Reset poses</button></div>
 <button id="fixture-learn" disabled style="width:100%">Learn motion in the middle</button><p id="fixture-learning" class="small">Teach both poses, then learn stopping response at the desired speed.</p>
 <svg id="fixture-dial" viewBox="0 0 300 95" role="img" aria-label="Measured motor angle and requested angle"><path d="M 60 85 A 90 70 0 0 1 240 85" fill="none" stroke="#42616b" stroke-width="8"/><line id="fixture-target-needle" x1="150" y1="85" x2="150" y2="25" stroke="#7aafff" stroke-width="3" stroke-dasharray="5 4"/><line id="fixture-needle" x1="150" y1="85" x2="150" y2="20" stroke="#91edc9" stroke-width="5" stroke-linecap="round"/><text x="52" y="94" fill="#afc8cf" font-size="11">Lower</text><text x="218" y="94" fill="#afc8cf" font-size="11">Upper</text></svg>
 <div id="fixture-position">—</div><p id="fixture-angle-note" class="small" style="text-align:center">Continuous motor angle · zero is not a travel stop</p>
 <label for="fixture-target">Move to a taught pose <span id="fixture-target-value" class="small"></span></label><input id="fixture-target" aria-label="Target pose between saved limits" type="range" min="0" max="100" value="50" step="0.1" disabled>
 <p class="small">Green = measured · blue = requested. Teach poses before contact. Z disables torque; releasing Q/A keeps active hold. Changing tabs stops drive.</p>
 <details id="fixture-advanced"><summary>Advanced settings &amp; feedback</summary>
 <label>PWM ceiling (%) <input id="fixture-pwm" type="number" min="0" max="100" step="0.1" value="10" required style="width:85px"></label>
 <p class="small">The controller adjusts effort within this ceiling. Holding gains are provisional until tested on this loaded fixture.</p>
 <button id="fixture-flip">Swap upper / lower direction</button>
 <div class="row"><button id="fixture-clear-lower">Reset lower only</button><button id="fixture-clear-upper">Reset upper only</button></div>
 <label>Single raw step <input id="fixture-step" type="number" min="-4095" max="4095" step="1" value="1" required style="width:85px"></label><button id="fixture-step-send">Send raw step</button>
 <div id="fixture-telemetry" class="small"></div><div id="fixture-motion"></div><button id="fixture-export">Download calibration</button>
 </details>`;
 document.body.append(panel);
 const $=id=>document.getElementById('fixture-'+id),client=crypto.randomUUID(),motionView=new ActuatorMotionView($('motion'));
 let state={},id=null,ready=false,busy=false,sequence=0,run=null,starting=false,epoch=0,intent='hold',targetRaw=0,heartbeatBusy=false,sweeping=false,learning=false,keys=new Set(),pointer=null;
 const api=async(path,body)=>{const r=await fetch('/calibration/'+path,{method:body?'POST':'GET',headers:{'X-Control-Token':token,'X-Client-Id':client,'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,keepalive:body?.action==='stop'});const v=await r.json();if(!r.ok)throw Error(v.error||'Request failed');return v;};
 const send=(action,extra={})=>api('command',{action,id,sequence:++sequence,...extra});
 const axis=()=>state.calibration?.axes?.[id]??{};
 const speed=()=>5*Math.pow(100,Number($('speed').value)/100);
 const input=()=>({speed_counts_s:speed(),drive_pwm:Math.round(Number($('pwm').value)*10),motion:intent,target_raw:targetRaw});
 const position=t=>t?.position_continuous??t?.position_raw;
 const angle=n=>`${(n*360/4096).toFixed(1)}°`;
 const fraction=(raw,a)=>a.lower!=null&&a.upper!=null?(raw-a.lower)/(a.upper-a.lower):a.reverse?1-((raw%4096+4096)%4096)/4095:((raw%4096+4096)%4096)/4095;
 function needle(el,f){const a=Math.PI*(1-Math.max(0,Math.min(1,f)));el.setAttribute('x2',150+85*Math.cos(a));el.setAttribute('y2',85-65*Math.sin(a));}
 function outsidePose(){
  const a=axis(),p=position(state.samples?.[id]);if(p==null)return null;
  if(a.coordinate_session&&a.coordinate_session!==state.coordinate_session)return 'reference';
  const reverse=a.lower!=null&&a.upper!=null?a.upper<a.lower:!!a.reverse;
  if(a.upper!=null&&(reverse?p<a.upper:p>a.upper))return 'upper';
  if(a.lower!=null&&(reverse?p>a.lower:p<a.lower))return 'lower';return null;
 }
 function render(){
  const a=axis(),t=state.samples?.[id],latest=state.sweep?.motor_id===id?state.sweep.latest:null,active=ready&&!busy;
  for(const b of panel.querySelectorAll('.motor')){b.setAttribute('aria-pressed',String(Number(b.dataset.id)===id));b.disabled=busy;}
  $('status').textContent=busy?'Connecting and checking this motor at zero drive…':id==null?'Choose the motor you want to calibrate.':state.message||'Ready';
  const outside=outsidePose();
  if(outside&&ready&&!busy)$('status').textContent=outside==='reference'?'The multi-turn reference was lost. Re-teach both poses after reconnecting.':`The motor is beyond its saved ${outside} pose. Reset that pose to re-teach it.`;
  $('plus').disabled=$('minus').disabled=!active||!!outside;
  $('plus').dataset.held=String(intent==='upper'&&run!=null);$('minus').dataset.held=String(intent==='lower'&&run!=null);
  $('speed-value').textContent=`${(speed()*360/4096).toFixed(2)}°/s motor`;
  $('position').textContent=t?angle(position(t))+' motor':'—';
  if(t){needle($('needle'),fraction(position(t),a));needle($('target-needle'),fraction(latest?.target_raw??position(t),a));}
  const taught=a.lower!=null&&a.upper!=null;
  $('target').disabled=!active||!taught||!!outside;
  if(t&&taught&&document.activeElement!==$('target'))$('target').value=100*fraction(latest?.target_raw??position(t),a);
  $('target-value').textContent=taught?'lower → upper':'teach both poses first';
  for(const b of ['lower','upper']){$(b+'-pose').textContent=a[b]!=null?angle(a[b])+' motor':'Not taught';$(b).disabled=!active||sweeping||intent!=='hold'||(run!=null&&!latest?.holding);}
  $('sweep').disabled=!active||!taught||!!outside;$('sweep').textContent=sweeping||intent==='target'?'Pause & hold':'Try saved range';$('reset').disabled=!active;$('reset').textContent=outside==='reference'?'Re-teach both poses':outside?`Reset ${outside} pose`:'Reset poses';
  $('learn').disabled=!active||!taught||!!outside;$('learn').textContent=learning?'Pause learning & hold':'Learn motion in the middle';
  const adaptive=latest?.adaptation;
  $('learning').textContent=adaptive?`${adaptive.status}. Stops learned: ${adaptive.decreasing_stops} / ${adaptive.increasing_stops}. Allowed now: ${(adaptive.permitted_speed_counts_s*360/4096).toFixed(2)}°/s motor.`:'Teach both poses, set your desired speed, then learn in the middle. Starts at crawl; effort stays within your PWM ceiling.';
  if(adaptive?.learning_complete&&learning){learning=false;intent='hold';update();}
  $('capture-message').textContent=state.capture_message||'';
  $('telemetry').textContent=t?`${t.voltage_v.toFixed(1)} V · ${t.temperature_c} °C · encoder ${t.position_raw} · effort ${((latest?.pwm??0)/10).toFixed(1)}%`:'';
  motionView.update({id,axis:a,telemetry:t,sweep:state.sweep});
 }
 function clearInput(){keys.clear();pointer=null;intent='hold';}
 async function stop(){++epoch;clearInput();run=null;starting=false;sweeping=false;learning=false;ready=false;render();if(id==null)return;try{state=await send('stop');}catch(e){state.message=e.message;}render();}
 async function selectMotor(next){
  if(busy)return;const e=++epoch;clearInput();run=null;starting=false;sweeping=false;learning=false;ready=false;busy=true;id=next;render();
  try{const s=await send('select');if(e===epoch){state=s;ready=true;}}
  catch(err){if(e===epoch)state.message=err.message;}
  finally{busy=false;render();}
 }
 async function update(){
  if(run==null||heartbeatBusy||!ready)return;const r=run,e=epoch;heartbeatBusy=true;
  try{await send('motion_update',{run_id:r,...input()});}
  catch(err){if(e===epoch&&r===run){await stop();state.message=err.message;render();}}
  finally{heartbeatBusy=false;}
 }
 async function begin(){
  if(!ready||busy||starting)return;
  if(run!=null){await update();return;}
  if(!$('pwm').reportValidity())return;
  const e=epoch;starting=true;
  try{const s=await send('motion_start',input());if(e===epoch&&ready){state=s;run=s.sweep?.run_id??null;await update();}}
  catch(err){if(e===epoch){ready=false;state.message=err.message;}}
  finally{if(e===epoch)starting=false;render();}
 }
 function move(direction){if(!ready||busy||outsidePose())return;intent=direction;sweeping=false;learning=false;begin();render();}
 function release(){if(intent==='upper'||intent==='lower'){intent='hold';update();render();}}
 for(const b of panel.querySelectorAll('.motor'))b.onclick=()=>selectMotor(Number(b.dataset.id));
 for(const [name,direction] of [['plus','upper'],['minus','lower']]){
  const b=$(name);b.onpointerdown=e=>{if(e.button!==0||!ready)return;e.preventDefault();b.setPointerCapture(e.pointerId);pointer=e.pointerId;move(direction);};
  b.onpointerup=b.onpointercancel=b.onlostpointercapture=e=>{if(pointer===e.pointerId){pointer=null;release();}};
  b.onkeydown=e=>{if([' ','Enter'].includes(e.key)){e.preventDefault();if(!e.repeat)move(direction);}};
  b.onkeyup=e=>{if([' ','Enter'].includes(e.key)){e.preventDefault();release();}};
 }
 function editing(e){return e.target instanceof Element&&!!e.target.closest('input,textarea,select,[contenteditable=true]');}
 document.addEventListener('keydown',e=>{
  if(panel.hidden||e.ctrlKey||e.metaKey||e.altKey)return;const k=e.key.toLowerCase();
  if(k==='z'||k==='escape'){e.preventDefault();e.stopImmediatePropagation();stop();return;}
  if(!['q','a'].includes(k)||editing(e))return;
  e.preventDefault();e.stopImmediatePropagation();if(e.repeat||!ready)return;keys.add(k);if(keys.size>1){intent='hold';update();}else move(k==='q'?'upper':'lower');
 },true);
 document.addEventListener('keyup',e=>{const k=e.key.toLowerCase();if(!keys.has(k))return;e.preventDefault();e.stopImmediatePropagation();keys.delete(k);release();},true);
 $('stop').onclick=stop;
 $('speed').oninput=()=>{update();render();};$('pwm').onchange=()=>{if($('pwm').reportValidity())update();};
 $('target').oninput=()=>{const a=axis();if(a.lower==null||a.upper==null)return;const sign=Math.sign(a.upper-a.lower);targetRaw=a.lower+sign*4+(a.upper-a.lower-sign*8)*Number($('target').value)/100;intent='target';sweeping=false;learning=false;begin();};
 $('target').onchange=()=>update();
 for(const boundary of ['lower','upper'])$(boundary).onclick=async()=>{
  try{if(run!=null){await send('capture_hold',{boundary,run_id:run,...input(),motion:'hold'});}else{state=await send('capture',{boundary});}render();}
  catch(e){state.capture_message=e.message;render();}
 };
 async function reset(boundary){if(id==null||busy)return;await stop();try{state=await send('clear',{boundary});await selectMotor(id);}catch(e){state.message=e.message;render();}}
 $('reset').onclick=()=>reset(outsidePose()==='reference'?'both':outsidePose()||'both');for(const b of ['lower','upper'])$('clear-'+b).onclick=()=>reset(b);
 $('flip').onclick=async()=>{if(id==null||busy)return;await stop();try{await selectMotor(id);state=await send('flip');render();}catch(e){state.message=e.message;render();}};
 $('sweep').onclick=async()=>{
  learning=false;
  if(sweeping||intent==='target'){intent='hold';sweeping=false;await update();render();return;}
  // Reuse the same energized session when changing intent; no off/on kick.
  if(run==null){intent='hold';await begin();}
  if(run!=null){intent='sweep';sweeping=true;$('speed').value='0';await update();render();}
 };
 $('learn').onclick=async()=>{
  if(learning){intent='hold';learning=false;await update();render();return;}
  if(run==null){intent='hold';await begin();}
  if(run!=null){intent='learn';learning=true;sweeping=false;await update();render();}
 };
 $('step-send').onclick=async()=>{if(!$('step').reportValidity()||!$('pwm').reportValidity()||!Number($('step').value)||id==null)return;await stop();await selectMotor(id);try{state=await send('jog',{delta:Number($('step').value),drive_pwm:input().drive_pwm});ready=state.enabled_id===id;render();}catch(e){state.message=e.message;render();}};
 function loss(){if(ready||starting||run!=null)stop();}
 window.addEventListener('blur',loss);window.addEventListener('pagehide',loss);document.addEventListener('visibilitychange',()=>{if(document.hidden)loss();});
 $('close').onclick=()=>{loss();panel.hidden=true;};toggle.onclick=()=>{panel.hidden=!panel.hidden;if(panel.hidden)loss();};
 $('export').onclick=async()=>{const c=await api('export'),u=URL.createObjectURL(new Blob([JSON.stringify(c,null,2)],{type:'application/json'}));const a=document.createElement('a');a.href=u;a.download='leg-calibration.json';a.click();URL.revokeObjectURL(u);};
 async function heartbeat(){await update();setTimeout(heartbeat,100);}heartbeat();
 async function poll(){const e=epoch;try{const s=await api('status');if(e===epoch&&!busy&&!starting){state=s;if(s.enabled_id!==id&&!run)ready=false;if(run!=null&&s.sweep?.run_id===run&&intent==='target'&&s.sweep.latest?.holding){intent='hold';update();}if(run!=null&&s.sweep?.run_id===run&&!s.sweep.running){run=null;ready=false;clearInput();sweeping=false;}render();}}catch(e){if(ready)await stop();state.message=e.message;render();}setTimeout(poll,run!=null?150:600);}poll();render();
}
