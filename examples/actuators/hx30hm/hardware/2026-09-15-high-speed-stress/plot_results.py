"""Plot recorded hardware feedback and shared Rust speed estimates."""
import json
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
root=Path(__file__).resolve().parent
name='cap550-01';r=json.loads((root/name/'fpga-recording.json').read_text());m=json.loads((root/name/'motion-review.json').read_text())
fig,axes=plt.subplots(4,1,figsize=(12,11),sharex=True,layout='constrained')
for motor,color in zip([10,11,12],['#0d8190','#cd681f','#6543a0']):
 obs=[o for f in r['frames'] for o in f['observations'] if o['id']==motor];t=[o['request_s'] for o in obs]
 axes[0].plot(t,[(o['telemetry']['position_raw']-r['home'][motor-4])*360/4096 for o in obs],label=f'Motor {motor}',color=color)
 speed=m['axes'][str(motor)]['speed_rad_s'];axes[1].plot([x['time_s'] for x in speed],[x['value']*180/3.141592653589793 for x in speed],color=color)
 axes[2].plot(t,[f['pwm_readback'][motor-4]/10 for f in r['frames']],color=color)
 axes[3].plot(t,[o['telemetry']['voltage_v'] for o in obs],color=color)
axes[0].plot([i*.01 for i in range(len(r['frames']))],[x[6]*360/4096 for x in r['plan']['targets'][:len(r['frames'])]],'--',color='gray',label='Commanded target')
axes[0].legend(ncol=4,loc='upper right',fontsize=10)
for ax,label in zip(axes,['Relative angle (degrees)','Encoder secant speed (degrees/s)','Audited PWM (%)','Motor voltage (V)']):ax.set_ylabel(label);ax.grid(alpha=.2)
axes[-1].set_xlabel('FPGA time (seconds)');axes[-1].set_xlim(0,1.99)
fig.suptitle('Three real motors at 100 Hz: 55% drive, repeated reversals\nCompleted and stopped; 12.6% voltage sag ended drive escalation',fontsize=15)
fig.savefig(root/'stress-trace.png',dpi=150);fig.savefig(root/'stress-trace.pdf')
