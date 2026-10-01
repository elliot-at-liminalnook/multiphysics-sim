"""The CAD → simulation export: masses, inertias, outlines, joints, ground."""

import json

from robocad.commands import Ops
from robocad.document import Document
from robocad.kernel import Plane
from robocad.simbridge import export_sim_model, joints_of


def test_export_model(tmp_path):
    doc = Document()
    ops = Ops(doc)
    hip = ops.box((-25, -20, 200), (50, 40, 40), name="ground")
    thigh = ops.box((-8, -6, 90), (16, 12, 120), name="thigh")
    ops.set_material([thigh], "petg")
    ops.plane_three_points((0, 0, 200), (1, 0, 200), (0, 0, 201), name="joint:thigh:ground")
    p = str(tmp_path / "m.simrobot.json")
    model = export_sim_model(doc, p, plane=Plane.xz(), version=2)
    with open(p) as f:
        back = json.load(f)
    assert back["format"] == "simrobot" and len(back["bodies"]) == 2 and len(back["joints"]) == 1
    g = next(b for b in back["bodies"] if b["name"] == "ground")
    t = next(b for b in back["bodies"] if b["name"] == "thigh")
    assert g["ground"] and not t["ground"]
    # 16×12×120 mm PETG bar: 23.04 cm³ × 1.27 → 29.3 g; rod inertia m(L²+w²)/12 about the plane normal (y).
    assert abs(t["mass_kg"] * 1000 - 29.26) < 0.1
    m, L, w = t["mass_kg"], 0.120, 0.016
    assert abs(t["inertia_zz"] - m * (L * L + w * w) / 12) / (m * (L * L + w * w) / 12) < 0.02
    assert t["outline"] and all(len(loop) >= 2 for loop in t["outline"])
    j = back["joints"][0]
    assert j["child"] == "thigh" and j["parent"] == "ground"
    assert j["pivot2"] == [0.0, 200.0]


def test_joint_parent_inferred():
    doc = Document()
    ops = Ops(doc)
    ops.box((0, 0, 0), (10, 10, 10), name="base")
    ops.box((0, 0, 10), (10, 10, 30), name="arm")
    ops.plane_three_points((5, 5, 10), (6, 5, 10), (5, 5, 11), name="joint:arm")
    js = joints_of(doc)
    assert js[0]["parent"] == "base"


# The live-loop viewer: sim-spatial robot mode, release then debug, else no
# launch and the build command (no other viewer is ever launched).

import os
from unittest import mock

import pytest

from robocad import simbridge
from robocad.simbridge import ROOT, SimLink, viewer_command, watch_and_run


def _built(*rel):
    present = {os.path.join(ROOT, "target", *r.split("/")) for r in rel}
    return mock.patch.object(simbridge.os.path, "exists", side_effect=lambda p: p in present)


def test_viewer_prefers_release_sim_spatial():
    with _built("release/sim-spatial", "debug/sim-spatial"):
        argv, message = viewer_command("/m/robot.simrobot.json")
    assert argv == [os.path.join(ROOT, "target", "release", "sim-spatial"), "--robot", "/m/robot.simrobot.json"]
    assert message == ""


def test_viewer_passes_an_absolute_model_path(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)  # a relative path from the caller's cwd, not the viewer's cwd=ROOT
    with _built("release/sim-spatial"):
        argv, _ = viewer_command("robot.simrobot.json")
    assert argv[1:] == ["--robot", str(tmp_path / "robot.simrobot.json")]


def test_viewer_debug_when_no_release():
    with _built("debug/sim-spatial"):
        argv, message = viewer_command("/m/robot.simrobot.json")
    assert argv == [os.path.join(ROOT, "target", "debug", "sim-spatial"), "--robot", "/m/robot.simrobot.json"]
    assert message == ""


def test_viewer_none_names_build_command():
    with _built():
        argv, message = viewer_command("/m/robot.simrobot.json")
    assert argv is None and message == "no simulator viewer built: cargo build --release -p sim-spatial"


def test_viewer_without_sim_spatial_launches_nothing_whatever_else_is_built():
    # Any other binary under target/ is never a fallback.
    with mock.patch.object(simbridge.os.path, "exists", side_effect=lambda p: not p.endswith(os.sep + "sim-spatial")):
        argv, message = viewer_command("/m/robot.simrobot.json")
    assert argv is None and "cargo build --release -p sim-spatial" in message


class _App:
    def __init__(self):
        self.messages = []

    def status(self, m):
        self.messages.append(m)


def _link(tmp_path):
    doc = Document()
    doc.path = str(tmp_path / "robot.rcad")
    return SimLink(doc, _App())


def test_simlink_launch_argv_cwd_env_and_no_relaunch_while_alive(tmp_path):
    link = _link(tmp_path)
    proc = mock.Mock()
    proc.poll.return_value = None  # alive
    with _built("release/sim-spatial"), mock.patch.object(simbridge.subprocess, "Popen", return_value=proc) as popen:
        link.launch()
        link.launch()  # a save while the viewer runs: it reloads the file itself
    popen.assert_called_once()
    args, kwargs = popen.call_args
    assert args[0] == [os.path.join(ROOT, "target", "release", "sim-spatial"), "--robot", str(tmp_path / "robot.simrobot.json")]
    assert kwargs["cwd"] == ROOT
    assert kwargs["env"]["PATH"].startswith(os.path.expanduser("~/.cargo/bin") + os.pathsep)
    assert link.app.messages == []
    proc.poll.return_value = 0  # the viewer was closed: the next launch starts a new one
    with _built("release/sim-spatial"), mock.patch.object(simbridge.subprocess, "Popen", return_value=proc) as popen:
        link.launch()
    popen.assert_called_once()


def test_simlink_without_sim_spatial_reports_build_command_and_launches_nothing(tmp_path):
    link = _link(tmp_path)
    with _built(), mock.patch.object(simbridge.subprocess, "Popen") as popen:
        link.launch()
    popen.assert_not_called()
    assert "cargo build --release -p sim-spatial" in link.app.messages[-1]


def test_watch_and_run_uses_viewer_command_once(tmp_path, capsys):
    doc_path = str(tmp_path / "robot.rcad")
    proc = mock.Mock()
    proc.poll.return_value = None
    mtimes = iter([1.0, 2.0])

    def getmtime(p):
        try:
            return next(mtimes)
        except StopIteration:
            raise KeyboardInterrupt  # end the CLI loop

    with _built("debug/sim-spatial"), \
            mock.patch.object(simbridge.os.path, "getmtime", side_effect=getmtime), \
            mock.patch.object(simbridge.Document, "load", return_value=Document()), \
            mock.patch.object(simbridge, "export_sim_model") as export, \
            mock.patch.object(simbridge.time, "sleep"), \
            mock.patch.object(simbridge.subprocess, "Popen", return_value=proc) as popen:
        with pytest.raises(KeyboardInterrupt):
            watch_and_run(doc_path)
    assert export.call_count == 2  # every change re-exports
    popen.assert_called_once()  # the live viewer is not relaunched
    args, kwargs = popen.call_args
    assert args[0] == [os.path.join(ROOT, "target", "debug", "sim-spatial"), "--robot", str(tmp_path / "robot.simrobot.json")]
    assert kwargs["cwd"] == ROOT and "PATH" in kwargs["env"]
