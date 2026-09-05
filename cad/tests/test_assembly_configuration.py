import math
import numpy as np
import pytest
from robocad.commands import Ops
from robocad.document import Document,Node
from robocad.kernel import KernelError
from robocad.physical import body_mass_properties
from robocad.robotics import MOTOR_LIBRARY,motor_physics

def test_configuration_atomic_undo_and_cycle_rejection():
 d=Document();o=Ops(d);a=o.box((0,0,0),(10,10,10));b=o.box((20,0,0),(10,10,10));before=d.revision
 r=o.configure_robot(before,updates={a:{'name':'base'}},groups=[{'key':'links','name':'Links'}],joints=[{'type':'revolute','parent':a,'child':b,'pivot':[15,0,0],'axis':[0,1,0],'group':'links','name':'hinge'}])
 assert len(r['joints'])==1 and d.nodes[a].name=='base'
 rev=d.revision;count=len(d.nodes)
 with pytest.raises(KernelError,match='cycle'):
  o.configure_robot(rev,updates={a:{'name':'bad'}},joints=[{'type':'fixed','parent':b,'child':a,'pivot':[0,0,0],'axis':[1,0,0]}])
 assert d.revision==rev and len(d.nodes)==count and d.nodes[a].name=='base'
 o.stack.undo();assert len(d.nodes)==2 and d.nodes[a].name!='base'
 o.stack.redo();assert d.nodes[a].name=='base'

def test_mass_declaration_and_rejection():
 d=Document();o=Ops(d);a=o.box((0,0,0),(10,20,30));I=np.diag([.052*(.02**2+.03**2)/12,.052*(.01**2+.03**2)/12,.052*(.01**2+.02**2)/12])
 meta={'mass_properties':{'mass_kg':.052,'com_mm':[5,10,15],'inertia_kg_m2':I.tolist(),'source':'specified mass; uniform box inertia estimate'}}
 o.configure_robot(d.revision,updates={a:{'robot':meta}})
 m,c,i,_=body_mass_properties(d,d.nodes[a]);assert m==.052 and np.allclose(c,[.005,.01,.015]) and np.allclose(i,I)
 meta['mass_properties']['inertia_kg_m2'][0][0]=-1
 with pytest.raises(KernelError):o.configure_robot(d.revision,updates={a:{'robot':meta}})

def test_per_solid_material_mass_and_parallel_axis():
 from robocad.kernel.occt import _compound
 from robocad.kernel import Body
 d=Document();o=Ops(d);a=o.box((0,0,0),(10,10,10));b=o.box((20,0,0),(10,10,10))
 node=Node(d.new_id(),'body','mixed',body=Body(_compound([d.nodes[a].body.shape,d.nodes[b].body.shape])),material='nylon',robot={'solid_materials':{'1':'steel'}});d.add(node)
 m,c,i,_=body_mass_properties(d,node);ma=d.materials['nylon'].density*.001;mb=d.materials['steel'].density*.001
 expected=(ma*.005+mb*.025)/(ma+mb)
 assert m==pytest.approx(ma+mb) and c[0]==pytest.approx(expected)
 assert i[1,1]==pytest.approx((ma+mb)*(.01**2+.01**2)/12+ma*(.005-expected)**2+mb*(.025-expected)**2)
 node.robot['solid_materials']['9']='steel'
 with pytest.raises(KernelError,match='stale'):body_mass_properties(d,node)

def test_hx30hm_output_ratio_and_resolution():
 spec=MOTOR_LIBRARY['hx30hm'];p=motor_physics(spec,5)
 assert spec.mass_g==52 and spec.stall_torque==pytest.approx(2.941995)
 assert p['gearbox']['max_output_torque']==pytest.approx(spec.stall_torque*5)
 assert p['gearbox']['max_output_speed']==pytest.approx(math.radians(60)/.19/5)
 assert p['firmware']['sensor_resolution_rad']==pytest.approx(2*math.pi/4096)
 assert 'estimates' in p['notes']

def test_routine_robot_summary_never_queries_geometry(monkeypatch):
 d=Document();o=Ops(d);a=o.box((0,0,0),(10,10,10));b=o.box((20,0,0),(10,10,10));o.add_joint('revolute',a,b,(15,0,0),(0,1,0))
 def expensive(*args,**kwargs):raise AssertionError('Routine UI refresh invoked kernel geometry')
 for method in ('contains','distance','mass_properties','sphere'):monkeypatch.setattr(d.kernel,method,expensive)
 info=o.robot()
 assert len(info['joints'])==1 and info['validation_scope'].startswith('topology only')

def test_inertial_fast_path_matches_exact_geometry():
 d=Document();o=Ops(d);nid=o.box((31,-7,18),(8,13,21));b=d.nodes[nid].body
 exact=d.kernel.mass_properties(b);fast=d.kernel.inertial_properties(b)
 assert fast.volume==pytest.approx(exact.volume)
 assert np.allclose(fast.inertia,exact.inertia) and np.allclose(fast.centroid,exact.centroid)
 assert fast.area==0 and exact.area>0

def test_current_cylinder_span_avoids_unrelated_face_integration(monkeypatch):
 from robocad.kernel import occt
 d=Document();body=d.kernel.cylinder((11,13,17),(0,1,0),4,23)
 face=d.kernel.cylindrical_faces(body)[0]
 expected=d.kernel._cylinder_span(body,face,0.)
 def unrelated(*args,**kwargs):raise AssertionError('integrated unrelated faces')
 monkeypatch.setattr(occt,'_face_ref',unrelated)
 actual=d.kernel._cylinder_span(body,face,0.,current_reference=True)
 assert np.allclose(actual[0],expected[0]) and actual[1]==pytest.approx(expected[1])
