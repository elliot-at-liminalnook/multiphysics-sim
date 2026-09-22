import copy
import json
import zipfile

import numpy as np
import pytest

from robocad.commands import Ops
from robocad.document import Document, Transform
from robocad.kernel import KernelError
from robocad.component_parameters import expression, validate_parameters
from robocad.simbridge import joints_of


def fixture():
    doc = Document(); ops = Ops(doc)
    group = ops.group([], 'Module')
    a = ops.box((0,0,0), (10,10,10)); b = ops.box((0,0,10), (4,4,30))
    doc.move(a, group); doc.move(b, group)
    joint = ops.add_joint('revolute', a, b, (0,0,10), (0,1,0), lower=-.5, upper=.5)
    definition = ops.create_component([group], 'Hinged module')
    ops.delete([group, joint])
    return doc, ops, definition, a, b, joint


def params(height=30):
    return {'height': {'value': height, 'unit': 'mm', 'min': 1, 'max': 100, 'provenance': 'estimated'}}


def test_four_occurrences_share_geometry_archive_and_have_independent_joints(tmp_path):
    doc, ops, definition, a, b, joint = fixture()
    instances = [ops.place_component(definition, Transform((i*50,0,0)), name=f'Module {i}') for i in range(4)]
    assert len(doc.bodies()) == 8 and len(joints_of(doc)) == 4
    maps = [doc.nodes[i].component_instance['node_map'] for i in instances]
    assert len({m[joint] for m in maps}) == 4
    for i, mapping in enumerate(maps):
        j = doc.nodes[mapping[joint]].joint
        assert (j.parent, j.child) == (mapping[a], mapping[b])
        assert j.pivot == pytest.approx((i*50, 0, 10))
    path = tmp_path/'assembly.rcad'; doc.save(str(path))
    with zipfile.ZipFile(path) as z:
        assert len([n for n in z.namelist() if n.endswith('.brep')]) == 2
        assert all(not n.startswith('brep/') for n in z.namelist())
    restored = Document.load(str(path))
    assert len(restored.bodies()) == 8 and len(joints_of(restored)) == 4
    assert [restored.nodes[i].component_instance for i in instances] == [doc.nodes[i].component_instance for i in instances]
    for node in doc.bodies():
        assert restored.kernel.mass_properties(restored.nodes[node.id].body).volume == pytest.approx(doc.kernel.mass_properties(node.body).volume)


def test_defaults_overrides_reset_undo_and_invalid_values():
    doc, ops, definition, a, b, joint = fixture()
    features = [{'node': b, 'kind': 'box', 'arguments': {'corner': [0,0,10], 'size': [4,4,'height']}}]
    ops.set_component_parameters(definition, params(), features)
    instances = [ops.place_component(definition, Transform((i*50,0,0))) for i in range(4)]
    def volumes():
        return [doc.kernel.mass_properties(doc.nodes[doc.nodes[i].component_instance['node_map'][b]].body).volume for i in instances]
    ops.set_component_overrides(instances[0], {'height': '5 cm'})
    ops.set_component_parameters(definition, params(40))
    assert volumes() == pytest.approx([800, 640, 640, 640])
    ids = list(doc.nodes)
    ops.undo(); assert volumes() == pytest.approx([800, 480, 480, 480])
    ops.redo(); assert volumes() == pytest.approx([800, 640, 640, 640])
    assert list(doc.nodes) == ids
    ops.set_component_overrides(instances[0], {})
    assert volumes() == pytest.approx([640]*4)
    before = doc.nodes; revision = doc.revision
    with pytest.raises(KernelError): ops.set_component_overrides(instances[0], {'height': -2})
    assert doc.nodes is before and doc.revision == revision
    with pytest.raises(KernelError): ops.set_component_parameters(definition, params(200))
    assert doc.nodes is before and doc.revision == revision


def test_rotation_transforms_joints_and_inertia_without_changing_mass():
    doc, ops, definition, a, b, joint = fixture()
    d = doc.component_definitions[definition]
    d.nodes[a].robot = {'mass_properties': {'mass_kg': 1., 'com_mm': [2,3,4], 'inertia_kg_m2': [[1,0,0],[0,2,0],[0,0,3]], 'source': 'measured'}}
    instance = ops.place_component(definition, Transform((10,20,30),(0,0,1),90))
    mapping = doc.nodes[instance].component_instance['node_map']
    j = doc.nodes[mapping[joint]].joint
    assert j.pivot == pytest.approx((10,20,40))
    assert j.axis == pytest.approx((-1,0,0))
    mass = doc.nodes[mapping[a]].robot['mass_properties']
    assert mass['mass_kg'] == 1.
    assert mass['com_mm'] == pytest.approx((7,22,34))
    np.testing.assert_allclose(mass['inertia_kg_m2'], np.diag([2,1,3]), atol=1e-12)


def test_parameter_units_and_expression_safety():
    _, quantities = validate_parameters(params())
    assert expression('height / 2 + 5 mm', 'mm', quantities) == 20
    assert expression('height', 'cm', quantities) == 3
    for bad in ('height + 2 deg', 'height / height', '__import__("os")', 'height.__class__', 'height[0]'):
        with pytest.raises(KernelError): expression(bad, 'mm', quantities)
    for bad in (True, float('nan'), float('inf')):
        with pytest.raises(KernelError): expression(bad, 'mm')


def test_missing_external_binding_does_not_default_to_world():
    doc = Document(); ops = Ops(doc)
    base = ops.box((0,0,0),(10,10,10)); link = ops.box((0,0,10),(2,2,20))
    joint = ops.add_joint('revolute', base, link, (0,0,10), (0,1,0))
    definition = ops.create_component([link], 'Link')
    port = next(iter(doc.component_definitions[definition].ports))
    before = doc.revision
    with pytest.raises(KernelError): ops.place_component(definition)
    assert doc.revision == before
    placed = ops.place_component(definition, bindings={port: base})
    mapping = doc.nodes[placed].component_instance['node_map']
    assert doc.nodes[mapping[joint]].joint.parent == base


def test_detach_keeps_geometry_and_identity(tmp_path):
    doc, ops, definition, a, b, joint = fixture()
    instance = ops.place_component(definition)
    ids = set(doc.nodes[instance].component_instance['node_map'].values())
    ops.detach_component(instance)
    assert doc.nodes[instance].component_instance is None
    assert all(doc.nodes[n].component_member is None and not doc.nodes[n].locked for n in ids)
    ops.undo()
    assert doc.nodes[instance].component_instance is not None


def test_nested_parameters_branch_override_and_portable_library(tmp_path):
    doc = Document(); ops = Ops(doc)
    child = ops.new_parametric_component('Link')
    child_instance = ops.place_component(child)
    parent = ops.make_component([child_instance], 'Leg')
    parent_id, first = parent['definition_id'], parent['instance_id']
    ops.set_component_parameters(parent_id, params(50), nested={child_instance: {'parameter_bindings': {'length': 'height / 2'}}})
    second = ops.place_component(parent_id, Transform((100,0,0)))
    def child_of(root): return doc.nodes[root].component_instance['node_map'][child_instance]
    def length(root):
        childroot = doc.nodes[child_of(root)]
        body = doc.nodes[next(iter(childroot.component_instance['node_map'].values()))]
        return doc.kernel.mass_properties(body.body).volume / 200
    assert [length(first), length(second)] == pytest.approx([25,25])
    ops.set_component_overrides(child_of(first), {'length': 70})
    ops.set_component_parameters(parent_id, params(80))
    assert [length(first), length(second)] == pytest.approx([70,40])
    ops.set_component_overrides(child_of(first), {})
    assert [length(first), length(second)] == pytest.approx([40,40])
    childparams = copy.deepcopy(doc.component_definitions[child].parameters)
    childparams['width']['value'] = 30
    ops.set_component_parameters(child, childparams)
    assert [length(first), length(second)] == pytest.approx([60,60])
    path=tmp_path/'nested.rcad'; doc.save(str(path))
    restored=Document.load(str(path))
    assert len(restored.bodies()) == 2
    with zipfile.ZipFile(path) as archive:
        assert len([n for n in archive.namelist() if n.endswith('.brep')]) == 1
    library=tmp_path/'leg.rcomp'; ops.export_component(parent_id,str(library))
    empty=Document(); other=Ops(empty)
    assert other.import_component(str(library)) == parent_id
    placed=other.place_component(parent_id)
    assert len(empty.bodies()) == 1
    assert len(empty.component_definitions)==2
    ops.delete([first]); ops.undo()
    assert length(first) == pytest.approx(60)


def test_nested_cycle_rejected_before_recursion():
    from robocad.components import validate_library
    doc=Document(); ops=Ops(doc)
    child=ops.new_parametric_component()
    instance=ops.place_component(child)
    parent=ops.make_component([instance], 'Parent')['definition_id']
    definition=copy.copy(doc.component_definitions[child])
    definition.nodes=copy.copy(doc.component_definitions[parent].nodes)
    definition.roots=list(doc.component_definitions[parent].roots)
    definition.features=[]
    with pytest.raises(KernelError,match='Circular'):
        validate_library({**doc.component_definitions, child: definition})


def test_nested_snapshot_and_physical_export_share_ordinary_runtime(tmp_path):
    from robocad.snapshots import capture
    from robocad.physical import export_physical_model
    doc=Document(); ops=Ops(doc)
    definition=ops.new_parametric_component('Bar')
    child=ops.place_component(definition)
    outer=ops.make_component([child],'Assembly')
    original_hash=capture(doc).physical_hash
    ops.place_component(outer['definition_id'],Transform((100,0,0)))
    captured=capture(doc)
    assert captured.physical_hash!=original_hash
    path=tmp_path/'snapshot.rcad'; path.write_bytes(captured.data)
    restored=Document.load(str(path))
    physical=export_physical_model(restored,flex=False)
    assert len(physical['links'])==2
    assert physical['links'][0]['mass']==pytest.approx(physical['links'][1]['mass'])
    coms=sorted(l['com'][0] for l in physical['links'])
    assert coms[1]-coms[0]==pytest.approx(.1)
    with zipfile.ZipFile(path) as archive: assert sum(n.endswith('.brep') for n in archive.namelist())==1


def test_three_levels_and_nested_external_port(tmp_path):
    doc=Document(); ops=Ops(doc)
    base=ops.box((0,0,0),(20,20,10))
    link=ops.box((0,0,10),(2,2,20))
    joint=ops.add_joint('revolute',base,link,(0,0,10),(0,1,0))
    child=ops.make_component([link],'Child')
    middle=ops.make_component([child['instance_id']],'Middle')
    outer=ops.make_component([middle['instance_id']],'Outer')
    d=doc.component_definitions[outer['definition_id']]
    assert len(d.ports)==1
    placed=ops.place_component(d.id,Transform((50,0,0)),bindings={next(iter(d.ports)):base})
    js=[n.joint for n in doc.nodes.values() if n.joint]
    assert len(js)==2 and all(j.parent==base for j in js)
    path=tmp_path/'three.rcad';doc.save(str(path)); restored=Document.load(str(path))
    assert len(restored.bodies())==3
    ops.set_component_overrides(doc.nodes[placed].component_instance['node_map'][child['instance_id']],{})


def test_linked_edit_guards_are_atomic():
    from robocad.kernel import Plane
    doc,ops,definition,a,b,joint=fixture()
    root=ops.place_component(definition)
    member=doc.nodes[root].component_instance['node_map'][a]
    before=doc.revision; ids=set(doc.nodes)
    for action in (lambda:ops.delete([member]),lambda:ops.set_material([member],'petg'),lambda:ops.instance(member),lambda:ops.mirror([root],Plane.xy(),live=True)):
        with pytest.raises(KernelError): action()
        assert doc.revision==before and set(doc.nodes)==ids


def test_component_family_preserves_distinct_topologies_and_nested_identity(tmp_path):
    doc=Document(); ops=Ops(doc)
    a=ops.new_parametric_component('Short')
    b=ops.new_parametric_component('Long')
    params_b=copy.deepcopy(doc.component_definitions[b].parameters);params_b['length']['value']=90
    ops.set_component_parameters(b,params_b)
    one=ops.place_component(a);two=ops.place_component(b,Transform((50,0,0)))
    family=ops.create_component_family('Link family',{'short':{'definition_id':a,'parameter_bindings':{'width':'width'}},'long':{'definition_id':b,'parameter_bindings':{'width':'width'}}}, {'width':{'value':20,'unit':'mm','min':1,'provenance':'derived'}},'short')
    before=set(doc.nodes)
    ops.link_component_family(one,family,'short');ops.link_component_family(two,family,'long')
    assert set(doc.nodes)==before
    root=ops.make_component([one,two],'Pair')
    ops.set_component_parameters(family,{'width':{'value':30,'unit':'mm','min':1,'provenance':'derived'}})
    volumes=sorted(doc.kernel.mass_properties(n.body).volume for n in doc.bodies())
    assert volumes==pytest.approx([30*10*60,30*10*90])
    path=tmp_path/'family.rcad';doc.save(str(path));restored=Document.load(str(path))
    assert set(restored.nodes)==set(doc.nodes)
    assert [restored.nodes[n].component_instance.get('variant') for n in (one,two)]==['short','long']
    library=tmp_path/'family.rcomp';ops.export_component(root['definition_id'],str(library))
    other=Document(); otherops=Ops(other); otherops.import_component(str(library));otherops.place_component(root['definition_id'])
    assert len(other.bodies())==2
    with zipfile.ZipFile(library) as z:assert sum(n.endswith('.brep') for n in z.namelist())==2


def test_assembly_placement_and_joint_recipe_preserve_frames():
    doc,ops,key,a,b,joint=fixture()
    parameters={'x':{'value':0,'unit':'mm','provenance':'derived'},'ratio':{'value':1,'unit':'1','min':.1,'provenance':'derived'}}
    recipe=[{'node':'*','kind':'assembly_placement','arguments':{'translation':['x',0,0],'axis':[0,0,1],'angle_deg':0}}, {'node':joint,'kind':'joint_ratio','arguments':{'ratio':'ratio'}}]
    ops.set_component_parameters(key,parameters,recipe)
    root=ops.place_component(key);mapping=doc.nodes[root].component_instance['node_map']
    original=doc.nodes[mapping[joint]].joint.to_json()
    ops.set_component_overrides(root,{'x':12,'ratio':2})
    changed=doc.nodes[mapping[joint]].joint
    assert changed.pivot==pytest.approx((12,0,10)) and changed.gear_ratio==2
    ops.undo();assert doc.nodes[mapping[joint]].joint.to_json()==original


def test_restored_identity_occurrences_retain_source_archive_bytes(tmp_path,monkeypatch):
    doc,ops,key,a,b,joint=fixture()
    root=ops.place_component(key)
    shifted=ops.place_component(key,Transform((50,0,0)))
    path=tmp_path/'identity.rcad';doc.save(str(path))
    restored=Document.load(str(path));mapping=restored.nodes[root].component_instance['node_map']
    for source,member in mapping.items():
        if restored.nodes[member].body is not None:
            body,payload=restored._snapshot_body_cache[member]
            assert body is restored.nodes[member].body
            assert payload==restored.component_definitions[key].source_bytes[source][1]
    shifted_body=restored.nodes[shifted].component_instance['node_map'][a]
    assert shifted_body not in restored._snapshot_body_cache
    editing=Ops(restored);editing.detach_component(root)
    recaptured=editing.create_component([root],'Recaptured')
    def unexpected(*args):raise AssertionError('Already-captured source geometry was reserialized')
    monkeypatch.setattr(restored.kernel,'serialize',unexpected)
    assert len(restored.component_definitions[recaptured].archive_entries(restored.kernel,'geometry'))==2
