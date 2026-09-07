"""Analytic consistency of CAD catalog derivations, not hardware calibration."""
import math
import pytest
from robocad.robotics import MOTOR_LIBRARY, MOTOR_DATASHEETS, motor_physics


def test_declared_stall_current_derives_consistent_missing_resistance():
    spec = MOTOR_LIBRARY["hx30hm"]
    result = motor_physics(spec)
    electrical, gearbox = result["electrical"], result["gearbox"]
    current = spec.voltage / electrical["resistance"]
    torque = current * electrical["torque_constant"] * gearbox["ratio"] * gearbox["efficiency"]
    assert current == pytest.approx(MOTOR_DATASHEETS[spec.id]["stall_current"])
    assert torque == pytest.approx(spec.stall_torque)
    assert electrical["resistance"] == pytest.approx(3.7)
    assert electrical["inductance"] == pytest.approx(3.7 * 0.0004)
    # Catalog fix does not retune the position controller or invent measured dynamics.
    assert result["firmware"]["kp"] == pytest.approx(spec.voltage / math.radians(5))


def test_explicit_resistance_is_preserved_and_external_reduction_keeps_motor_current():
    for name, data in MOTOR_DATASHEETS.items():
        if "resistance" in data:
            assert motor_physics(MOTOR_LIBRARY[name])["electrical"]["resistance"] == data["resistance"]
    spec = MOTOR_LIBRARY["hx30hm"]
    base, reduced = motor_physics(spec), motor_physics(spec, 5)
    assert reduced["electrical"] == base["electrical"]
    assert reduced["gearbox"]["max_output_torque"] == pytest.approx(spec.stall_torque * 5)
