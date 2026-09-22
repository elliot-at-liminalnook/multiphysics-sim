"""Summarize recorded telemetry; motion estimation stays in the shared Rust library."""
import json, pathlib, subprocess, sys
root = pathlib.Path(__file__).resolve().parent
for name in sys.argv[1:]:
    folder = root / name
    r = json.loads((folder / 'fpga-recording.json').read_text())
    motion = folder / 'motion-review.json'
    if not motion.exists() and r['completed'] and r['stop_verified']:
        subprocess.run(['target/debug/examples/review_controller', 'measure-fpga-motion',
                        str(folder / 'fpga-recording.json'),
                        str(root / ('estimator-10ms.json' if r['plan']['period_s'] < .04 else 'estimator-baseline.json')),
                        str(motion)], check=True)
    axes = []
    for motor in r['plan']['ids']:
        samples = [o['telemetry'] for f in r['frames'] for o in f['observations'] if o['id'] == motor]
        base = r['initial']['physical_preflight'][str(motor)]['voltage_v']
        low = min(t['voltage_v'] for t in samples)
        axes.append(dict(id=motor, baseline_v=base, minimum_v=low,
                         drop_percent=100*(base-low)/base,
                         maximum_temperature_c=max(t['temperature_c'] for t in samples),
                         maximum_current_raw_uncalibrated=max(t['current_raw'] for t in samples),
                         maximum_travel_counts=max(abs(t['position_raw']-r['home'][motor-4]) for t in samples),
                         minimum_pwm=min(f['pwm_readback'][motor-4] for f in r['frames']),
                         maximum_pwm=max(f['pwm_readback'][motor-4] for f in r['frames'])))
    summary = dict(capture=name, completed=r['completed'], stop_verified=r['stop_verified'],
                   failure=r['failure'], frames=len(r['frames']), axes=axes)
    path = folder / 'telemetry-summary.json'
    with path.open('x') as f: json.dump(summary, f, indent=2)
    print(json.dumps(summary, indent=2))
