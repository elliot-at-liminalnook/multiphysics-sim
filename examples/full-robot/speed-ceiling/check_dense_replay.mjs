import fs from 'node:fs';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
const d='examples/full-robot/speed-ceiling',root='runs/speed-ceiling/dense-replay-checks';
const read=p=>JSON.parse(fs.readFileSync(p));
const bin=process.env.SIM_EXAMPLES??'/Users/elliot/physics-simulator/target/gait-exploration/release/examples';
fs.mkdirSync(root,{recursive:true});
const prefix='runs/speed-ceiling/validation/smooth-dynamic-v150-scale1-human-fine';
const windowPath=`${prefix}.dense-forward.window.json`,requirements=`${prefix}.dense-forward.requirements.json`;
const results=[];
function write(name,value){const path=`${root}/${name}.json`;fs.writeFileSync(path,JSON.stringify(value)+'\n',{flag:'wx'});return path;}
function reject(name,args){const result=spawnSync(`${bin}/${args[0]}`,args.slice(1),{encoding:'utf8',maxBuffer:1024*1024,timeout:30000});assert(result.status!==0&&result.status!==null,`${name} must fail validation`);assert.equal(result.stdout,'');results.push({name,exit:result.status,error:result.stderr.trim()});}
reject('zero-sample-period',['capture_embedded_window','--replay',`${prefix}.dense.recording.json`,'2','2.4','0']);
reject('beyond-recorded-horizon',['capture_embedded_window','--replay',`${prefix}.dense.recording.json`,'19','21','.00125']);
const duplicate=read(`${prefix}.dense.recording.json`);duplicate.input_events[1].at_step=duplicate.input_events[0].at_step;
reject('duplicate-input-events',['capture_embedded_window','--replay',write('duplicate-events',duplicate),'2','2.4','.00125']);
const unfinished=read(windowPath);unfinished.window_complete=false;
reject('incomplete-window',['evaluate_lift',`${prefix}.scene.json`,write('incomplete-window',unfinished),requirements,'--simulation-time']);
const scene=read(`${prefix}.scene.json`);scene.robot.world.floor_z+=.001;
reject('different-world',['evaluate_lift',write('different-world',scene),windowPath,requirements,'--simulation-time']);
const report={passed:true,results,scope:'Rejection checks for bounded replay and lift audit. Positive evidence and exact common-endpoint comparisons are in the dense-summary report; these checks do not certify other trajectories.'};
fs.writeFileSync(`${d}/dense-replay-checks.json`,JSON.stringify(report,null,2)+'\n');console.log(report);
