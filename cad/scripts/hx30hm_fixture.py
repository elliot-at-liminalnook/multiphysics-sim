"""Rebuild the review-only modular servo fixture through the shared CAD kernel.
Run from repository root: cad/.venv/bin/python cad/scripts/hx30hm_fixture.py
All dimensions mm. No structural simulation or hardware strength rating.
"""
from pathlib import Path
import sys, json, hashlib
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from robocad.document import Document
from robocad.robotics import MOTOR_LIBRARY
from robocad.kernel import BooleanOp, Plane, Sketch
from robocad.io.exporters import export_stl, export_step, export_3mf, ThreeMfSettings
from robocad.io.snapshot import render
from robocad.printing import validate_for_export, mesh_open_edges

OUT = Path(__file__).resolve().parents[2] / 'examples/actuators/hx30hm/fixture-draft'
OUT.mkdir(parents=True, exist_ok=True)
spec = MOTOR_LIBRARY['hx30hm']
length, thickness, depth = spec.size
# Motor lies on its broad side. X=case length; Y=shaft; Z=case thickness.
base_w, base_d, base_t = 100., 86., 8.
front = -base_d/2
cy = front + depth/2
floor = 12.
clearance = .4
inner = length/2 + clearance
bolt_x = 31.
bolt_ys = (cy-8, cy+8)
cap_z = floor + thickness
cap_t = 7.
doc = Document(); k=doc.kernel

def box(c,s): return k.box(c,s)
def union(a,b): return k.boolean(a,b,BooleanOp.UNION)
def cut(a,b): return k.boolean(a,b,BooleanOp.SUBTRACT)
def hole(x,y,z,d,h): return k.cylinder((x,y,z),(0,0,1),d/2,h)
def slot(a,b,w,z,h):
    sk=Sketch(Plane.xy(z)); sk.slot(a,b,w)
    return k.extrude(sk.to_body(),(0,0,1),h)

def add(d,name,b,color,printed=True):
    n=d.add_body(b,name,'pla' if printed else 'al'); n.color=color
    n.robot={'fixture': {'revision':'draft-01','role':name,'units':'mm',
        'geometry_status':'estimated fit, untested', 'printed':printed,
        'material_status':'generic library PLA estimates; actual filament/process uncalibrated'}}
    return n

base=box((-50,-43,0),(100,86,8))
# Broad lower pads; the middle stays open to air above the plate.
for x in (-length/2,length/2-7):
    base=union(base,box((x,front+2,7),(7,depth-4,5)))
# Side fences react torque through the case; long bolts pass through these pillars.
for x in (-37,inner):
    base=union(base,box((x,front+2,7),(37-inner,depth-4,21)))
# Rounded mounting slots at the four corners, plus independent row-link holes.
for x in (-41,41):
    for y in (-24,24): base=cut(base,slot((x,y-5),(x,y+5),6.6,-1,10))
for x in (-44,44): base=cut(base,hole(x,0,-1,4.5,10))
# Four clamp bolts. Underside hex nut recesses let the feet sit flat.
for x in (-bolt_x,bolt_x):
    for y in bolt_ys:
        base=cut(base,hole(x,y,-1,4.5,48))
        sk=Sketch(Plane.xy(-.1)); sk.polygon((x,y),7.5/(3**.5),6,30)
        base=cut(base,k.extrude(sk.to_body(),(0,0,1),4.7))
# Cable tie slots in the rear pad (strain relief, not structural restraint).
for x in (-13,13): base=cut(base,slot((x-4,17),(x+4,17),3.2,-1,10))
cap=box((-37,front+2,cap_z),(74,depth-4,cap_t))
# Open center, leaving two broad clamp beams and two bolt rails.
cap=cut(cap,box((-16,cy-5,cap_z-1),(32,10,cap_t+2)))
for x in (-bolt_x,bolt_x):
    for y in bolt_ys: cap=cut(cap,slot((x,y-1),(x,y+1),4.5,cap_z-1,cap_t+2))
# Optional alignment-only row link for 110 mm module pitch.
link=box((-20,-8,0),(40,16,6))
for x in (-11,11): link=cut(link,hole(x,0,-1,4.5,8))
# A small cavity coupon avoids printing twelve unverified saddles.
coupon=box((-inner-4,-4,0),(2*inner+8,8,4))
for x in (-inner-4,inner): coupon=union(coupon,box((x,-4,3),(4,8,7)))

parts={'cradle':base,'cap':cap,'row-link':link,'fit-coupon':coupon}
colors={'cradle':(.18,.55,.73),'cap':(.98,.59,.18),'row-link':(.45,.65,.72),'fit-coupon':(.6,.75,.8)}
checks={}
for name,b in parts.items():
    d=Document(); bb=k.transform(b,translation=(0,0,-cap_z)) if name=='cap' else b
    n=add(d,name,bb,colors[name])
    ok,msg=validate_for_export(k,[(name,bb)])
    edges=mesh_open_edges(d.mesh_of(n.id))
    p=k.mass_properties(bb)
    assert ok and edges==0, (name,msg,edges)
    export_stl(d,str(OUT/f'{name}.stl'))
    checks[name]={'valid_solid':ok,'mesh_open_edges':edges,'size_mm':list(p.size),
        'volume_cm3':p.volume/1000,'solid_PLA_mass_g':p.volume/1000*1.24,'messages':msg}
    print('exported',name,flush=True)

# One small print plate; cap flat on its broad face, all Z >= 0.
plate=Document()
placements={'cradle':(55,48,0),'cap':(55,153,-cap_z),'row-link':(140,45,0),'fit-coupon':(140,160,0)}
for name,b in parts.items(): add(plate,name,k.transform(b,translation=placements[name]),colors[name])
plate.robot_settings={'fixture_draft':{'revision':'draft-01','units':'mm','parameters':{
    'motor_library_id':spec.id,'motor_size_mm':list(spec.size),'case_clearance_per_side_mm':clearance,
    'base_mm':[100,86,8],'case_floor_z_mm':floor,'clamp_cap_thickness_mm':cap_t},
    'source':'cad/scripts/hx30hm_fixture.py','rated_load_Nm':None,
    'print_intent':{'material':'PLA','layer_mm':.2,'walls':6,'infill_percent':50,
        'infill':'gyroid','top_bottom_layers':6,'supports':False,'status':'proposed, not sliced or printed'},
    'provenance':{'motor_dimensions':'manufacturer nominal envelope via MOTOR_LIBRARY; not measured',
        'shaft_position':'visual estimate only; not used to locate a fixture hole',
        'PLA':'shared generic material; no calibrated print strength or creep data'},
    'frame':'X case long direction, Y shaft axis (output toward -Y), Z up; mm'}}
plate.save(str(OUT/'print-kit.rcad'))
export_3mf(plate,str(OUT/'print-kit.3mf'),settings=ThreeMfSettings(colors=False))
export_step(plate,str(OUT/'print-kit.step'))
render(plate,str(OUT/'print-layout.png'),size=(1400,1000),view=(-.8,-1.2,1.8),edges=False,title='HX-30HM modular fixture | PLA print kit | draft 01')

# Review assembly: motor and fasteners are reference geometry, excluded from print kit.
assembly=Document(); scene=[]
def record(name,b,color,role):
    n=add(assembly,name,b,color,role in ('cradle','cap'))
    m=assembly.mesh_of(n.id,.15)
    scene.append({'name':name,'role':role,'color':list(color),'vertices':m.vertices,'triangles':m.triangles})
record('Cradle',base,colors['cradle'],'cradle')
record('Removable cap',cap,colors['cap'],'cap')
motor=box((-length/2,front,floor),(length,depth,thickness))
# Off-center output and opposite rear boss are illustrative only.
shaft_x=length/2-thickness/2
motor=union(motor,k.cylinder((shaft_x,front,floor+thickness/2),(0,-1,0),3,4))
motor=union(motor,k.cylinder((shaft_x,front+depth,floor+thickness/2),(0,1,0),3,3))
record('HX-30HM envelope (fit unverified)',motor,(.23,.25,.29),'motor')
for x in (-bolt_x,bolt_x):
    for y in bolt_ys:
        bolt=hole(x,y,.7,4,45)
        bolt=union(bolt,hole(x,y,cap_z+cap_t+2,7,4))
        record('M4 clamp screw (reference)',bolt,(.68,.71,.76),'bolt')
assembly.robot_settings=plate.robot_settings
assembly.save(str(OUT/'assembly.rcad'))
render(assembly,str(OUT/'assembled.png'),size=(1500,1050),view=(-1,-1.5,1),edges=False,title='HX-30HM modular fixture | assembled | draft 01')
for n in assembly.bodies():
    if n.name=='Removable cap': n.body=k.transform(n.body,translation=(0,0,32))
    if n.name.startswith('M4'): n.body=k.transform(n.body,translation=(0,0,38))
assembly.mesh_cache.clear()
render(assembly,str(OUT/'exploded.png'),size=(1500,1050),view=(-1,-1.5,1),edges=False,title='HX-30HM modular fixture | removable cap | draft 01')
(OUT/'scene.json').write_text(json.dumps(scene,separators=(',',':')))
checks['status']='CAD solids / mesh checks only; no physical fit, slicing, or strength validation'
checks['source_sha256']=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
bounds=[plate.mesh_of(n.id).bounds() for n in plate.bodies()]
checks['plate_bounds_mm']=[max(b[1][i] for b in bounds)-min(b[0][i] for b in bounds) for i in range(3)]
for i,a in enumerate(bounds):
    for b in bounds[i+1:]:
        assert any(a[1][j]<b[0][j] or b[1][j]<a[0][j] for j in (0,1)), 'Print parts overlap in XY'
checks['print_layout_nonoverlapping']=True
(OUT/'validation.json').write_text(json.dumps(checks,indent=2)+'\n')
print('DONE',OUT,flush=True)
