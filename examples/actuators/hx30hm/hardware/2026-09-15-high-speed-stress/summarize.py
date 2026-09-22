"""Summarize evidence; kinematic estimates come from the shared Rust analyzer."""
import json,math,subprocess
from pathlib import Path
root=Path(__file__).resolve().parent
results=[]
for p in sorted(root.glob('*/fpga-recording.json')):
 r=json.loads(p.read_text());folder=p.parent;review=folder/'motion-review.json'
 if r['completed'] and r['stop_verified'] and not review.exists():
  subprocess.run(['target/debug/examples/review_controller','measure-fpga-motion',str(p),str(root/'estimator-10ms.json'),str(review)],check=True)
 m=json.loads(review.read_text()) if review.exists() else None
 item=dict(capture=folder.name,completed=r['completed'],stop_verified=r['stop_verified'],failure=r['failure'],frames=len(r['frames']),cap_percent=r['plan']['gains']['limit']/10,motors={})
 for motor in r['plan']['ids']:
  obs=[o for f in r['frames'] for o in f['observations'] if o['id']==motor]
  if not obs:continue
  t=[o['telemetry'] for o in obs];baseline=r['initial']['physical_preflight'][str(motor)]['voltage_v'];low=min(x['voltage_v'] for x in t)
  a=dict(voltage_min_v=low,voltage_sag_percent=100*(baseline-low)/baseline,temperature_max_c=max(x['temperature_c'] for x in t),current_max_raw_uncalibrated=max(x['current_raw'] for x in t))
  if m:
   s=m['scores']['motors'][str(motor)];v=m['axes'][str(motor)]['speed_rad_s'];acc=m['axes'][str(motor)]['acceleration_rad_s2']
   a.update(peak_secant_speed_deg_s=max(abs(x['value']) for x in v)*180/math.pi,peak_estimated_acceleration_deg_s2=max(abs(x['value']) for x in acc)*180/math.pi,rms_error_deg=s['rms_degrees'],peak_error_deg=s['peak_degrees'],saturated_fraction=s['saturated_fraction'],tracking_pass=s['tracking_pass'])
  item['motors'][str(motor)]=a
 results.append(item)
output=dict(captures=results,total_frames=sum(x['frames'] for x in results),audited_motor_updates=3*sum(x['frames'] for x in results),all_completed=all(x['completed'] for x in results),all_stops_verified=all(x['stop_verified'] for x in results),interpretation='Two-second bursts separated by upload, preflight, review and stop. Peak speeds are encoder secants, not settled maximum speed. Internal sample age unknown. Current register is uncalibrated, so no validated amps or watts. No model fit or promotion.')
(root/'results.json').write_text(json.dumps(output,indent=2)+'\n')
for x in results:
 print(x['capture'],x['completed'],x['frames'],'speed',[round(a.get('peak_secant_speed_deg_s',0),1) for a in x['motors'].values()],'sag',round(max(a['voltage_sag_percent'] for a in x['motors'].values()),1),'temp',max(a['temperature_max_c'] for a in x['motors'].values()),'rms',[round(a.get('rms_error_deg',0),2) for a in x['motors'].values()])
