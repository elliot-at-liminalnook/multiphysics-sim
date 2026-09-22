// Offline evidence tooling only: no serial ports, flashing, or motor commands.
// Reuses frozen production RTL and its complete UART/supervisor test fixture.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
const here=path.dirname(fileURLToPath(import.meta.url));
const baseline=path.resolve(here,'../2026-09-15-fast-loop');
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const stats=a=>{const s=[...a].sort((a,b)=>a-b);return {min:s[0],median:s[Math.floor(s.length/2)],p95:s[Math.ceil(s.length*.95)-1],max:s.at(-1)};};
const sources={};
const read=p=>{sources[path.relative(here,p)]=sha(p);return fs.readFileSync(p,'utf8');};
const physical=[];
for(const entry of fs.readdirSync(baseline)) {
    if(!entry.includes('10ms'))continue;
    const p=path.join(baseline,entry,'device-review.json');if(!fs.existsSync(p))continue;
    const review=JSON.parse(read(p)), groups=new Map();
    for(const e of review.events)if(['Telemetry','Control','Audit'].includes(e.kind)) {
        if(!groups.has(e.frame))groups.set(e.frame,[]);groups.get(e.frame).push(e);
    }
    for(const [frame,events] of groups) {
        if(events.length!==7)throw Error(`Incomplete frame ${p}:${frame}`);
        events.sort((a,b)=>a.request_ticks-b.request_ticks);
        const first=events[0].request_ticks, row={capture:entry,frame,events:[],read_ms:0,control_ms:0,audit_ms:0,gaps_ms:0};
        let prev=first;
        for(const e of events) {
            if(e.request_ticks<prev)throw Error('Overlapping events');
            const ms=t=>t*1000/review.clock_hz;
            const duration=ms(e.completion_ticks-e.request_ticks),gap=ms(e.request_ticks-prev);
            row[{Telemetry:'read_ms',Control:'control_ms',Audit:'audit_ms'}[e.kind]]+=duration;row.gaps_ms+=gap;
            row.events.push({kind:e.kind,id:e.motor_id,start_ms:ms(e.request_ticks-first),end_ms:ms(e.completion_ticks-first),duration_ms:duration,gap_before_ms:gap});
            prev=e.completion_ticks;
        }
        row.window_ms=(prev-first)*1000/review.clock_hz;
        row.oldest_feedback_to_command_ms=row.events[3].end_ms-row.events[0].end_ms;
        row.feedback_completion_skew_ms=row.events[2].end_ms-row.events[0].end_ms;
        if(Math.abs(row.window_ms-row.read_ms-row.control_ms-row.audit_ms-row.gaps_ms)>1e-9)throw Error('Timing accounting');
        physical.push(row);
    }
}
const worst=physical.reduce((a,b)=>a.window_ms>b.window_ms?a:b);
const physicalSummary={frames:physical.length,captures:new Set(physical.map(x=>x.capture)).size,
    stats:Object.fromEntries(['window_ms','gaps_ms','read_ms','control_ms','audit_ms','oldest_feedback_to_command_ms','feedback_completion_skew_ms'].map(k=>[k,stats(physical.map(x=>x[k]))])),worst};
fs.writeFileSync(path.join(here,'physical-summary.json'),JSON.stringify(physicalSummary,null,2)+'\n');
const files=['uart_tx','uart_rx','host_packet_queue','trajectory_store','trajectory_upload','bridge_control','hx_safety','button','experiment_session','experiment_transactions','experiment_scheduler','experiment_packet','experiment_reply','experiment_event_stream','experiment_event_packet'];
const run=(cmd,args,cwd,log)=>{
    const fd=fs.openSync(log,'w');const r=spawnSync(cmd,args,{cwd,stdio:['ignore',fd,fd]});fs.closeSync(fd);
    if(r.status!==0)throw Error(`${cmd} failed (${r.status}): ${log}`);
};
const sim=[];
for(const hclks of [50,25]) {
    // Only the host UART changes between cases; the servo bus remains 1 Mbaud.
    // 30 us mock reply turnaround approximates physical transaction durations.
    // This is NOT a model or measurement of the servo's internal sensor age.
    const dir=fs.mkdtempSync(path.join(os.tmpdir(),`hx-latency-host${hclks}-`));
    fs.mkdirSync(path.join(dir,'src'));fs.mkdirSync(path.join(dir,'impl/fixed-pd'),{recursive:true});fs.mkdirSync(path.join(dir,'tb/trajectory-fast-loop'),{recursive:true});
    for(const f of files)fs.writeFileSync(path.join(dir,'src',f+'.v'),read(path.join(baseline,'source/fpga/src',f+'.v')));
    fs.writeFileSync(path.join(dir,'impl/fixed-pd/fixed_pd.v'),read(path.join(baseline,'source/fpga/fixed_pd.v')));
    for(const f of ['packets.hex','lengths.hex','crc.hex'])fs.writeFileSync(path.join(dir,'tb/trajectory-fast-loop',f),read(path.join(baseline,'verification/rtl-upload',f)));
    let tb=read(path.join(baseline,'source/fpga/tb/bridge_experiment_tb.v'));
    for(const needle of ['localparam HCLKS=CASE==5 ? 500 : 50;','repeat(50000)@(negedge clk);','endmodule'])if(!tb.includes(needle))throw Error(`Fixture changed: ${needle}`);
    tb=tb.replace('localparam HCLKS=CASE==5 ? 500 : 50;',`localparam HCLKS=${hclks};`)
        .replace('repeat(50000)@(negedge clk);','repeat(1500)@(negedge clk);')
        .replace('endmodule',read(path.join(here,'gap_monitor.vh'))+'\nendmodule');
    fs.writeFileSync(path.join(dir,'tb/bridge_experiment_tb.v'),tb);
    const name=`host-${50/hclks}mbaud`, args=['--language','1364-2005','--binary','--timing','-j','2','-Wno-fatal','--top-module','bridge_experiment_tb','-GCASE=0','-GFAST=1','--Mdir','impl/obj','tb/bridge_experiment_tb.v',...files.map(f=>`src/${f}.v`),'impl/fixed-pd/fixed_pd.v'];
    process.stdout.write(`Building ${name} in ${dir}\n`);
    run('verilator',args,dir,path.join(here,name+'-build.log'));
    run(path.join(dir,'impl/obj/Vbridge_experiment_tb'),[],dir,path.join(here,name+'-run.log'));
    fs.copyFileSync(path.join(dir,'gap-trace.csv'),path.join(here,name+'-gaps.csv'));
    fs.copyFileSync(path.join(dir,'impl/bridge_experiment_fast_case0.hex'),path.join(here,name+'-uart.hex'));
    const output=fs.readFileSync(path.join(here,name+'-run.log'),'utf8');
    if(!output.includes('PASS full UART case 0'))throw Error('Missing complete UART acceptance');
    const rows=fs.readFileSync(path.join(here,name+'-gaps.csv'),'utf8').trim().split('\n');const header=rows.shift().split(',');
    const gaps=rows.map(l=>Object.fromEntries(l.split(',').map((v,i)=>[header[i],Number(v)])));
    if(gaps.length!==150)throw Error(`Expected six gaps for each of 25 frames, got ${gaps.length}`);
    for(const g of gaps)if(g.gap_ticks!==header.slice(4).reduce((s,k)=>s+g[k],0)||g.boundary_ticks!==1)throw Error('Gap monitor accounting');
    const events=[...output.matchAll(/^EVENT (\d+) (\d+) (\d+) (\d+) (\d+)$/gm)].map(m=>({frame:+m[1],kind:+m[2],id:+m[3],request:+m[4],completion:+m[5]}));
    const frames=Array.from({length:25},(_,frame)=>{
        const es=events.filter(e=>e.frame===frame);if(es.length!==7)throw Error('Missing simulation event');
        const gs=gaps.filter(g=>g.frame===frame);return {frame,window_ms:(es.at(-1).completion-es[0].request)/50000,
            gaps_ms:gs.reduce((s,g)=>s+g.gap_ticks,0)/50000,
            oldest_feedback_to_command_ms:(es[3].completion-es[0].completion)/50000,
            classification_ms:Object.fromEntries(header.slice(4).map(k=>[k.replace('_ticks',''),gs.reduce((s,g)=>s+g[k],0)/50000]))};
    });
    sim.push({name,host_baud:50000000/hclks,servo_baud:1000000,mock_turnaround_us:30,frames,
        summary:Object.fromEntries(['window_ms','gaps_ms','oldest_feedback_to_command_ms'].map(k=>[k,stats(frames.map(f=>f[k]))])),
        exclusive_gap_mean_ms:Object.fromEntries(header.slice(4).map(k=>[k.replace('_ticks',''),gaps.reduce((s,g)=>s+g[k],0)/50000/25])),
        acceptance:'25 complete frames; complete event count; positive/negative full-scale arithmetic; modeled final zero/off; UART checksums and contention guards pass',build_directory:dir});
    process.stdout.write(`${name}: ${JSON.stringify(sim.at(-1).summary)}\n`);
}
fs.writeFileSync(path.join(here,'simulation-summary.json'),JSON.stringify(sim,null,2)+'\n');
sources['investigate.mjs']=sha(fileURLToPath(import.meta.url));
fs.writeFileSync(path.join(here,'provenance.json'),JSON.stringify({created_utc:new Date().toISOString(),sources,commands:'node investigate.mjs; exact Verilator options are in this script',node:process.version,verilator:spawnSync('verilator',['--version'],{encoding:'utf8'}).stdout.trim(),scope:'Offline analysis and UART RTL simulation only; no hardware access; no production firmware changes'},null,2)+'\n');
console.log(JSON.stringify({physical:physicalSummary.stats,simulation:sim.map(s=>({name:s.name,summary:s.summary,gaps:s.exclusive_gap_mean_ms}))},null,2));
