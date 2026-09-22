"""Pure comparisons of native capture files; no equations are reimplemented."""
import json,math
from pathlib import Path
HERE=Path(__file__).resolve().parent
names=['be6400','be12800','be25600','sdirk800','sdirk1600','sdirk6400','sdirk12800']
runs={n:json.loads((HERE/(n+'.json')).read_text()) for n in names}
for r in runs.values():assert r['window_complete'] and not r['error'] and len(r['frames'])==81

def compare(a,b):
    maximum=dict(motor_angle_rad=0,link_position_m=0,current_a=0,contact_resultant_n=0)
    differing_contact_frames=0
    def forces(frame):
        out={}
        for c in frame['contacts']:
            v=out.setdefault((c['link'],c['other']),[0.,0.,0.])
            for j in range(3):v[j]+=c['force_n'][j]
        return out
    for x,y in zip(a['frames'],b['frames']):
        assert x['time_s']==y['time_s']
        maximum['motor_angle_rad']=max(maximum['motor_angle_rad'],*(abs(x['joint_positions'][i]-y['joint_positions'][i]) for i in a['metadata']['independent_joint_indices']))
        maximum['link_position_m']=max(maximum['link_position_m'],*(math.dist(i['position_m'],j['position_m']) for i,j in zip(x['poses'],y['poses'])))
        maximum['current_a']=max(maximum['current_a'],*(abs(i['current_a']-j['current_a']) for i,j in zip(x['motor_readings'],y['motor_readings'])))
        differing_contact_frames+= [(c['link'],c['other']) for c in x['contacts']]!=[(c['link'],c['other']) for c in y['contacts']]
        f,g=forces(x),forces(y)
        for key in f.keys()|g.keys():maximum['contact_resultant_n']=max(maximum['contact_resultant_n'],math.dist(f.get(key,[0,0,0]),g.get(key,[0,0,0])))
    return {'max_error':maximum,'different_contact_identity_frames':differing_contact_frames}
pairs=[('be6400','be12800'),('be12800','be25600'),('sdirk800','sdirk12800'),('sdirk1600','sdirk12800'),('sdirk6400','sdirk12800'),('be25600','sdirk12800')]
report={'comparisons':{a+'_vs_'+b:compare(runs[a],runs[b]) for a,b in pairs},'scope':'Unpowered zero-voltage 0.1 s settling diagnostic, no controller or gait. Reference is another finite-step method, not an exact solution. Contact resultant is summed by link/other pair, including absent pairs as zero; it is not the per-contact gate used for gait qualification. No timing acceptance: compilation overlapped these runs.'}
(HERE/'comparison.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
