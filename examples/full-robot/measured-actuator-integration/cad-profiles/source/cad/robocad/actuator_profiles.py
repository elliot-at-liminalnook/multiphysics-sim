"""CAD authoring of profiles; numerical validation belongs to the shared Rust registry."""
from copy import deepcopy
import json
import os
from pathlib import Path
import subprocess
from .kernel import KernelError


def validate_profiles(doc, profiles):
    from .physical import _joint_records
    from .robotics import MOTOR_LIBRARY
    joints = _joint_records(doc)
    joint_of = {}
    for joint in joints:
        if joint.get('motor'):
            mid = joint['motor']['id']
            if mid in joint_of:
                raise KernelError('An actuator profile requires an unambiguous motor-to-joint binding')
            joint_of[mid] = joint
    def motor_record(node):
        joint = joint_of.get(node.id)
        ratio = (joint['motor']['gear_ratio'] / MOTOR_LIBRARY[node.robot['spec']].gear_ratio
                 if joint else 1.0)
        return {'id': node.id, 'name': node.name,
                'joint': joint['name'] if joint else None, 'gear_ratio': ratio}
    model = {'actuator_profiles': profiles,
             'motors': [motor_record(n)
                        for n in doc.bodies() if (n.robot or {}).get('kind') == 'motor'],
             'joints': [{'name': j['name'], 'child': j['child']} for j in joints],
             'identification': doc.robot_settings.get('identification', {})}
    executable = os.environ.get('ROBOCAD_ACTUATOR_PROFILE_TOOL')
    if not executable:
        root = Path(__file__).resolve().parents[2]
        executable = next((str(root/'target'/profile/'sim-actuator-profiles')
                           for profile in ('debug', 'release')
                           if (root/'target'/profile/'sim-actuator-profiles').is_file()), None)
    if not executable:
        raise KernelError('Build sim-actuator-profiles to validate actuator profiles with the shared Rust registry')
    try:
        result = subprocess.run([executable], input=json.dumps(model, allow_nan=False),
                                text=True, capture_output=True, timeout=15, check=False)
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        raise KernelError(f'Actuator profile validation failed: {error}') from error
    if result.returncode:
        raise KernelError(result.stderr.strip() or 'Native actuator profile validation failed')
    try:
        receipt = json.loads(result.stdout)
    except ValueError as error:
        raise KernelError('Invalid native actuator profile receipt') from error
    if receipt.get('version') != 1 or set(receipt.get('resolved', {})) != set(profiles.get('bindings', {})):
        raise KernelError('Native actuator profile binding receipt mismatch')
    return deepcopy(profiles)


class ChangeProfiles:
    label = 'Edit actuator profiles'

    def __init__(self, doc, profiles):
        self.before = deepcopy(doc.robot_settings.get('actuator_profiles'))
        self.after = validate_profiles(doc, profiles) if profiles is not None else None

    def do(self, doc):
        self._set(doc, self.after)

    def undo(self, doc):
        self._set(doc, self.before)

    def redo(self, doc):
        self.do(doc)

    @staticmethod
    def _set(doc, value):
        if value is None:
            doc.robot_settings.pop('actuator_profiles', None)
        else:
            doc.robot_settings['actuator_profiles'] = deepcopy(value)
        doc.touch()
