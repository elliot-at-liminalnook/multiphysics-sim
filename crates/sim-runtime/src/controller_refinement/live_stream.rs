//! Finite streamed references for the shared FPGA experiment scheduler.
//! No physics or PWM law here: packet compilation, reference interpolation and
//! correspondence between acknowledged rows and device-clock evidence only.
use super::{fpga::Plan, fpga_events::{self,Review}, fpga_upload::Upload};
use crate::acquisition::servo_bus::{packet,reply};
use serde::{Serialize,Deserialize};

pub const PERIOD_S:f64=0.01;
pub const MAX_FRAMES:usize=1200;
pub const BUFFER_FRAMES:usize=16;

pub fn configuration(run:u32,frames:usize)->Result<Vec<u8>,String>{
    if !(2..=MAX_FRAMES).contains(&frames){return Err("Live frame count outside 2..1200".into())}
    let mut p=vec![0,1];p.extend(run.to_le_bytes());p.extend((frames as u16).to_le_bytes());
    packet(254,0xa4,&p).map_err(str::to_owned)
}
pub fn append_packet(run:u32,first:usize,ids:&[u8],rows:&[[i16;9]])->Result<Vec<u8>,String>{
    super::fpga::validate_physical_scope(ids,ids)?;
    if ids.len()!=3 || rows.is_empty() || rows.len()>8 || first+rows.len()>MAX_FRAMES {return Err("Invalid live reference batch".into())}
    let mut p=vec![1];p.extend(run.to_le_bytes());p.extend((first as u16).to_le_bytes());p.push(rows.len() as u8);
    for row in rows {
        if row.iter().any(|v|v.unsigned_abs()>80) || (0..9).any(|i|!ids.contains(&(i as u8+4)) && row[i]!=0){return Err("Live reference outside declared axes/bounds".into())}
        for id in ids {p.extend(row[usize::from(*id-4)].to_le_bytes());}
    }
    packet(254,0xa4,&p).map_err(str::to_owned)
}
/// Linear reference segment, distinct from motor/plant interpolation. Every
/// returned row must independently satisfy the hardware excursion and slew gates.
pub fn segment(previous:[i16;9],target:[i16;9],count:usize)->Result<Vec<[i16;9]>,String>{
    if count==0 || count>8{return Err("Invalid segment length".into())}
    let mut result=Vec::new();let mut last=previous;
    for n in 1..=count {
        let row=std::array::from_fn(|i|(f64::from(previous[i])+(f64::from(target[i])-f64::from(previous[i]))*n as f64/count as f64).round() as i16);
        if row.iter().any(|v|v.unsigned_abs()>80) || (0..9).any(|i|(i32::from(row[i])-i32::from(last[i])).unsigned_abs()>32){return Err("Segment exceeds travel/slew gate".into())}
        last=row;result.push(row);
    }
    Ok(result)
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {pub op:u8,pub failed:bool,pub enabled:bool,pub busy:bool,pub mask:u16,pub frames:u16,pub written:u16,pub period_ticks:u32,pub run_id:u32,pub capabilities:u8}
impl Receipt {
    pub fn decode(bytes:&[u8])->Result<Self,String>{
        let r=reply(bytes,254,25)?;let p=r.parameters;
        let u16at=|i|u16::from_le_bytes([p[i],p[i+1]]);
        let u32at=|i|u32::from_le_bytes(p[i..i+4].try_into().unwrap());
        if r.error!=0 || p[0]!=1 || p[1]!=0xa4 || p[2]>1 || p[3..6].iter().any(|x|*x>1) || u32at(20)!=50_000_000 || p[24]&8==0 {return Err("Invalid live receipt".into())}
        Ok(Self{op:p[2],failed:p[3]!=0,enabled:p[4]!=0,busy:p[5]!=0,mask:u16at(6),frames:u16at(8),written:u16at(10),period_ticks:u32at(12),run_id:u32at(16),capabilities:p[24]})
    }
    pub fn check(&self,run:u32,frames:usize,ids:&[u8],written:usize,op:u8)->Result<(),String>{
        let mask=ids.iter().fold(0u16,|a,i|a|(1<<(*i-4)));
        if self.failed || !self.enabled || self.busy || self.op!=op || self.run_id!=run || usize::from(self.frames)!=frames || self.mask!=mask || usize::from(self.written)!=written || self.period_ticks!=500_000 {return Err("Live receipt differs from requested queue state".into())}
        Ok(())
    }
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedBatch {pub request:Vec<u8>,pub receipt:Vec<u8>}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub base:Upload,pub plan:Plan,pub run_id:u32,pub configuration_receipt:Vec<u8>,pub batches:Vec<AcceptedBatch>,pub packets:Vec<Vec<u8>>,
}
impl Capture {
    pub fn review(&self)->Result<Review,String>{
        self.base.validate()?;self.plan.validate()?;
        let p=&self.plan;let b=&self.base.source_plan;
        if p.control!="fpga_device_pd" || p.ids.len()!=3 || p.ids!=b.ids || (p.gains.kp_q8,p.gains.kd_q8,p.gains.kv_q8,p.gains.limit)!=(b.gains.kp_q8,b.gains.kd_q8,b.gains.kv_q8,b.gains.limit) || p.period_s!=PERIOD_S || p.period_s!=b.period_s || p.targets.len()>MAX_FRAMES || p.bitstream_blake3!=b.bitstream_blake3 || b.targets.iter().flatten().any(|v|*v!=0){return Err("Live plan differs from sealed controller configuration".into())}
        Receipt::decode(&self.configuration_receipt)?.check(self.run_id,p.targets.len(),&p.ids,0,0)?;
        let mut written=0;
        for batch in &self.batches {
            let q=&batch.request;
            if q.len()<20 || q[4]!=0xa4 || q[5]!=1 || q[2]!=254 || q[3] as usize+4!=q.len() || q[2..].iter().fold(0u8,|a,b|a.wrapping_add(*b))!=255{return Err("Invalid retained append packet".into())}
            let run=u32::from_le_bytes(q[6..10].try_into().unwrap());let first=usize::from(u16::from_le_bytes(q[10..12].try_into().unwrap()));let count=usize::from(q[12]);
            if run!=self.run_id || first!=written || count==0 || count>8 || q.len()!=14+count*6 || written+count>p.targets.len(){return Err("Append order/count differs from execution".into())}
            if append_packet(self.run_id,first,&p.ids,&p.targets[first..first+count])? != *q{return Err("Executed reference differs from accepted append bytes".into())}
            written+=count;Receipt::decode(&batch.receipt)?.check(self.run_id,p.targets.len(),&p.ids,written,1)?;
        }
        let review=fpga_events::review_execution(p,self.base.homes,self.base.plan_crc32,&self.packets)?;
        if review.run_id!=self.run_id || review.events.iter().any(|e|matches!(e.kind,fpga_events::Kind::Control) && usize::from(e.frame)>=written){return Err("Device executed an unacknowledged/foreign reference".into())}
        Ok(review)
    }
    pub fn unverified_recording(&self)->Result<super::fpga::Recording,String>{fpga_events::recording_from_review(&self.plan,self.base.homes,self.review()?)}
}
#[cfg(test)]
mod tests {
 use super::*;
 #[test]fn segment_and_packets_enforce_excursion_slew_and_scope(){
  let mut t=[0;9];t[6]=64;t[7]=-64;t[8]=16;
  let rows=segment([0;9],t,8).unwrap();assert_eq!(rows.last(),Some(&t));assert_eq!(rows[0][6],8);
  assert!(segment([0;9],t,1).is_err());t[6]=81;assert!(segment([0;9],t,8).is_err());
  assert_eq!(append_packet(3,0,&[10,11,12],&rows).unwrap().len(),62);
  assert!(append_packet(3,0,&[4,5,6],&rows).is_err());assert!(configuration(3,1201).is_err());
 }
}
