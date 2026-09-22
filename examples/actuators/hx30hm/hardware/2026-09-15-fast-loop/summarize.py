"""Evidence summaries; motor-response estimation uses the shared Rust analyzer."""
import json, math, statistics, subprocess
from pathlib import Path
root=Path(__file__).resolve().parent
captures=[]
for path in sorted(root.glob('*/fpga-recording.json')):
    r=json.loads(path.read_text());ms=r['plan']['period_s']*1000
    out=dict(capture=path.parent.name,period_ms=ms,drive_cap_percent=r['plan']['gains']['limit']/10,
             completed=r['completed'],stop_verified=r['stop_verified'],frames=len(r['frames']),motors={})
    if ms==10 and r['plan']['gains']['limit']:
        target=path.parent/'motion-40ms-window.json'
        if not target.exists():subprocess.run(['target/debug/examples/review_controller','measure-fpga-motion',str(path),str(root/'estimator-40ms-window.json'),str(target)],check=True)
    for motor in r['plan']['ids']:
        obs=[o for f in r['frames'] for o in f['observations'] if o['id']==motor]
        gaps=[1000*(b['request_s']-a['request_s']) for a,b in zip(obs,obs[1:])]
        changed=sum(a['telemetry']['position_raw']!=b['telemetry']['position_raw'] for a,b in zip(obs,obs[1:]))
        m=dict(minimum_request_gap_ms=min(gaps),median_request_gap_ms=statistics.median(gaps),maximum_request_gap_ms=max(gaps),
               position_changed_pairs=changed,total_adjacent_pairs=len(gaps),
               voltage_min_v=min(o['telemetry']['voltage_v'] for o in obs),max_temperature_c=max(o['telemetry']['temperature_c'] for o in obs))
        review=path.parent/'motion-review.json'
        if review.exists():
            scores=json.loads(review.read_text())['scores']['motors'][str(motor)]
            m.update(tracking_rms_deg=scores['rms_degrees'],tracking_peak_deg=scores['peak_degrees'],tracking_pass=scores['tracking_pass'])
        out['motors'][str(motor)]=m
    event=json.loads((path.parent/'device-review.json').read_text());hz=event['clock_hz'];frames={}
    for e in event['events']:
        if e['kind'] in ['Telemetry','Control','Audit']:frames.setdefault(e['frame'],[]).append(e)
    windows=[1000*(max(e['completion_ticks'] for e in es)-min(e['request_ticks'] for e in es))/hz for es in frames.values()]
    out['maximum_frame_transaction_window_ms']=max(windows)
    captures.append(out)
comparisons=[]
for motor in [10,11,12]:
    row={'id':motor}
    for ms in [40,10]:
        group=[r for r in captures if r['period_ms']==ms and r['drive_cap_percent']==10]
        errs=[r['motors'][str(motor)]['tracking_rms_deg'] for r in group]
        row[f'{ms}ms_pooled_rms_deg']=math.sqrt(sum(e*e for e in errs)/len(errs))
    row['rms_reduction_percent']=100*(1-row['10ms_pooled_rms_deg']/row['40ms_pooled_rms_deg']);comparisons.append(row)
summary={'captures':captures,'comparison':comparisons,'total_frames':sum(x['frames'] for x in captures),'all_completed':all(x['completed'] for x in captures),'all_stop_verified':all(x['stop_verified'] for x in captures),
         'interpretation':'Matched two-second physical trajectories at 10 percent drive; fixed discrete gains unchanged. Feedforward depends on per-tick target delta, so this compares configurations rather than isolating cadence. Reply timestamps do not bound internal sensor age; encoder change fractions are not sensor freshness certification. No model fit or promotion.'}
(root/'results.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({'comparison':comparisons,'total_frames':summary['total_frames'],'all_completed':summary['all_completed'],'all_stop_verified':summary['all_stop_verified']},indent=2))
print('Fast intervals:',[(r['capture'],r['motors']['10']['minimum_request_gap_ms'],r['motors']['10']['maximum_request_gap_ms'],r['maximum_frame_transaction_window_ms']) for r in captures if r['period_ms']==10])
