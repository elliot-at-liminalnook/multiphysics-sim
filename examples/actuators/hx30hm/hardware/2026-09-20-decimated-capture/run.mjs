// Offline RTL qualification. Never accesses hardware or changes commissioned limits.
import fs from 'node:fs';import path from 'node:path';import os from 'node:os';
import {spawnSync} from 'node:child_process';import {fileURLToPath} from 'node:url';
const here=path.dirname(fileURLToPath(import.meta.url)), source=path.join(here,'firmware');
const work=process.env.HX_REUSE || fs.mkdtempSync(path.join(os.tmpdir(),'hx-decimated-'));
if(!process.env.HX_REUSE) fs.cpSync(source,work,{recursive:true});fs.mkdirSync(path.join(here,'results'),{recursive:true});
const files=['uart_tx','uart_rx','host_packet_queue','trajectory_store','trajectory_upload','bridge_control','hx_safety','button','experiment_session','experiment_transactions','experiment_scheduler','experiment_packet','experiment_reply','experiment_event_stream','experiment_event_packet','experiment_frame_log'];
function run(cmd,args,log){const fd=fs.openSync(log,'w');const r=spawnSync(cmd,args,{cwd:work,stdio:['ignore',fd,fd]});fs.closeSync(fd);if(r.status!==0)throw Error(`${cmd} failed ${r.status}: ${log}`);}
console.log('Build',work);
if(!process.env.HX_REUSE) run('verilator',['--language','1364-2005','--binary','--timing','-j','2','-Wno-fatal','--top-module','bridge_experiment_tb','-GFAST=1','-GLOG_STRIDE=2','--Mdir','impl/obj','tb/bridge_experiment_tb.v',...files.map(f=>'src/'+f+'.v'),'impl/fixed-pd/fixed_pd.v'],path.join(here,'results/build.log'));
const original=fs.readFileSync(path.join(source,'tb/trajectory-fast-loop/packets.hex'),'utf8').trim().split('\n').map(s=>Buffer.from(s,'hex').reverse());
const lengths=fs.readFileSync(path.join(source,'tb/trajectory-fast-loop/lengths.hex'),'utf8').trim().split(/\s+/).map(s=>parseInt(s,16));
function crc(bytes){let c=0xffffffff;for(const b of bytes){c^=b;for(let j=0;j<8;j++)c=(c>>>1)^((c&1)?0xedb88320:0);}return (~c)>>>0;}
function configure(period){const ps=original.map(b=>Buffer.from(b));ps[0].writeUInt32LE(period,11);const checksum=crc(Buffer.concat([ps[0].subarray(7,41),...ps.slice(1,-1).map(p=>p.subarray(8,44))]));ps.at(-1).writeUInt32LE(checksum,6);for(let i=0;i<ps.length;i++){let sum=0;for(let j=2;j<lengths[i]-1;j++)sum+=ps[i][j];ps[i][lengths[i]-1]=(~sum)&255;}
fs.writeFileSync(path.join(work,'tb/trajectory-fast-loop/packets.hex'),ps.map(p=>Buffer.from(p).reverse().toString('hex')).join('\n')+'\n');fs.writeFileSync(path.join(work,'tb/trajectory-fast-loop/crc.hex'),checksum.toString(16).padStart(8,'0')+'\n');}
const cases=process.env.HX_CASES ? JSON.parse(process.env.HX_CASES) : [[0,125000],[1,125000],[2,250000],[3,125000],[4,125000],[5,125000],[6,125000],[7,125000]];
const results=process.env.HX_REUSE ? JSON.parse(fs.readFileSync(path.join(here,'results/summary.json'),'utf8')) : [];
for(const [scenario,period] of cases){configure(period);const name=`case${scenario}-${50000000/period}hz`,dir=path.join(here,'results',name);fs.mkdirSync(dir,{recursive:true});
console.log('Run',name);run(path.join(work,'impl/obj/Vbridge_experiment_tb'),['+case='+scenario],path.join(dir,'run.log'));
fs.copyFileSync(path.join(work,`impl/bridge_experiment_fast_case${scenario}.hex`),path.join(dir,'uart.hex'));
const text=fs.readFileSync(path.join(dir,'run.log'),'utf8');if(!text.includes('PASS full UART'))throw Error('Missing acceptance');
const events=[...text.matchAll(/^EVENT (\d+) (\d+) (\d+) (\d+) (\d+)$/gm)].map(m=>({frame:+m[1],kind:+m[2],id:+m[3],request:+m[4],completion:+m[5]}));
fs.writeFileSync(path.join(dir,'events.json'),JSON.stringify(events,null,2));
const frames=[];for(const f of [...new Set(events.map(e=>e.frame))]){const es=events.filter(e=>e.frame===f);if(es.length!==7)continue;frames.push({frame:f,window_ms:(es.at(-1).completion-es[0].request)/50000,gaps_ms:es.slice(1).reduce((s,e,i)=>s+e.request-es[i].completion,0)/50000,feedback_to_command_ms:(es[3].completion-es[0].completion)/50000});}
const stats=k=>{const a=frames.map(f=>f[k]).sort((a,b)=>a-b);return {min:a[0],median:a[Math.floor(a.length/2)],max:a.at(-1)};};
const result={name,scenario,period_ticks:period,frames,stats:Object.fromEntries(['window_ms','gaps_ms','feedback_to_command_ms'].map(k=>[k,stats(k)])),acceptance:text.match(/PASS full UART[^\n]*/)?.[0]};results.push(result);fs.writeFileSync(path.join(here,'results/summary.json'),JSON.stringify(results,null,2));console.log(JSON.stringify({name,stats:result.stats}));}
fs.writeFileSync(path.join(here,'results/workspace.txt'),work+'\n');
