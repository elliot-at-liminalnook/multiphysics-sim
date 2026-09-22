"""Preserve the baseline while making its +X leg a reusable component.

Run from the repository root with PYTHONPATH=cad.
"""
from pathlib import Path
import hashlib,json,zipfile
from robocad.document import Document
from robocad.commands import Ops

source=Path('examples/full-robot/baseline/robot.rcad')
output=Path('examples/components/quadruped-linked-leg');output.mkdir(parents=True,exist_ok=True)
sha=hashlib.sha256(source.read_bytes()).hexdigest()
doc=Document.load(str(source));ops=Ops(doc)
original={nid:(n.body,n.joint.to_json() if n.joint else None,n.robot,n.name) for nid,n in doc.nodes.items()}
original_bodies=len(doc.bodies())
# The detailed +X leg is reusable without silently replacing the other designs.
leg=ops.make_component(['8f483dd5bfb8'],'Quadruped leg +X')
for nid,(body,joint,robot,name) in original.items():
 n=doc.nodes[nid]
 assert n.body is body,(name,'geometry changed')
 assert (n.joint.to_json() if n.joint else None)==joint,(name,'joint changed')
 assert n.robot==robot,(name,'physical metadata changed')
 assert n.name==name,(name,'name changed')
assert len(doc.bodies())==original_bodies
path=output/'robot.rcad';doc.save(str(path))
with zipfile.ZipFile(source) as baseline, zipfile.ZipFile(path) as candidate:
 names=set(candidate.namelist()); identical=0
 for name in baseline.namelist():
  if not name.startswith('brep/'): continue
  target=name if name in names else 'components/'+leg['definition_id']+'/'+name.split('/')[-1]
  assert baseline.read(name)==candidate.read(target),name
  identical+=1
ops.export_component(leg['definition_id'],str(output/'quadruped-leg.rcomp'))
restored=Document.load(str(path))
assert len(restored.bodies())==original_bodies
for nid,(_,joint,robot,name) in original.items():
 n=restored.nodes[nid]
 assert (n.joint.to_json() if n.joint else None)==joint,(name,'reload joint')
 assert n.robot==robot,(name,'reload metadata')
assert hashlib.sha256(source.read_bytes()).hexdigest()==sha
report={'version':1,'source':str(source),'source_sha256':sha,'result_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),
 'definition_id':leg['definition_id'],'instance_id':leg['instance_id'],'body_count':original_bodies,'identical_source_brep_payloads':identical,
 'validation':'Every source body handle, joint, physical metadata record, node ID and name preserved at conversion; joints and metadata preserved after reload.',
 'remaining_legs':'Original legs retained because their part counts and structures differ. No assertion that replacing them with +X preserves the original design.'}
(output/'verification.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report),flush=True)
