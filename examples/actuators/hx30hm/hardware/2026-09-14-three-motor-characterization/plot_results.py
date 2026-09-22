"""Plot physical recordings and shared Rust motion estimates; no plant implementation."""
import json, math
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
root=Path(__file__).resolve().parent
plt.rcParams.update({'font.size':10,'axes.spines.top':False,'axes.spines.right':False})
colors={10:'#087e8b',11:'#ca5b20',12:'#6f42a7'}
def read(name,file):return json.loads((root/name/file).read_text())
fig,axes=plt.subplots(3,1,figsize=(11,9),sharex=True,layout='constrained')
name='extended-fast-small-group-350-03';r=read(name,'fpga-recording.json');m=read(name,'motion-review.json')
for motor in [10,11,12]:
 c=colors[motor];a=m['axes'][str(motor)];series=a['speed_rad_s']
 axes[0].plot([x['time_s'] for x in series],[x['value']*180/math.pi for x in series],lw=1.2,color=c,label=f'ID {motor}')
 obs=[o for f in r['frames'] for o in f['observations'] if o['id']==motor]
 t=[(o['request_s']+o['completion_s'])/2 for o in obs]
 axes[1].plot(t,[o['telemetry']['voltage_v'] for o in obs],lw=1,color=c)
 axes[2].plot(t,[(o['telemetry']['position_raw']-r['home'][motor-4])*360/4096 for o in obs],lw=1,color=c)
axes[2].step([f['command_receipt_s'] for f in r['frames']],[row[6]*360/4096 for row in r['plan']['targets']],where='post',lw=1,color='#303030',ls='--',label='Requested position')
axes[0].legend(ncol=3,loc='upper right');axes[2].legend(loc='upper right')
for a in axes:a.grid(alpha=.2)
axes[0].set_ylabel('Sampled speed (degrees/s)');axes[1].set_ylabel('Servo voltage (V)');axes[2].set_ylabel('Position from home (degrees)');axes[2].set_xlabel('Device time (seconds)')
fig.suptitle('Three real motors — third 10-second reversal block at 35% drive\n12 V supply setting; all stops verified; existing FPGA controller')
fig.savefig(root/'extended-response.png',dpi=160);fig.savefig(root/'extended-response.pdf');plt.close(fig)
fig,ax=plt.subplots(figsize=(9,5),layout='constrained')
labels=['25% short','50% short','75% short','45% repeated','35% extended'];names=['stage-250-01','stage-500-01','stage-750-01','training-fast-group-450-01','extended-fast-small-group-350-03']
for motor in [10,11,12]:
 drops=[next(a['drop_percent'] for a in read(n,'telemetry-summary.json')['axes'] if a['id']==motor) for n in names]
 ax.plot(labels,drops,'o-',color=colors[motor],label=f'ID {motor}')
ax.axhline(10,color='gray',ls='--',label='Provisional low-sag target')
ax.set_ylabel('Drop from each motor’s preflight voltage (%)');ax.set_title('Voltage sag depends on drive and motion pattern\n75% exceeds the target; 45% longer trials hit the tracking guard')
ax.legend(ncol=2);ax.grid(alpha=.2);fig.savefig(root/'voltage-sag.png',dpi=160);plt.close(fig)
fig,axs=plt.subplots(3,1,figsize=(10,8),sharex=True,layout='constrained')
r=read('validation-hold-small-group-350-01','fpga-recording.json')
for ax,motor in zip(axs,[10,11,12]):
 obs=[o for f in r['frames'] for o in f['observations'] if o['id']==motor]
 ax.plot([(o['request_s']+o['completion_s'])/2 for o in obs],[(o['telemetry']['position_raw']-r['home'][motor-4])*360/4096 for o in obs],color='#151515',label='Measured',lw=2)
 for model,color in [('baseline','#ba6842'),('candidate','#087e8b')]:
  p=next(p for p in json.loads((root/f'validation-{model}-closed-loop.json').read_text())['predictions'] if p['id']==motor)
  ax.plot([x[0] for x in p['samples_time_encoder_duty_angle']],[(x[1]-r['home'][motor-4])*360/4096 for x in p['samples_time_encoder_duty_angle']],color=color,label=model.capitalize(),lw=1.3)
 ax.set_ylabel(f'ID {motor}\nPosition (degrees)');ax.grid(alpha=.2)
axs[0].legend(ncol=3);axs[-1].set_xlabel('Device time (seconds)')
fig.suptitle('Separate validation pattern: real motion versus closed-loop simulation\nIndividual candidates; every motor still fails the accuracy target')
fig.savefig(root/'model-validation.png',dpi=160);plt.close(fig)
