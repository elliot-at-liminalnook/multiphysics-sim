"""Build turntable.system.json through the shared command layer.

    python3 examples/camera-turntable/build_system.py | target/release/sim-system apply examples/camera-turntable/turntable.system.json -
    # then the CAD-derived values (recorded as derived, with the CAD file's hash):
    for x in belt:belt-drive disc:disc-inertia pulley:pulley-inertia bearings:bearing-friction; do
      target/release/sim-system cad-params examples/camera-turntable/turntable.system.json / ${x%%:*} examples/camera-turntable/cad/${x##*:}.physics.json
    done

Values come from their single sources: the HX-30HM drive train, driver and
FPGA controller from the accepted actuator registry (family `hx30hm-knee-
measured`, referenced by content hash), geometry-derived values from the CAD
derivations in cad/. Only design choices live here: supply voltage and the
stop-and-shoot schedule.

Assumption (labelled in the run settings): the controller runs the FPGA's
integer PD with `multi_turn = 1`. The deployed RTL is single-turn, so this is
the loop the FPGA would run once it counts turns.
"""
import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, '..', '..'))
REGISTRY = os.path.join(REPO, 'examples/actuators/hx30hm/accepted/registry.json')
FAMILY = 'hx30hm-knee-measured'

registry = json.load(open(REGISTRY))
entry = registry['families'][FAMILY]
family_path = os.path.join(os.path.dirname(REGISTRY), entry['path'])
family = json.load(open(family_path))
belt = json.load(open(os.path.join(HERE, 'cad/belt-drive.physics.json')))
ratio = belt['parameters']['radius_driven']['value'] / belt['parameters']['radius_driver']['value']
C = belt['pulleys'][1]['center_mm'] if belt['pulleys'][1]['role'] == 'driver' else belt['pulleys'][0]['center_mm']
# Display models are exported in their instance's frame; place each instance at that origin.
ORIGIN = json.load(open(os.path.join(HERE, 'models/catalog.json')))['origins_mm']


def from_family(value, block):
    return {'value': value, 'provenance': {'kind': 'derived', 'rule': f'accepted actuator family {FAMILY}: {block}',
                                           'inputs_hash': entry['content_hash']}}


def est(value, why, unit=None):
    b = {'value': value, 'provenance': {'kind': 'estimated', 'explanation': why}}
    if unit: b['unit'] = unit
    return b


def place(x_mm=0.0, y_mm=0.0, z_mm=0.0):
    """CAD (mm, Z up) → viewer (m, Y up)."""
    return {'position': [x_mm / 1000, z_mm / 1000, -y_mm / 1000], 'rotation_xyzw': [0, 0, 0, 1]}


def element(name, component_type, label, parameters=None, placement=None, model=None, shape=None, color=(0.6, 0.6, 0.62)):
    spec = {'label': label, 'kind': {'kind': 'element', 'component_type': component_type},
            'parameters': parameters or {}, 'placement': placement or place()}
    if model or shape:
        spec['appearance'] = {'shape': shape or {'kind': 'box', 'size': [0.01, 0.01, 0.01]}, 'color_srgb': list(color), **({'model': model} if model else {})}
    return {'command': 'add_instance', 'name': name, 'instance': spec}


ctl, drv = family['controller'], family['driver']
commands = [
    {'command': 'set_title', 'title': 'Camera turntable (belt-driven, stop and shoot)'},
    {'command': 'set_run_settings', 'run': {'integrator': 'backward_euler', 'interval': 0.0005, 'rationale':
        'The FPGA controller switches its held PWM every 10 ms (scheduled events) and gearbox friction is a stiff '
        'regularised state; backward Euler is L-stable for both. 0.5 ms resolves the 2 ms command latency. '
        'ASSUMPTION: the controller runs the FPGA integer PD with multi_turn = 1; the deployed RTL is single-turn.'}},
    element('supply', 'electrical.voltage_source', 'Servo supply (12 V bench supply)',
            {'voltage': est(12.0, 'design choice: the bench supply for the servo; set to what you use', 'V')}, place(-60, -230, 5)),
    element('gnd', 'electrical.ground', 'Ground', {}, place(-100, -230, 5)),
    element('driver', 'robot.h_bridge', 'Servo motor driver (averaged PWM bridge)', {
        'on_resistance': from_family(drv['on_resistance']['value'], 'driver.on_resistance'),
        'current_limit': from_family(drv['current_limit']['value'], 'driver.current_limit')}, place(-20, -230, 5)),
    # The averaged bridge fixes only v_p − v_n; 10 MΩ bleeds define the common mode (as library h_bridge_averaged does).
    element('bleed_p', 'electrical.resistor', 'Common-mode bleed (+)', {'resistance': est(1e7, 'numerical: defines the averaged bridge common mode; draws < 2 µA', 'Ω')}, place(-20, -260, 5)),
    element('bleed_n', 'electrical.resistor', 'Common-mode bleed (−)', {'resistance': est(1e7, 'numerical: defines the averaged bridge common mode; draws < 2 µA', 'Ω')}, place(0, -260, 5)),
    {'command': 'add_instance', 'name': 'servo', 'instance': {'label': 'HX-30HM gearmotor (measured knee family)',
        'kind': {'kind': 'subsystem', 'definition': 'hx30hm_knee_gearmotor'}, 'placement': place(C[0], C[1] + 10, 30)}},
    element('controller', 'control.sampled_fixed_pd', 'FPGA position loop (integer PD, multi-turn)', {
        'kp_q8': from_family(ctl['gains']['kp_q8'], 'controller.gains.kp_q8'),
        'kd_q8': from_family(ctl['gains']['kd_q8'], 'controller.gains.kd_q8'),
        'kv_q8': from_family(ctl['gains']['kv_q8'], 'controller.gains.kv_q8'),
        'limit': from_family(ctl['gains']['limit'], 'controller.gains.limit'),
        'period': from_family(ctl['period']['value'], 'controller.period'),
        'latency': from_family(ctl['latency']['value'], 'controller.latency'),
        'encoder_quantum': from_family(ctl['encoder_quantum']['value'], 'controller.encoder_quantum'),
        'offset': est(0.0, 'controller clock aligned with the simulation start'),
        'encoder_zero': est(0.0, 'multi-turn counting from the start pose'),
        'encoder_direction': est(1.0, 'positive duty turns the servo output positive'),
        'initial_target': est(0.0, 'start pose'),
        'multi_turn': est(1.0, 'ASSUMPTION: continuous rotation needs turn counting; the deployed FPGA RTL is single-turn'),
    }, place(20, -230, 5)),
    element('servo_angle', 'rotational.angle_sensor', 'Servo output encoder (what the servo reports)', {}, place(C[0], C[1] - 20, 44)),
    element('schedule', 'part.index_move', 'Stop-and-shoot schedule (10° per shot)', {
        'step': est(0.17453292519943295, 'design choice: 36 photos per turn (102° field of view → ~92° overlap)', 'rad'),
        'period': est(2.0, 'design choice: 1.2 s move + 0.8 s still for the exposure', 's'),
        'move': est(1.2, 'design choice', 's'),
        'gain': {'value': ratio, 'provenance': {'kind': 'derived', 'rule': 'belt ratio r_driven / r_driver from cad/belt-drive.physics.json',
                                                'inputs_hash': belt['cad']['sha256']}},
    }, place(60, -230, 5)),
    element('pulley', 'rotational.inertia', 'GT2 30T drive pulley + horn', {'inertia': est(3e-7, 'replaced by the CAD derivation', 'kg·m²')},
            place(*ORIGIN['turntable_pulley']), model='turntable_pulley', shape={'kind': 'cylinder', 'radius': 0.0115, 'length': 0.01}, color=(0.93, 0.55, 0.16)),
    element('belt', 'part.belt_drive', 'GT2 610 mm closed belt', {}, place(*ORIGIN['turntable_belt']), model='turntable_belt',
            shape={'kind': 'box', 'size': [0.29, 0.007, 0.19]}, color=(0.16, 0.17, 0.19)),
    element('disc', 'rotational.inertia', 'Turntable disc, camera and ESP32 (turning)', {'inertia': est(0.003, 'replaced by the CAD derivation', 'kg·m²')},
            place(*ORIGIN['turntable_disc']), model='turntable_disc', shape={'kind': 'cylinder', 'radius': 0.15, 'length': 0.004}, color=(0.93, 0.55, 0.16)),
    element('bearings', 'rotational.coulomb_friction', 'Bearing seals + slip-ring drag', {'torque': est(0.026, 'replaced by the CAD derivation', 'N·m')},
            place(0, 0, 20)),
    element('camera_angle', 'rotational.angle_sensor', 'True camera (disc) angle', {}, place(0, 140, 84)),
    element('frame', 'rotational.ground', 'Base, post and servo mount (fixed)', {}, place(*ORIGIN['turntable_base']), model='turntable_base',
            shape={'kind': 'cylinder', 'radius': 0.15, 'length': 0.005}, color=(0.2, 0.46, 0.72)),
    element('bearing_drag', 'rotational.damper', 'Bearing grease drag (disc to base)',
            {'damping': est(2e-4, 'estimate: viscous drag of two greased, sealed 6808 bearings; measure a coast-down', 'N·m·s/rad')}, place(0, 0, 30)),
    {'command': 'connect', 'label': 'V+', 'terminals': [{'instance': 'supply', 'port': 'p'}, {'instance': 'driver', 'port': 'supply_p'}]},
    {'command': 'connect', 'label': '0 V', 'terminals': [{'instance': 'supply', 'port': 'n'}, {'instance': 'driver', 'port': 'supply_n'}, {'instance': 'gnd', 'port': 'pin'}]},
    {'command': 'connect', 'label': 'motor +', 'terminals': [{'instance': 'driver', 'port': 'p'}, {'instance': 'servo', 'port': 'p'}, {'instance': 'bleed_p', 'port': 'p'}]},
    {'command': 'connect', 'label': 'motor −', 'terminals': [{'instance': 'driver', 'port': 'n'}, {'instance': 'servo', 'port': 'n'}, {'instance': 'bleed_n', 'port': 'p'}]},
    {'command': 'connect', 'label': '0 V', 'terminals': [{'instance': 'gnd', 'port': 'pin'}, {'instance': 'bleed_p', 'port': 'n'}, {'instance': 'bleed_n', 'port': 'n'}]},
    {'command': 'connect', 'label': 'servo output', 'terminals': [{'instance': 'servo', 'port': 'output'}, {'instance': 'pulley', 'port': 'shaft'},
                                                                  {'instance': 'belt', 'port': 'driver'}, {'instance': 'servo_angle', 'port': 'shaft'}]},
    {'command': 'connect', 'label': 'turntable', 'terminals': [{'instance': 'belt', 'port': 'driven'}, {'instance': 'disc', 'port': 'shaft'},
                                                               {'instance': 'bearings', 'port': 'shaft'}, {'instance': 'camera_angle', 'port': 'shaft'},
                                                               {'instance': 'bearing_drag', 'port': 'a'}]},
    {'command': 'connect', 'label': 'base', 'terminals': [{'instance': 'frame', 'port': 'flange'}, {'instance': 'bearing_drag', 'port': 'b'}]},
    {'command': 'connect', 'label': 'target', 'terminals': [{'instance': 'schedule', 'port': 'angle'}, {'instance': 'controller', 'port': 'target'}]},
    {'command': 'connect', 'label': 'measured', 'terminals': [{'instance': 'servo_angle', 'port': 'angle'}, {'instance': 'controller', 'port': 'position'}]},
    {'command': 'connect', 'label': 'duty', 'terminals': [{'instance': 'controller', 'port': 'duty'}, {'instance': 'driver', 'port': 'command'}]},
]
print(json.dumps(commands, indent=1))
