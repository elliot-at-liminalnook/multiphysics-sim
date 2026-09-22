"""Plot saved Rust results only. No acquisition, control or physics calculations."""
import json
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

ROOT = Path(__file__).resolve().parent
DEG = 360 / 4096
RAD_TO_DEG = 180 / 3.141592653589793
def read(path): return json.loads((ROOT / path).read_text())
plt.rcParams.update({'font.size': 10, 'axes.spines.top': False,
                     'axes.spines.right': False, 'axes.grid': True, 'grid.alpha': .18})
fig, axes = plt.subplots(3, 1, figsize=(12, 10), constrained_layout=True)
fig.suptitle('Actual FPGA controller · unloaded motor bench · September 14\n'
             'Requested ramp: 3.52°/s · drive cap: 7.5% · all nine stop checks passed', fontsize=14)
r = read('ramp-nine-repeat/fpga-recording.json')
for motor in r['plan']['ids']:
    samples = [next(o for o in f['observations'] if o['id'] == motor) for f in r['frames']]
    axes[0].plot([(o['request_s'] + o['completion_s']) / 2 for o in samples],
                 [(o['telemetry']['position_raw'] - r['home'][motor-4])*DEG for o in samples],
                 label=f'ID {motor}', lw=1.2)
axes[0].step([f['command_receipt_s'] for f in r['frames']],
             [r['plan']['targets'][f['tick']][0]*DEG for f in r['frames']],
             where='post', color='black', linestyle='--', label='Target', lw=2)
axes[0].set(title='All nine motors moving together — repeated trial', ylabel='Angle from start (°)')
axes[0].legend(ncol=5, fontsize=9, loc='upper right')

comparison = read('nine-repeat-comparison.json')
t = comparison['trials'][0]
for label, color, motion in [('Real motor ID4', '#1675ba', t['measured_motion']),
                             ('Same controller in simulation', '#278347', t['predicted_motion'])]:
    values = motion['speed_rad_s']
    axes[1].plot([s['time_s'] for s in values], [s['value']*RAD_TO_DEG for s in values],
                 label=label, color=color, marker='.', lw=1.4)
axes[1].set(title='Speed varies within the ramp; stable-speed settling is unresolved', ylabel='Estimated speed (°/s)')
axes[1].legend(loc='lower left', fontsize=9)
for tick in [17, 37]: axes[1].axvline(r['frames'][tick]['command_receipt_s'], color='#888', linestyle=':')
axes[1].text(.99, .97, 'Dotted lines: commanded reversal\n0.30 s secants; sensor sample age unknown',
             transform=axes[1].transAxes, ha='right', va='top', fontsize=9)

p = read('nine-repeat-predictions.json')
scores = read('ramp-nine-repeat/scores.json')['motors']
ids = r['plan']['ids']
axes[2].bar([i-.18 for i in ids], [scores[str(i)]['rms_degrees'] for i in ids],
            width=.36, label='Actual controller tracking error', color='#1675ba')
axes[2].bar([i+.18 for i in ids], [next(x for x in p['predictions'] if x['id']==i)['rms_prediction_degrees'] for i in ids],
            width=.36, label='Simulation vs measurement error', color='#278347')
axes[2].axhline(3*DEG, linestyle='--', color='black', label='Unchanged RMS acceptance limit')
axes[2].set(title='Both tracking and model prediction still miss the accuracy target',
            xlabel='Motor ID', ylabel='RMS error (°)', xticks=ids, ylim=(0, .95))
axes[2].legend(fontsize=9, loc='upper right')
for ax in axes[:2]: ax.set_xlabel('Captured host time (s)')
fig.savefig(ROOT/'measured-response.png', dpi=160)
fig.savefig(ROOT/'measured-response.svg')
