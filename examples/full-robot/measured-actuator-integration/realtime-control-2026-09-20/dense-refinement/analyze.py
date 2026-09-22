"""Compare captured observations; no simulation or controller implementation."""
import json
from pathlib import Path
HERE=Path(__file__).resolve().parent
ROOT=HERE.parent
FIELDS=['joint_positions','joint_velocities','motor_states','servo_states','servo_commands','servo_targets_rad','motor_readings','driver_readings','poses','contacts','original_rows','policy']
sources={'be6400':'warm-probes/shared-body','be12800':'reference-12800hz/reference','sdirk6400':'stage-proposal-reuse/step-1/candidate'}
dense={k:json.loads((HERE/(k+'.json')).read_text()) for k in sources}
parity={}
for name,source in sources.items():
    original=json.loads((ROOT/(source+'.native.json')).read_text())
    checked=0
    for frame in original['frames']:
        if frame['time_s']>.200000001: break
        replay=dense[name]['frames'][round(frame['time_s']/.00125)]
        for field in FIELDS: assert frame.get(field)==replay.get(field),(name,frame['time_s'],field)
        checked+=1
    assert dense[name]['window_complete'] and not dense[name]['error']
    parity[name]={'exact_matching_source_frames':checked,'dense_frames':len(dense[name]['frames'])}
pairs={}
for name,left,right in [('be_refinement','be6400','be12800'),('fine_method','be12800','sdirk6400')]:
    first={}
    for key in ['servo_targets_rad','servo_states','servo_commands']:
        for a,b in zip(dense[left]['frames'],dense[right]['frames']):
            if a[key]!=b[key]:
                first[key]={'time_s':a['time_s'],'differences':[{'index':i,'left':x,'right':y} for i,(x,y) in enumerate(zip(a[key],b[key])) if x!=y]}
                break
    pairs[name]=first
report={'replay_parity':parity,'first_controller_differences':pairs,'fields_checked':FIELDS,'interpretation':'BE6400 vs BE12800 first differ in policy targets at the 21.25 ms observation, then encoder history and pending PWM at 25 ms; held PWM first differs at 26.25 ms by 0.016 for motor 11. These observed quantized branches can amplify integration differences. This does not establish which integrator is accurate or waive the fidelity gates. 800 Hz snapshots resolve these scheduled boundaries, not all unscheduled motor/contact events.','scope':'0 to 0.2 s replay windows. Original 50 Hz physical/policy frame fields match exactly. Input values at host boundaries, observation wall time and report stride metadata are excluded.'}
(HERE/'comparison.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'parity':parity,'first_times':{k:{f:v['time_s'] for f,v in p.items()} for k,p in pairs.items()}},indent=2))
