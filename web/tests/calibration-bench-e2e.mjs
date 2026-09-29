// End-to-end calibration test with no hardware: the real calibration server and
// browser panel against the simulated bench (FPGA policy + three servo models)
// on a pseudo-terminal. Exercises select, held jogs, hold, tuning, the servo
// control modes, sweep-all and stop, and checks the physical traces it logs.
//
//   cargo build --release -p sim-runtime --example serve_actuator_calibration --example hx_virtual_bench
//   node web/tests/calibration-bench-e2e.mjs [report.json]
import {chromium} from 'playwright';
import {spawn} from 'node:child_process';
import {mkdtemp,readFile,writeFile,mkdir} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import assert from 'node:assert/strict';

const root=resolve(import.meta.dirname,'../..');
const bin=name=>join(root,'target/release/examples',name);
const reportPath=process.argv[2];
const work=await mkdtemp(join(tmpdir(),'calibration-bench-'));
const out=join(work,'measurements');await mkdir(out);
const children=[];const checks=[];
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
function start(cmd,args){const c=spawn(cmd,args,{stdio:['ignore','pipe','pipe']});children.push(c);return c;}
let browser;
try{
 // Simulated bench on a pty.
 const pathFile=join(work,'pty');const bench=start(bin('hx_virtual_bench'),[pathFile]);
 let pty;for(let i=0;i<100&&!pty;i++){await sleep(50);pty=await readFile(pathFile,'utf8').catch(()=>null);}
 assert(pty,'bench did not publish its serial path');
 // Server configuration: the fixture's settings, pointed at the bench and a scratch folder.
 const config=JSON.parse(await readFile(join(root,'examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/server.json'),'utf8'));
 config.serial=pty;config.output=out;config.viewer=join(root,'runs/interactive/calibration-mirror');
 // A short campaign (one level per stage) keeps the real-time run brief.
 const plan=JSON.parse(await readFile(join(root,'examples/actuators/hx30hm/hardware/characterization-campaign/plan.hardware.json'),'utf8'));
 Object.assign(plan,{a_speeds_counts_s:[80],b_positions:2,c_speeds_counts_s:[150],d_duties:[0.2,0.35],f_step_counts:0,g_duties:[0.3],k_repeat:false});
 await writeFile(join(work,'plan.json'),JSON.stringify(plan));config.campaign_plan=join(work,'plan.json');
 const axis=(role,lower,upper)=>({role,lower,upper,reference:Math.round((lower+upper)/2),reverse:false,coordinate_session:null});
 await writeFile(join(out,'calibration.json'),JSON.stringify({schema_version:1,fixture:'Simulated bench',provenance:'Test fixture',units:'counts',axes:{1:axis('knee',2872,3336),2:axis('worm',500,2500),3:axis('belt/hip',1200,1800)}},null,2));
 await writeFile(join(work,'server.json'),JSON.stringify(config));
 const port=4196;const server=start(bin('serve_actuator_calibration'),[join(work,'server.json'),String(port)]);
 let up=false;for(let i=0;i<100&&!up;i++){await sleep(100);up=await fetch(`http://127.0.0.1:${port}/`).then(r=>r.ok).catch(()=>false);}
 assert(up,'server did not start');
 browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_EXECUTABLE||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
 const page=await browser.newPage({viewport:{width:1500,height:950}});
 const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(`http://127.0.0.1:${port}/`);
 const el=id=>page.locator('#fixture-'+id);const motor=id=>page.locator(`.motor[data-id="${id}"]`);
 const token=await page.locator('meta[name="calibration-token"]').getAttribute('content');
 const status=()=>fetch(`http://127.0.0.1:${port}/calibration/status`,{headers:{'X-Control-Token':token}}).then(r=>r.json());
 const log=async()=>(await readFile(join(out,'serial.jsonl'),'utf8')).trim().split('\n').map(l=>{try{return JSON.parse(l)}catch{return null}}).filter(Boolean);
 const samples=async(id,since)=>(await log()).filter(e=>e.event==='sweep_sample'&&e.id===id&&(e.unix_ms??Infinity)>=since).map(e=>e.sample);
 const ready=()=>page.waitForFunction(()=>!document.querySelector('#fixture-plus').disabled,null,{timeout:30000});
 async function cruise(id,mode){
  await el('drive-mode').selectOption(mode);
  await page.keyboard.press('z');await sleep(400);await motor(id).click();await ready();
  const before=(await status()).samples[id].position_continuous;
  const down=Date.now();await page.keyboard.down('q');await sleep(1500);const up=Date.now();await page.keyboard.up('q');await sleep(900);
  const after=(await status()).samples[id].position_continuous;
  // Cruise: from 0.6 s after the key went down until it was released.
  const v=(await log()).filter(e=>e.event==='sweep_sample'&&e.id===id&&e.unix_ms>=down+600&&e.unix_ms<=up).map(e=>e.sample.velocity_counts_s);
  const requested=(await log()).filter(e=>e.event==='sweep_sample'&&e.id===id&&e.unix_ms>=down).map(e=>e.sample.requested_speed_counts_s).at(-1);
  const mean=v.reduce((a,b)=>a+b,0)/Math.max(1,v.length);const spread=Math.sqrt(v.reduce((a,b)=>a+(b-mean)**2,0)/Math.max(1,v.length));
  const st=await status();
  return {mode,moved:after-before,requested,mean,spread,samples:v.length,message:st.message,error:st.error,panel:await el('status').textContent()};
 }
 await page.locator('#fixture-advanced summary').click();
 // 1. Select the worm alone and jog it with the shared gains.
 await el('hold-others').uncheck();await el('speed').fill('70');await el('speed').dispatchEvent('input');
 await motor(2).click();await ready();
 checks.push('select proves watchdogs on the simulated FPGA');
 const shared=await cruise(2,'pwm');assert(shared.moved>100,`worm jogged under PWM: ${JSON.stringify(shared)}`);
 // 2. Tune the worm, then jog again with its fitted gains and feed-forward.
 await el('tune-ok').check();await el('tune').click();
 await page.waitForFunction(()=>/Tuned gains in use/.test(document.querySelector('#fixture-tune-status').textContent),null,{timeout:90000});
 const tuned=(await status()).calibration.axes[2].tuning;
 assert(Math.abs(tuned.gain_counts_s_per_duty-3290)<900,`identified gain ${tuned.gain_counts_s_per_duty}`);
 checks.push(`tune: identified ${tuned.gain_counts_s_per_duty.toFixed(0)} counts/s per duty (bench 3290), friction ${(tuned.friction_duty*100).toFixed(1)}%`);
 const pwm=await cruise(2,'pwm');
 const position=await cruise(2,'servo_position');
 const speed=await cruise(2,'servo_speed');
 for(const r of [pwm,position,speed]){assert(r.moved>100,`${r.mode} jog moved: ${JSON.stringify(r)}`);}
 assert(pwm.spread<shared.spread||pwm.spread<25,`tuned PWM smoother: ${pwm.spread} vs ${shared.spread}`);
 checks.push(...[shared,pwm,position,speed].map(r=>`${r.mode}${r===shared?' (shared gains)':''}: moved ${r.moved} counts, cruise ${r.mean.toFixed(0)} ± ${r.spread.toFixed(0)} counts/s (requested ${r.requested?.toFixed(0)}, ${r.samples} samples)`));
 // Tune the belt/hip too (gravity-loaded), so automatic sweeps use measured braking.
 await page.keyboard.press('z');await sleep(400);await motor(3).click();await ready();
 await el('tune-ok').check();await el('tune').click();
 await page.waitForFunction(()=>/Tuned gains in use/.test(document.querySelector('#fixture-tune-status').textContent),null,{timeout:90000});
 const belt=(await status()).calibration.axes[3].tuning;
 checks.push(`tune belt/hip: ${belt.gain_counts_s_per_duty.toFixed(0)} counts/s per duty (bench 3030), breakaway ${belt.breakaway_duty.map(b=>(b*100).toFixed(1)+'%').join('/')}`);
 // 3. Sweep every enabled taught motor together (knee disabled first).
 await page.keyboard.press('z');await sleep(400);
 await motor(1).click();await ready();await el('disable').click();await page.waitForFunction(()=>document.querySelector('.motor[data-id="1"]').classList.contains('off'));
 await el('sweep-all').click();
 await page.waitForFunction(()=>/^(Swept|Sweep-all stopped)/.test(document.querySelector('#fixture-sequence').textContent),null,{timeout:180000});
 const sweepText=await el('sequence').textContent();assert.match(sweepText,/^Swept worm, belt\/hip/,sweepText);
 checks.push(sweepText);
 // 4. Characterization campaign on the enabled, tuned motors; stop it part way,
 //    then resume from its receipts.
 await page.keyboard.press('z');await sleep(400);
 await page.locator('#fixture-campaign-box summary').click();
 await motor(2).click();await ready();
 await el('campaign-ok').check();await el('campaign').click();
 await page.waitForFunction(()=>/stage results saved/.test(document.querySelector('#fixture-campaign-status').textContent)&&!/ 0 stage/.test(document.querySelector('#fixture-campaign-status').textContent),null,{timeout:180000});
 const partial=(await status()).campaign;
 await page.keyboard.press('z');
 await page.waitForFunction(()=>/Last campaign stopped/.test(document.querySelector('#fixture-campaign-status').textContent),null,{timeout:30000});
 const stopped=await status();assert.match(stopped.campaign.error,/cancelled|Operator stop/,JSON.stringify(stopped.campaign));
 checks.push(`campaign on ${partial.axes.map(a=>a.role).join(', ')} (skipped ${partial.skipped.join(', ')}); stopped after ${stopped.campaign.completed} stage results: ${stopped.campaign.error}`);
 await sleep(400);await motor(2).click();await ready();
 await el('campaign-ok').check();await el('campaign-resume').click();
 await page.waitForFunction(()=>/stage results saved/.test(document.querySelector('#fixture-campaign-status').textContent),null,{timeout:60000});
 await page.waitForFunction(()=>/^(Finished|Last campaign stopped)/.test(document.querySelector('#fixture-campaign-status').textContent),null,{timeout:400000});
 const done=(await status()).campaign;
 assert(done.result,`campaign finished: ${JSON.stringify(done.error)}`);
 const reused=done.log.filter(m=>/reused receipt/.test(m)).length;
 assert(reused>=1,`resumed from receipts: ${JSON.stringify(done.log)}`);
 const fitted=JSON.parse(await readFile(join(done.result.directory,'report.json'),'utf8')).fitted;
 const gain=Object.fromEntries(fitted.map(([id,f])=>[id,f.find(x=>x.name==='speed_gain')?.value]));
 checks.push(`campaign resumed (${reused} stages reused) and finished: ${done.result.headline}; fitted speed gain worm ${gain[2]?.toFixed(0)} (bench 3290), belt ${gain[3]?.toFixed(0)} (bench 3030); aborted ${JSON.stringify(done.result.aborted)}`);
 // 5. Gait playback: sim only, then sim + leg on one clock.
 await page.keyboard.press('z');await sleep(400);
 await page.locator('#fixture-gait-box > summary').click();
 await page.waitForFunction(()=>document.querySelector('#fixture-gait').options.length>0,null,{timeout:30000});
 const gaitName=await el('gait').evaluate(s=>s.options[s.selectedIndex].textContent);
 await el('gait-speed').fill('50');await el('gait-speed').dispatchEvent('input');
 await el('gait-play').click();
 await page.waitForFunction(()=>/Sim only · gait time [1-9]/.test(document.querySelector('#fixture-gait-status').textContent),null,{timeout:30000});
 const simText=await el('gait-status').textContent();
 await el('gait-stop').click();
 checks.push(`gait sim only (${gaitName}): ${simText.split('\n')[0]}`);
 await motor(2).click();await ready();
 await page.locator('input[name="fixture-gait-mode"][value="both"]').check();await el('gait-ok').check();
 const before={2:(await status()).samples[2].position_continuous,3:(await status()).samples[3].position_continuous};
 await el('gait-play').click();
 await page.waitForFunction(()=>/Leg: playing/.test(document.querySelector('#fixture-gait-status').textContent),null,{timeout:60000});
 const t0=(await status()).gait.t;await sleep(4000);
 const g=(await status()).gait;
 assert(g.running&&g.t>t0+0.5,`gait clock advanced on the leg: ${JSON.stringify(g)}`);
 const moved=Object.fromEntries([2,3].map(k=>[k,Math.abs((g.targets[k]??0)-before[k])]));
 const gaitErrors=Object.values(g.errors).map(Math.abs);
 assert(gaitErrors.every(e=>e<150),`leg follows the gait: ${JSON.stringify(g.errors)}`);
 const bothText=await el('gait-status').textContent();
 await el('gait-stop').click();await sleep(800);
 const after=await status();assert(!after.gait.running,'gait stopped');assert.equal(after.enabled_id,null);
 const stats=after.gait.statistics;assert(Object.keys(stats).length>=2,`statistics recorded: ${JSON.stringify(after.gait)}`);
 assert(after.gait_runs?.length>=1,'run saved to history');
 checks.push(`gait statistics: ${Object.values(stats).map(x=>`${x.role} RMS ${x.tracking_rms_deg.toFixed(2)}° peak ${x.tracking_peak_deg.toFixed(1)}° effort ${(x.mean_effort*100).toFixed(0)}% governed ${(x.governor_limited_fraction*100).toFixed(0)}%`).join('; ')}; limits ${JSON.stringify(Object.values(after.gait.limits).map(l=>[l.role,Math.round(l.governor_speed_counts_s),Math.round(l.governor_acceleration_counts_s2)]))}`);
 checks.push(`gait sim + leg: clock ${t0.toFixed(2)} → ${g.t.toFixed(2)} s, errors ${JSON.stringify(g.errors)} counts, targets moved ${JSON.stringify(moved)}, clamped ${g.clamped}; ${bothText.replace(/\n/g,' | ')}`);
 // 6. Stop leaves every servo torque-off.
 await page.keyboard.press('z');await sleep(800);
 const s=await status();assert.equal(s.enabled_id,null);
 assert.deepEqual(errors,[],'no page errors');
 checks.push('stop: no enabled motor, no page errors');
 const report={passed:true,checks,bench:pty,work};
 console.log(JSON.stringify(report,null,1));
 if(reportPath)await writeFile(reportPath,JSON.stringify(report,null,2));
}finally{await browser?.close();for(const c of children)c.kill();}
