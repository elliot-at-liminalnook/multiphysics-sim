import {ActuatorMotionView} from './actuator-motion-view.mjs';
import {LegMirror} from './calibration-mirror.mjs';
// UI supplies intent only; shared Rust feedback control owns motion and holding.
const token=document.querySelector('meta[name="calibration-token"]')?.content;
if(token){
 const style=document.createElement('style');style.textContent=`
 #fixture-panel{position:fixed;right:14px;top:66px;width:390px;max-height:calc(100vh - 88px);overflow:auto;background:#12232b;color:#edf6f5;border:1px solid #536c74;border-radius:16px;box-shadow:0 12px 60px #0009;padding:18px;z-index:80;font:14px/1.45 system-ui}#fixture-panel h2{margin:0;font-size:20px;white-space:nowrap}#fixture-panel p{margin:8px 0}#fixture-panel button,#fixture-panel input,#fixture-panel select{font:inherit;accent-color:#85dbc4}#fixture-panel button,#fixture-panel input[type=number],#fixture-panel select{color:inherit;background:#283e48;border:1px solid #6c838c;border-radius:8px;padding:10px;cursor:pointer}#fixture-panel button:disabled{opacity:.4;cursor:default}#fixture-panel .row{display:flex;gap:8px;margin:10px 0;align-items:center}#fixture-panel .row>*{flex:1;min-width:0}#fixture-panel .small{font-size:12px;color:#b5c9cf}#fixture-panel .motor[aria-pressed=true]{background:#285e50;border-color:#9febd4}#fixture-stop{background:#943c43!important;font-weight:750}#fixture-panel .motor.off{opacity:.5;text-decoration:line-through}#fixture-sequence:empty{display:none}#fixture-panel .top{position:sticky;top:-18px;background:#12232b;z-index:2;padding:10px 0}#fixture-panel .jog{font-size:19px;padding:15px;touch-action:none;user-select:none}#fixture-panel .jog[data-held=true]{background:#347363}#fixture-status{min-height:40px;border-left:3px solid #85dbc4;padding-left:10px}#fixture-panel label{display:block;margin:10px 0}#fixture-panel input[type=range]{width:100%}#fixture-panel kbd{font:inherit;border:1px solid #abc4ca;border-radius:5px;padding:1px 6px;margin-right:5px}#fixture-panel .pose{font-size:12px;display:block;color:#aaddcd}#fixture-position{font-size:27px;font-variant-numeric:tabular-nums;text-align:center}#fixture-dial{width:100%;height:68px}#fixture-panel details{margin-top:14px;border-top:1px solid #405761;padding-top:10px}#fixture-panel summary{cursor:pointer}#fixture-toggle{margin-left:12px;padding:8px;background:#285e50;color:white;border:1px solid #82c6b6;border-radius:6px}#fixture-panel[hidden]{display:none}@media(max-width:700px){#fixture-panel{left:8px;right:8px;width:auto}}`;
 document.head.append(style);
 const toggle=document.createElement('button');toggle.id='fixture-toggle';toggle.textContent='Leg calibration';document.querySelector('header')?.append(toggle);
 const panel=document.createElement('aside');panel.id='fixture-panel';panel.setAttribute('aria-label','Physical leg calibration');panel.innerHTML=`
 <div class="top"><div class="row"><h2>Leg calibration</h2><button id="fixture-stop"><kbd>Z</kbd> Stop</button><button id="fixture-close" style="flex:0" aria-label="Close calibration">×</button></div></div>
 <p class="small">Select a motor. Hold to move. Release to hold its pose.</p>
 <div class="row" role="group" aria-label="Choose motor"><button class="motor" data-id="1" aria-pressed="false">Knee <span class="small">1</span></button><button class="motor" data-id="2" aria-pressed="false">Worm <span class="small">2</span></button><button class="motor" data-id="3" aria-pressed="false">Belt <span class="small">3</span></button></div>
 <div class="row"><button id="fixture-disable" disabled>Disable this motor</button><button id="fixture-sweep-all">Sweep all enabled motors</button></div>
 <label class="small"><input type="checkbox" id="fixture-hold-others" checked> Hold the other enabled motors in place while one moves</label>
 <p id="fixture-sequence" class="small" role="status"></p>
 <p id="fixture-warnings" class="small" role="log" aria-label="Recent warnings" style="color:#ffd27a;white-space:pre-line"></p>
 <p id="fixture-status" role="status">Choose the motor you want to calibrate.</p>
 <div class="row"><button class="jog" id="fixture-plus" disabled><kbd>Q</kbd> Upper ↑</button><button class="jog" id="fixture-minus" disabled><kbd>A</kbd> Lower ↓</button></div>
 <label for="fixture-speed">Movement speed <span id="fixture-speed-value" class="small"></span></label><input id="fixture-speed" aria-label="Movement speed" type="range" min="0" max="100" value="0"><div class="row small"><span>Slow crawl</span><span style="text-align:right">Faster</span></div>
 <div class="row"><button id="fixture-lower" disabled>Save lower here<span id="fixture-lower-pose" class="pose">Not taught</span></button><button id="fixture-upper" disabled>Save upper here<span id="fixture-upper-pose" class="pose">Not taught</span></button></div>
 <button id="fixture-reference" disabled style="width:100%">Save sim alignment here<span id="fixture-reference-pose" class="pose">Not aligned</span></button>
 <p id="fixture-capture-message" class="small"></p>
 <div class="row"><button id="fixture-sweep" disabled>Try saved range</button><button id="fixture-reset" disabled>Reset poses</button></div>
 <button id="fixture-learn" disabled style="width:100%">Learn motion in the middle</button><p id="fixture-learning" class="small">Teach both poses, then learn stopping response at the desired speed.</p>
 <details id="fixture-tune-box" open><summary>Tune this motor</summary>
 <p class="small">Measures this motor's friction and response with short moves (±200 counts at most) and saves gains fitted to it. Takes about 20 seconds.</p>
 <label class="small"><input type="checkbox" id="fixture-tune-ok"> The motor is mid-travel with room to move both ways</label>
 <button id="fixture-tune" disabled style="width:100%">Tune this motor</button>
 <p id="fixture-tune-status" class="small" role="status"></p></details>
 <details id="fixture-campaign-box"><summary>Characterization campaign</summary>
 <p class="small">Staged tests on every enabled, tuned motor with both poses taught: slow sweeps, holds, braking, steps, servo steps, an effort ladder, all motors together, then repeats. Each test is rehearsed on the tuned model and stops on divergence, supply sag, heat, drift or travel. Results are fitted with uncertainty; nothing is promoted to CAD automatically.</p>
 <label class="small"><input type="checkbox" id="fixture-campaign-ok"> The leg is suspended with clear space around every joint</label>
 <div class="row"><button id="fixture-campaign">Run campaign</button><button id="fixture-campaign-resume">Resume</button></div>
 <p id="fixture-campaign-status" class="small" role="status" style="white-space:pre-line"></p></details>
 <details id="fixture-gait-box"><summary>Gait playback</summary>
 <p class="small">Plays a gait found by the gait search. <b>Sim only</b> animates the simulated robot. <b>Leg only</b> drives the real leg's aligned motors through the same controller, taught poses and FPGA window. <b>Both</b> runs both on one clock (the real leg shown in blue). Every consumer samples the gait with the same Rust code. ★ = found with the measured motor profiles.</p>
 <label>Gait <select id="fixture-gait" style="width:100%"></select></label>
 <div class="row" role="radiogroup" aria-label="Where to play"><label><input type="radio" name="fixture-gait-mode" value="sim" checked> Sim only</label><label><input type="radio" name="fixture-gait-mode" value="leg"> Leg only</label><label><input type="radio" name="fixture-gait-mode" value="both"> Both</label></div>
 <label for="fixture-gait-speed">Playback speed <span id="fixture-gait-speed-value" class="small"></span></label><input id="fixture-gait-speed" type="range" min="5" max="100" value="100">
 <label for="fixture-gait-effort">Leg effort <span id="fixture-gait-effort-value" class="small"></span></label><input id="fixture-gait-effort" type="range" min="10" max="100" value="50">
 <p class="small">Both run the gait through its own reference governor, the one the simulation used. Leg effort caps each real motor at that fraction of its measured speed and acceleration (accepted motor profiles). The belt/hip also stays at the campaign plan's belt limit. The PWM ceiling in Advanced still applies.</p>
 <label class="small"><input type="checkbox" id="fixture-gait-ok"> The leg is suspended with clear space around every joint (needed for Leg and Both)</label>
 <div class="row"><button id="fixture-gait-play">Play</button><button id="fixture-gait-stop">Stop</button></div>
 <p id="fixture-gait-status" class="small" role="status" style="white-space:pre-line"></p>
 <div id="fixture-gait-stats" class="small" style="overflow-x:auto"></div>
 <details id="fixture-gait-history"><summary class="small">Recent leg runs</summary><div id="fixture-gait-runs" class="small" style="overflow-x:auto"></div></details></details>
 <svg id="fixture-dial" viewBox="0 0 300 95" role="img" aria-label="Measured motor angle and requested angle"><path d="M 60 85 A 90 70 0 0 1 240 85" fill="none" stroke="#42616b" stroke-width="8"/><line id="fixture-target-needle" x1="150" y1="85" x2="150" y2="25" stroke="#7aafff" stroke-width="3" stroke-dasharray="5 4"/><line id="fixture-needle" x1="150" y1="85" x2="150" y2="20" stroke="#91edc9" stroke-width="5" stroke-linecap="round"/><text x="52" y="94" fill="#afc8cf" font-size="11">Lower</text><text x="218" y="94" fill="#afc8cf" font-size="11">Upper</text></svg>
 <div id="fixture-position">—</div><p id="fixture-angle-note" class="small" style="text-align:center">Continuous motor angle · zero is not a travel stop</p>
 <label for="fixture-target">Move to a taught pose <span id="fixture-target-value" class="small"></span></label><input id="fixture-target" aria-label="Target pose between saved limits" type="range" min="0" max="100" value="50" step="0.1" disabled>
 <p class="small">Green = measured · blue = requested. Teach poses before contact. Z disables torque; releasing Q/A keeps active hold. Changing tabs stops drive.</p>
 <details id="fixture-mirror" open></details>
 <details id="fixture-advanced"><summary>Advanced settings &amp; feedback</summary>
 <label>Control mode <select id="fixture-drive-mode" aria-label="Control mode"><option value="pwm">PWM · host feedback loop (default)</option><option value="servo_position">Servo position loop · experimental</option><option value="servo_speed">Servo speed loop · experimental</option></select></label>
 <p class="small">Servo modes use the servo's own fast loop; the host streams goals from the same reference, within the same saved poses. Applies when a motion session starts. Tuning always uses PWM.</p>
 <label>PWM ceiling (%) <input id="fixture-pwm" type="number" min="0" max="100" step="0.1" value="100" autocomplete="off" required style="width:85px"></label>
 <p class="small">The controller adjusts effort within this ceiling. Holding gains are provisional until tested on this loaded fixture.</p>
 <button id="fixture-flip">Swap upper / lower direction</button>
 <div class="row"><button id="fixture-clear-lower">Reset lower only</button><button id="fixture-clear-upper">Reset upper only</button></div>
 <label>Single raw step <input id="fixture-step" type="number" min="-4095" max="4095" step="1" value="1" required style="width:85px"></label><button id="fixture-step-send">Send raw step</button>
 <div id="fixture-telemetry" class="small"></div><div id="fixture-motion"></div><button id="fixture-export">Download calibration</button>
 </details>`;
 document.body.append(panel);
 const $=id=>document.getElementById('fixture-'+id),client=crypto.randomUUID(),motionView=new ActuatorMotionView($('motion'));
 let tuning=false,campaigning=false,gaits=[],gaitRun=null,gaitSampling=false,warnings=[],mirror=null,sweepAllRun=null,sequenceText='',state={},id=null,ready=false,busy=false,sequence=0,run=null,starting=false,epoch=0,intent='hold',targetRaw=0,heartbeatBusy=false,sweeping=false,learning=false,keys=new Set(),pointer=null;
 const api=async(path,body)=>{const r=await fetch('/calibration/'+path,{method:body?'POST':'GET',headers:{'X-Control-Token':token,'X-Client-Id':client,'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,keepalive:body?.action==='stop'});const v=await r.json();if(!r.ok)throw Error(v.error||'Request failed');return v;};
 const send=(action,extra={})=>api('command',{action,id,sequence:++sequence,...extra});
 const axis=()=>state.calibration?.axes?.[id]??{};
 const speed=()=>5*Math.pow((state.maximum_speed_counts_s??500)/5,Number($('speed').value)/100);
 const input=()=>({speed_counts_s:speed(),drive_pwm:Math.round(Number($('pwm').value)*10),motion:intent,target_raw:targetRaw,hold_others:$('hold-others').checked,drive_mode:$('drive-mode').value});
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
  for(const b of panel.querySelectorAll('.motor')){b.setAttribute('aria-pressed',String(Number(b.dataset.id)===id));b.disabled=busy;b.classList.toggle('off',!!state.calibration?.axes?.[b.dataset.id]?.disabled);b.title=state.calibration?.axes?.[b.dataset.id]?.disabled?'Disabled':'';}
  $('disable').disabled=id==null||busy||!!sweepAllRun;$('disable').textContent=a.disabled?'Enable this motor':'Disable this motor';
  $('sweep-all').textContent=sweepAllRun?'Stop sweeping all':'Sweep all enabled motors';$('sweep-all').disabled=busy&&!sweepAllRun;$('sequence').textContent=sequenceText;
  $('status').textContent=busy?'Connecting and checking this motor at zero drive…':id==null?'Choose the motor you want to calibrate.':state.message||'Ready';
  const outside=outsidePose();
  if(a.disabled&&!busy)$('status').textContent=`${a.role??'This motor'} is disabled. Enable it to move it.`;
  if(outside&&ready&&!busy)$('status').textContent=outside==='reference'?'Saved poses are from an earlier session and are ignored until re-taught; manual moves still work.':`Beyond the saved ${outside} pose. Move back inward freely; driving further out is blocked.`;
  $('plus').disabled=$('minus').disabled=!active;
  const fresh=[latest?.warnings??[],...Object.values(state.sweep?.axes??{}).map(x=>x?.warnings??[])].flat();
  for(const w of fresh){if(warnings[0]?.text!==w)warnings.unshift({text:w,at:new Date().toLocaleTimeString()});}
  warnings.length=Math.min(warnings.length,6);
  $('warnings').textContent=warnings.map(w=>`${w.at} · ${w.text}`).join('\n');
  $('plus').dataset.held=String(intent==='upper'&&run!=null);$('minus').dataset.held=String(intent==='lower'&&run!=null);
  const allowed=latest?.adaptation?.permitted_speed_counts_s;$('speed-value').textContent=`${(speed()*360/4096).toFixed(2)}°/s motor`+(run!=null&&intent!=='hold'&&allowed!=null&&allowed<speed()*0.95?` · limited to ${(allowed*360/4096).toFixed(2)}°/s here`:'');
  $('position').textContent=t?angle(position(t))+' motor':'—';
  if(t){needle($('needle'),fraction(position(t),a));needle($('target-needle'),fraction(latest?.target_raw??position(t),a));}
  const taught=a.lower!=null&&a.upper!=null;
  $('target').disabled=!active||!taught||outside==='reference';
  if(t&&taught&&document.activeElement!==$('target'))$('target').value=100*fraction(latest?.target_raw??position(t),a);
  $('target-value').textContent=taught?'lower → upper':'teach both poses first';
  for(const b of ['lower','upper','reference']){$(b+'-pose').textContent=a[b]!=null?angle(a[b])+' motor':b==='reference'?'Not aligned':'Not taught';$(b).disabled=!active||sweeping||intent!=='hold'||(run!=null&&!latest?.holding);}
  $('sweep').disabled=!active||!taught||outside==='reference';$('sweep').textContent=sweeping||intent==='target'?'Pause & hold':'Try saved range';$('reset').disabled=!active;$('reset').textContent=outside==='reference'?'Re-teach both poses':outside?`Reset ${outside} pose`:'Reset poses';
  $('learn').disabled=!active||!taught||outside==='reference';$('learn').textContent=learning?'Pause learning & hold':'Learn motion in the middle';
  const tuned=a.tuning,tn=state.tuning?.motor_id===id?state.tuning:null;
  $('tune').disabled=!ready||busy||tuning||!!sweepAllRun||!$('tune-ok').checked;$('tune').textContent=tuning?'Tuning…':'Tune this motor';
  $('tune-status').textContent=tn?.running?`Tuning: ${tn.stage}`:tn?.error?`Last tuning stopped: ${tn.error}`:tuned?`Tuned gains in use: kp ${tuned.pid.kp.toFixed(2)}, ki ${tuned.pid.ki.toFixed(2)}, kd ${tuned.pid.kd.toFixed(3)}, friction ${(tuned.friction_duty*100).toFixed(0)}% · ${tuned.record}`:'Using the shared provisional gains.';
  renderGait();
  const cp=state.campaign;
  for(const b of ['campaign','campaign-resume'])$(b).disabled=id==null||busy||tuning||campaigning||!!sweepAllRun||!$('campaign-ok').checked;
  $('campaign').textContent=campaigning?'Campaign running…':'Run campaign';
  $('campaign-status').textContent=cp?.running?`${cp.stage} · ${cp.completed} stage results saved${cp.last?.abort?` · last stopped by ${Object.keys(cp.last.abort)[0]}`:''}`:cp?.error?`Last campaign stopped: ${cp.error}`:cp?.result?`Finished: ${cp.result.headline}. ${cp.result.directory}`:'Tune each motor and teach both poses first.';
  const adaptive=latest?.adaptation;
  $('learning').textContent=adaptive?`${adaptive.status}. Stops learned: ${adaptive.decreasing_stops} / ${adaptive.increasing_stops}. Allowed now: ${(adaptive.permitted_speed_counts_s*360/4096).toFixed(2)}°/s motor.`:'Teach both poses, set your desired speed, then learn in the middle. Starts at crawl; effort stays within your PWM ceiling.';
  if(adaptive?.learning_complete&&learning){learning=false;intent='hold';update();}
  $('capture-message').textContent=state.capture_message||'';
  $('telemetry').textContent=t?`${t.voltage_v.toFixed(1)} V · ${t.temperature_c} °C · encoder ${t.position_raw} · effort ${((latest?.pwm??0)/10).toFixed(1)}%`:'';
  motionView.update({id,axis:a,telemetry:t,sweep:state.sweep});
  if(!mirror&&state.calibration?.axes){mirror=new LegMirror($('mirror'),Object.fromEntries(Object.entries(state.calibration.axes).map(([k,v])=>[k,v.role])),()=>!panel.hidden);mirror.begin();}
  mirror?.update(state);
 }
 function clearInput(){keys.clear();pointer=null;intent='hold';}
 async function stop(){if(gaitRun?.leg){if(gaitRun.heartbeat)clearInterval(gaitRun.heartbeat);gaitRun=null;mirror?.setGait(null);}sweepAllRun=null;++epoch;clearInput();run=null;starting=false;sweeping=false;learning=false;ready=false;render();if(id==null)return;try{state=await send('stop');}catch(e){state.message=e.message;}render();}
 async function selectMotor(next,holdAll=false,solo=false){
  if(state.calibration?.axes?.[next]?.disabled){if(busy)return;await stop();id=next;render();return;}
  if(busy)return;const e=++epoch;clearInput();run=null;starting=false;sweeping=false;learning=false;ready=false;busy=true;id=next;render();
  try{const s=await send('select',{hold_others:!solo&&(holdAll||$('hold-others').checked)});if(e===epoch){state=s;ready=true;}}
  catch(err){if(e===epoch)state.message=err.message;}
  finally{busy=false;render();}
 }
 async function update(){
  if(run==null||heartbeatBusy||!ready)return;const r=run,e=epoch;heartbeatBusy=true;
  try{await send('motion_update',{run_id:r,...input()});}
  catch(err){
   if(e!==epoch||r!==run)return;
   // A session that ended on the server (finished sweep, fault already
   // stopped and verified) is not a heartbeat failure: adopt its final state.
   const s=await api('status').catch(()=>null);
   if(s&&s.sweep?.run_id===r&&!s.sweep.running){state=s;run=null;ready=false;clearInput();sweeping=false;render();return;}
   await stop();state.message=err.message;render();
  }
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
 function move(direction){if(!ready||busy)return;intent=direction;sweeping=false;learning=false;begin();render();}
 function release(){if(intent==='upper'||intent==='lower'){intent='hold';update();render();}}
 for(const b of panel.querySelectorAll('.motor'))b.onclick=()=>{if(sweepAllRun){sequenceText='Sweep-all stopped: another motor was chosen.';}selectMotor(Number(b.dataset.id));};
 $('disable').onclick=async()=>{
  if(id==null||busy||sweepAllRun)return;const off=!axis().disabled;
  if(off)await stop();
  try{state=await send('set_disabled',{disabled:off});}catch(e){state.message=e.message;}render();
 };
 const sleep=ms=>new Promise(r=>setTimeout(r,ms));
 // Visit each enabled, fully taught motor in turn: one full lower↔upper traversal each.
 // Uses the same shared commands and safety envelope as Try saved range.
 // Sweep every enabled, fully taught motor together in one session (FPGA profile 6):
 // each traverses lower↔upper once, then holds until all are done.
 async function sweepAll(){
  if(sweepAllRun){sequenceText='Sweep-all stopped.';await stop();render();return;}
  const axes=state.calibration?.axes??{};
  const ids=Object.keys(axes).map(Number).filter(k=>!axes[k].disabled&&axes[k].lower!=null&&axes[k].upper!=null).sort();
  if(!ids.length){sequenceText='No enabled motor has both poses taught.';render();return;}
  const mine=sweepAllRun={};
  const fail=reason=>{if(sweepAllRun===mine){sweepAllRun=null;sequenceText='Sweep-all stopped: '+reason;render();}};
  sequenceText='Sweep-all: checking every enabled motor at zero drive…';render();
  await selectMotor(ids[0],true);
  if(sweepAllRun!==mine)return;
  if(!ready)return fail(state.message||'could not connect');
  intent='sweep';sweeping=true;
  try{const s=await send('sweep_all',input());if(sweepAllRun!==mine)return;state=s;run=s.sweep?.run_id??null;}
  catch(e){intent='hold';sweeping=false;return fail(e.message);}
  await update();
  const start={};
  while(sweepAllRun===mine){
   await sleep(250);
   const sw=state.sweep??{};
   const parts=(sw.motor_ids??[]).map(k=>{const h=sw.axes?.[k]?.half_cycles;if(h==null)return `${axes[k].role} starting`;start[k]??=h;return `${axes[k].role} ${Math.min(2,h-start[k])}/2 ends`;});
   sequenceText='Sweep-all: '+parts.join(' · ')+(sw.skipped?.length?' · skipped '+sw.skipped.join(', '):'');render();
   if(run==null)break;
  }
  if(sweepAllRun!==mine)return;
  sweepAllRun=null;intent='hold';sweeping=false;
  const err=state.sweep?.motion_error;
  sequenceText=err?'Sweep-all stopped: '+err:'Swept '+(state.sweep?.motor_ids??[]).map(k=>axes[k].role).join(', ')+' through their saved ranges.'+(state.sweep?.skipped?.length?' Skipped: '+state.sweep.skipped.join(', ')+'.':'');render();
 }
 $('sweep-all').onclick=sweepAll;
 $('tune-ok').onchange=render;
 // Remember operator preferences for this browser (not safety state).
 for(const [key,prop] of [['drive-mode','value'],['hold-others','checked']]){
  try{const v=localStorage.getItem('calibration-'+key);if(v!=null)$(key)[prop]=prop==='checked'?v==='true':v;}catch{}
  $(key).addEventListener('change',()=>{try{localStorage.setItem('calibration-'+key,String($(key)[prop]));}catch{}});
 }
 $('tune').onclick=async()=>{
  if(!$('tune-ok').checked||id==null||tuning)return;const target=id;
  await stop();await selectMotor(target,false,true);if(!ready||id!==target)return;
  tuning=true;render();
  try{state=await send('tune',{supported:true,drive_pwm:input().drive_pwm});}catch(e){state.message=e.message;tuning=false;render();return;}
  while(tuning){await sleep(300);if(!state.tuning?.running)tuning=false;render();}
  ready=false;$('tune-ok').checked=false;render();
 };
 // ---- Gait playback (shared Rust sampler; leg sessions go through the server) ----
 const gaitMode=()=>panel.querySelector('input[name="fixture-gait-mode"]:checked').value;
 const gaitScale=()=>Number($('gait-speed').value)/100;
 function renderGait(){
  const g=state.gait,legRunning=gaitRun?.leg&&g?.running;
  $('gait-speed-value').textContent=`${$('gait-speed').value}% of the gait's timing`;
  $('gait-effort-value').textContent=`${$('gait-effort').value}% of measured motor capability`;$('gait-effort').disabled=!!gaitRun?.leg;
  $('gait-play').textContent=!gaitRun?'Play':gaitRun.playing?'Pause':'Resume';
  $('gait-play').disabled=!gaits.length||(gaitMode()!=='sim'&&!gaitRun&&!$('gait-ok').checked)||campaigning||tuning;
  $('gait-stop').disabled=!gaitRun;
  for(const r of panel.querySelectorAll('input[name="fixture-gait-mode"]'))r.disabled=!!gaitRun;
  let text=gaits.length?'':'No gaits found yet.';
  if(gaitRun){
   text=`${gaitRun.mode==='sim'?'Sim only':gaitRun.mode==='leg'?'Leg only':'Sim + leg'} · gait time ${gaitRun.t.toFixed(2)} s of ${gaitRun.info.period_s.toFixed(2)} s period · ${((gaitRun.leg?state.gait?.speed_scale??gaitRun.scale:gaitRun.scale)*100).toFixed(0)}% speed`;
   if(gaitRun.leg&&g?.limits)text+=`\nLimits: ${Object.values(g.limits).map(l=>`${l.role} ≤ ${Math.round(l.governor_speed_counts_s)} counts/s, ${Math.round(l.governor_acceleration_counts_s2)} counts/s²`).join(' · ')}`;
   if(gaitRun.leg){text+=`\nLeg: ${g?.phase??'starting'}`+(g?.errors?` · error ${Object.entries(g.errors).map(([k,v])=>`${state.calibration?.axes?.[k]?.role??k} ${Math.round(v)}`).join(', ')} counts`:'')+(g?.clamped?` · ${g.clamped} targets clamped to taught poses`:'');if(gaitRun.skipped?.length)text+=`\nNot driven: ${gaitRun.skipped.join(', ')}`;}
  }else if(g?.error)text=`Last leg gait stopped: ${g.error}`;
  if(legRunning===false&&gaitRun?.leg&&gaitRun.started){endGait();}
  $('gait-status').textContent=text;
  const stats=g?.statistics&&Object.keys(g.statistics).length?g.statistics:null;
  $('gait-stats').innerHTML=stats?statsTable(stats):'';
  $('gait-runs').innerHTML=(state.gait_runs??[]).map(r=>`<div style="margin:6px 0"><b>${(r.gait??'').split('/').slice(-2,-1)[0]}</b> · effort ${Math.round((r.effort??0)*100)}% · speed ${Math.round((r.speed_scale??0)*100)}% · ${(r.gait_time_s??0).toFixed(1)} s · ${r.outcome}${statsTable(r.statistics??{})}</div>`).join('')||'No leg runs yet.';
 }
 function statsTable(st){
  const rows=Object.values(st);if(!rows.length)return '';
  const f=(v,d=1)=>v==null||!isFinite(v)?'—':Number(v).toFixed(d);
  return `<table style="border-collapse:collapse;width:100%"><tr><th align=left>Motor</th><th>RMS error</th><th>Peak</th><th>Sim RMS</th><th>Lag</th><th>Effort</th><th>At ceiling</th><th>Governed</th><th>Peak accel</th><th>Min V</th><th>Max °C</th></tr>`+
   rows.map(r=>`<tr><td>${r.role}</td><td align=center>${f(r.tracking_rms_deg,2)}°</td><td align=center>${f(r.tracking_peak_deg,1)}°</td><td align=center>${r.simulated_tracking_rms_counts==null?'—':f(r.simulated_tracking_rms_counts*360/4096,2)+'°'}</td><td align=center>${f(r.lag_s*1000,0)} ms</td><td align=center>${f(r.mean_effort*100,0)}%</td><td align=center>${f(r.saturated_fraction*100,0)}%</td><td align=center>${f(r.governor_limited_fraction*100,0)}%</td><td align=center>${f(r.peak_measured_acceleration_counts_s2,0)}</td><td align=center>${f(r.minimum_voltage_v,1)}</td><td align=center>${f(r.maximum_temperature_c,0)}</td></tr>`).join('')+'</table>';
 }
 async function loadGaits(){try{gaits=(await api('gaits')).gaits;$('gait').replaceChildren(...gaits.map((g,i)=>{const o=document.createElement('option');o.value=i;o.textContent=g.kind==='pose_sequence'?`Poses · ${g.study} · ${g.trial}`:`${g.measured_actuators?'★ ':''}${(g.speed_m_s??0).toFixed(3)} m/s · ${g.study.replace(/^gait-search-/,'')} · ${g.trial}`;if(g.summary)o.title=g.summary;return o;}));}catch(e){$('gait-status').textContent='Gait list unavailable: '+e.message;}render();}
 function endGait(){if(gaitRun?.heartbeat)clearInterval(gaitRun.heartbeat);gaitRun=null;mirror?.setGait(null);render();}
 async function gaitFrame(now){
  if(!gaitRun)return;
  if(gaitRun.leg){gaitRun.t=state.gait?.t??0;if(state.gait?.running)gaitRun.started=true;}
  else if(gaitRun.playing)gaitRun.t+=(now-gaitRun.last)/1000*gaitRun.scale;
  gaitRun.last=now;
  if(gaitRun.mode!=='leg'&&!gaitSampling){gaitSampling=true;try{const dt=Math.min(0.2,Math.max(0.001,(now-(gaitRun.lastSample??now))/1000));gaitRun.lastSample=now;const pose=await mirror.sampleGait(gaitRun.t,dt,gaitRun.leg?(state.gait?.speed_scale??gaitRun.scale):gaitRun.scale,!gaitRun.sampled);gaitRun.sampled=true;if(gaitRun)mirror.setGait(pose,gaitRun.mode==='both');}catch(e){$('gait-status').textContent='Gait sample failed: '+e.message;}finally{gaitSampling=false;}}
  renderGait();requestAnimationFrame(gaitFrame);
 }
 async function gaitPlay(){
  if(gaitRun){gaitRun.playing=!gaitRun.playing;if(gaitRun.leg)api('command',{action:'gait_update',speed_scale:gaitRun.scale,playing:gaitRun.playing}).catch(()=>{});render();return;}
  const g=gaits[$('gait').value];if(!g||!mirror)return;const mode=gaitMode();
  try{
   const info=await mirror.loadGait(await api('gait?path='+encodeURIComponent(g.path)),g.trial);
   const run={mode,info,t:0,last:performance.now(),playing:true,scale:gaitScale(),leg:mode!=='sim'};
   if(run.leg){
    const {bindings,skipped}=mirror.gaitBindings(state);run.skipped=skipped;
    if(!bindings.length)throw Error('No motor is aligned, taught and enabled: '+skipped.join(', '));
    await stop();await selectMotor(bindings[0].id,true);if(!ready)throw Error(state.message||'Could not enable the motors');
    state=await send('gait_start',{supported:true,gait:g.path,bindings,speed_scale:run.scale,effort:Number($('gait-effort').value)/100,drive_pwm:input().drive_pwm,drive_mode:$('drive-mode').value});
    run.heartbeat=setInterval(()=>{if(gaitRun===run)api('command',{action:'gait_update',speed_scale:run.scale,playing:run.playing}).catch(()=>{});},300);
    ready=false;
   }
   gaitRun=run;mirror.gaitRealLeg=mode==='both';requestAnimationFrame(gaitFrame);
  }catch(e){$('gait-status').textContent=e.message;gaitRun=null;}
  render();
 }
 $('gait-play').onclick=gaitPlay;
 $('gait-stop').onclick=async()=>{const leg=gaitRun?.leg;endGait();if(leg)await stop();};
 $('gait-speed').oninput=()=>{if(gaitRun){gaitRun.scale=gaitScale();if(gaitRun.leg)api('command',{action:'gait_update',speed_scale:gaitRun.scale,playing:gaitRun.playing}).catch(()=>{});}render();};
 $('gait-ok').onchange=render;$('gait-effort').oninput=render;for(const r of panel.querySelectorAll('input[name="fixture-gait-mode"]'))r.onchange=render;
 $('gait-box').addEventListener('toggle',()=>{if($('gait-box').open&&!gaits.length)loadGaits();});
 $('campaign-ok').onchange=render;
 async function campaign(resume){
  if(!$('campaign-ok').checked||id==null||campaigning)return;const target=id;
  await stop();await selectMotor(target,true);if(!ready||id!==target)return;
  campaigning=true;render();
  try{state=await send('campaign',{supported:true,resume});}catch(e){state.message=e.message;campaigning=false;render();return;}
  while(campaigning){await sleep(500);if(!state.campaign?.running)campaigning=false;render();}
  ready=false;$('campaign-ok').checked=false;render();
 }
 $('campaign').onclick=()=>campaign(false);$('campaign-resume').onclick=()=>campaign(true);
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
 for(const boundary of ['lower','upper','reference'])$(boundary).onclick=async()=>{
  try{const joint=boundary==='reference'?{reference_joint_rad:mirror?.alignmentAngle(id)}:{};if(run!=null){await send('capture_hold',{boundary,run_id:run,...input(),motion:'hold',...joint});}else{state=await send('capture',{boundary,...joint});}render();}
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
 window.addEventListener('pagehide',loss);document.addEventListener('visibilitychange',()=>{if(document.hidden)loss();});
 $('close').onclick=()=>{loss();panel.hidden=true;window.robotViewer?.end();};toggle.onclick=()=>{panel.hidden=!panel.hidden;if(panel.hidden){loss();window.robotViewer?.end();}else mirror?.begin();};
 $('export').onclick=async()=>{const c={...await api('export'),display_mirror:mirror?.record()},u=URL.createObjectURL(new Blob([JSON.stringify(c,null,2)],{type:'application/json'}));const a=document.createElement('a');a.href=u;a.download='leg-calibration.json';a.click();URL.revokeObjectURL(u);};
 async function heartbeat(){await update();setTimeout(heartbeat,100);}heartbeat();
 async function poll(){const e=epoch;try{const s=await api('status');if(e===epoch&&!busy&&!starting){state=s;if(s.enabled_id!==id&&!run)ready=false;if(run!=null&&s.sweep?.run_id===run&&intent==='target'&&s.sweep.latest?.holding){intent='hold';update();}if(run!=null&&s.sweep?.run_id===run&&!s.sweep.running){run=null;ready=false;clearInput();sweeping=false;}render();}}catch(e){if(ready)await stop();state.message=e.message;render();}setTimeout(poll,run!=null||gaitRun?.leg?150:600);}poll();render();
}
