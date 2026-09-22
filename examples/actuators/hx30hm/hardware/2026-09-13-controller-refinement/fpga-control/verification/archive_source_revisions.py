import json,hashlib,shutil
from pathlib import Path
import blake3
root=Path('/Users/elliot/physics-simulator');out=root/'examples/actuators/hx30hm/hardware/2026-09-13-controller-refinement/fpga-control/verification/source-revisions';out.mkdir(parents=True,exist_ok=True)
s=(out/'continuous_torque.rs.txt').read_text();versions={'continuous_torque':s}
s=s.replace('let mut pwm_readback=[0i16;9];let mut torque_readback=[0u8;9];let mut arithmetic_matches=true;','let mut pwm_readback=[0i16;9];let mut arithmetic_matches=true;').replace('let bytes=bus.read(o.id,0x28,6)?;torque_readback[i]=bytes[0];let raw=u16::from_le_bytes([bytes[4],bytes[5]]);','let bytes=bus.read(o.id,0x2c,2)?;let raw=u16::from_le_bytes([bytes[0],bytes[1]]);').replace('pwm_readback,torque_readback:Some(torque_readback),arithmetic_matches','pwm_readback,arithmetic_matches').replace('            if plan.ids.iter().any(|id|torque_readback[(*id-4) as usize]!=1){return Err("Torque became disabled during the controller trial".into());}\n','')
versions['initial_torque_audit']=s
s=s.replace('''        for &id in &plan.ids {
            let torque=bus.read(id,0x28,1)?;
            r.initial[id.to_string()]["torque_enable_readback_before_motion"]=json!(torque);
            if torque!=[1] {return Err(format!("ID {id}: torque enable not confirmed; no trajectory started").into());}
        }
''','')
versions['batch_heartbeat']=s
s=s.replace('''            let mask=plan.ids.iter().fold(0u16,|m,id|m|(1<<(*id-4)));
            let status=servo_safety::Status::decode(&bus.txn(254,0xa0,&[5,mask as u8,(mask>>8) as u8],servo_safety::STATUS_WIDTH)?)?;
            for &id in &plan.ids {status.require_armed(id)?;}
            for &id in &plan.ids {
                let request_s''','''            for &id in &plan.ids {
                supervisor(bus,servo_safety::Command::Heartbeat(id))?.require_armed(id)?;
                let request_s''')
versions['initial_per_id_heartbeat']=s
hashes={}
for name,text in versions.items():
 h=blake3.blake3(text.encode()).hexdigest();hashes[h]=name;(out/(name+'.rs.txt')).write_text(text)
coverage={}
for path in out.parent.parent.glob('*/fpga-recording.json'):
 r=json.loads(path.read_text());coverage[path.parent.name]={'host_blake3':r['sources']['host'],'archived_revision':hashes.get(r['sources']['host'])}
assert all(v['archived_revision'] for v in coverage.values()),coverage
for p in ['crates/sim-domain-control/src/fixed_pd.rs','crates/sim-runtime/examples/characterize_hx_bridge.rs','crates/sim-runtime/examples/plan_fpga_controller.rs','crates/sim-runtime/src/controller_refinement/fpga.rs']:
 if not (out/(Path(p).name+'.txt')).exists(): shutil.copy2(root/p,out/(Path(p).name+'.txt'))
(out/'coverage.json').write_text(json.dumps(coverage,indent=2)+'\n')
print('All',len(coverage),'recordings match an archived exact host source revision')
