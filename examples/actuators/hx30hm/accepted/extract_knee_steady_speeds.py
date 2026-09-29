"""Extract servo 1's (knee) steady free-running speeds from campaign
campaign-1790182543587 into a small dataset the generic part fitter uses.

Source: stage D (and its repeat) fitted each commanded duty's steady speed
from the encoder trace on the suspended leg. Output speed is converted with
the encoder's 4096 counts per output revolution. The supply voltage is the
median reported while the servo was driven. The report itself (4.7 MB) is
referenced by path and SHA-256, not copied.

    python3 examples/actuators/hx30hm/accepted/extract_knee_steady_speeds.py
"""
import hashlib, json, math, os, statistics

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..', '..', '..'))
REPORT = 'examples/actuators/hx30hm/hardware/2026-09-21-leg-calibration/measurements/campaigns/campaign-1790182543587/report.json'
OUT = 'examples/actuators/hx30hm/accepted/knee-steady-speeds.json'
COUNTS_PER_RAD = 4096 / (2 * math.pi)

with open(os.path.join(ROOT, REPORT), 'rb') as f:
    raw = f.read()
report = json.loads(raw)
driven = [s['voltage_v'] for s in report['samples'] if s['id'] == 1 and s['duty'] is not None and abs(s['duty']) > 0.1 and s['voltage_v'] is not None]
supply = statistics.median(driven)
points = []
for run, stage in enumerate(s for s in report['stages'] if s['stage'] == 'D' and s['id'] == 1):
    for fit in stage['metrics']['fits']:
        points.append({'run': run, 'duty': fit['duty'], 'speed_rad_s': fit['speed_counts_s'] / COUNTS_PER_RAD, 'time_constant_s': fit['time_constant_s']})
dataset = {
    'schema': 'sim.fit-data/1',
    'description': 'HX-30HM servo 1 (knee) steady output speed versus commanded duty, suspended leg, both directions, two runs.',
    'source': {'path': REPORT, 'sha256': hashlib.sha256(raw).hexdigest(), 'stages': 'D (+ repeat), servo id 1'},
    'conditions': {'supply_v': supply, 'counts_per_rad': COUNTS_PER_RAD, 'temperature_c': 38.0},
    'points': points,
}
with open(os.path.join(ROOT, OUT), 'w') as f:
    json.dump(dataset, f, indent=1)
    f.write('\n')
print(f'{len(points)} points at {supply} V → {OUT}')
