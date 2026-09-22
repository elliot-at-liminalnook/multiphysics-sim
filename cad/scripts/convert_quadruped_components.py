"""Backup-gated, lossless conversion of the full quadruped to nested components.

The four as-modeled leg topologies are named variants of one parameter family.
Imported solid geometry is retained exactly; recovered feature histories are not
invented. Shared CAD joint properties and whole-assembly placement are explicit.

Usage from repository root:
  PYTHONPATH=cad cad/.venv/bin/python cad/scripts/convert_quadruped_components.py BACKUP_DIRECTORY OUTPUT_DIRECTORY
"""
from copy import deepcopy
from pathlib import Path
import hashlib
import json
import math
import sys
import zipfile

from robocad.commands import Ops
from robocad.components import dependencies
from robocad.document import Document
from robocad.physical import _joint_records, link_groups

LEG_IDS = {'minus_y':'ed8b04d3c9e3','plus_x':'8f483dd5bfb8','plus_y':'744cdc5411cf','minus_x':'9163abfb9f6e'}
ROOT='5c20c68f1edd'
CHASSIS='93c3343067fe'


def checksum(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def records(doc):
    return {nid:{'name':n.name,'kind':n.kind,'material':n.material,'robot':deepcopy(n.robot),
                 'disabled':n.disabled,'joint':n.joint.to_json() if n.joint else None}
            for nid,n in doc.nodes.items()}


def run(backup, destination):
    backup=Path(backup);destination=Path(destination);destination.mkdir(parents=True,exist_ok=True)
    backup_records=json.loads((backup/'backup.json').read_text())
    entry=next(r for r in backup_records if Path(r['backup']).name=='linked-robot.rcad')
    source=backup/Path(entry['backup']).name;assert checksum(source)==entry['sha256'],'Backup hash mismatch'
    path=destination/'robot.rcad'
    if path.exists():raise RuntimeError('Destination already exists; choose a fresh conversion directory')
    print('Loading verified backup',flush=True)
    doc=Document.load(str(source));ops=Ops(doc)
    before=records(doc); original_bodies={n.id:n.body for n in doc.bodies()}
    original_joints={j['id']:j for j in _joint_records(doc)}
    original_links=link_groups(doc,list(original_joints.values()))
    robot_settings=deepcopy(doc.robot_settings);annotations=deepcopy(doc.annotations)
    materials={key:m.to_json() for key,m in doc.materials.items()}
    # Resolve the previous partial migration, retaining all body and joint IDs.
    for node in list(doc.nodes.values()):
        if node.component_instance and not node.component_member:ops.detach_component(node.id)
    def contains_geometry(nid):
        n=doc.nodes[nid]
        return n.body is not None or any(contains_geometry(child) for child in n.children)
    def depth(nid):
        result=0
        while doc.nodes[nid].parent:nid=doc.nodes[nid].parent;result+=1
        return result
    def below(nid,ancestor):
        while nid:
            if nid==ancestor:return True
            nid=doc.nodes[nid].parent
        return False
    groups=[n.id for n in doc.nodes.values() if n.kind=='group' and any(below(n.id,leg) for leg in LEG_IDS.values()) and contains_geometry(n.id)]
    for index,gid in enumerate(sorted(groups,key=depth,reverse=True)):
        name=doc.nodes[gid].name
        print(f'Nested assemblies {index+1}/{len(groups)}: {name}',flush=True)
        ops.make_component([gid],name)
    # Every physical body now belongs either to a leg hierarchy or the chassis.
    chassis=ops.make_component([CHASSIS],'Chassis and hip mounts')
    ops.move_nodes([chassis['instance_id']],ROOT)
    # Parameterize existing CAD drive declarations at their owning components.
    roles={}
    for jid,data in before.items():
        if not data['joint'] or not data['joint'].get('motor'):continue
        title=data['name'].lower()
        role='hip' if 'hip servo' in title else 'worm' if 'worm servo' in title else 'foot' if 'foot servo' in title else None
        if role is None:raise RuntimeError('Unclassified actuator coupling: '+title)
        roles[jid]=role
    assert len(roles)==12
    configured=set()
    def configure(key):
        if key in configured:return
        definition=doc.component_definitions[key]
        params={};features=[];nested={}
        for nid,node in definition.nodes.items():
            if node.component_member:continue
            if node.component_instance:
                childkey=node.component_instance['definition_id'];configure(childkey)
                child=doc.component_definitions[childkey]
                for name,spec in child.parameters.items():
                    if name in params and params[name]['value']!=spec['value']:raise RuntimeError('Conflicting source defaults')
                    params[name]=deepcopy(spec)
                if child.parameters:nested[nid]={'parameter_bindings':{name:name for name in child.parameters}}
            if nid in roles:
                role=roles[nid];j=node.joint
                ratio=role+'_drive_ratio';home=role+'_home_deg'
                params[ratio]={'value':j.gear_ratio,'unit':'1','min':1e-6,'provenance':'derived','description':'Imported CAD actuator-output to joint ratio; not the motor internal gearbox ratio. Positive-value bound is mathematical, not a hardware limit.'}
                params[home]={'value':math.degrees(j.home),'unit':'deg','provenance':'derived','description':'Existing CAD joint home angle, not a new controller command or calibration.'}
                if j.lower is not None:params[home]['min']=math.degrees(j.lower)
                if j.upper is not None:params[home]['max']=math.degrees(j.upper)
                features.extend([{'node':nid,'kind':'joint_ratio','arguments':{'ratio':ratio}}, {'node':nid,'kind':'joint_home','arguments':{'angle_deg':home}}])
        if params:ops.set_component_parameters(key,params,features,nested)
        configured.add(key)
    for leg in LEG_IDS.values():configure(doc.nodes[leg].component_instance['definition_id'])
    variant_ids={name:doc.nodes[nid].component_instance['definition_id'] for name,nid in LEG_IDS.items()}
    common=deepcopy(doc.component_definitions[variant_ids['plus_x']].parameters)
    assert len(common)==6,common
    # Every variant retains its original limits. The family exposes their shared
    # default and accepts only values valid in all four variant definitions.
    for name,spec in common.items():
        bounds=[doc.component_definitions[key].parameters[name] for key in variant_ids.values()]
        lows=[b['min'] for b in bounds if 'min' in b];highs=[b['max'] for b in bounds if 'max' in b]
        if lows:spec['min']=max(lows)
        if highs:spec['max']=min(highs)
    family=ops.create_component_family('Quadruped leg',{
        name:{'definition_id':key,'parameter_bindings':{p:p for p in common}} for name,key in variant_ids.items()},common,'plus_x')
    for name,nid in LEG_IDS.items():ops.link_component_family(nid,family,name)
    print('Building the complete nested quadruped',flush=True)
    robot=ops.make_component([ROOT],'Quadruped')
    # Joint parameters remain shared defaults on the leg family. The robot root
    # owns only coherent world-frame placement of the entire physical assembly.
    pose={name:{'value':0,'unit':unit,'provenance':'derived','description':'Rigid placement of the entire source CAD assembly in its declared world frame.'}
          for name,unit in (('position_x','mm'),('position_y','mm'),('position_z','mm'),('heading_deg','deg'))}
    ops.set_component_parameters(robot['definition_id'],pose,[{'node':'*','kind':'assembly_placement','arguments':{
        'translation':['position_x','position_y','position_z'],'axis':[0,0,1],'angle_deg':'heading_deg'}}])
    # Drop only unreferenced definitions left from intermediate captures; source
    # bytes in all reachable definitions are retained exactly once per owner.
    live=set()
    def include(key):
        if key in live:return
        live.add(key)
        for child in dependencies(doc.component_definitions[key]):include(child)
    include(robot['definition_id'])
    doc.component_definitions={key:d for key,d in doc.component_definitions.items() if key in live}
    print('Checking source geometry, joints, metadata and runtime link grouping',flush=True)
    after=records(doc)
    for nid,data in before.items():assert after[nid]==data,('Changed original node',nid,data,after[nid])
    assert {n.id for n in doc.bodies()}==set(original_bodies)
    assert all(doc.nodes[nid].body is body for nid,body in original_bodies.items()),'Source geometry handle changed'
    assert all(n.component_member for n in doc.bodies())
    assert all(n.component_member for n in doc.nodes.values() if n.joint)
    assert doc.robot_settings==robot_settings and doc.annotations==annotations
    assert {key:m.to_json() for key,m in doc.materials.items()}==materials
    assert {j['id']:j for j in _joint_records(doc)}==original_joints
    assert link_groups(doc,_joint_records(doc))==original_links
    # Exercise shared defaults and one nested occurrence override on the actual model.
    old_params=deepcopy(doc.component_definitions[family].parameters)
    changed=deepcopy(old_params);changed['hip_drive_ratio']['value']=1.25
    ops.set_component_overrides(LEG_IDS['plus_x'],{'hip_drive_ratio':1.5})
    ops.set_component_parameters(family,changed)
    ratios={name:doc.nodes[next(jid for jid,role in roles.items() if role=='hip' and below(jid,nid))].joint.gear_ratio for name,nid in LEG_IDS.items()}
    assert ratios=={'minus_y':1.25,'plus_x':1.5,'plus_y':1.25,'minus_x':1.25},ratios
    ops.undo();ops.undo()
    assert {j['id']:j for j in _joint_records(doc)}==original_joints
    assert all(doc.nodes[nid].body is body for nid,body in original_bodies.items())
    # Validate rigid placement against all original joint frames, then undo.
    ops.set_component_overrides(ROOT,{'position_x':1})
    for jid,data in before.items():
        if data['joint']:
            expected=list(data['joint']['pivot']);expected[0]+=1
            assert max(abs(a-b) for a,b in zip(doc.nodes[jid].joint.pivot,expected))<1e-9
    ops.undo()
    assert {j['id']:j for j in _joint_records(doc)}==original_joints
    print('Saving self-contained parametric CAD',flush=True)
    doc.save(str(path))
    # Durable geometry equality, independent of in-memory object identity.
    original_payloads={}
    with zipfile.ZipFile(source) as archive:
        for name in archive.namelist():
            if name.endswith('.brep'):original_payloads[name.rsplit('/',1)[-1]]=archive.read(name)
    with zipfile.ZipFile(path) as archive:
        payloads={name.rsplit('/',1)[-1]:archive.read(name) for name in archive.namelist() if name.endswith('.brep')}
        assert not any(name.startswith('brep/') for name in archive.namelist())
        assert len([name for name in archive.namelist() if name.endswith('.brep')])==len(original_payloads)==114
    assert payloads==original_payloads
    print('Reloading and checking persisted references',flush=True)
    restored=Document.load(str(path))
    for nid,data in before.items():assert records(restored)[nid]==data,('Reload changed node',nid)
    assert {j['id']:j for j in _joint_records(restored)}==original_joints
    assert link_groups(restored,_joint_records(restored))==original_links
    assert restored.robot_settings==robot_settings and restored.annotations==annotations
    assert len(restored.bodies())==114
    reloaded_ops=Ops(restored)
    reloaded_ops.set_component_parameters(family,changed)
    assert all(restored.nodes[jid].joint.gear_ratio==1.25 for jid,role in roles.items() if role=='hip')
    reloaded_ops.undo()
    assert {j['id']:j for j in _joint_records(restored)}==original_joints
    ops.export_component(family,str(destination/'quadruped-leg.rcomp'))
    ops.export_component(chassis['definition_id'],str(destination/'chassis.rcomp'))
    report={'version':1,'backup':str(backup.resolve()),'backup_sha256':entry['sha256'],
        'source_document_id':doc.document_id,'result':str(path.resolve()),'result_sha256':checksum(path),
        'robot_definition':robot['definition_id'],'leg_family':family,'leg_variants':variant_ids,
        'definitions':len(doc.component_definitions),'component_occurrences':sum(bool(n.component_instance) for n in doc.nodes.values()),
        'physical_bodies':114,'joint_records':len(original_joints),'actuators':len(roles),'runtime_links':len(set(original_links.values())),
        'identical_brep_payloads':len(payloads),'all_physical_nodes_linked':True,'original_ids_names_materials_metadata_preserved':True,
        'robot_settings_annotations_preserved':True,'reload_verified':True,'reload_parameter_propagation_verified':True,'shared_and_local_override_probe':ratios,
        'whole_assembly_translation_probe_mm':1,'probe_changes_undone':True,
        'geometry_scope':'Exact imported B-reps retained as variant source assets; original solid construction histories were not recovered.',
        'physics_scope':'Original joint records and rigid-link grouping preserved. No new calibration or walking claim.',
        'variants_scope':'Four distinct source topologies remain explicit variants under one shared six-parameter leg family.'}
    (destination/'verification.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2),flush=True)
    assert checksum(source)==entry['sha256']
    return report

if __name__=='__main__':run(*sys.argv[1:])
