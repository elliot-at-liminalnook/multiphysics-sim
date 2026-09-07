"""Reproduce the calf interference study using exact B-rep distances.

Samples and bisection locate the observed collision; this is not continuous
collision detection or a hardware travel certification.
"""
import json,zipfile,time,math
from pathlib import Path
import numpy as np
from robocad.document import Document,Node
from robocad.robotics import Joint
from robocad.pose import PoseModel
from robocad.kernel import Body
from OCP.gp import gp_Trsf
from OCP.BRepBuilderAPI import BRepBuilderAPI_Transform
root=Path(__file__).resolve().parents[2]
import argparse
parser=argparse.ArgumentParser(description='Offline +X knee/calf B-rep clearance sweep; does not edit the CAD document')
parser.add_argument('--cad',type=Path,default=root/'runs/robot-imports/Full_Bot-knee-01.rcad')
path=parser.parse_args().cad
(root/'runs/robot-imports').mkdir(parents=True,exist_ok=True)
contract=json.loads((root/'examples/full-robot/assembly-contract.json').read_text());parts=contract['legs']['+X']['parts']
d=Document()
with zipfile.ZipFile(path) as z:
 data=json.loads(z.read('manifest.json'));d.revision=data['revision'];d.robot_settings=data['robot_settings']
 for n in data['nodes']:
  node=Node(n['id'],n['kind'],n['name'],joint=Joint.from_json(n['joint']) if n.get('joint') else None,robot=n.get('robot'),material=n.get('material'),disabled=n.get('disabled',False));d.nodes[node.id]=node
  if node.id in [parts[k] for k in ('crank','coupler','guide_tube','lower_guide')]:
   node.body=d.kernel.deserialize(z.read(f'brep/{node.id}.brep'),n.get('body_kind','solid'))
   print(node.name,'material',node.material,'regions',(node.robot or {}).get('solid_materials'),flush=True)
# Study beyond the configured stop on this metadata-only offline copy.
for node in d.nodes.values():
 if node.name=='+X | Foot servo output':node.joint.upper=None;node.joint.lower=None
model=PoseModel(d);jid=next(i for i,j in model.joints.items() if model.names[i]=='+X | Foot servo output')
def transformed(nid,matrices):
 m=matrices[nid];t=gp_Trsf();t.SetValues(*[float(x) for x in m[:3,:].flat])
 return Body(BRepBuilderAPI_Transform(d.nodes[nid].body.shape,t,False).Shape())

# Load every body fixed to the crank output, including the nylon crank plate.
crank_ids={parts['crank']}
while True:
 children={child for child,(parent,joint) in model.parents.items() if parent in crank_ids and joint and model.joints[joint].type=='fixed'}
 if children<=crank_ids:break
 crank_ids.update(children)
with zipfile.ZipFile(path) as z:
 for nid in crank_ids:
  if d.nodes[nid].body is None:d.nodes[nid].body=d.kernel.deserialize(z.read(f'brep/{nid}.brep'))
print('Crank rigid members',[(n,d.nodes[n].name) for n in crank_ids],flush=True)
rows=[]
def probe(degrees, ids=None):
 start=time.monotonic();matrices=model.matrices({jid:math.radians(degrees)});stationary=transformed(parts['guide_tube'],matrices);pairs={}
 for nid in (ids if ids is not None else [*crank_ids,parts['coupler']]):
  gap,a,b=d.kernel.distance(transformed(nid,matrices),stationary)
  pairs[nid]={'name':d.nodes[nid].name,'gap_mm':float(gap),'moving_point':a,'fixed_point':b}
 row={'degrees':degrees,'pairs':pairs};rows.append(row)
 (root/'runs/robot-imports/knee-clearance-refinement.json').write_text(json.dumps(rows,indent=2))
 print(degrees,{v['name']:round(v['gap_mm'],6) for v in pairs.values()},'s',round(time.monotonic()-start,2),flush=True)
 return min(v['gap_mm'] for v in pairs.values())
for angle in (0,-5,-15,-30,-45,-60,-70):probe(angle)
# Bracket the first sampled positive contact, then refine the curved-link pair.
lo=0.
for hi in range(1,16):
 if probe(float(hi),[parts['coupler']])<=1e-5:break
 lo=float(hi)
else:raise RuntimeError('No contact bracket below 15 degrees; review geometry and study range')
while hi-lo>.02:
 mid=(lo+hi)/2
 if probe(mid,[parts['coupler']])<=1e-5:hi=mid
 else:lo=mid
print('POSITIVE CONTACT BRACKET',lo,hi,flush=True)
# An explicit provisional 1 mm geometric margin, independently of control latency.
lo_margin,hi_margin=0.,lo
while hi_margin-lo_margin>.02:
 mid=(lo_margin+hi_margin)/2
 if probe(mid,[parts['coupler']])<1.:hi_margin=mid
 else:lo_margin=mid
print('POSITIVE 1MM BOUND',lo_margin,hi_margin,flush=True)
(root/'runs/robot-imports/knee-positive-boundary.json').write_text(json.dumps({'joint':jid,'revision':d.revision,'contact_clear_deg':lo,'contact_deg':hi,'margin_mm':1,'margin_clear_deg':lo_margin,'margin_crossed_deg':hi_margin,'samples':rows},indent=2))
