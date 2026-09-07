// Browser UI acceptance around the real WASM worker, not a mocked renderer.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {resolve,dirname} from 'node:path';
import {chromium} from 'playwright';
const directory=resolve(process.argv[2]||'runs/interactive/viewer');
const reportPath=resolve(process.argv[3]||'runs/interactive/viewer-report.json');
await mkdir(dirname(reportPath),{recursive:true});
const server=spawn(process.execPath,['web/serve-viewer.mjs',directory,'0']);
const url=await new Promise((resolve,reject)=>{server.once('error',reject);server.stdout.on('data',chunk=>{const found=String(chunk).match(/http:\/\/127.0.0.1:\d+/);if(found)resolve(found[0]);});server.once('exit',code=>reject(new Error(`server exited ${code}`)));});
let browser;const errors=[];const checks=[];
try {
 browser=await chromium.launch({headless:true,...(process.env.CHROME_EXECUTABLE?{executablePath:process.env.CHROME_EXECUTABLE}:{})});
 const page=await browser.newPage({viewport:{width:1440,height:950}});page.on('pageerror',e=>errors.push(e.message));
 const ready=()=>page.locator('#overlay').waitFor({state:'hidden',timeout:30000});
 await page.goto(url);await ready();assert(await page.locator('canvas').isVisible());
 const transport=await page.locator('.transport').boundingBox();assert(transport.y+transport.height<=950,'desktop transport stays inside viewport');
 const catalog=JSON.parse(await readFile(resolve(directory,'catalog.json')));const recorded=catalog.presets.find(p=>p.mode==='recorded');
 if(recorded){
  assert.equal(await page.locator('#mode').textContent(),'RECORDED PHYSICS');
  assert.equal(await page.locator('#parts button').count(),29);
  await page.locator('#search').fill('Sliding foot crosshead');assert.equal(await page.locator('#parts button:visible').count(),4);
  await page.locator('#parts button:visible').first().click();assert(await page.locator('#fit-selected').isEnabled());await page.locator('#fit-selected').click();await page.locator('#fit').click();
  await page.locator('#timeline').fill('0.85');assert(parseFloat(await page.locator('#sim-time').textContent())>=.84);
  await page.locator('#play').click();await page.waitForFunction(()=>parseFloat(document.querySelector('#sim-time').textContent)>.9);await page.locator('#play').click();
  await page.locator('#contacts').uncheck();await page.locator('#contacts').check();checks.push('recorded robot selection, fit, scrubbing, playback and contact toggle');
 }
 if(catalog.presets.some(p=>p.id==='robot-lift-live')){
  await page.locator('#preset').selectOption('robot-lift-live');await ready();
  assert.equal(await page.locator('#parts button').count(),29);
  await page.locator('#play').click();await page.locator('#fit').click();
  await page.waitForFunction(()=>parseFloat(document.querySelector('#sim-time').textContent)>=.01);
  await page.locator('#play').click();await page.waitForFunction(()=>document.querySelector('#execution-state').textContent==='Paused');
  assert.match(await page.locator('#joint-readings').textContent(),/→/);
  const robotTime=await page.locator('#sim-time').textContent();const robotDownload=page.waitForEvent('download');await page.locator('#download').click();await robotDownload;
  await page.locator('#replay').click();await ready();assert.equal(await page.locator('#sim-time').textContent(),robotTime);
  await page.screenshot({path:reportPath.replace(/\.json$/,'.live-robot.png')});
  await page.locator('#reset').click();await ready();assert.equal(await page.locator('#sim-time').textContent(),'0.000 s');
  checks.push('live quadruped worker advance, camera fit, pause, recipe replay and reset');
 }
 for(const checkpointId of ['robot-landing-checkpoint','robot-forward-10mm','robot-forward-slow','robot-forward-solid-sign']){
  if(!catalog.presets.some(p=>p.id===checkpointId))continue;
  await page.locator('#preset').selectOption(checkpointId);await ready();
  assert.match(await page.locator('#motion-progress').textContent(),/Preparing motion/);
  await page.locator('#play').click();await page.waitForFunction(()=>parseFloat(document.querySelector('#sim-time').textContent)>=.02);
  await page.locator('#play').click();await page.waitForFunction(()=>document.querySelector('#execution-state').textContent==='Paused');
  assert.match(await page.locator('#motion-progress').textContent(),/Following the plan/);
  assert.match(await page.locator('#motion-progress').textContent(),/100 ms/);
  const before=await page.locator('#motion-progress').textContent();
  const saved=page.waitForEvent('download');await page.locator('#download').click();await saved;
  await page.locator('#replay').click();await ready();assert.equal(await page.locator('#motion-progress').textContent(),before);
  if(checkpointId==='robot-forward-10mm')assert.equal(await page.locator('#readiness').getAttribute('data-state'),'failed');
  if(checkpointId==='robot-forward-10mm')await page.screenshot({path:reportPath.replace(/\.json$/,'.forward-placement.png')});
  if(checkpointId==='robot-forward-slow'){
   assert.match(await page.locator('#readiness').textContent(),/internal hip contact and timestep sensitivity/);
   await page.screenshot({path:reportPath.replace(/\.json$/,'.forward-slow.png')});
  }
  if(checkpointId==='robot-forward-solid-sign'){
   assert.match(await page.locator('#readiness').textContent(),/false hip contact is removed/);
   await page.screenshot({path:reportPath.replace(/\.json$/,'.corrected-placement.png')});
  }
  checks.push(checkpointId+' motion checkpoint status and replay');
 }
 if(catalog.presets.some(p=>p.id==='robot-task-observations')){
  await page.locator('#preset').selectOption('robot-task-observations');await ready();
  await page.locator('#task-observation-details summary').click();
  assert.match(await page.locator('#task-observation-readings').textContent(),/Waiting/);
  await page.locator('#play').click();await page.waitForFunction(()=>parseFloat(document.querySelector('#sim-time').textContent)>=.02);
  await page.locator('#play').click();await page.waitForFunction(()=>document.querySelector('#execution-state').textContent==='Paused');
  const observed=await page.locator('#task-observation-readings').textContent();
  assert.match(observed,/Controller sample:/);assert.match(observed,/Gravity direction:/);assert.match(observed,/-Y-foot-surface/);assert.match(observed,/upward support/);assert(!observed.includes('NaN'));
  const saved=page.waitForEvent('download');await page.locator('#download').click();await saved;
  await page.locator('#replay').click();await ready();assert.equal(await page.locator('#task-observation-readings').textContent(),observed);
  await page.locator('#task-observation-panel').scrollIntoViewIfNeeded();
  await page.screenshot({path:reportPath.replace(/\.json$/,'.observations.png')});
  checks.push('full robot ideal body/foot observations, sample timestamp, force readout and identical replay');
 }
 for(const pointPreset of ['robot-point-feedback','robot-point-block-factor','robot-point-sample-reuse','robot-point-final-refresh','robot-point-analytic-positions','robot-drive-definition']){
  if(!catalog.presets.some(p=>p.id===pointPreset))continue;
  await page.locator('#preset').selectOption(pointPreset);await ready();
  await page.locator('#task-observation-details').evaluate(el=>el.open=true);
  assert.equal(await page.locator('#inputs input').count(),3);
  assert.equal(await page.locator('#inputs input').nth(2).inputValue(),'0.25');
  await page.locator('#play').click();await page.locator('#fit').click();
  await page.waitForFunction(()=>parseFloat(document.querySelector('#sim-time').textContent)>=.02);
  await page.locator('#play').click();await page.waitForFunction(()=>document.querySelector('#execution-state').textContent==='Paused');
  const observed=await page.locator('#task-observation-readings').textContent();
  assert.match(observed,/Foot \/ point position feedback/);assert.match(observed,/-Y-foot-surface world tracking/);
  assert.match(observed,/activation: 0%/);assert.match(observed,/Bounded point suggestion/);assert(!observed.includes('NaN'));
  const saved=page.waitForEvent('download');await page.locator('#download').click();await saved;
  await page.locator('#inputs input').nth(2).fill('0.1');
  await page.locator('#replay').click();await ready();
  assert.equal(await page.locator('#inputs input').nth(2).inputValue(),'0.25');
  assert.equal(await page.locator('#task-observation-readings').textContent(),observed);
  await page.locator('#task-observation-panel').scrollIntoViewIfNeeded();
  await page.screenshot({path:reportPath.replace(/\.json$/,'.'+pointPreset+'.png')});
  checks.push(pointPreset+' gain, world foot tracking, phase activation and replay');
 }
 if(catalog.presets.some(p=>p.id==='robot-body-feedback')){
  await page.locator('#preset').selectOption('robot-body-feedback');await ready();
  await page.locator('#task-observation-details').evaluate(el=>el.open=true);
  assert.equal(await page.locator('#inputs input').count(),2);
  assert.equal(await page.locator('#inputs input').nth(1).inputValue(),'0.25');
  await page.locator('#play').click();await page.locator('#fit').click();
  await page.waitForFunction(()=>parseFloat(document.querySelector('#sim-time').textContent)>=.02);
  await page.locator('#play').click();await page.waitForFunction(()=>document.querySelector('#execution-state').textContent==='Paused');
  const observed=await page.locator('#task-observation-readings').textContent();
  assert.match(observed,/Body position feedback/);assert.match(observed,/world target \(mm\)/);assert.match(observed,/actual \(mm\)/);assert.match(observed,/Bounded joint suggestion/);assert(!observed.includes('NaN'));
  const saved=page.waitForEvent('download');await page.locator('#download').click();await saved;
  await page.locator('#inputs input').nth(1).fill('0.1');
  await page.locator('#replay').click();await ready();
  assert.equal(await page.locator('#inputs input').nth(1).inputValue(),'0.25');
  assert.equal(await page.locator('#task-observation-readings').textContent(),observed);
  await page.locator('#task-observation-panel').scrollIntoViewIfNeeded();
  await page.screenshot({path:reportPath.replace(/\.json$/,'.body-feedback.png')});
  checks.push('body feedback gain, world target/actual diagnostics, support weights and replay');
 }
 if(catalog.presets.some(p=>p.id==='robot-feedback-0.5')){
  await page.locator('#preset').selectOption('robot-feedback-0.5');await ready();
  assert.equal(await page.locator('#parts button').count(),29);
  assert.equal(await page.locator('#inputs input').inputValue(),'0.5');
  await page.locator('#inputs input').fill('0.25');await page.locator('#play').click();
  await page.locator('#fit').click();await page.waitForFunction(()=>parseFloat(document.querySelector('#sim-time').textContent)>=.02);
  await page.locator('#play').click();await page.waitForFunction(()=>document.querySelector('#execution-state').textContent==='Paused');
  assert.equal(await page.locator('#readings-label').textContent(),'Plan → motor target → actual');
  const saved=page.waitForEvent('download');await page.locator('#download').click();const artifact=await saved;
  const recipe=JSON.parse(await readFile(await artifact.path()));
  assert.deepEqual(recipe.input_events,[{at_step:0,values:[.25]}]);
  const before=await page.locator('#joint-readings').textContent();
  await page.locator('#inputs input').fill('0.1');await page.locator('#replay').click();await ready();
  assert.equal(await page.locator('#inputs input').inputValue(),'0.25');
  assert.equal(await page.locator('#joint-readings').textContent(),before);
  await page.screenshot({path:reportPath.replace(/\.json$/,'.robot-policy.png')});
  checks.push('full robot Rhai gain control, responsive camera, plan/target/actual distinction and recorded-command replay');
 }
 await page.locator('#preset').selectOption('pendulum-live');await ready();assert.equal(await page.locator('#mode').textContent(),'LIVE · Rust / WASM');
 await page.locator('#inputs input').fill('0.6');await page.locator('#play').click();await page.waitForFunction(()=>parseFloat(document.querySelector('#sim-time').textContent)>=.1);
 await page.locator('#play').click();assert.match(await page.locator('#joint-readings').textContent(),/34\.4/);
 const download=page.waitForEvent('download');await page.locator('#download').click();const file=await download;const recording=JSON.parse(await readFile(await file.path()));assert(recording.actions.length>0);
 await page.locator('#replay').click();await ready();assert(Math.abs(parseFloat(await page.locator('#sim-time').textContent())-recording.actions.length*recording.scene.period_s)<.0006);
 checks.push('live Rust/Rhai input, movement, recording and input replay');
 await page.locator('#reset').click();await ready();assert.equal(await page.locator('#sim-time').textContent(),'0.000 s');
 await page.route('**/data/pendulum-live.json',route=>route.fulfill({status:503,body:'test failure'}));await page.locator('#reset').click();await page.locator('#overlay.error').waitFor();assert(await page.locator('#reset').isEnabled());
 await page.unroute('**/data/pendulum-live.json');await page.locator('#reset').click();await ready();checks.push('load error display and reset recovery');
 let releaseLoad;const heldLoad=new Promise(resolve=>{releaseLoad=resolve;});
 await page.route('**/data/pendulum-live.json',async route=>{await heldLoad;try{await route.continue();}catch{}});
 await page.locator('#reset').click();await page.locator('#cancel').waitFor({state:'visible'});await page.locator('#cancel').click();
 assert.match(await page.locator('#status').textContent(),/Cancelled/);releaseLoad();await page.unroute('**/data/pendulum-live.json');
 await page.locator('#reset').click();await ready();checks.push('cancel pending load and restart without stale results');
 for(const presetId of ['pendulum-embedded','pendulum-condensed','pendulum-drive-backlash']){
  if(!catalog.presets.some(p=>p.id===presetId))continue;
  await page.locator('#preset').selectOption(presetId);await ready();
  await page.locator('#play').click();await page.waitForFunction(()=>document.querySelector('#execution-state').textContent==='Experiment complete');
  assert.equal(await page.locator('#sim-time').textContent(),'0.020 s');assert.match(await page.locator('#joint-readings').textContent(),/11\.5/);
  const saved=page.waitForEvent('download');await page.locator('#download').click();const artifact=await saved;
  const recipe=JSON.parse(await readFile(await artifact.path()));assert.equal(recipe.kind,'embedded_session');assert.equal(recipe.completed_steps,80);
  if(presetId==='pendulum-condensed'){
   assert.equal(recipe.config.implicit.condense_auxiliary,true);
   assert.equal(recipe.config.implicit.color_auxiliary_jacobian,true);
   assert.equal(recipe.config.implicit.auxiliary_endpoint_correction_scale,true);
  }
  await page.locator('#replay').click();await ready();assert.equal(await page.locator('#sim-time').textContent(),'0.020 s');
  checks.push(`${presetId}: incremental servo preset execution, target readings and recipe replay`);
  await page.locator('#preset').selectOption('pendulum-live');await ready();
 }
 if(catalog.presets.some(p=>p.id==='pendulum-policy')){
  await page.locator('#preset').selectOption('pendulum-policy');await ready();
  assert.match(await page.locator('#input-help').textContent(),/ideal simulated joint state/);
  await page.locator('#inputs input').fill('0.6');await page.locator('#play').click();
  await page.waitForFunction(()=>parseFloat(document.querySelector('#sim-time').textContent)>=.1);
  await page.locator('#play').click();await page.waitForFunction(()=>document.querySelector('#execution-state').textContent==='Paused');
  assert.match(await page.locator('#joint-readings').textContent(),/34\.4/);
  const saved=page.waitForEvent('download');await page.locator('#download').click();
  const recipe=JSON.parse(await readFile(await (await saved).path()));
  assert.equal(recipe.version,3);assert.deepEqual(recipe.input_events,[{at_step:0,values:[.6]}]);
  const readings=await page.locator('#joint-readings').textContent();
  await page.locator('#inputs input').fill('-0.2');await page.locator('#replay').click();await ready();
  assert.equal(await page.locator('#inputs input').inputValue(),'0.6');
  assert.equal(await page.locator('#joint-readings').textContent(),readings);
  assert.equal(await page.locator('#sim-time').textContent(),`${(recipe.completed_steps*recipe.config.step_s).toFixed(3)} s`);
  await page.screenshot({path:reportPath.replace(/\.json$/,'.policy.png')});
  checks.push('sampled Rhai motor commands, recorded input events, exact visible replay and restored command slider');
 }
 if(catalog.presets.some(p=>p.id==='pendulum-policy')){
  const fixture=JSON.parse(await readFile(resolve(directory,catalog.presets.find(p=>p.id==='pendulum-policy').path)));
  fixture.scene.robot.source.cad_sha256='synthetic-motion-gate-test';
  fixture.config.motors.expected_cad_sha256='synthetic-motion-gate-test';
  fixture.config.motors.target_coordinates=['joint.pivot'];
  fixture.config.motors.target_trajectory={interpolation:'linear',keyframes:[{time_s:0,values:[.2]},{time_s:.4,values:[.2]}]};
  fixture.config.motion_gate={clock:{period_s:.002,duration_s:.4,guard_start_s:.02,guard_end_s:.3,qualification_s:.02,maximum_pause_s:.04},support_links:['pendulum'],minimum_upward_force_n:1,observation_source:'ideal_runtime_floor_force'};
  await page.route('**/data/pendulum-policy.json',route=>route.fulfill({contentType:'application/json',body:JSON.stringify(fixture)}));
  await page.locator('#preset').selectOption('pendulum-policy');await ready();await page.locator('#play').click();
  await page.locator('#overlay.error').waitFor({timeout:30000});
  assert.match(await page.locator('#status').textContent(),/timed out/);
  assert.match(await page.locator('#motion-progress').textContent(),/Support checkpoint timed out/);
  assert.equal(await page.locator('#sim-time').textContent(),'0.060 s');
  const saved=page.waitForEvent('download');await page.locator('#download').click();await saved;
  await page.locator('#replay').click();await page.locator('#overlay.error').waitFor({timeout:30000});
  assert.match(await page.locator('#status').textContent(),/timed out/);
  await page.unroute('**/data/pendulum-policy.json');await page.locator('#reset').click();await ready();
  assert(!(await page.locator('#motion-progress').isVisible()));
  checks.push('synthetic missing-support timeout is visible, replayable, and recoverable by reset');
 }
 for(const id of ['pendulum-environment','robot-teacher-environment','robot-effective-servo','robot-crawl-startup','robot-online-steps','robot-reversal-crawl','robot-terrain-contact','robot-residual-policy','robot-neural-teacher']) {
  if(!catalog.presets.some(p=>p.id===id))continue;
  await page.locator('#preset').selectOption(id);await ready();
  assert(await page.locator('#learning-progress').isVisible());
  assert.match(await page.locator('#input-help').textContent(),/20 ms/);
  if(id==='robot-neural-teacher'){
    assert.equal(await page.locator('#inputs input:disabled').count(),16);
    assert(!(await page.locator('#residual-inputs').isVisible()));
    await page.locator('#neural-residuals summary').click();
    assert.equal(await page.locator('#neural-residuals [data-target]').count(),12);
  }
  if(id==='robot-residual-policy'){
    assert.equal(await page.locator('#inputs input').count(),18);
    assert.equal(await page.locator('#inputs input:disabled').count(),4);
    await page.locator('#residual-inputs summary').click();
    assert.equal(await page.locator('#residual-inputs input').count(),12);
    await page.locator('#residual-inputs input').first().fill('0.001');
    assert.match(await page.locator('#residual-inputs label').first().textContent(),/0\.0010 rad/);
  }
  await page.locator('#step').click();await page.waitForFunction(()=>document.querySelector('#sim-time').textContent==='0.020 s');
  assert.equal(await page.locator('#sim-time').textContent(),'0.020 s');
  assert.match(await page.locator('#learning-progress').textContent(),/Last 20 ms score:/);
  const score=await page.locator('#learning-progress').textContent();
  const neuralText=id==='robot-neural-teacher'?await page.locator('#neural-residuals').textContent():null;
  if(neuralText){
    const outputs=[...neuralText.matchAll(/: (-?\d+\.\d+) rad/g)].map(m=>Number(m[1]));
    assert.equal(outputs.length,12);assert(outputs.some(v=>v!==0));assert(outputs.every(v=>Math.abs(v)<=.001));
  }
  const download=page.waitForEvent('download');await page.locator('#download').click();
  const recipe=JSON.parse(await readFile(await (await download).path()));
  assert.equal(recipe.kind,'sampled_environment_recording');assert.equal(recipe.runtime.completed_steps*recipe.runtime.config.step_s,0.02);
  await page.locator('#replay').click();await ready();
  assert.equal(await page.locator('#learning-progress').textContent(),score);
  if(neuralText){
    assert.equal(await page.locator('#neural-residuals').textContent(),neuralText);
    assert.equal(recipe.runtime.config.policy.neural_residual.outputs.length,12);
    checks.push('neural teacher: live bounded output readouts, policy artifact recording and exact visible replay');
  }
  if(id==='robot-residual-policy'){
    assert.equal(recipe.runtime.input_events[0].values[6],.001);
    assert.equal(Number(await page.locator('#residual-inputs input').first().inputValue()),.001);
    await page.locator('#clear-residuals').click();
    for(const slider of await page.locator('#residual-inputs input').all())assert.equal(Number(await slider.inputValue()),0);
    checks.push('teacher motor corrections: bounded sliders, readable precision, recorded action replay and clear');
  }
  await page.locator('#fit').click();
  await page.screenshot({path:reportPath.replace(/\.json$/,`.${id}.png`)});
  await page.locator('#reset').click();await ready();assert.equal(await page.locator('#sim-time').textContent(),'0.000 s');
  checks.push(id+': 50 Hz task score, held-action recording, visible replay and reset');
 }
 if(catalog.presets.some(p=>p.id==='robot-online-steps')) {
  await page.locator('#preset').selectOption('robot-online-steps');await ready();
  assert(await page.locator('#teleop').isVisible());
  await page.locator('#viewport').click({position:{x:20,y:200}});
  const speed=page.getByLabel('command.forward_speed',{exact:true}),turn=page.getByLabel('command.yaw_rate',{exact:true});
  await page.keyboard.down('w');assert(Number(await speed.inputValue())>0);
  await page.keyboard.down('a');assert(Number(await turn.inputValue())>0);
  await page.keyboard.up('w');assert.equal(Number(await speed.inputValue()),0);assert(Number(await turn.inputValue())>0);
  await page.keyboard.up('a');assert.equal(Number(await turn.inputValue()),0);
  await page.keyboard.down('s');await page.keyboard.down('d');assert(Number(await speed.inputValue())<0&&Number(await turn.inputValue())<0);
  await page.evaluate(()=>window.dispatchEvent(new Event('blur')));assert.equal(Number(await speed.inputValue()),0);assert.equal(Number(await turn.inputValue()),0);
  await page.keyboard.up('s');await page.keyboard.up('d');
  await page.locator('#search').fill('');await page.locator('#search').press('w');assert.equal(Number(await speed.inputValue()),0);
  await page.locator('#search').fill('');await page.locator('#viewport').click({position:{x:20,y:200}});
  await page.keyboard.down('w');await page.locator('#stop-motion').click();assert.equal(Number(await speed.inputValue()),0);await page.keyboard.up('w');
  checks.push('WASD requests, simultaneous move/turn, key release, focus loss, typing isolation and stop button');
 }
 await page.setViewportSize({width:500,height:900});assert(await page.locator('canvas').isVisible());assert(await page.locator('#inputs input').count());
 assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));checks.push('narrow layout without horizontal overflow; controller input remains available');
 await page.screenshot({path:reportPath.replace(/\.json$/,'.mobile.png'),fullPage:true});
 await page.setViewportSize({width:1440,height:950});if(recorded){await page.locator('#preset').selectOption(recorded.id);await ready();}
 await page.screenshot({path:reportPath.replace(/\.json$/,'.desktop.png')});assert.deepEqual(errors,[]);
 await mkdir(dirname(reportPath),{recursive:true});await writeFile(reportPath,JSON.stringify({passed:true,checks,recorded_robot_tested:Boolean(recorded),live_fixture_tested:true,page_errors:errors,scope:'Viewer UI checks. Native/WASM numerical parity is a separate runtime gate; Robot presets are checked when present; small fixtures do not establish robot controller accuracy.'},null,2));console.log(JSON.stringify({passed:true,checks}));
}finally{await browser?.close();server.kill();}
