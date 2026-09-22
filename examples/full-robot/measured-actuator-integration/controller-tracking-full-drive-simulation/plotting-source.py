import json, pathlib, math
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
root=pathlib.Path('/Users/elliot/physics-simulator/examples/full-robot/measured-actuator-integration')
full=root/'controller-tracking-full-drive-simulation'
plt.rcParams.update({'font.size':10,'axes.spines.top':False,'axes.spines.right':False})
fig,axs=plt.subplots(3,3,figsize=(14,9),constrained_layout=True)
for row,pattern in enumerate(['hold','reversal','full-gait']):
 for col,id in enumerate([10,11,12]):
  ax=axs[row,col]
  traces={label:json.load(open(full/f'id{id}-{pattern}-nominal-{label}-1000.json')) for label in ['original','selected']}
  s=traces['selected']['samples']; t=[s['time_s'] for s in s]
  ax.plot(t,[x['desired_counts']*360/4096 for x in s],color='#343b49',lw=1.4,label='Requested')
  for label,color in [('original','#e49c38'),('selected','#178079')]:
   s=traces[label]['samples']; rms=traces[label]['metrics']['rms_degrees']
   ax.plot(t,[x['encoder_counts']*360/4096 for x in s],color=color,lw=1,label=f'{label.title()} ({rms:.2f}° RMS)')
  ax.set_title(f'Motor estimate {id} · {pattern}');ax.set_xlabel('Seconds');ax.set_ylabel('Degrees');ax.grid(alpha=.15);ax.legend(fontsize=8)
fig.suptitle('Simulation only · same motor models, original vs tuned controller\n100 Hz, 11.8 V, 2 ms delay, full drive available — full gait still fails to track',fontsize=14)
fig.savefig(full/'tracking-comparison.png',dpi=150)
fig.savefig(full/'tracking-comparison.svg')
plt.close(fig)
paths=[full/'robustness'/f'id{id}-governed-1000.json' for id in [10,11,12]]
if all(p.exists() for p in paths):
 fig,axs=plt.subplots(3,1,figsize=(12,9),constrained_layout=True)
 for ax,id,p in zip(axs,[10,11,12],paths):
  data=json.load(open(p));s=data['simulation']['samples'];t=[x['time_s'] for x in s]
  ax.plot(t,[x*360/4096 for x in data['original_counts']],color='#a8acb2',lw=1,label='Original gait request')
  ax.plot(t,[x['desired_counts']*360/4096 for x in s],color='#426fc0',lw=1.4,label='Speed/acceleration limited command')
  ax.plot(t,[x['encoder_counts']*360/4096 for x in s],color='#178079',lw=1.2,label='Simulated encoder')
  ax.set_title(f'Motor estimate {id} · error to limited command {data["simulation"]["metrics"]["rms_degrees"]:.2f}° RMS');ax.set_xlabel('Seconds');ax.set_ylabel('Degrees');ax.grid(alpha=.15);ax.legend(fontsize=8)
 fig.suptitle('Simulation only · limiting impossible commands changes the gait\nErrors to the original gait are retained separately; balance and foot placement are unverified',fontsize=14)
 fig.savefig(full/'governed-reference.png',dpi=150);fig.savefig(full/'governed-reference.svg');plt.close(fig)
p=full/'reference-envelope/summary.json'
if p.exists():
 data=json.load(open(p))
 fig,axs=plt.subplots(1,2,figsize=(12,4.5),constrained_layout=True)
 for id,color in [(10,'#426fc0'),(11,'#178079'),(12,'#cb7133')]:
  rows=[r for r in data if r['model']==f'id{id}']
  speed=[r['maximum_command_speed_degrees_s'] for r in rows]
  axs[0].plot(speed,[r['motor_tracking_command']['rms_degrees'] for r in rows],'o-',color=color,label=f'Motor estimate {id}')
  axs[1].plot(speed,[r['motor_error_to_original_rms_degrees'] for r in rows],'o-',color=color,label=f'Motor estimate {id}')
 for ax in axs:ax.set_xlabel('Command speed limit (degrees/s)');ax.set_ylabel('RMS angle error (degrees)');ax.grid(alpha=.2);ax.legend()
 axs[0].set_title('Motor error to the limited command')
 axs[1].set_title('Motor error to the original gait request')
 fig.suptitle('Simulation only · better tracking of a limited command does not preserve the gait\nAcceleration limit = 5 × speed limit per second; full drive available',fontsize=12)
 fig.savefig(full/'reference-envelope.png',dpi=160);fig.savefig(full/'reference-envelope.svg');plt.close(fig)
