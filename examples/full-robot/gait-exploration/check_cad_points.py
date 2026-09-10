"""Check sampled SDF rejection points against the versioned CAD B-rep.

Read-only CAD-side diagnostic. This does not certify whole-link or continuous
clearance. Runtime poses map CAD world coordinates relative to exported link COM.
"""
import argparse,json,zipfile,hashlib,time
from pathlib import Path
import numpy as np
from robocad.kernel import default_kernel, Body
from OCP.gp import gp_Pnt
from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeVertex
from OCP.BRepClass3d import BRepClass3d_SolidClassifier
from OCP.TopExp import TopExp_Explorer
from OCP.TopAbs import TopAbs_SOLID,TopAbs_IN,TopAbs_ON
root=Path('examples/full-robot/gait-exploration')
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--scene',type=Path,default=Path('examples/full-robot/whole-swing/settled-integral-candidate.scene.json'))
parser.add_argument('--probes',type=Path,default=root/'cad-probes.poses.json')
parser.add_argument('--output',type=Path,default=root/'cad-point-check.json')
args=parser.parse_args()
scene=json.loads(args.scene.read_text())
probes=json.loads(args.probes.read_text())
cad=Path('examples/full-robot/baseline/robot.rcad')
assert hashlib.sha256(cad.read_bytes()).hexdigest()==scene['robot']['source']['cad_sha256']==probes['cad_sha256']
kernel=default_kernel();cache={};rows=[];started=time.monotonic()
with zipfile.ZipFile(cad) as archive:
 manifest=json.loads(archive.read('manifest.json'));nodes={n['id']:n for n in manifest['nodes']}
 def body(n):
  if n not in cache:cache[n]=kernel.deserialize(archive.read(f'brep/{n}.brep'),nodes[n].get('body_kind','solid'))
  return cache[n]
 for row in probes['rows']:
  for hit in row['sampled_penetrations']:
   name=probes['link_names'][hit['other']]
   link=next(l for l in scene['robot']['links'] if l['name']==name)
   pose=next(p for p in row['poses'] if p['name']==name)
   point=(np.array(link['com'])+np.array(pose['rotation']).T@(np.array(hit['point_m'])-pose['position_m']))*1000
   p=gp_Pnt(*[float(x) for x in point]);vertex=Body(BRepBuilderAPI_MakeVertex(p).Vertex())
   members=[]
   for member in link['members']:
    b=body(member);gap,_,_=kernel.distance(vertex,b);states=[];solids=TopExp_Explorer(b.shape,TopAbs_SOLID)
    while solids.More():
     state=BRepClass3d_SolidClassifier(solids.Current(),p,1e-7).State()
     states.append('inside' if state==TopAbs_IN else 'on' if state==TopAbs_ON else str(state));solids.Next()
    members.append({'id':member,'name':nodes[member]['name'],'surface_distance_mm':gap,'solid_classifications':states})
   result={'sample':row['id'],'source_link':probes['link_names'][hit['link']],'target_link':name,'runtime_penetration_mm':1000*hit['penetration_m'],'cad_point_world_mm':point.tolist(),'inside_any_solid':any('inside' in m['solid_classifications'] for m in members),'on_any_solid':any('on' in m['solid_classifications'] for m in members),'nearest_surface_mm':min(m['surface_distance_mm'] for m in members),'members':members}
   rows.append(result);print(json.dumps({k:v for k,v in result.items() if k!='members'}),flush=True)
   args.output.write_text(json.dumps({'cad_sha256':probes['cad_sha256'],'rows':rows,'wall_s':time.monotonic()-started,'scope':__doc__},indent=2)+'\n')
