"""Build the system builder's component display models in CAD and export them.

Each model is a real-size (millimetre) CAD part made of coloured bodies,
exported as OBJ + MTL in metres with +Y up (the physical viewer's frame), so
the Rust viewer shows recognisable components instead of primitive shapes.
Models are presentation only: the physics comes from the system file.

    cd cad && .venv/bin/python scripts/component_models.py ../library/models

Dimensions follow common package drawings (TO-220, DO-41, SOIC-8, SOT-23,
18650) or are representative (servo body, heatsink, flywheel).
"""
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from robocad.commands import Ops  # noqa: E402
from robocad.document import Document  # noqa: E402
from robocad.io.exporters import ObjSettings, export_obj  # noqa: E402
from robocad.kernel import BooleanOp  # noqa: E402

BLACK = (0.07, 0.07, 0.08)
EPOXY = (0.12, 0.12, 0.13)
TIN = (0.78, 0.79, 0.80)
COPPER = (0.80, 0.50, 0.28)
ALUMINIUM = (0.74, 0.76, 0.79)
NICKEL = (0.66, 0.67, 0.66)
SLEEVE_BLUE = (0.10, 0.22, 0.48)
BEIGE = (0.84, 0.74, 0.56)
BATTERY_BLUE = (0.16, 0.36, 0.72)
WHITE = (0.92, 0.92, 0.90)


class Model:
    def __init__(self, name):
        self.name = name
        self.doc = Document()
        self.ops = Ops(self.doc)
        self.ids = []

    def box(self, corner, size, color, name='body'):
        i = self.ops.box(corner, size, name=name)
        self.ops.set_color([i], color)
        self.ids.append(i)
        return i

    def cyl(self, base, axis, radius, height, color, name='body'):
        i = self.ops.cylinder(base, axis, radius, height, name=name)
        self.ops.set_color([i], color)
        self.ids.append(i)
        return i

    def cut(self, target, tool):
        self.ids.remove(tool)
        self.ops.boolean(target, [tool], BooleanOp.SUBTRACT)

    def round(self, target, radius):
        try:
            self.ops.fillet_all(target, radius)
        except Exception:
            pass  # A missed cosmetic fillet never blocks the model.

    def export(self, out):
        path = os.path.join(out, f'{self.name}.obj')
        export_obj(self.doc, path, self.ids, ObjSettings(tolerance=0.03, scale=0.001, up_axis='Y', uvs=False))
        return path


def to220():
    m = Model('to220')
    body = m.box((-5.0, -2.25, 3.5), (10.0, 3.25, 9.0), EPOXY, 'package')
    m.round(body, 0.4)
    tab = m.box((-5.0, 1.0, 3.5), (10.0, 1.3, 15.0), TIN, 'tab')
    hole = m.cyl((0.0, 0.5, 15.2), (0.0, 1.0, 0.0), 1.8, 2.5, TIN)
    m.cut(tab, hole)
    for x in (-2.54, 0.0, 2.54):
        m.box((x - 0.4, -0.5, 0.0), (0.8, 0.5, 3.6), TIN, 'lead')
    return m


def radial_capacitor():
    m = Model('radial_capacitor')
    can = m.cyl((0, 0, 1.0), (0, 0, 1), 4.0, 11.0, SLEEVE_BLUE, 'sleeve')
    m.round(can, 0.6)
    m.cyl((0, 0, 11.95), (0, 0, 1), 3.4, 0.1, ALUMINIUM, 'top')
    m.box((-0.8, -4.05, 1.4), (1.6, 0.2, 10.2), (0.75, 0.78, 0.84), 'polarity stripe')
    for x in (-1.75, 1.75):
        m.cyl((x, 0, 0), (0, 0, 1), 0.3, 1.2, TIN, 'lead')
    return m


def axial_resistor():
    m = Model('axial_resistor')
    body = m.cyl((-3.2, 0, 2.0), (1, 0, 0), 1.2, 6.4, BEIGE, 'body')
    m.round(body, 0.5)
    for x, color in ((-2.1, (0.45, 0.25, 0.10)), (-1.1, BLACK), (-0.1, (0.95, 0.45, 0.10)), (1.9, (0.80, 0.66, 0.25))):
        m.cyl((x, 0, 2.0), (1, 0, 0), 1.24, 0.5, color, 'band')
    for sign in (-1, 1):
        m.cyl((sign * 3.2, 0, 2.0), (sign, 0, 0), 0.3, 2.4, TIN, 'lead')
        m.cyl((sign * 5.35, 0, 0.0), (0, 0, 1), 0.3, 2.3, TIN, 'lead')
    return m


def do41_diode():
    m = Model('do41_diode')
    m.cyl((-2.6, 0, 1.8), (1, 0, 0), 1.35, 5.2, BLACK, 'body')
    m.cyl((1.6, 0, 1.8), (1, 0, 0), 1.37, 0.7, (0.80, 0.80, 0.82), 'cathode band')
    for sign in (-1, 1):
        m.cyl((sign * 2.6, 0, 1.8), (sign, 0, 0), 0.4, 2.0, TIN, 'lead')
        m.cyl((sign * 4.4, 0, 0.0), (0, 0, 1), 0.4, 1.9, TIN, 'lead')
    return m


def power_inductor():
    m = Model('power_inductor')
    core = m.box((-6.0, -6.0, 0.2), (12.0, 12.0, 8.0), (0.18, 0.18, 0.20), 'shielded core')
    m.round(core, 1.0)
    m.cyl((0, 0, 8.15), (0, 0, 1), 4.0, 0.1, COPPER, 'winding window')
    for x in (-6.2, 3.7):
        m.box((x, -2.5, 0.0), (2.5, 5.0, 0.4), TIN, 'terminal')
    return m


def soic8():
    m = Model('soic8')
    body = m.box((-2.45, -1.95, 0.25), (4.9, 3.9, 1.5), EPOXY, 'package')
    m.round(body, 0.15)
    for i in range(4):
        x = -1.905 + i * 1.27
        for y in (-3.0, 1.95):
            m.box((x - 0.2, y, 0.0), (0.4, 1.05, 0.25), TIN, 'lead')
    m.cyl((-1.6, -1.1, 1.75), (0, 0, 1), 0.35, 0.02, (0.35, 0.35, 0.37), 'pin 1')
    return m


def sot23():
    m = Model('sot23')
    body = m.box((-1.45, -0.65, 0.15), (2.9, 1.3, 1.0), EPOXY, 'package')
    m.round(body, 0.1)
    for x, y in ((-0.95, -1.15), (0.95, -1.15), (0.0, 0.65)):
        m.box((x - 0.2, y, 0.0), (0.4, 0.5, 0.15), TIN, 'lead')
    return m


def battery_3s():
    m = Model('battery_3s')
    for y in (-19.0, 0.0, 19.0):
        cell = m.cyl((-32.5, y, 9.2), (1, 0, 0), 9.0, 65.0, BATTERY_BLUE, 'cell')
        m.round(cell, 1.0)
        for x in (-32.9, 32.5):
            m.cyl((x, y, 9.2), (1, 0, 0), 6.5, 0.4, NICKEL, 'cap')
    for x in (-33.2, 32.9):
        m.box((x, -24.0, 5.0), (0.3, 48.0, 8.4), NICKEL, 'strip')
    return m


def servo():
    m = Model('servo')
    body = m.box((-20.0, -10.0, 0.0), (40.0, 20.0, 36.0), BLACK, 'case')
    m.round(body, 1.2)
    m.box((-27.0, -10.0, 26.0), (54.0, 20.0, 2.5), BLACK, 'mounting ears')
    m.cyl((10.0, 0.0, 36.0), (0, 0, 1), 6.0, 2.0, (0.20, 0.20, 0.22), 'boss')
    m.cyl((10.0, 0.0, 38.0), (0, 0, 1), 2.5, 3.5, TIN, 'output spline')
    m.cyl((10.0, 0.0, 41.5), (0, 0, 1), 10.0, 2.0, WHITE, 'horn')
    return m


def dc_motor():
    m = Model('dc_motor')
    can = m.cyl((0, 0, 0), (0, 0, 1), 12.0, 30.0, ALUMINIUM, 'can')
    m.round(can, 1.0)
    m.cyl((0, 0, 30.0), (0, 0, 1), 5.0, 2.0, (0.25, 0.25, 0.27), 'bearing boss')
    m.cyl((0, 0, 32.0), (0, 0, 1), 1.0, 9.0, TIN, 'shaft')
    return m


def flywheel():
    m = Model('flywheel')
    disc = m.cyl((0, 0, 0), (0, 0, 1), 15.0, 6.0, ALUMINIUM, 'disc')
    m.round(disc, 0.8)
    m.cyl((0, 0, 6.0), (0, 0, 1), 4.0, 4.0, (0.55, 0.57, 0.60), 'hub')
    m.box((9.0, -1.0, 6.0), (5.0, 2.0, 0.2), (0.95, 0.60, 0.20), 'index mark')
    return m


STEEL = (0.62, 0.64, 0.67)
BRONZE = (0.72, 0.52, 0.24)
FRAME_GREY = (0.30, 0.32, 0.35)
MARK = (0.95, 0.60, 0.20)


def worm():
    """Single-start worm, module 0.5: pitch Ø8, root Ø6.8, tip Ø9, lead π·m ≈ 1.571 mm.

    The thread is drawn as a stepped helix of short segments (16 per turn),
    which reads as a thread at viewing distance without a helical sweep.
    """
    m = Model('worm')
    m.cyl((0, 0, -13.0), (0, 0, 1), 1.5, 26.0, TIN, 'shaft')
    m.cyl((0, 0, -9.0), (0, 0, 1), 3.4, 18.0, STEEL, 'root')
    lead, per_turn = 3.14159 * 0.5, 16
    steps = int(17.0 / lead * per_turn)
    for k in range(steps):
        z = -8.5 + k * lead / per_turn
        i = m.box((3.2, -0.55, z - 0.3), (1.3, 1.1, 0.6), STEEL, 'thread')
        m.ops.transform([i], axis=(0, 0, 1), angle_deg=360.0 * k / per_turn, center=(0, 0, 0))
    m.box((1.5, -0.4, 9.0), (1.4, 0.8, 1.0), MARK, 'index mark')
    return m


def worm_wheel_drum():
    """30-tooth bronze worm wheel (module 0.5, pitch Ø15) on a Ø20 winch drum."""
    m = Model('worm_wheel_drum')
    m.cyl((0, 0, -2.0), (0, 0, 1), 7.1, 4.0, BRONZE, 'wheel rim')
    for k in range(30):
        i = m.box((7.0, -0.4, -1.8), (0.9, 0.8, 3.6), BRONZE, 'tooth')
        m.ops.transform([i], axis=(0, 0, 1), angle_deg=12.0 * k, center=(0, 0, 0))
    m.cyl((0, 0, -6.0), (0, 0, 1), 1.5, 24.0, TIN, 'shaft')
    m.cyl((0, 0, 2.0), (0, 0, 1), 11.5, 1.0, ALUMINIUM, 'drum flange')
    m.cyl((0, 0, 3.0), (0, 0, 1), 10.0, 10.0, ALUMINIUM, 'drum')
    m.cyl((0, 0, 13.0), (0, 0, 1), 11.5, 1.0, ALUMINIUM, 'drum flange')
    m.cyl((0, 0, 3.2), (0, 0, 1), 10.15, 9.6, (0.85, 0.82, 0.70), 'rope wraps')
    m.box((9.5, -0.6, 14.0), (2.5, 1.2, 0.3), MARK, 'index mark')
    return m


def worm_gear_frame():
    """Base plate and bearing blocks for a worm set whose worm axis runs along
    +X at 30 mm and whose wheel axis runs along the viewer's +Z at 18.5 mm."""
    m = Model('worm_gear_frame')
    m.box((-20.0, -12.0, -2.0), (40.0, 26.0, 2.0), FRAME_GREY, 'base plate')
    for x in (-15.0, 12.0):
        b = m.box((x, -3.0, 0.0), (3.0, 6.0, 33.0), FRAME_GREY, 'worm bearing block')
        m.cut(b, m.cyl((x - 0.5, 0.0, 30.0), (1, 0, 0), 1.6, 4.0, FRAME_GREY))
    b = m.box((-3.0, 4.0, 0.0), (6.0, 2.5, 21.5), FRAME_GREY, 'wheel bearing block')
    m.cut(b, m.cyl((0.0, 3.5, 18.5), (0, 1, 0), 1.6, 3.5, FRAME_GREY))
    return m


def lead_screw_nut():
    """Tr8 lead screw (100 mm shown) with a flanged bronze nut."""
    m = Model('lead_screw_nut')
    m.cyl((0, 0, 0), (0, 0, 1), 3.5, 100.0, STEEL, 'screw')
    for k in range(50):
        m.cyl((0, 0, 1.0 + 2.0 * k), (0, 0, 1), 4.0, 0.8, STEEL, 'thread crest')
    m.cyl((0, 0, 40.0), (0, 0, 1), 7.5, 15.0, BRONZE, 'nut')
    m.cyl((0, 0, 40.0), (0, 0, 1), 11.0, 3.5, BRONZE, 'nut flange')
    return m


RUBBER = (0.10, 0.10, 0.11)
BLUE_ANODISED = (0.15, 0.35, 0.75)
PCB_GREEN = (0.10, 0.42, 0.22)
PROP_GREY = (0.20, 0.20, 0.22)


def nema17():
    """NEMA 17 stepper: 42.3 mm square, 40 mm body, 5 mm shaft, pilot boss."""
    m = Model('nema17')
    m.box((-21.15, -21.15, 0.0), (42.3, 42.3, 8.0), ALUMINIUM, 'end cap')
    m.box((-21.15, -21.15, 8.0), (42.3, 42.3, 24.0), BLACK, 'stator stack')
    m.box((-21.15, -21.15, 32.0), (42.3, 42.3, 8.0), ALUMINIUM, 'front cap')
    m.cyl((0, 0, 40.0), (0, 0, 1), 11.0, 2.0, ALUMINIUM, 'pilot boss')
    m.cyl((0, 0, 42.0), (0, 0, 1), 2.5, 22.0, TIN, 'shaft')
    m.box((1.5, -2.5, 50.0), (1.0, 5.0, 14.0), MARK, 'shaft flat')
    return m


def outrunner():
    """2212-class outrunner: 28 mm bell on a cross mount."""
    m = Model('outrunner')
    m.box((-16.0, -2.0, 0.0), (32.0, 4.0, 2.0), BLACK, 'mount arm')
    m.box((-2.0, -16.0, 0.0), (4.0, 32.0, 2.0), BLACK, 'mount arm')
    m.cyl((0, 0, 2.0), (0, 0, 1), 12.0, 4.0, (0.25, 0.25, 0.27), 'stator base')
    m.cyl((0, 0, 6.0), (0, 0, 1), 14.0, 20.0, BLUE_ANODISED, 'bell')
    m.cyl((0, 0, 26.0), (0, 0, 1), 3.0, 8.0, TIN, 'prop shaft')
    m.box((10.0, -1.0, 24.0), (3.5, 2.0, 1.0), MARK, 'index mark')
    return m


def propeller():
    """10-inch two-blade propeller (254 mm), hub at the origin."""
    m = Model('propeller')
    m.cyl((0, 0, -4.0), (0, 0, 1), 8.0, 8.0, PROP_GREY, 'hub')
    for sign in (1, -1):
        m.box((8.0 if sign > 0 else -127.0, -9.0, -1.5), (119.0, 18.0, 3.0), PROP_GREY, 'blade')
    m.box((110.0, -9.0, 1.4), (15.0, 18.0, 0.3), MARK, 'tip mark')
    return m


def wheel():
    """80 mm wheel: rubber tyre, hub, axle."""
    m = Model('wheel')
    tyre = m.cyl((0, 0, -12.0), (0, 0, 1), 40.0, 24.0, RUBBER, 'tyre')
    m.cut(tyre, m.cyl((0, 0, -13.0), (0, 0, 1), 30.0, 26.0, RUBBER))
    m.cyl((0, 0, -11.0), (0, 0, 1), 30.0, 22.0, (0.85, 0.55, 0.15), 'rim')
    m.cyl((0, 0, -16.0), (0, 0, 1), 3.0, 32.0, TIN, 'axle')
    for k in range(5):
        b = m.box((8.0, -3.0, 11.0), (18.0, 6.0, 1.5), (0.95, 0.70, 0.25), 'spoke mark')
        m.ops.transform([b], axis=(0, 0, 1), angle_deg=72.0 * k, center=(0, 0, 0))
    return m


def gt2_pulley():
    """GT2 20-tooth pulley (12.7 mm pitch) with a belt loop section."""
    m = Model('gt2_pulley')
    m.cyl((0, 0, 0.0), (0, 0, 1), 8.0, 1.0, ALUMINIUM, 'flange')
    m.cyl((0, 0, 1.0), (0, 0, 1), 6.1, 7.0, ALUMINIUM, 'toothed body')
    m.cyl((0, 0, 8.0), (0, 0, 1), 8.0, 1.0, ALUMINIUM, 'flange')
    m.cyl((0, 0, 9.0), (0, 0, 1), 7.0, 7.0, ALUMINIUM, 'hub')
    m.box((-0.6, 6.1, 1.5), (60.0, 1.4, 6.0), BLACK, 'belt')
    m.box((-0.6, -7.5, 1.5), (60.0, 1.4, 6.0), BLACK, 'belt')
    return m


def rack_pinion():
    """Module-1 pinion (20 teeth) on a rack section."""
    m = Model('rack_pinion')
    m.cyl((0, 0, -5.0), (0, 0, 1), 9.0, 10.0, TIN, 'pinion body')
    for k in range(20):
        b = m.box((9.0, -0.8, -5.0), (1.8, 1.6, 10.0), TIN, 'pinion tooth')
        m.ops.transform([b], axis=(0, 0, 1), angle_deg=18.0 * k, center=(0, 0, 0))
    m.box((-40.0, -21.0, -5.0), (80.0, 8.0, 10.0), STEEL, 'rack bar')
    for k in range(26):
        m.box((-39.0 + 3.1416 * k, -13.0, -5.0), (1.6, 2.0, 10.0), STEEL, 'rack tooth')
    return m


def solenoid():
    """Open-frame pull solenoid with its plunger."""
    m = Model('solenoid')
    m.box((-10.0, -8.0, 0.0), (20.0, 16.0, 1.5), STEEL, 'frame')
    m.box((-10.0, -8.0, 26.5), (20.0, 16.0, 1.5), STEEL, 'frame')
    m.box((-10.0, -8.0, 0.0), (1.5, 16.0, 28.0), STEEL, 'frame')
    m.cyl((0, 0, 1.5), (0, 0, 1), 7.5, 25.0, COPPER, 'coil')
    m.cyl((0, 0, 20.0), (0, 0, 1), 2.5, 18.0, TIN, 'plunger')
    return m


def driver_board():
    """Motor driver board (H-bridge) with heatsink and terminals."""
    m = Model('driver_board')
    m.box((-20.0, -15.0, 0.0), (40.0, 30.0, 1.6), PCB_GREEN, 'board')
    m.box((-6.0, -6.0, 1.6), (12.0, 12.0, 8.0), ALUMINIUM, 'heatsink')
    for x in (-17.0, -12.0, 12.0, 17.0):
        m.box((x - 2.0, 9.0, 1.6), (4.0, 5.0, 6.0), (0.15, 0.45, 0.85), 'terminal')
    return m


def heatsink():
    m = Model('heatsink')
    m.box((-15.0, -10.0, 0.0), (30.0, 20.0, 3.0), ALUMINIUM, 'base')
    for i in range(7):
        m.box((-14.5 + i * 4.6, -10.0, 3.0), (1.2, 20.0, 12.0), ALUMINIUM, 'fin')
    return m


# ---- Joinery lesson models -------------------------------------------------
# Built in the lesson's world frame (mm, +Y up) around the part's placement,
# then turned into the part's own frame: `orient='X'` parts slide along world
# +X (their local +Y), so they are turned +90° about world Z.
PLA_ORANGE = (0.93, 0.55, 0.16)
PLA_BLUE = (0.20, 0.46, 0.72)
STEEL = (0.66, 0.68, 0.71)
BRASS = (0.82, 0.64, 0.27)


class WorldModel(Model):
    def __init__(self, name, orient='Y'):
        super().__init__(name)
        self.orient = orient

    def wbox(self, center, size, color, name='body'):
        cx, cy, cz = center
        sx, sy, sz = size
        return self.box((cx - sx / 2, -(cz + sz / 2), cy - sy / 2), (sx, sz, sy), color, name)

    def wcyl(self, base, axis, radius, height, color, name='body'):
        return self.cyl((base[0], -base[2], base[1]), (axis[0], -axis[2], axis[1]), radius, height, color, name)

    def _add(self, body, color, name):
        n = self.doc.add_body(body, name)
        n.color = color
        self.ids.append(n.id)
        return n.id

    def wprism_z(self, points_xy, z0, length, color, name='body'):
        """Profile in the world XY plane at z0, extruded along world +Z."""
        from robocad.kernel import Plane, Sketch
        sk = Sketch(Plane.xz(-z0)); sk.polyline(points_xy, closed=True)
        return self._add(self.doc.kernel.extrude(sk.to_body(), (0, -1, 0), length), color, name)

    def wprism_x(self, points_yz, x0, length, color, name='body'):
        """Profile in the world YZ plane at x0, extruded along world +X."""
        from robocad.kernel import Plane, Sketch
        sk = Sketch(Plane.yz(x0)); sk.polyline([(-z, y) for y, z in points_yz], closed=True)
        return self._add(self.doc.kernel.extrude(sk.to_body(), (1, 0, 0), length), color, name)

    def export(self, out):
        if self.orient == 'X':
            self.ops.transform(self.ids, axis=(0, -1, 0), angle_deg=90.0, center=(0, 0, 0))
        return super().export(out)


def _pin_plate(name, holes, width):
    m = WorldModel(name, 'X')
    plate = m.wbox((0, 0, 0), (width, 6, 40), PLA_ORANGE, 'plate')
    for x in holes:
        m.cut(plate, m.wcyl((x, -4, 0), (0, 1, 0), 1.6, 8, PLA_ORANGE))
    m.round(plate, 0.6)
    return m


def _pin_base(name, holes):
    m = WorldModel(name, 'Y')
    base = m.wbox((0, 0, 0), (100, 12, 50), PLA_BLUE, 'base')
    for x in holes:
        m.cut(base, m.wcyl((x, -7, 0), (0, 1, 0), 1.55, 14, PLA_BLUE))
    m.round(base, 0.8)
    return m


def pin_plate():
    return _pin_plate('pin_plate', [0], 70)


def pin_plate_two():
    return _pin_plate('pin_plate_two', [-30, 30], 90)


def pin_base():
    return _pin_base('pin_base', [0])


def pin_base_two():
    return _pin_base('pin_base_two', [-30, 30])


def dowel_pin():
    m = WorldModel('dowel_pin', 'Y')
    pin = m.wcyl((0, -7, 0), (0, 1, 0), 1.5, 14, STEEL, 'pin')
    m.round(pin, 0.3)
    return m


TAN15 = 0.2679


def dovetail_socket():
    m = WorldModel('dovetail_socket', 'Y')
    block = m.wbox((0, 0, 0), (40, 20, 30), PLA_BLUE, 'socket')
    groove = m.wprism_z([(-6, 10.5), (6, 10.5), (6 + 8.5 * TAN15, 2), (-6 - 8.5 * TAN15, 2)], -16, 32, PLA_BLUE)
    m.cut(block, groove)
    return m


def dovetail_tail():
    m = WorldModel('dovetail_tail', 'Y')
    m.round(m.wbox((0, 0, 0), (24, 24, 30), PLA_ORANGE, 'body'), 0.8)
    m.wprism_z([(-5.9, -11.9), (5.9, -11.9), (5.9 + 8 * TAN15, -20), (-5.9 - 8 * TAN15, -20)], -15, 30, PLA_ORANGE, 'tail')
    return m


def dovetail_groove_long():
    m = WorldModel('dovetail_groove_long', 'Y')
    block = m.wbox((0, 0, 0), (80, 20, 30), PLA_BLUE, 'groove part')
    groove = m.wprism_x([(10.5, -6), (10.5, 6), (2, 6 + 8.5 * TAN15), (2, -6 - 8.5 * TAN15)], -41, 82, PLA_BLUE)
    m.cut(block, groove)
    return m


def dovetail_slider():
    m = WorldModel('dovetail_slider', 'X')
    m.round(m.wbox((0, 0, 0), (24, 24, 30), PLA_ORANGE, 'body'), 0.8)
    m.wprism_x([(-11.9, -5.9), (-11.9, 5.9), (-20, 5.9 + 8 * TAN15), (-20, -5.9 - 8 * TAN15)], -12, 24, PLA_ORANGE, 'tail')
    return m


def insert_boss():
    m = WorldModel('insert_boss', 'Y')
    boss = m.wcyl((0, -12, 0), (0, 1, 0), 6.0, 24, PLA_BLUE, 'boss')
    m.cut(boss, m.wcyl((0, 6.2, 0), (0, 1, 0), 2.8, 6, PLA_BLUE))
    insert = m.wcyl((0, 6.3, 0), (0, 1, 0), 2.8, 5.7, BRASS, 'heat-set insert')
    m.cut(insert, m.wcyl((0, 6.0, 0), (0, 1, 0), 1.5, 7, BRASS))
    for i in range(3):
        m.wcyl((0, 7.0 + 1.8 * i, 0), (0, 1, 0), 2.95, 0.6, BRASS, 'knurl')
    return m


def tapped_boss():
    m = WorldModel('tapped_boss', 'Y')
    boss = m.wcyl((0, -12, 0), (0, 1, 0), 6.0, 24, PLA_BLUE, 'boss')
    m.cut(boss, m.wcyl((0, 5.8, 0), (0, 1, 0), 1.25, 7, PLA_BLUE))
    return m


def m3_screw():
    m = WorldModel('m3_screw', 'Y')
    m.wcyl((0, -12, 0), (0, 1, 0), 1.5, 18, STEEL, 'shank')
    head = m.wcyl((0, 6, 0), (0, 1, 0), 2.75, 3, STEEL, 'head')
    m.round(head, 0.4)
    return m


MODELS = {
    'to220': (to220, 'TO-220 power MOSFET (upright)'),
    'radial_capacitor': (radial_capacitor, 'Radial electrolytic capacitor, 8 × 11 mm'),
    'axial_resistor': (axial_resistor, 'Axial through-hole resistor'),
    'do41_diode': (do41_diode, 'DO-41 axial diode'),
    'power_inductor': (power_inductor, 'Shielded SMD power inductor, 12 × 12 × 8 mm'),
    'soic8': (soic8, 'SOIC-8 IC (controllers, PWM and gate drivers)'),
    'sot23': (sot23, 'SOT-23 small-signal IC (sense amplifiers)'),
    'battery_3s': (battery_3s, '3S 18650 battery pack'),
    'servo': (servo, 'Standard-size servo body with horn'),
    'dc_motor': (dc_motor, 'Can-type brushed DC motor'),
    'flywheel': (flywheel, 'Aluminium flywheel / inertia load'),
    'heatsink': (heatsink, 'Finned aluminium heatsink, 30 × 20 × 15 mm'),
    'worm': (worm, 'Single-start steel worm, module 0.5, pitch Ø8'),
    'worm_wheel_drum': (worm_wheel_drum, '30-tooth bronze worm wheel on a Ø20 winch drum'),
    'worm_gear_frame': (worm_gear_frame, 'Worm-gear base plate and bearing blocks'),
    'lead_screw_nut': (lead_screw_nut, 'Tr8 lead screw with flanged bronze nut'),
    'nema17': (nema17, 'NEMA 17 stepper motor'),
    'outrunner': (outrunner, '2212-class brushless outrunner on a cross mount'),
    'propeller': (propeller, '10-inch two-blade propeller'),
    'wheel': (wheel, '80 mm robot wheel with rubber tyre'),
    'gt2_pulley': (gt2_pulley, 'GT2 20-tooth pulley with belt'),
    'rack_pinion': (rack_pinion, 'Module-1 pinion on a rack'),
    'solenoid': (solenoid, 'Open-frame pull solenoid'),
    'driver_board': (driver_board, 'H-bridge motor driver board'),
    'pin_plate': (pin_plate, 'Printed plate with one Ø3.2 mm pin hole (slides along X)'),
    'pin_plate_two': (pin_plate_two, 'Printed plate with two pin holes 60 mm apart (slides along X)'),
    'pin_base': (pin_base, 'Printed base with one pin hole'),
    'pin_base_two': (pin_base_two, 'Printed base with two pin holes 60 mm apart'),
    'dowel_pin': (dowel_pin, 'Ø3 × 14 mm steel dowel pin'),
    'dovetail_socket': (dovetail_socket, 'Printed block with a 15° dovetail groove'),
    'dovetail_tail': (dovetail_tail, 'Printed part with a 15° dovetail tail (pulled upward)'),
    'dovetail_groove_long': (dovetail_groove_long, 'Printed rail with an 80 mm dovetail groove along X'),
    'dovetail_slider': (dovetail_slider, 'Printed part with a dovetail tail sliding along X'),
    'insert_boss': (insert_boss, 'Printed Ø12 mm boss with an M3 brass heat-set insert'),
    'tapped_boss': (tapped_boss, 'Printed Ø12 mm boss with a Ø2.5 mm pilot hole for a self-tapped M3 screw'),
    'm3_screw': (m3_screw, 'M3 socket-head screw, 18 mm shank'),
}

# Default model per registry element; instances can override in their appearance.
DEFAULTS = {
    'electrical.mosfet': 'to220',
    'electrical.capacitor': 'radial_capacitor',
    'electrical.resistor': 'axial_resistor',
    'bridge.thermistor': 'axial_resistor',
    'electrical.diode': 'do41_diode',
    'electrical.inductor': 'power_inductor',
    'control.pwm': 'soic8',
    'control.h_bridge_pwm': 'soic8',
    'control.pi': 'soic8',
    'electrical.voltage_sense': 'sot23',
    'robot.battery': 'battery_3s',
    'robot.motor_unit': 'servo',
    'bridge.brushed_motor': 'dc_motor',
    'bridge.motor': 'dc_motor',
    'rotational.inertia': 'flywheel',
    'rotational.worm_gear': 'worm_gear_frame',
    'bridge.lead_screw': 'lead_screw_nut',
    'part.stepper_motor': 'nema17',
    'part.bldc_motor': 'outrunner',
    'part.propeller': 'propeller',
    'part.drive_wheel': 'wheel',
    'part.timing_belt': 'gt2_pulley',
    'part.rack_pinion': 'rack_pinion',
    'part.solenoid': 'solenoid',
    'robot.h_bridge': 'driver_board',
    'robot.switchable_h_bridge': 'driver_board',
    'actuator.pwm_driver': 'driver_board',
    'robot.battery': 'battery_3s',
    'part.coreless_motor': 'dc_motor',
    'part.brushed_motor_eq': 'dc_motor',
}


def main(out='../library/models'):
    os.makedirs(out, exist_ok=True)
    catalog = {'schema': 'sim.models/1', 'units': 'm', 'up': '+Y', 'source': 'cad/scripts/component_models.py', 'models': {}, 'defaults': DEFAULTS}
    for key, (build, description) in MODELS.items():
        model = build()
        path = model.export(out)
        catalog['models'][key] = {'file': os.path.basename(path), 'description': description}
        print(f'{key}: {len(model.ids)} bodies -> {path}')
    with open(os.path.join(out, 'catalog.json'), 'w') as f:
        json.dump(catalog, f, indent=2)
        f.write('\n')


if __name__ == '__main__':
    main(*sys.argv[1:])
