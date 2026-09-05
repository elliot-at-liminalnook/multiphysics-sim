"""References survive missing source files; pose motion preserves design geometry."""
import math
import numpy as np
import pytest
from PIL import Image
from robocad.commands import Ops
from robocad.document import Document
from robocad.kernel import KernelError, Plane
from robocad.pose import PoseModel


def test_reference_import_calibration_and_persistence(tmp_path):
    d = Document(); o = Ops(d)
    paths = [tmp_path/'front.png',tmp_path/'side.png']
    for p in paths: Image.new('RGB',(200,100),'white').save(p)
    ids = o.import_references(paths,Plane.xz())
    assert len(ids)==2 and all(d.nodes[i].locked for i in ids)
    o.undo(); assert not d.nodes
    o.redo()
    o.calibrate_reference(ids[0],[10,0,0],[30,0,0],40)
    im = d.nodes[ids[0]].image
    assert im['width']==200 and im['height']==100
    assert im['plane'].origin==(-10,0,0)  # first landmark remains fixed
    o.undo(); assert d.nodes[ids[0]].image['width']==100
    o.redo()
    o.update_reference(ids[1],width=80,rotation_deg=90,opacity=.25,visible=False)
    assert d.nodes[ids[1]].image['plane'].x_axis == pytest.approx((0,0,1))
    saved = tmp_path/'references.rcad'
    d.nodes[ids[0]].image['_tex'] = np.uint32(42)  # legacy runtime cache must never enter the file
    d.save(str(saved))
    for p in paths: p.unlink()
    loaded = Document.load(str(saved))
    assert loaded.nodes[ids[0]].image['data'] == d.nodes[ids[0]].image['data']
    assert loaded.nodes[ids[0]].image['width']==200
    assert '_tex' not in loaded.nodes[ids[0]].image
    assert not loaded.nodes[ids[1]].visible
    with pytest.raises(KernelError): o.calibrate_reference(ids[0],[0,0,0],[0,0,0],10)


def test_reference_batch_is_atomic(tmp_path):
    p = tmp_path/'good.png'; Image.new('RGB',(20,10)).save(p)
    d = Document(); o = Ops(d)
    with pytest.raises(FileNotFoundError): o.import_references([p,tmp_path/'missing.png'])
    assert not d.nodes


def mechanism():
    d = Document(); o = Ops(d)
    a = o.box((0,0,0),(10,2,2))
    b = o.box((10,0,0),(10,2,2))
    c = o.box((20,0,0),(2,2,2))
    j1 = o.add_joint('revolute',None,a,(0,0,0),lower=-math.pi,upper=math.pi)
    j2 = o.add_joint('revolute',a,b,(10,0,0),lower=-math.pi,upper=math.pi)
    o.connect_fixed(b,c)
    return d,o,a,b,c,j1,j2


def point(matrix,p): return matrix[:3,:3]@np.array(p)+matrix[:3,3]


def test_pose_chain_fixed_parts_motor_mount_and_home():
    d,o,a,b,c,j1,j2 = mechanism()
    motor = o.box((10,0,0),(2,2,2))
    d.nodes[motor].robot = {'kind':'motor','mounted_on':a}
    unconnected = o.box((100,0,0),(1,1,1))
    model = PoseModel(d)
    matrices = model.matrices({j1:math.pi/2,j2:math.pi/2})
    assert point(matrices[b],(20,0,0)) == pytest.approx((-10,10,0))
    assert np.allclose(matrices[c],matrices[b])
    assert point(matrices[motor],(10,0,0)) == pytest.approx((0,10,0))
    assert np.allclose(matrices[unconnected],np.eye(4))
    o.set_joint(j1,home=.4)
    assert all(np.allclose(m,np.eye(4)) for m in PoseModel(d).matrices({j1:.4,j2:0}).values())


def test_prismatic_axis_follows_parent_and_limits():
    d,o,a,b,c,j1,j2 = mechanism()
    o.set_joint(j2,type='prismatic',axis=(1,0,0),lower=0,upper=20)
    model = PoseModel(d)
    m = model.matrices({j1:math.pi/2,j2:5})
    assert point(m[b],(20,0,0)) == pytest.approx((0,25,0))
    for values in ({j2:21},{j2:float('nan')},{'missing':0}):
        with pytest.raises(KernelError): model.matrices(values)


def test_pose_rejects_cycles_and_closed_constraints():
    d,o,a,b,c,j1,j2 = mechanism()
    o.set_joint(j1,parent=b)
    with pytest.raises(KernelError,match='cycle'): PoseModel(d)
    o.undo()
    o.add_joint('loop_revolute',c,a,(0,0,0))
    with pytest.raises(KernelError,match='constraint solver'): PoseModel(d)
