"""Generic motorized pendulum and two-joint chain for the Rhai workspace."""
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT/'cad'))
from robocad.commands import Ops
from robocad.document import Document


def build(two_joint=False):
    doc = Document(); ops = Ops(doc)
    # Create moving parts before the base to catch accidental tree/index order
    # assumptions in simulation replay and merged-link result mappings.
    first = ops.box((-8, -6, 80), (16, 12, 120), name='first')
    ops.set_material([first], 'petg')
    second = None
    if two_joint:
        second = ops.box((-6, -5, 0), (12, 10, 80), name='second')
        ops.set_material([second], 'petg')
    base = ops.box((-25, -20, 200), (50, 40, 40), name='base')
    ops.set_material([base], 'al'); ops.set_ground(base)
    motor1 = ops.add_motor('mg996r' if two_joint else 'mg90s', (0, -20, 200), (0, 1, 0),
                          mount_on=base, cut_mount=False, name='drive1')
    joint1 = ops.add_joint('revolute', base, first, (0, 0, 200), (0, 1, 0), lower=-1, upper=1, name='hinge1')
    ops.attach_motor(joint1, motor1)
    joints = [joint1]
    if second:
        motor2 = ops.add_motor('mg90s', (0, -6, 80), (0, 1, 0), mount_on=first, cut_mount=False, name='drive2')
        joint2 = ops.add_joint('revolute', first, second, (0, 0, 80), (0, 1, 0), lower=-1, upper=1, name='hinge2')
        ops.attach_motor(joint2, motor2); joints.append(joint2)
    attachment = ops.box((6, -3, 10 if second else 90), (8, 6, 10), name='fixed marker')
    ops.set_material([attachment], 'petg'); ops.connect_fixed(second or first, attachment)
    for joint in joints:
        ops.set_joint_physics(joint, source='declared', backlash=0.,
            friction={'coulomb': 0., 'viscous': 0., 'stribeck': 0., 'stribeck_speed': .1, 'static_ratio': 1.})
    ops.set_control(period_s=.02, latency_s=0., targets={doc.nodes[j].name: 0. for j in joints})
    return doc, {'base': base, 'first': first, 'second': second, 'attachment': attachment, 'joints': joints}


def build_thermal():
    """The same actuator, with an explicitly derived case and visible heat paths.

    The library's motor envelope is treated as a uniform ABS solid for this
    example. This is a declared lumped approximation, not a calibrated servo.
    Bindings replace existing parameters; they do not add a second motor/case.
    """
    from robocad.component_graph import empty_graph
    doc, ids = build(False)
    motor = next(n.id for n in doc.nodes.values() if n.name == 'drive1')
    ids['motor'] = motor
    graph = empty_graph()
    def bound(identity, name, kind, body, role, parameters=None):
        graph['components'][identity] = {'id': identity, 'name': name, 'type': kind,
            'body_id': body, 'binding': f'cad/{body}/{role}', 'parameters': parameters or {}}
    bound('motor', 'Motor winding and shaft', 'robot.motor_unit', motor, 'unit')
    bound('winding', 'Winding storage', 'thermal.capacitance', motor, 'winding')
    bound('housing', 'Derived housing', 'thermal.capacitance', motor, 'case')
    graph['components']['housing']['derivation'] = {'kind': 'body_thermal_capacity', 'specific_heat': 1000.}
    bound('transfer', 'Winding to housing', 'thermal.conductance', motor, 'g_wc', {'conductance': 1.})
    bound('cooling', 'Housing to air', 'thermal.conductance', motor, 'g_ca', {'conductance': .2})
    bound('mount_path', 'Housing to mount', 'thermal.conductance', motor, 'g_cm', {'conductance': .1})
    bound('mount', 'Mount storage', 'thermal.capacitance', ids['base'], 'mount')
    graph['components']['ambient'] = {'id': 'ambient', 'name': 'Ambient', 'type': 'thermal.ambient',
        'binding': 'ambient', 'parameters': {}}
    graph['components']['sensor'] = {'id': 'sensor', 'name': 'Housing sensor', 'type': 'robot.thermal_probe',
        'body_id': motor, 'parameters': {}}
    for name, ports in {
        'winding': [('motor', 'winding'), ('winding', 'node'), ('transfer', 'a')],
        'housing': [('housing', 'node'), ('transfer', 'b'), ('cooling', 'a'), ('mount_path', 'a'), ('sensor', 'node')],
        'air': [('cooling', 'b'), ('ambient', 'node')],
        'mount': [('mount_path', 'b'), ('mount', 'node')],
        'temperature': [('sensor', 'temperature')],
    }.items():
        graph['connections'][name] = {'id': name, 'ports': [{'component_id': c, 'port': p} for c, p in ports]}
    Ops(doc).set_component_graph(graph)
    return doc, ids


if __name__ == '__main__':
    output = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT/'runs'/'experimentation-models'
    output.mkdir(parents=True, exist_ok=True)
    for name, two in [('pendulum', False), ('two-joint', True)]:
        doc, _ = build(two); doc.save(str(output/f'{name}.rcad'))
        print(output/f'{name}.rcad')
    doc, _ = build_thermal(); doc.save(str(output/'electromechanical-thermal.rcad'))
    print(output/'electromechanical-thermal.rcad')
