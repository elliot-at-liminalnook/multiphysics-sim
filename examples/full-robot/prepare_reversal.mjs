// Author one controller/profile from the versioned CAD-derived online recipe.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const output=process.argv[2]||'examples/full-robot/browser-reversal';
const source='examples/full-robot/browser-online-steps',read=p=>JSON.parse(readFileSync(p));
const scene=read(`${source}/scene.json`),short=read(`${source}/scheduled-posture-screen.config.json`),task=read(`${source}/task.json`);
short.policy.step_reference.sequence.restart_order_on_translation_reversal=true;
short.policy.step_reference.sequence.command_postures[0].support_offsets_m[1][0]=-.018;
const old=scene.controller.sources.files[scene.controller.sources.entry];
assert(old.includes('    let body_gain = sensors["command.body_gain"];'));
const entry='standing-position-feedback.rhai';
scene.controller.sources={entry,files:{[entry]:old.replace('    let body_gain = sensors["command.body_gain"];',`    // The typed Rust observations below are privileged simulated forces (N).
    // Preserve moving gain; strengthen stop/stand correction with four supports.
    let p = parameters();
    let support = 1.0;
    for name in p.support_force_channels {
        support = support.min((sensors[name] / p.full_support_force_n).max(0.0).min(1.0));
    }
    let stopped = true;
    for name in p.motion_command_channels { if sensors[name] != 0.0 { stopped = false; } }
    let extra_gain = if stopped { p.standing_gain_increment * support } else { 0.0 };
    let body_gain = sensors["command.body_gain"] + extra_gain;`)}};
scene.controller.parameters={full_support_force_n:short.policy.body_feedback.full_support_force_n,standing_gain_increment:.5,
 support_force_channels:short.policy.task_observations.markers.map(m=>`marker.${m.id}.floor_force_world.z`),
 motion_command_channels:short.policy.step_reference.command_channels};
assert(short.policy.task_observations.floor_forces);
const config=structuredClone(short);config.steps=Math.round(60/config.step_s);
const refined=c=>{const f=structuredClone(c);f.step_s/=2;f.steps*=2;f.report_every*=2;f.implicit.newton.max_iterations=80;return f;};
mkdirSync(output,{recursive:true});const write=(n,j)=>writeFileSync(`${output}/${n}`,JSON.stringify(j)+'\n');
write('scene.json',scene);write('config.json',config);write('short.config.json',short);write('refined.config.json',refined(config));write('short-refined.config.json',refined(short));write('task.json',task);
const actions=(duration,command)=>Array.from({length:Math.round(duration/task.period_s)},(_,i)=>[.5,.25,.25,...command(i*task.period_s)]);
write('forward-reverse.actions.json',actions(24,t=>[t<8.4?.00125:t<16.8?-.00125:0,0,0]));
write('reverse-forward.actions.json',actions(24,t=>[t<8.4?-.00125:t<16.8?.00125:0,0,0]));
write('turn-reverse.actions.json',actions(60,t=>[t<8.4?.00125:t<16.8?0:t<20?-.00125:0,0,t>=8.4&&t<16.8?.001:0]));
write('sustained.actions.json',actions(60,t=>[t<56?.00125:0,0,0]));
for(const at of [2.4,4.4,6.4])write(`switch-${at}.actions.json`,actions(24,t=>[t<at?.00125:t<20?-.00125:0,0,0]));
const paths=[`${source}/scene.json`,`${source}/scheduled-posture-screen.config.json`,`${source}/task.json`,'examples/full-robot/prepare_reversal.mjs'];
write('manifest.json',{version:1,source_cad_sha256:scene.robot.source.cad_sha256,
 scope:'Explicit provisional gait/controller changes; CAD geometry, masses, contact and actuator properties retained. Restart foot order after translational reversal; reduce reverse body shift by 2 mm; extra standing gain requires zero motion request and four loaded supports. Ideal force observations, not hardware sensors.',
 parameter_units:{full_support_force_n:'N',standing_gain_increment:'1'},inputs:Object.fromEntries(paths.map(p=>[p,createHash('sha256').update(readFileSync(p)).digest('hex')]))});
console.log(output);
