"""Declarative, unit-aware joint motion patterns for CAD inspection."""
import math
from copy import deepcopy
from .kernel import KernelError
from .pose import PoseModel


def validate_program(doc, program):
    if not isinstance(program,dict) or set(program)-{'name','duration','loop','tracks'}:
        raise KernelError('Motion needs name, duration, loop and tracks')
    p=deepcopy(program)
    if not isinstance(p.get('name'),str) or not p['name'].strip(): raise KernelError('Name the motion pattern')
    def finite(v):return isinstance(v,(int,float)) and not isinstance(v,bool) and math.isfinite(v)
    if not finite(p.get('duration')) or not .05 <= p['duration'] <= 600: raise KernelError('Duration must be 0.05–600 seconds')
    if not isinstance(p.get('loop',False),bool): raise KernelError('loop must be boolean')
    if not isinstance(p.get('tracks'),list) or not 1 <= len(p['tracks']) <= 100: raise KernelError('Supply 1–100 motion tracks')
    model=PoseModel(doc); seen=set()
    for t in p['tracks']:
        if not isinstance(t,dict) or set(t)-{'joint','unit','keys'}: raise KernelError('Track needs joint, unit and keys')
        jid=t.get('joint')
        if jid not in model.joints:
            matches=[i for i,n in model.names.items() if n==jid]
            if len(matches)!=1: raise KernelError(f'Unknown or ambiguous joint: {jid}')
            jid=matches[0]
        if jid not in model.drivers: raise KernelError('Animate a driver joint; coupled and passive joints follow automatically')
        if jid in seen: raise KernelError('Duplicate joint track')
        seen.add(jid);t['joint']=jid
        linear=model.joints[jid].type=='prismatic'
        if t.get('unit') not in (('mm',) if linear else ('deg','rad')): raise KernelError('Use mm for sliders, deg or rad for rotation')
        keys=t.get('keys')
        if not isinstance(keys,list) or not 2 <= len(keys) <= 1000: raise KernelError('Each track needs 2–1000 [seconds, value] keys')
        if any(not isinstance(k,list) or len(k)!=2 or not all(finite(x) for x in k) for k in keys): raise KernelError('Keyframes must contain finite time and value')
        if keys[0][0]!=0 or keys[-1][0]!=p['duration'] or any(a[0]>=b[0] for a,b in zip(keys,keys[1:])): raise KernelError('Key times must increase from 0 to duration')
        if p.get('loop') and abs(keys[0][1]-keys[-1][1])>1e-9: raise KernelError('A looping pattern must return to its starting value')
        from .pose import joint_range
        lo,hi=joint_range(model.joints[jid]);factor=math.pi/180 if t['unit']=='deg' else 1.
        if any(not lo-1e-9 <= k[1]*factor <= hi+1e-9 for k in keys): raise KernelError('A keyframe exceeds joint preview bounds')
    return p


def sample_program(program, seconds):
    if not isinstance(seconds,(float,int)) or not math.isfinite(seconds): raise KernelError('Time must be finite')
    t=max(0.,min(program['duration'],seconds));out={}
    for track in program['tracks']:
        keys=track['keys'];value=keys[-1][1]
        for a,b in zip(keys,keys[1:]):
            if t<=b[0]:
                f=(1-math.cos(math.pi*(t-a[0])/(b[0]-a[0])))/2
                value=a[1]+(b[1]-a[1])*f;break
        out[track['joint']]=math.radians(value) if track['unit']=='deg' else value
    return out


def sweep_program(model,jid,duration=4.):
    from .pose import joint_range
    j=model.joints[jid];linear=j.type=='prismatic';factor=1. if linear else 180/math.pi
    lo,hi=joint_range(j);home=j.home*factor
    amplitude=10. if linear else 15.
    return {'name':model.names[jid]+' sweep','duration':duration,'loop':True,
            'tracks':[{'joint':jid,'unit':'mm' if linear else 'deg','keys':[[0,home],[duration/4,min(hi*factor,home+amplitude)],[duration*3/4,max(lo*factor,home-amplitude)],[duration,home]]}]}
