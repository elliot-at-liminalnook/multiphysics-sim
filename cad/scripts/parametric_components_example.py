"""A shared articulated assembly with nested, explicitly parametric parts.

Run: PYTHONPATH=cad cad/.venv/bin/python cad/scripts/parametric_components_example.py
This is a CAD/component acceptance example, not a calibrated walking robot.
"""
from copy import deepcopy
from pathlib import Path
import json

from robocad.commands import Ops
from robocad.document import Document, Transform
from robocad.physical import export_physical_model


def build(destination):
    destination=Path(destination); destination.mkdir(parents=True,exist_ok=True)
    doc=Document(); ops=Ops(doc)
    mount=ops.new_parametric_component('Mount block')
    link=ops.new_parametric_component('Rectangular link')
    for key,values in ((mount,{'width':20,'depth':20,'length':10}), (link,{'width':8,'depth':8,'length':80})):
        parameters=deepcopy(doc.component_definitions[key].parameters)
        for name,value in values.items():parameters[name]['value']=value
        ops.set_component_parameters(key,parameters)
        for node in doc.component_definitions[key].nodes.values():node.material='petg'
    a=ops.place_component(mount, name='Mount')
    b=ops.place_component(link, Transform((6,6,10)), name='Link')
    group=ops.group([a,b],'Leg assembly')
    parent=next(iter(doc.nodes[a].component_instance['node_map'].values()))
    child=next(iter(doc.nodes[b].component_instance['node_map'].values()))
    ops.add_joint('revolute',parent,child,(10,10,10),(0,1,0),lower=-.7,upper=.7,name='Hinge')
    leg=ops.make_component([group],'Hinged leg')
    spec={'leg_length':{'value':80,'unit':'mm','min':20,'max':200,'provenance':'estimated','description':'Link length from hinge'}}
    ops.set_component_parameters(leg['definition_id'],spec,nested={b:{'parameter_bindings':{'length':'leg_length'}}})
    instances=[leg['instance_id']]
    for i,(x,y) in enumerate(((100,0),(100,100),(0,100)),start=2):
        instances.append(ops.place_component(leg['definition_id'],Transform((x,y,0)),name=f'Leg {i}'))
    ops.rename(instances[0],'Leg 1')
    ops.set_component_overrides(instances[0],{'leg_length':100})
    # Shared default changes leave the first occurrence's explicit override intact.
    spec['leg_length']['value']=90
    ops.set_component_parameters(leg['definition_id'],spec)
    doc.view={'distance':350}
    doc.save(str(destination/'assembly.rcad'))
    ops.export_component(leg['definition_id'],str(destination/'hinged-leg.rcomp'))
    model=export_physical_model(doc,flex=False)
    assert len(model['links'])==8 and len(model['joints'])==4
    report={'schema':1,'definitions':len(doc.component_definitions),'occurrences':instances,
        'leg_definition':leg['definition_id'],'nested_link_template':b,'parameters':spec,
        'expected_link_lengths_mm':[100,90,90,90],'physical_links':len(model['links']),
        'independent_joints':len(model['joints']),'status':'CAD acceptance example; estimated dimensions; no walking controller or calibration'}
    (destination/'verification.json').write_text(json.dumps(report,indent=2)+'\n')
    return doc

if __name__=='__main__':build('examples/components/parametric-quadruped')
