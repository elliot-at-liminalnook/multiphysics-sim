"""Prepare twelve fixture sets as three independently printable four-set plates.
Geometry-only 3MFs; machine/filament settings and slicing remain in Bambu Studio.
Run: cad/.venv/bin/python cad/scripts/hx30hm_print_batch.py
"""
from pathlib import Path
import sys, json, hashlib, zipfile
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from robocad.document import Document
from robocad.io.exporters import export_3mf, ThreeMfSettings
from robocad.printing import mesh_open_edges
ROOT=Path(__file__).resolve().parents[2]
SOURCE=ROOT/'examples/actuators/hx30hm/fixture-draft/print-kit.rcad'
OUT=SOURCE.parent/'twelve-motor-batch'; OUT.mkdir(exist_ok=True)
source=Document.load(str(SOURCE));k=source.kernel
shapes={}
for n in source.bodies():
    if n.name in ('cradle','cap'):
        lo,hi=source.mesh_of(n.id).bounds()
        shapes[n.name]=k.transform(n.body,translation=tuple(-x for x in lo))
manifest={'schema_version':1,'total_holders':12,'total_cradles':12,'total_caps':12,
 'plate_count':3,'units':'mm','source_sha256':hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
 'generator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
 'status':'geometry prepared; not sliced or sent to printer','plates':[]}
for plate_index in range(3):
    doc=Document();doc.robot_settings=source.robot_settings.copy();bounds=[]
    for i in range(4):
        number=plate_index*4+i+1
        for name,xy in [('cradle',(20+(i%2)*110,20+(i//2)*96)),('cap',(20+(i%2)*85,215+(i//2)*41))]:
            body=k.transform(shapes[name],translation=(*xy,0))
            n=doc.add_body(body,f'{name}-{number:02d}','pla')
            n.robot={'fixture':{'revision':'draft-01','motor_number':number,'role':name,'units':'mm','fit_status':'unverified'}}
            mesh=doc.mesh_of(n.id);assert mesh_open_edges(mesh)==0
            lo,hi=mesh.bounds();assert lo[2]>=-1e-6
            assert all(lo[j]>=0 and hi[j] <= (300,320,325)[j] for j in range(3))
            bounds.append((lo,hi))
    for i,(lo,hi) in enumerate(bounds):
        for blo,bhi in bounds[i+1:]:
            assert any(hi[j]<blo[j] or bhi[j]<lo[j] for j in (0,1)), 'Overlapping parts'
    stem=f'plate-{plate_index+1}-motors-{plate_index*4+1:02d}-{plate_index*4+4:02d}'
    doc.save(str(OUT/f'{stem}.rcad'))
    warnings=export_3mf(doc,str(OUT/f'{stem}.3mf'),settings=ThreeMfSettings(colors=False))
    assert not warnings,warnings
    # Reopen the actual 3MF and verify the delivered print object count.
    import xml.etree.ElementTree as ET
    with zipfile.ZipFile(OUT/f'{stem}.3mf') as z:
        xml=ET.fromstring(z.read('3D/3dmodel.model'))
        assert len(xml.findall('.//{*}build/{*}item'))==8
    manifest['plates'].append({'file':f'{stem}.3mf','objects':8,'cradles':4,'caps':4,
        'xy_min_mm':[min(b[0][j] for b in bounds) for j in (0,1)],
        'xy_max_mm':[max(b[1][j] for b in bounds) for j in (0,1)],
        'nonoverlapping':True,'closed_meshes':True,
        'sha256':hashlib.sha256((OUT/f'{stem}.3mf').read_bytes()).hexdigest()})
(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
(OUT/'README.md').write_text('''# Twelve HX-30HM holders\n\nThree separate print plates, each with four cradles and four caps (24 parts total). Open each 3MF as its own job in Bambu Studio. Import as separate objects and retain the laid-out positions. All geometry is in millimeters, flat at Z=0, inside X=20..230 / Y=20..287 mm.\n\nThese are geometry-only projects. Select the actual H2C nozzle, build plate and loaded PLA before slicing; no G-code or printer commands have been generated. Proposed process: 0.20 mm layers with a 0.4 mm nozzle, six walls, 50% gyroid, six top/bottom layers. Check bridges over the underside nut pockets. The printer must be cleared between batches.\n\nEach delivered 3MF was reopened and checked to contain eight print objects. Meshes are closed and laid-out XY bounds do not overlap. Mechanical fit and load capacity are still those of draft 01, unverified. The optional alignment links and fit coupon remain in the original print kit and are not duplicated here.\n''')
with zipfile.ZipFile(OUT.parent/'twelve-motor-batch.zip','w',zipfile.ZIP_DEFLATED) as z:
    for path in sorted(OUT.iterdir()):z.write(path,path.name)
print(json.dumps({'holders':12,'parts':24,'plates':3,'validated':True,'path':str(OUT)}))
