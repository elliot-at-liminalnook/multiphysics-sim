import math
import numpy as np
import pytest
from robocad.document import Document, Node
from robocad.commands import Ops
from robocad.robotics import Joint
from robocad.kernel import KernelError
from robocad.pose import PoseModel
from robocad.motion import validate_program, sample_program
from robocad.api import Service


def crank_slider():
    d=Document()
    for nid in ('base','crank','rod','foot','motor'):
        d.nodes[nid]=Node(nid,'body',nid)
    joints={
        'input':Joint('revolute','base','crank',(0,0,0),motor='motor'),
        'link':Joint('revolute','crank','rod',(75,0,0)),
        'slide':Joint('prismatic','base','foot',(175,0,0),axis=(1,0,0),lower=-100,upper=0),
        'closure':Joint('loop_revolute','rod','foot',(175,0,0)),
    }
    for nid,j in joints.items():d.nodes[nid]=Node(nid,'joint',nid,joint=j)
    return d


def test_closed_knee_matches_analytic_slider_crank_and_returns_home():
    d=crank_slider();model=PoseModel(d)
    for degrees in (0,15,45,-45,0):
        angle=math.radians(degrees);matrices=model.matrices({'input':angle})
        expected=75*math.cos(angle)+math.sqrt(100**2-(75*math.sin(angle))**2)-175
        assert model.last_positions['slide']==pytest.approx(expected,abs=1e-5)
        assert model.last_error_mm<1e-5
        if degrees==0:assert all(np.allclose(m,np.eye(4),atol=1e-5) for m in matrices.values())
    assert set(model.drivers)=={'input'}
    # Failed solve must leave the last valid pose available to the viewer.
    old=dict(model.last_positions)
    with pytest.raises(KernelError):model.matrices({'input':math.pi})
    assert model.last_positions==old


def test_transmission_sign_home_and_chain():
    d=Document()
    for i in range(3):
        nid=f'part{i}';jid=f'joint{i}'
        d.nodes[nid]=Node(nid,'body',nid)
        d.nodes[jid]=Node(jid,'joint',jid,joint=Joint('revolute',None,nid,(0,0,0),home=.2))
    d.robot_settings['transmissions']=[{'driver_joint':'joint0','driven_joint':'joint1','ratio':5}, {'driver_joint':'joint1','driven_joint':'joint2','ratio':-2}]
    m=PoseModel(d);m.matrices({'joint0':1.2})
    assert m.last_positions['joint1']==pytest.approx(.4)
    assert m.last_positions['joint2']==pytest.approx(.1)
    d.robot_settings['transmissions'].append({'driver_joint':'joint2','driven_joint':'joint0','ratio':1})
    with pytest.raises(KernelError,match='cycle'):PoseModel(d)


def pattern():
    return {'name':'Knee cycle','duration':4.,'loop':True,'tracks':[{'joint':'input','unit':'deg','keys':[[0,0],[2,45],[4,0]]}]}


def test_program_units_validation_sampling_persistence_and_undo(tmp_path):
    d=crank_slider();o=Ops(d);p=validate_program(d,pattern())
    assert sample_program(p,1)['input']==pytest.approx(math.radians(22.5))
    assert sample_program(p,4)['input']==0
    o.save_motion(p); assert 'Knee cycle' in d.robot_settings['motion_programs']
    # Metadata persistence uses the ordinary CAD manifest (no preview transforms).
    path=tmp_path/'motion.rcad';d.save(str(path))
    assert Document.load(str(path)).robot_settings['motion_programs']['Knee cycle']==p
    o.undo();assert 'motion_programs' not in d.robot_settings
    o.redo();o.delete_motion(p['name']);assert not d.robot_settings['motion_programs']
    for mutate in (lambda t:t.update(unit='mm'),lambda t:t.update(joint='slide'),lambda t:t.update(keys=[[0,0],[4,float('nan')]]),lambda t:t.update(keys=[[0,0],[4,1]])):
        bad=pattern();mutate(bad['tracks'][0])
        with pytest.raises(KernelError):validate_program(d,bad)


def test_motion_service_program_crud_and_headless_playback_error():
    from robocad.api import ApiError
    d=crank_slider();s=Service(d)
    s.motion_request('POST',['motion','programs'],pattern())
    assert 'Knee cycle' in s.motion_request('GET',['motion','programs'],{})
    with pytest.raises(ApiError):s.motion_request('POST',['motion','play'],{'program':'Knee cycle'})
    s.motion_request('DELETE',['motion','programs'],{'name':'Knee cycle'})
    assert not s.motion_request('GET',['motion','programs'],{})


def test_positive_contact_stop_rejects_program_and_manual_motion():
    d=crank_slider();d.nodes['input'].joint.upper=math.radians(4.5)
    p=pattern()
    with pytest.raises(KernelError,match='bounds'):validate_program(d,p)
    p['tracks'][0]['keys'][1][1]=-45
    validate_program(d,p)
    m=PoseModel(d);m.matrices({'input':math.radians(-45)})
    previous=dict(m.last_positions)
    with pytest.raises(KernelError,match='bounds'):m.matrices({'input':math.radians(5)})
    assert m.last_positions==previous


def test_passive_slider_can_retract_past_widget_default_without_inventing_limits():
    d=crank_slider();d.nodes['slide'].joint.lower=None;d.nodes['slide'].joint.upper=None
    model=PoseModel(d)
    for degrees in (-45,-90,-120,0):
        angle=math.radians(degrees);model.matrices({'input':angle})
        expected=75*math.cos(angle)+math.sqrt(100**2-(75*math.sin(angle))**2)-175
        assert model.last_positions['slide']==pytest.approx(expected,abs=1e-5)
    assert d.nodes['slide'].joint.stroke==0
    assert d.nodes['slide'].joint.lower is None and d.nodes['slide'].joint.upper is None
    d.nodes['slide'].joint.lower=-100
    with pytest.raises(KernelError):PoseModel(d).matrices({'input':-math.pi/2})


def test_large_scrub_keeps_slider_crank_on_its_imported_assembly_branch():
    d=crank_slider();d.nodes['slide'].joint.lower=None;d.nodes['slide'].joint.upper=None
    model=PoseModel(d)
    for degrees in (-170,0,-130,0):
        angle=math.radians(degrees);model.matrices({'input':angle})
        expected=75*math.cos(angle)+math.sqrt(100**2-(75*math.sin(angle))**2)-175
        assert model.last_positions['slide']==pytest.approx(expected,abs=1e-5)
