"""Offline plots of retained evidence; no simulation or hardware access."""
import base64, csv, hashlib, html, json
from pathlib import Path
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
ROOT=Path(__file__).resolve().parent.parent
DEG=360/4096
plt.rcParams.update({'font.size':10,'axes.spines.top':False,'axes.spines.right':False,'axes.grid':True,'grid.alpha':.2,'figure.facecolor':'#fafbfe','axes.facecolor':'#fafbfe'})
def read(name): return json.loads((ROOT/name/'fpga-recording.json').read_text())
def series(r,id):
    obs=[next(o for o in f['observations'] if o['id']==id) for f in r['frames']]
    t=np.array([(o['request_s']+o['completion_s'])/2 for o in obs])
    actual=np.array([o['telemetry']['position_raw']-r['home'][id-4] for o in obs])*DEG
    ct=np.array([f['command_receipt_s'] for f in r['frames']])
    target=np.array([r['plan']['targets'][f['tick']][id-4] for f in r['frames']])*DEG
    held=np.array([r['plan']['targets'][max(f['tick']-1,0)][id-4] for f in r['frames']])*DEG
    return t,actual,ct,target,held,obs
runs={}
for file in sorted(ROOT.glob('*/fpga-recording.json')):
    r=json.loads(file.read_text());motor={}
    for id in r['plan']['ids']:
        if not r['frames']:continue
        t,x,ct,y,held,obs=series(r,id);err=x-held
        motor[str(id)]={'rms_deg':float(np.sqrt(np.mean(err**2))),'peak_deg':float(max(abs(err))),
            'encoder_span_counts':int(round((max(x)-min(x))/DEG)),
            'positive_sampled_travel_counts':int(round(sum(np.maximum(np.diff(x),0))/DEG)),
            'negative_sampled_travel_counts':int(round(sum(np.maximum(-np.diff(x),0))/DEG)),
            'maximum_temperature_c':max(o['telemetry']['temperature_c'] for o in obs),
            'voltage_range_v':[min(o['telemetry']['voltage_v'] for o in obs),max(o['telemetry']['voltage_v'] for o in obs)],
            'per_frame_torque_verified':all(f.get('torque_readback') and f['torque_readback'][id-4]==1 for f in r['frames'])}
    runs[file.parent.name]={'completed':r['completed'],'failure':r['failure'],'stop_verified':r['stop_verified'],
        'frames':len(r['frames']),'motor_updates':len(r['frames'])*len(r['plan']['ids']),
        'all_pwm_arithmetic_matched':all(f['arithmetic_matches'] for f in r['frames']),
        'period_s':r['plan']['period_s'],'drive_limit_percent':r['plan']['gains']['limit']/10,
        'role':r['plan']['role'],'motors':motor,'recording_sha256':hashlib.sha256(file.read_bytes()).hexdigest()}
name='nine-faster-7p5pct-validation';r=read(name)
fig,axs=plt.subplots(3,3,figsize=(14,10),sharex=True,sharey=True)
for id,ax in zip(r['plan']['ids'],axs.flat):
    t,x,ct,y,held,obs=series(r,id);s=runs[name]['motors'][str(id)]
    ax.step(np.r_[0,ct],np.r_[0,y],where='post',color='#687386',ls='--',label='FPGA setpoint')
    ax.plot(t,x,color='#1674b8',marker='.',ms=3,label='Measured encoder')
    ax.set_title(f"Motor {id}  |  RMS {s['rms_deg']:.2f}° / peak {s['peak_deg']:.2f}°")
    ax.set_ylim(-7,7)
for ax in axs[-1,:]:ax.set_xlabel('Captured host time (s)')
for ax in axs[:,0]:ax.set_ylabel('Relative encoder angle (°)')
fig.suptitle('Actual FPGA-controlled motors: faster nine-axis validation',fontsize=19,y=.99)
handles,labels=axs[0,0].get_legend_handles_labels();fig.legend(handles,labels,loc='lower center',ncol=2,bbox_to_anchor=(.5,.027))
fig.text(.5,.008,'7.5% PWM cap • 150 ms host schedule • per-frame torque and PWM readbacks • unloaded bench • all accuracy gates failed',ha='center',fontsize=10,color='#8b3a27')
fig.tight_layout(rect=[0,.07,1,.955]);fig.savefig(ROOT/'nine-motor-tracking.png',dpi=160);fig.savefig(ROOT/'nine-motor-tracking.svg');plt.close(fig)

prediction=json.loads((ROOT/'nine-faster-closed-loop-prediction.json').read_text())['predictions']
fig,axs=plt.subplots(3,3,figsize=(14,10),sharex=True,sharey=True)
for p,ax in zip(prediction,axs.flat):
    id=p['id'];t,x,ct,y,held,obs=series(r,id);s=np.array(p['samples_time_encoder_duty_angle'])
    ax.plot(t,x,color='#1674b8',marker='.',ms=3,label='Actual motor')
    ax.plot(s[:,0],(s[:,1]-r['home'][id-4])*DEG,color='#ce762b',label='Same controller in simulation')
    ax.set_title(f"Motor {id}  |  prediction RMS {p['rms_prediction_degrees']:.2f}°")
    ax.set_ylim(-7,7)
for ax in axs[-1,:]:ax.set_xlabel('Captured host time (s)')
for ax in axs[:,0]:ax.set_ylabel('Relative encoder angle (°)')
fig.suptitle('Same integer controller, simulated feedback vs physical feedback',fontsize=18,y=.99)
h,l=axs[0,0].get_legend_handles_labels();fig.legend(h,l,loc='lower center',ncol=2,bbox_to_anchor=(.5,.027))
fig.text(.5,.008,'Original motor hypothesis; independently simulated quantized feedback • mean measured supply boundary • not an accepted calibration',ha='center',fontsize=10,color='#8b3a27')
fig.tight_layout(rect=[0,.07,1,.955]);fig.savefig(ROOT/'nine-motor-simulation-comparison.png',dpi=160);plt.close(fig)

fig,axs=plt.subplots(3,1,figsize=(13,9),sharex=True)
t,x,ct,y,held,obs=series(r,4)
axs[0].plot(t,x,label='Measured motor 4',color='#1674b8');axs[0].step(ct,y,where='post',ls='--',label='FPGA setpoint',color='#687386');axs[0].set_ylabel('Position (°)');axs[0].legend(ncol=2)
v=np.gradient(x,t);a=np.gradient(v,t)
axs[1].plot(t,v,color='#1674b8');axs[1].set_ylabel('Estimated velocity (°/s)')
axs[2].plot(t,a,color='#1674b8');axs[2].set_ylabel('Estimated acceleration (°/s²)');axs[2].set_xlabel('Captured host time (s)')
fig.suptitle('Changing speed and acceleration: sampled physical response',fontsize=18)
fig.text(.5,.015,'Velocity/acceleration are finite differences of the internal encoder, not independent sensors.\n150 ms sampling and quantization limit resolved transients and amplify derivative noise.',ha='center',fontsize=10)
fig.tight_layout(rect=[0,.075,1,.95]);fig.savefig(ROOT/'sampled-kinematics.png',dpi=160);plt.close(fig)

stationary={}
with (ROOT/'final-stationary-observation/telemetry.csv').open() as f:rows=list(csv.DictReader(f))
for id in range(4,13):
    rs=[x for x in rows if int(x['id'])==id];pos=[int(x['position_raw']) for x in rs]
    stationary[str(id)]={'samples':len(rs),'encoder_span_counts':max(pos)-min(pos),'all_reported_speeds_zero':all(int(x['speed_raw'])==0 for x in rs),'window_s':float(rs[-1]['completion_s'])-float(rs[0]['request_s'])}
summary={'scope':'Physical unloaded bench; internal encoder feedback; not full-load or endurance acceptance','runs':runs,'final_stationary_observation':stationary,
    'model_validation':[{'id':p['id'],'rms_deg':p['rms_prediction_degrees'],'pass':p['prediction_pass']} for p in prediction],
    'limitations':['FPGA computes control; host schedules feedback and setpoints','No calibrated measured amps or watts','Independent motor simulations do not validate shared battery behavior','No motor model was accepted or promoted']}
(ROOT/'results-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
rows=''.join(f"<tr><td>{id}</td><td>{runs[name]['motors'][str(id)]['rms_deg']:.2f}°</td><td>{runs[name]['motors'][str(id)]['peak_deg']:.2f}°</td><td>{p['rms_prediction_degrees']:.2f}°</td></tr>" for id,p in zip(r['plan']['ids'],prediction))
def picture(name):return '<img alt="Measured motor experiment plot" src="data:image/png;base64,'+base64.b64encode((ROOT/name).read_bytes()).decode()+'">'
page=f'''<!doctype html><meta charset="utf-8"><title>FPGA motor controller experiments</title><style>body{{max-width:1180px;margin:45px auto;padding:0 24px;font:17px/1.6 system-ui;color:#203043;background:#fafbfe}}h1{{font-size:36px;line-height:1.15}}h2{{margin-top:36px}}img{{width:100%;height:auto}}table{{border-collapse:collapse;width:100%}}th,td{{text-align:left;border-bottom:1px solid #d5dce5;padding:9px}}.note{{padding:18px;background:#fff0db;border-left:4px solid #bd7124}}a{{color:#176fa7}}</style>
<h1>Actual FPGA controller, measured motors, shared Rust simulation</h1>
<p>The FPGA executed the feedback-to-PWM controller on the connected motors. Rust executes the same integer expression graph against the shared motor model. All nine motors moved in both final coordinated 7.5% runs; every recorded PWM and torque-enable readback in those runs matched the expected state.</p>
<p><b>Best small single-motor pilot:</b> 0.20° RMS, 0.62° peak. <b>Faster nine-motor validation:</b> 0.91–1.36° RMS, 3.60° worst peak. These are different trajectories and must not be treated as a like-for-like improvement.</p>
<p class="note"><b>Not yet accepted for accurate sim-to-real use.</b> Every motor missed the frozen faster-trajectory tracking gates (0.264° RMS / 0.879° peak). The model also missed the 3-count prediction gate on every axis. A 60-evaluation parameter fit was rejected after its reserved validation failed.</p>
<h2>Faster simultaneous-motor result</h2><table><tr><th>Motor</th><th>Tracking RMS</th><th>Tracking peak</th><th>Own-feedback prediction RMS</th></tr>{rows}</table>
{picture('nine-motor-tracking.png')}
<h2>Controller transfer to simulation</h2><p>The simulated controller uses its own quantized encoder feedback. The physical observations are used only for comparison. Recorded-PWM replay is retained separately; controller feedback can mask motor-model errors, so neither test substitutes for the other.</p>{picture('nine-motor-simulation-comparison.png')}
<h2>Changing velocities and accelerations</h2>{picture('sampled-kinematics.png')}
<h2>What the tests exposed</h2><p>The first all-nine 100 ms loop took 174 ms and was rejected. Batch watchdog renewal enabled a measured 150 ms host schedule. Low-drive motion varied between motors and trials: some axes did not move, or moved mainly one way, at 5%. Explicit torque readbacks and a bounded 7.5% stage produced measurable motion on all nine. These observations do not uniquely identify friction, driver behavior or load.</p>
<p>The host still schedules polling and targets. The next controller improvement is deterministic FPGA polling/trajectory timing, followed by per-axis tuning and better-tested low-speed motor dynamics. Calibrated current/power sensing and loaded joint experiments are needed before battery or full-load validation.</p>
<h2>Stopped state and evidence</h2><p>All nine were finally read back at PWM zero, torque disabled and zero reported speed. A separate stationary observation retained encoder positions after the run. Independent command-loss, telemetry-loss and absent-host-traffic watchdog tests passed on each loaded image. No physical S2, disconnected-bus or supply-isolation test is claimed.</p>
<p>Voltage is a 0.1 V register measurement. Current remains uncalibrated raw counts; measured amps and watts are unavailable. These are short unloaded tests at at most 7.5% PWM, not endurance or rated-current stress tests.</p><p><a href="results-summary.json">Machine-readable results</a> · <a href="STATUS.md">Status and remaining work</a> · <a href="README.md">Reproduction commands</a></p>'''
(ROOT/'report.html').write_text(page)
print(json.dumps({'runs':len(runs),'motor_updates':sum(x['motor_updates'] for x in runs.values()),'stationary':stationary},indent=2))
