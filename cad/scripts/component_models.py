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


def heatsink():
    m = Model('heatsink')
    m.box((-15.0, -10.0, 0.0), (30.0, 20.0, 3.0), ALUMINIUM, 'base')
    for i in range(7):
        m.box((-14.5 + i * 4.6, -10.0, 3.0), (1.2, 20.0, 12.0), ALUMINIUM, 'fin')
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
