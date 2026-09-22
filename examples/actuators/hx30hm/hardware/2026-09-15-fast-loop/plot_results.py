"""Plot acquired encoder data and already-computed shared Rust scores."""
from pathlib import Path
import json
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
r=Path(__file__).resolve().parent
plt.rcParams.update({'font.size':10,'axes.spines.top':False,'axes.spines.right':False})
fig,axes=plt.subplots(3,1,figsize=(10,8),sharex=True,layout='constrained')
for ax,motor in zip(axes,[10,11,12]):
 for ms,color in [(40,'#c36d33'),(10,'#087e8b')]:
  rec=json.loads((r/f'motion-{ms}ms-cap100-01/fpga-recording.json').read_text());obs=[o for f in rec['frames'] for o in f['observations'] if o['id']==motor]
  ax.plot([(o['request_s']+o['completion_s'])/2 for o in obs],[(o['telemetry']['position_raw']-rec['home'][motor-4])*360/4096 for o in obs],'.-',ms=3,lw=1.5,color=color,label=f'{ms} ms measured')
  ax.step([f['command_receipt_s'] for f in rec['frames']],[p[motor-4]*360/4096 for p in rec['plan']['targets']],where='post',lw=.8,ls='--',color=color,alpha=.6,label=f'{ms} ms applied target')
 ax.set_ylabel(f'ID {motor}\nAngle (degrees)');ax.grid(alpha=.2);ax.set_xlim(0,.8)
axes[0].legend(ncol=2);axes[-1].set_xlabel('Device time (seconds)')
fig.suptitle('Real three-motor motion: 40 ms versus 10 ms at 10% drive\nFirst trial of each configuration; first 0.8 s shown; fixed gains unchanged')
fig.savefig(r/'cadence-comparison.png',dpi=160);fig.savefig(r/'cadence-comparison.pdf')
