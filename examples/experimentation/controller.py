"""Python parity reference for controller.rhai, using the same sampled seam."""
import json
import os
from simloop import Loop
from reference import step

parameters = json.loads(os.environ['SIM_PARAMETERS'])
with Loop.stdio() as loop:
    for frame in loop:
        commands = {channel.name: step(frame.t, parameters['target2' if channel.name.startswith('hinge2.') else 'target1'])
                    for channel in loop.contract.actuators}
        loop.send(**commands)
