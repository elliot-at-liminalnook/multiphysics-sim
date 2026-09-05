"""Robotic parts: motors and actuators from a library (with the geometry
they occupy and the numbers the simulator needs), joints between bodies
(revolute, continuous, prismatic, fixed, ball) with axes, limits and the
motor that drives them, fixed connections, joint inference from coaxial
features, and validation of the whole mechanism as a tree.

A joint is a document node of kind `joint` carrying a `Joint`; a motor is
a body node whose `robot` metadata names its library entry and shaft
axis. Both are persisted in the manifest, drawn by the viewport, editable
in the Robot panel and over the REST API, and exported by `simbridge`.
"""

from __future__ import annotations

import math
from dataclasses import asdict, dataclass, field
from typing import Optional, Sequence

from .kernel import Body, BooleanOp, GeometryKernel, KernelError, Plane, SurfaceKind, Vec3
from .kernel.base import v_add, v_cross, v_dist, v_dot, v_norm, v_scale, v_sub, v_unit

JOINT_TYPES = ("revolute", "continuous", "prismatic", "fixed", "ball", "loop_revolute", "loop_spherical")
TREE_JOINT_TYPES = ("revolute", "continuous", "prismatic", "fixed", "ball")


@dataclass
class MotorSpec:
    """An actuator from the library. Sizes in mm, mass in g, torque in N·m
    at the *output* shaft, speed in rad/s at the output, gear ratio already
    applied to torque and speed."""

    id: str
    name: str
    kind: str  # stepper | servo | dc_gearmotor | bldc | linear
    shape: str  # box | cylinder
    size: tuple[float, float, float]  # box: w, d, h (h along the shaft); cylinder: diameter, -, length
    shaft_diameter: float
    shaft_length: float
    mass_g: float
    stall_torque: float
    no_load_speed: float
    gear_ratio: float = 1.0
    voltage: float = 5.0
    rotor_inertia: float = 0.0  # kg·m² at the rotor
    mount_holes: list[tuple[float, float, float]] = field(default_factory=list)  # (x, y, diameter) on the shaft face
    flange: Optional[tuple[float, float]] = None  # (diameter, height) of a pilot boss around the shaft
    stroke: float = 0.0  # linear actuators
    color: tuple[float, float, float] = (0.25, 0.27, 0.31)
    notes: str = ""

    def to_json(self) -> dict:
        return asdict(self)


def _nema(id_: str, name: str, side: float, length: float, shaft: float, mass: float, torque: float, holes: float, hole_d: float, flange: tuple[float, float]) -> MotorSpec:
    h = holes / 2
    return MotorSpec(id_, name, "stepper", "box", (side, side, length), shaft, 22.0, mass, torque, 2 * math.pi * 5.0, 1.0, 12.0, 5.4e-6 if side > 40 else 2.0e-6, [(h, h, hole_d), (-h, h, hole_d), (h, -h, hole_d), (-h, -h, hole_d)], flange, 0.0, (0.2, 0.21, 0.24), "1.8° bipolar stepper, ~5 rev/s usable")


MOTOR_LIBRARY: dict[str, MotorSpec] = {m.id: m for m in [
    MotorSpec("hx30hm", "Hiwonder HX-30HM serial bus servo", "servo", "box", (45.2, 24.7, 35.0), 6.0, 4.0, 52.0, 30.0 * 0.0980665, math.radians(60) / 0.19, voltage=11.1, notes="Manufacturer: https://www.hiwonder.com/products/hx-30hm . 30 kgf cm at 11.1 V, 52 g, 0.19 s/60 deg. Shaft dimensions are envelope estimates; retain imported CAD. Internal dynamics are provisional equivalent output-shaft parameters; bus IDs and calibration are not assigned."),
    _nema("nema14", "NEMA 14 stepper", 35.2, 34.0, 5.0, 200.0, 0.14, 26.0, 3.0, (22.0, 2.0)),
    _nema("nema17", "NEMA 17 stepper", 42.3, 40.0, 5.0, 280.0, 0.40, 31.0, 3.0, (22.0, 2.0)),
    _nema("nema17_pancake", "NEMA 17 pancake stepper", 42.3, 24.0, 5.0, 170.0, 0.16, 31.0, 3.0, (22.0, 2.0)),
    _nema("nema23", "NEMA 23 stepper", 56.4, 56.0, 6.35, 700.0, 1.26, 47.1, 4.5, (38.1, 1.6)),
    MotorSpec("sg90", "SG90 micro servo", "servo", "box", (22.8, 12.2, 22.5), 4.8, 3.5, 9.0, 0.18, math.radians(60) / 0.1, 1.0, 5.0, 0.0, [(0.0, 13.9, 2.0), (0.0, -13.9, 2.0)], (5.9, 2.0), 0.0, (0.3, 0.55, 0.85), "180° hobby servo; spline output; tabs at ±13.9 mm on the long axis"),
    MotorSpec("mg90s", "MG90S metal-gear micro servo", "servo", "box", (22.8, 12.2, 22.5), 4.8, 3.5, 13.4, 0.22, math.radians(60) / 0.08, 1.0, 5.0, 0.0, [(0.0, 13.9, 2.0), (0.0, -13.9, 2.0)], (5.9, 2.0), 0.0, (0.3, 0.55, 0.85), "180° hobby servo, metal gears"),
    MotorSpec("mg996r", "MG996R servo", "servo", "box", (40.7, 19.7, 42.9), 5.9, 4.0, 55.0, 1.0, math.radians(60) / 0.17, 1.0, 6.0, 0.0, [(0.0, 24.5, 4.2), (0.0, -24.5, 4.2), (10.0, 24.5, 4.2), (10.0, -24.5, 4.2)], (7.0, 2.5), 0.0, (0.2, 0.2, 0.22), "standard-size 180° servo, 25T spline"),
    MotorSpec("ds3218", "DS3218 20 kg servo", "servo", "box", (40.0, 20.0, 40.5), 5.9, 4.0, 60.0, 2.0, math.radians(60) / 0.16, 1.0, 6.8, 0.0, [(0.0, 24.5, 4.2), (0.0, -24.5, 4.2), (10.0, 24.5, 4.2), (10.0, -24.5, 4.2)], (7.0, 2.5), 0.0, (0.15, 0.15, 0.17), "standard-size 270° high-torque servo"),
    MotorSpec("n20_100", "N20 gearmotor 100:1", "dc_gearmotor", "box", (12.0, 10.0, 24.0), 3.0, 9.0, 10.0, 0.20, 2 * math.pi * 140 / 60, 100.0, 6.0, 1.0e-8, [(4.5, 0.0, 1.6), (-4.5, 0.0, 1.6)], (4.0, 1.0), 0.0, (0.75, 0.72, 0.6), "micro metal gearmotor, D shaft; 140 rpm"),
    MotorSpec("n20_298", "N20 gearmotor 298:1", "dc_gearmotor", "box", (12.0, 10.0, 24.0), 3.0, 9.0, 10.0, 0.45, 2 * math.pi * 45 / 60, 298.0, 6.0, 1.0e-8, [(4.5, 0.0, 1.6), (-4.5, 0.0, 1.6)], (4.0, 1.0), 0.0, (0.75, 0.72, 0.6), "micro metal gearmotor; 45 rpm"),
    MotorSpec("ga25_150", "25GA-370 gearmotor 150:1", "dc_gearmotor", "cylinder", (25.0, 0.0, 60.0), 4.0, 12.0, 90.0, 0.9, 2 * math.pi * 100 / 60, 150.0, 12.0, 5.0e-8, [(8.5, 0.0, 2.5), (-8.5, 0.0, 2.5), (0.0, 8.5, 2.5), (0.0, -8.5, 2.5)], (7.0, 2.0), 0.0, (0.6, 0.62, 0.65), "25 mm gearmotor with encoder; 100 rpm"),
    MotorSpec("gb37_100", "37GB-520 gearmotor 100:1", "dc_gearmotor", "cylinder", (37.0, 0.0, 70.0), 6.0, 15.0, 200.0, 2.5, 2 * math.pi * 110 / 60, 100.0, 12.0, 1.2e-7, [(15.5, 0.0, 3.0), (-15.5, 0.0, 3.0), (0.0, 15.5, 3.0), (0.0, -15.5, 3.0), (11.0, 11.0, 3.0), (-11.0, -11.0, 3.0)], (12.0, 2.0), 0.0, (0.6, 0.62, 0.65), "37 mm gearmotor; 110 rpm"),
    MotorSpec("gm2804", "GM2804 gimbal BLDC", "bldc", "cylinder", (35.0, 0.0, 26.0), 4.0, 6.0, 40.0, 0.12, 2 * math.pi * 6.0, 1.0, 12.0, 4.0e-6, [(9.5, 9.5, 2.0), (-9.5, 9.5, 2.0), (9.5, -9.5, 2.0), (-9.5, -9.5, 2.0)], (8.0, 1.0), 0.0, (0.3, 0.3, 0.32), "direct-drive gimbal motor, FOC"),
    MotorSpec("d5065", "D5065 270KV BLDC", "bldc", "cylinder", (65.0, 0.0, 50.0), 8.0, 20.0, 420.0, 1.6, 2 * math.pi * 40.0, 1.0, 24.0, 6.4e-5, [(12.5, 12.5, 4.0), (-12.5, 12.5, 4.0), (12.5, -12.5, 4.0), (-12.5, -12.5, 4.0)], (20.0, 2.0), 0.0, (0.35, 0.36, 0.4), "outrunner for robot actuators (ODrive class)"),
    MotorSpec("cycloid_8108", "8108 BLDC + 9:1 planetary", "bldc", "cylinder", (88.0, 0.0, 60.0), 12.0, 12.0, 700.0, 12.0, 2 * math.pi * 3.0, 9.0, 24.0, 9.0e-5, [(35.0, 0.0, 4.0), (-35.0, 0.0, 4.0), (0.0, 35.0, 4.0), (0.0, -35.0, 4.0)], (30.0, 3.0), 0.0, (0.28, 0.3, 0.34), "quasi-direct-drive leg actuator"),
    MotorSpec("linear_l12", "L12 micro linear actuator 50 mm", "linear", "box", (15.0, 12.0, 85.0), 4.0, 50.0, 40.0, 40.0, 0.012, 100.0, 6.0, 0.0, [(0.0, 0.0, 4.0)], None, 50.0, (0.25, 0.28, 0.3), "force 40 N (stall_torque field = N), speed 12 mm/s (no_load_speed = m/s)"),
]}


# Datasheet values per library motor: (winding R Ω, L H, stall current A, no-load current A,
# internal gear ratio, gearbox efficiency, output backlash rad, firmware kind, loop Hz, deadband rad,
# sensor resolution rad, max winding °C, notes). None = derived from stall torque, no-load speed and voltage.
MOTOR_DATASHEETS: dict[str, dict] = {
    "hx30hm": {"stall_current": 3.0, "no_load_current": 0.1, "internal_ratio": 1.0, "efficiency": 1.0, "firmware": "servo", "resolution": 2 * math.pi / 4096, "driver": "servo_internal", "notes": "HX-30HM manufacturer mass, output stall torque/speed, current and encoder resolution. Output-equivalent actuator (internal gearing is already reflected). Electrical constants, servo gains/latency, rotor inertia, thermal parameters, friction and backlash are estimates pending identification. 0.3 degree specified accuracy is distinct from encoder resolution. https://www.hiwonder.com/products/hx-30hm"},
    "sg90": {"resistance": 7.4, "stall_current": 0.65, "no_load_current": 0.1, "internal_ratio": 262.0, "efficiency": 0.55, "backlash": math.radians(1.5), "firmware": "servo", "loop_hz": 250.0, "deadband": math.radians(0.9), "resolution": math.radians(180) / 1024, "max_c": 90.0, "driver": "servo_internal", "gear_stiffness": 8.0, "notes": "R and currents from stall/no-load datasheet; nylon gears"},
    "mg90s": {"resistance": 6.0, "stall_current": 0.8, "no_load_current": 0.12, "internal_ratio": 262.0, "efficiency": 0.6, "backlash": math.radians(1.0), "firmware": "servo", "loop_hz": 250.0, "deadband": math.radians(0.8), "resolution": math.radians(180) / 1024, "max_c": 100.0, "driver": "servo_internal", "gear_stiffness": 15.0, "notes": "metal gears; R from stall current"},
    "mg996r": {"resistance": 2.4, "stall_current": 2.5, "no_load_current": 0.17, "internal_ratio": 260.0, "efficiency": 0.6, "backlash": math.radians(1.0), "firmware": "servo", "loop_hz": 300.0, "deadband": math.radians(0.6), "resolution": math.radians(180) / 1024, "max_c": 100.0, "driver": "servo_internal", "gear_stiffness": 60.0, "notes": "stall 2.5 A at 6 V"},
    "ds3218": {"resistance": 2.2, "stall_current": 3.0, "no_load_current": 0.2, "internal_ratio": 300.0, "efficiency": 0.62, "backlash": math.radians(0.8), "firmware": "servo", "loop_hz": 300.0, "deadband": math.radians(0.5), "resolution": math.radians(270) / 4096, "max_c": 100.0, "driver": "servo_internal", "gear_stiffness": 120.0, "notes": "digital 270° servo, 12-bit position"},
    "n20_100": {"resistance": 8.0, "stall_current": 0.75, "no_load_current": 0.07, "internal_ratio": 100.0, "efficiency": 0.7, "backlash": math.radians(2.0), "firmware": "position", "loop_hz": 1000.0, "deadband": 0.0, "resolution": 2 * math.pi / (12 * 100), "max_c": 110.0, "driver": "h_bridge", "gear_stiffness": 5.0, "notes": "6 V micro metal gearmotor; 12 CPR magnetic encoder assumed"},
    "n20_298": {"resistance": 8.0, "stall_current": 0.75, "no_load_current": 0.07, "internal_ratio": 298.0, "efficiency": 0.65, "backlash": math.radians(2.5), "firmware": "position", "loop_hz": 1000.0, "deadband": 0.0, "resolution": 2 * math.pi / (12 * 298), "max_c": 110.0, "driver": "h_bridge", "gear_stiffness": 5.0, "notes": "6 V micro metal gearmotor"},
    "ga25_150": {"resistance": 4.0, "stall_current": 3.0, "no_load_current": 0.15, "internal_ratio": 150.0, "efficiency": 0.7, "backlash": math.radians(1.5), "firmware": "position", "loop_hz": 1000.0, "deadband": 0.0, "resolution": 2 * math.pi / (11 * 4 * 150), "max_c": 120.0, "driver": "h_bridge", "gear_stiffness": 60.0, "notes": "12 V, 11 PPR quadrature encoder"},
    "gb37_100": {"resistance": 2.4, "stall_current": 5.0, "no_load_current": 0.3, "internal_ratio": 100.0, "efficiency": 0.75, "backlash": math.radians(1.2), "firmware": "position", "loop_hz": 1000.0, "deadband": 0.0, "resolution": 2 * math.pi / (11 * 4 * 100), "max_c": 120.0, "driver": "h_bridge", "gear_stiffness": 200.0, "notes": "12 V, 11 PPR encoder"},
    "gm2804": {"resistance": 5.6, "stall_current": 2.1, "no_load_current": 0.05, "internal_ratio": 1.0, "efficiency": 0.95, "backlash": 0.0, "firmware": "torque", "loop_hz": 8000.0, "deadband": 0.0, "resolution": 2 * math.pi / 16384, "max_c": 120.0, "driver": "esc", "gear_stiffness": 1e6, "notes": "gimbal FOC, 14-bit magnetic encoder"},
    "d5065": {"resistance": 0.039, "stall_current": 40.0, "no_load_current": 0.5, "internal_ratio": 1.0, "efficiency": 0.95, "backlash": 0.0, "firmware": "torque", "loop_hz": 8000.0, "deadband": 0.0, "resolution": 2 * math.pi / 8192, "max_c": 150.0, "driver": "esc", "gear_stiffness": 1e6, "notes": "270 KV, 24 V; ODrive class driver"},
    "cycloid_8108": {"resistance": 0.13, "stall_current": 35.0, "no_load_current": 0.6, "internal_ratio": 9.0, "efficiency": 0.85, "backlash": math.radians(0.3), "firmware": "torque", "loop_hz": 8000.0, "deadband": 0.0, "resolution": 2 * math.pi / 16384, "max_c": 150.0, "driver": "esc", "gear_stiffness": 3000.0, "notes": "quasi-direct-drive; planetary backlash 0.3°"},
    "nema14": {"resistance": 5.6, "stall_current": 0.8, "no_load_current": 0.8, "internal_ratio": 1.0, "efficiency": 0.9, "backlash": 0.0, "firmware": "stepper", "loop_hz": 20000.0, "deadband": 0.0, "resolution": math.radians(1.8) / 16, "max_c": 80.0, "driver": "stepper", "gear_stiffness": 1e6, "notes": "0.8 A/phase; holding torque as stall; runs at full current"},
    "nema17": {"resistance": 1.5, "stall_current": 1.7, "no_load_current": 1.7, "internal_ratio": 1.0, "efficiency": 0.9, "backlash": 0.0, "firmware": "stepper", "loop_hz": 20000.0, "deadband": 0.0, "resolution": math.radians(1.8) / 16, "max_c": 80.0, "driver": "stepper", "gear_stiffness": 1e6, "notes": "1.7 A/phase, 12–24 V chopper"},
    "nema17_pancake": {"resistance": 3.0, "stall_current": 1.0, "no_load_current": 1.0, "internal_ratio": 1.0, "efficiency": 0.9, "backlash": 0.0, "firmware": "stepper", "loop_hz": 20000.0, "deadband": 0.0, "resolution": math.radians(1.8) / 16, "max_c": 80.0, "driver": "stepper", "gear_stiffness": 1e6, "notes": "1 A/phase pancake"},
    "nema23": {"resistance": 0.9, "stall_current": 2.8, "no_load_current": 2.8, "internal_ratio": 1.0, "efficiency": 0.9, "backlash": 0.0, "firmware": "stepper", "loop_hz": 20000.0, "deadband": 0.0, "resolution": math.radians(1.8) / 16, "max_c": 80.0, "driver": "stepper", "gear_stiffness": 1e6, "notes": "2.8 A/phase"},
    "linear_l12": {"resistance": 12.0, "stall_current": 0.5, "no_load_current": 0.06, "internal_ratio": 100.0, "efficiency": 0.4, "backlash": 0.3e-3, "firmware": "position", "loop_hz": 100.0, "deadband": 0.2e-3, "resolution": 0.05e-3, "max_c": 90.0, "driver": "servo_internal", "gear_stiffness": 2e4, "notes": "50 mm stroke, 100:1; backlash and resolution in metres"},
}


def motor_physics(spec: "MotorSpec", gear_extra: float = 1.0) -> dict:
    """Electrical, gearbox, thermal, firmware and driver blocks (SI) for a
    library motor with an extra external reduction `gear_extra`.
    Constants not on a datasheet are derived from stall torque, no-load
    speed and voltage: ke = V/ω_motor,no-load, kt = ke, R = kt·V/τ_motor,stall."""
    d = MOTOR_DATASHEETS.get(spec.id, {})
    ratio_int = d.get("internal_ratio", spec.gear_ratio if spec.gear_ratio > 1 else 1.0)
    eta = d.get("efficiency", 0.7 if ratio_int > 1 else 0.95)
    V = spec.voltage
    linear = spec.kind == "linear"
    # Output → motor side.
    if linear:
        # stall_torque is a force (N) and no_load_speed a velocity (m/s): a lead of 1 mm/rev is assumed.
        lead = 1.0e-3
        tau_out, w_out = spec.stall_torque * lead / (2 * math.pi), spec.no_load_speed * 2 * math.pi / lead
    else:
        tau_out, w_out = spec.stall_torque, spec.no_load_speed
    tau_m = tau_out / max(ratio_int * eta, 1e-9)
    w_m = w_out * ratio_int
    ke = V / max(w_m, 1e-9)
    kt = ke
    R = d.get("resistance") or (kt * V / max(tau_m, 1e-12))
    i_stall = d.get("stall_current") or V / R
    if d.get("resistance") and not d.get("stall_current"):
        i_stall = V / R
    # With a datasheet resistance, kt is rebalanced to the measured stall current.
    if d.get("stall_current"):
        kt = tau_m / max(i_stall, 1e-9)
    L = d.get("inductance") or R * (0.4e-3 if spec.kind in ("servo", "dc_gearmotor", "linear") else 1.2e-3)
    rotor_inertia = spec.rotor_inertia if spec.rotor_inertia > 0 else 1.2e-9 * (spec.mass_g / 9.0) ** (5.0 / 3.0)
    out_inertia = rotor_inertia * (ratio_int * gear_extra) ** 2
    firmware_kind = d.get("firmware", "position")
    loop = d.get("loop_hz", 1000.0)
    # Position gains: full voltage at 5° error for servos, a 20 Hz loop bandwidth otherwise.
    if firmware_kind == "servo":
        kp, kd, ki = V / math.radians(5.0), V / math.radians(5.0) * 0.02, 0.0
    elif firmware_kind == "position":
        kp, kd, ki = V / math.radians(10.0), V / math.radians(10.0) * 0.03, V / math.radians(10.0) * 0.5
    elif firmware_kind == "torque":
        kp, kd, ki = 0.0, 0.0, 0.0
    else:
        kp, kd, ki = 0.0, 0.0, 0.0
    copper = 0.12 * spec.mass_g * 1e-3
    case = max(spec.mass_g * 1e-3 - copper, 1e-3)
    small = spec.mass_g < 100
    return {
        "electrical": {"resistance": R, "inductance": L, "torque_constant": kt, "back_emf_constant": ke, "no_load_current": d.get("no_load_current", 0.1 * i_stall), "rotor_inertia": rotor_inertia, "supply_voltage": V, "current_limit": i_stall, "stall_current": i_stall, "poles": 14 if spec.kind == "bldc" else 0},
        "gearbox": {"ratio": ratio_int * gear_extra, "efficiency": eta, "backlash_rad": d.get("backlash", math.radians(1.0) if ratio_int > 1 else 0.0), "inertia": out_inertia, "stiffness": d.get("gear_stiffness", 50.0) * gear_extra ** 2, "max_output_torque": tau_out * gear_extra, "max_output_speed": w_out / max(gear_extra, 1e-9)},
        "thermal": {"winding_heat_capacity": copper * 385.0, "case_heat_capacity": case * (900.0 if spec.kind == "servo" else 470.0), "r_winding_case": 12.0 if small else 4.0, "r_case_mount": 6.0 if small else 2.5, "r_case_ambient": 45.0 if small else 15.0, "resistance_temp_coeff": 0.0039, "torque_derating_per_c": 0.0012, "max_winding_c": d.get("max_c", 110.0), "ambient_c": 25.0},
        "firmware": {"kind": firmware_kind, "loop_rate_hz": loop, "latency_s": 1.0 / loop, "deadband_rad": d.get("deadband", 0.0), "sensor_resolution_rad": d.get("resolution", 2 * math.pi / 4096), "kp": kp, "ki": ki, "kd": kd, "output": "current" if firmware_kind in ("torque", "stepper") else "voltage"},
        "driver": {"kind": d.get("driver", "h_bridge"), "pwm_hz": 50.0 if firmware_kind == "servo" else 20000.0, "on_resistance": 0.05 if small else 0.01, "current_limit": i_stall},
        "notes": d.get("notes", "no datasheet entry: R, kt and ke derived from stall torque, no-load speed and voltage"),
    }


# ----------------------------------------------------------------- motors


def motor_body(k: GeometryKernel, spec: MotorSpec, mount_point: Vec3, shaft_dir: Vec3, rotation_deg: float = 0.0) -> tuple[Body, dict]:
    """The motor as one solid: housing behind the mount face at
    `mount_point`, shaft along `shaft_dir` in front of it, pilot flange
    and mounting holes. Returns the body and its robot metadata (shaft
    axis, mount plane) in world coordinates."""
    n = v_unit(shaft_dir)
    frame = Plane.from_normal(mount_point, n)
    if rotation_deg:
        a = math.radians(rotation_deg)
        x = v_add(v_scale(frame.x_axis, math.cos(a)), v_scale(frame.y_axis, math.sin(a)))
        frame = Plane(mount_point, n, v_unit(x))
    w, d, L = spec.size
    behind = v_scale(n, -1.0)
    if spec.shape == "box":
        from .kernel import Sketch

        sk = Sketch(frame)
        sk.rectangle_center((0.0, 0.0), (w, d))
        body = k.extrude(sk.to_body(), behind, L)
    else:
        body = k.cylinder(mount_point, behind, w / 2, L)
    if spec.flange:
        fd, fh = spec.flange
        body = k.boolean(body, k.cylinder(mount_point, n, fd / 2, fh), BooleanOp.UNION)
    shaft_len = spec.shaft_length + (spec.flange[1] if spec.flange else 0.0)
    if spec.kind == "linear":
        body = k.boolean(body, k.cylinder(mount_point, n, spec.shaft_diameter / 2, spec.stroke + 10.0), BooleanOp.UNION)
    else:
        body = k.boolean(body, k.cylinder(mount_point, n, spec.shaft_diameter / 2, shaft_len), BooleanOp.UNION)
    for hx, hy, hd in spec.mount_holes:
        p = frame.to_world(hx, hy)
        if abs(hx) < w / 2 + 1e-6 and abs(hy) < d / 2 + 1e-6 or spec.shape == "cylinder":
            hole = k.cylinder(v_add(p, v_scale(n, 1.0)), behind, hd / 2, 8.0)
            try:
                body = k.boolean(body, hole, BooleanOp.SUBTRACT)
            except KernelError:
                pass
    meta = {"kind": "motor", "spec": spec.id, "mount_point": list(mount_point), "shaft_axis": list(n), "shaft_tip": list(v_add(mount_point, v_scale(n, shaft_len))), "rotation_deg": rotation_deg, "mounted_on": None, "drives": None}
    return body, meta


def motor_mount_holes_tool(k: GeometryKernel, spec: MotorSpec, mount_point: Vec3, shaft_dir: Vec3, depth: float = 20.0, clearance: float = 0.3, rotation_deg: float = 0.0) -> Optional[Body]:
    """A cutter for the motor's mounting-hole pattern and shaft/flange
    pass-through, to subtract from the bracket the motor bolts to."""
    n = v_unit(shaft_dir)
    frame = Plane.from_normal(mount_point, n)
    if rotation_deg:
        a = math.radians(rotation_deg)
        frame = Plane(mount_point, n, v_unit(v_add(v_scale(frame.x_axis, math.cos(a)), v_scale(frame.y_axis, math.sin(a)))))
    tool = None
    fd = (spec.flange[0] if spec.flange else spec.shaft_diameter) + 2 * clearance
    tool = k.cylinder(v_sub(mount_point, v_scale(n, 1.0)), n, fd / 2, depth + 1.0)
    for hx, hy, hd in spec.mount_holes:
        p = frame.to_world(hx, hy)
        hole = k.cylinder(v_sub(p, v_scale(n, 1.0)), n, (hd + clearance) / 2, depth + 1.0)
        tool = k.boolean(tool, hole, BooleanOp.UNION)
    return tool


# ----------------------------------------------------------------- joints


@dataclass
class Joint:
    type: str  # revolute | continuous | prismatic | fixed | ball
    parent: Optional[str]  # body node id (None = world)
    child: str  # body node id
    pivot: Vec3
    axis: Vec3 = (0.0, 0.0, 1.0)
    lower: Optional[float] = None  # rad (or mm for prismatic)
    upper: Optional[float] = None
    motor: Optional[str] = None  # motor body node id
    gear_ratio: float = 1.0  # extra reduction between motor output and joint
    damping: float = 0.0  # N·m·s/rad
    friction: float = 0.0  # N·m
    home: float = 0.0  # rad offset of the CAD pose from the joint's zero
    stroke: float = 0.0  # prismatic travel (mm) when limits are unset

    def to_json(self) -> dict:
        d = asdict(self)
        d["pivot"] = list(self.pivot)
        d["axis"] = list(self.axis)
        return d

    @staticmethod
    def from_json(d: dict) -> "Joint":
        return Joint(d["type"], d.get("parent"), d["child"], tuple(d["pivot"]), tuple(d.get("axis", (0, 0, 1))), d.get("lower"), d.get("upper"), d.get("motor"), d.get("gear_ratio", 1.0), d.get("damping", 0.0), d.get("friction", 0.0), d.get("home", 0.0), d.get("stroke", 0.0))

    @property
    def dof(self) -> int:
        # Loop-closing joints remove freedom rather than add it.
        return {"revolute": 1, "continuous": 1, "prismatic": 1, "fixed": 0, "ball": 3, "loop_revolute": -5, "loop_spherical": -3}[self.type]

    @property
    def is_loop(self) -> bool:
        return self.type.startswith("loop_")


def infer_joints(doc, min_overlap: float = 1.0) -> list[Joint]:
    """Revolute joints where two bodies share a coaxial cylindrical pair
    (a shaft in a bore, a pin through a hole) with the same radius within
    0.6 mm and overlapping along the axis. The body containing the hole is
    the parent by default (a bracket carries a shaft)."""
    k = doc.kernel
    bodies = [n for n in doc.bodies() if n.kind == "body"]
    cyls = []
    for n in bodies:
        for f in k.faces(n.body):
            if f.kind == SurfaceKind.CYLINDER and f.radius and f.axis_point and f.axis_dir:
                hole = k._cylinder_is_hole(n.body, f) if hasattr(k, "_cylinder_is_hole") else False
                base, height = k._cylinder_span(n.body, f, 0.0)
                cyls.append((n, f, hole, base, height))
    found: list[Joint] = []
    seen: set[tuple[str, str]] = set()
    for i, (na, fa, ha, ba, la) in enumerate(cyls):
        for nb, fb, hb, bb, lb in cyls[i + 1:]:
            if na.id == nb.id or ha == hb:
                continue
            if abs(fa.radius - fb.radius) > 0.6:
                continue
            da, db = v_unit(fa.axis_dir), v_unit(fb.axis_dir)
            if abs(abs(v_dot(da, db)) - 1.0) > 1e-3:
                continue
            # Same axis line?
            off = v_sub(fb.axis_point, fa.axis_point)
            perp = v_sub(off, v_scale(da, v_dot(off, da)))
            if v_norm(perp) > 0.6:
                continue
            # Overlap along the axis.
            ta0 = v_dot(v_sub(ba, fa.axis_point), da)
            tb0 = v_dot(v_sub(bb, fa.axis_point), da)
            tb1 = tb0 + lb * (1.0 if v_dot(db, da) > 0 else -1.0)
            lo, hi = max(ta0, min(tb0, tb1)), min(ta0 + la, max(tb0, tb1))
            if hi - lo < min_overlap:
                continue
            parent, child = (na, nb) if ha else (nb, na)
            key = (parent.id, child.id)
            if key in seen:
                continue
            seen.add(key)
            pivot = v_add(fa.axis_point, v_scale(da, 0.5 * (lo + hi)))
            found.append(Joint("revolute", parent.id, child.id, pivot, da))
    return found


@dataclass
class RobotIssue:
    severity: str
    message: str
    node: Optional[str] = None


def robot_summary(doc, exact=False) -> dict:
    joints = [(n, n.joint) for n in doc.walk() if n.kind == "joint" and n.joint is not None]
    motors = [n for n in doc.walk() if n.robot and n.robot.get("kind") == "motor"]
    # Summing spatial constraint counts gives misleading (even negative)
    # mobility for planar closed loops; their constraint rank must be solved.
    has_loops = any(j.is_loop for _, j in joints)
    dof = None if has_loops else sum(j.dof for _, j in joints)
    links = {n.id for n in doc.bodies()}
    return {
        "joints": [{"id": n.id, "name": n.name, **j.to_json(), "parent_name": doc.nodes[j.parent].name if j.parent in doc.nodes else None, "child_name": doc.nodes[j.child].name if j.child in doc.nodes else None, "motor_name": doc.nodes[j.motor].name if j.motor in doc.nodes else None} for n, j in joints],
        "motors": [{"id": n.id, "name": n.name, **n.robot, "spec_name": MOTOR_LIBRARY[n.robot["spec"]].name if n.robot.get("spec") in MOTOR_LIBRARY else n.robot.get("spec")} for n in motors],
        "links": len(links), "dof": dof, "has_closed_loops": has_loops,
        "ground": [n.id for n in doc.bodies() if n.name.lower() == "ground" or (n.robot or {}).get("ground")],
        "issues": [asdict(i) for i in validate_robot(doc, exact=exact)],
        "validation_scope": "geometry and topology" if exact else "topology only; exact geometry checks are explicit",
    }


def validate_robot(doc, exact=True) -> list[RobotIssue]:
    issues: list[RobotIssue] = []
    joints = [(n, n.joint) for n in doc.walk() if n.kind == "joint" and n.joint is not None]
    bodies = {n.id: n for n in doc.bodies()}
    k = doc.kernel
    children: dict[str, list[str]] = {}
    parents: dict[str, str] = {}
    for n, j in joints:
        if j.type not in JOINT_TYPES:
            issues.append(RobotIssue("error", f"{n.name}: unknown joint type {j.type}", n.id))
        if j.child not in bodies:
            issues.append(RobotIssue("error", f"{n.name}: child body is missing", n.id))
            continue
        if j.parent is not None and j.parent not in bodies:
            issues.append(RobotIssue("error", f"{n.name}: parent body is missing", n.id))
            continue
        if j.parent == j.child:
            issues.append(RobotIssue("error", f"{n.name}: a body cannot be jointed to itself", n.id))
        if j.is_loop:
            if j.parent is None:
                issues.append(RobotIssue("error", f"{n.name}: a loop joint needs two bodies", n.id))
            continue
        if j.child in parents:
            issues.append(RobotIssue("error", f"{n.name}: {bodies[j.child].name} already has a parent joint (the mechanism must be a tree; use a fixed joint or remove one)", n.id))
        parents[j.child] = j.parent or "world"
        children.setdefault(j.parent or "world", []).append(j.child)
        if v_norm(j.axis) < 1e-9:
            issues.append(RobotIssue("error", f"{n.name}: zero axis", n.id))
        if j.type in ("revolute", "prismatic") and j.lower is not None and j.upper is not None and j.lower >= j.upper:
            issues.append(RobotIssue("error", f"{n.name}: lower limit is not below the upper limit", n.id))
        # The pivot should touch both bodies (within 5 mm of each).
        for role, bid in (("parent", j.parent), ("child", j.child)):
            if exact and bid and bid in bodies:
                try:
                    d = 0.0 if k.contains(bodies[bid].body, j.pivot) else k.distance(bodies[bid].body, k.sphere(j.pivot, 0.01))[0]
                    if d > 5.0:
                        issues.append(RobotIssue("warning", f"{n.name}: pivot is {d:.1f} mm from the {role} body", n.id))
                except Exception:
                    pass
        if j.motor:
            m = doc.nodes.get(j.motor)
            if m is None or not m.robot or m.robot.get("kind") != "motor":
                issues.append(RobotIssue("error", f"{n.name}: its motor is missing", n.id))
            else:
                axis = tuple(m.robot["shaft_axis"])
                if abs(abs(v_dot(v_unit(axis), v_unit(j.axis))) - 1.0) > 1e-2 and j.type != "prismatic":
                    issues.append(RobotIssue("warning", f"{n.name}: the motor shaft is not aligned with the joint axis", n.id))
                spec = MOTOR_LIBRARY.get(m.robot.get("spec", ""))
                if exact and spec and j.type in ("revolute", "continuous"):
                    load = gravity_torque(doc, j)
                    if load > spec.stall_torque * j.gear_ratio:
                        issues.append(RobotIssue("warning", f"{n.name}: {spec.name} stalls at {spec.stall_torque * j.gear_ratio:.2f} N·m but the arm's gravity load is {load:.2f} N·m", n.id))
        elif j.type in ("revolute", "continuous", "prismatic"):
            issues.append(RobotIssue("info", f"{n.name}: no motor assigned (passive joint)", n.id))
    # Cycles / multiple roots.
    def reaches(a, target, depth=0):
        if depth > 100:
            return True
        return any(c == target or reaches(c, target, depth + 1) for c in children.get(a, []))

    for n, j in joints:
        if j.parent and not j.is_loop and reaches(j.child, j.parent):
            issues.append(RobotIssue("error", f"{n.name}: closes a loop in the tree; make it a loop_revolute/loop_spherical joint", n.id))
    # Mounted motors ride on their body; they are not separate links.
    roots = [b for b in bodies if b not in parents and not ((bodies[b].robot or {}).get("kind") == "motor" and (bodies[b].robot or {}).get("mounted_on"))]
    if joints and len(roots) > 1:
        issues.append(RobotIssue("info", f"{len(roots)} unconnected root bodies: {', '.join(bodies[r].name for r in roots)}"))
    return issues


def gravity_torque(doc, j: Joint) -> float:
    """Worst-case gravity torque (N·m) of everything hanging from a joint:
    Σ m·g·(horizontal distance of COM from the pivot) with the arm level."""
    k = doc.kernel
    subtree = subtree_bodies(doc, j.child)
    total = 0.0
    for bid in subtree:
        n = doc.nodes[bid]
        if n.body is None:
            continue
        p = k.mass_properties(n.body)
        mass_kg = p.mass(doc.density_of(bid)) / 1000.0
        r = v_dist(p.centroid, j.pivot) * 1e-3
        total += mass_kg * 9.81 * r
    return total


def subtree_bodies(doc, root: str) -> list[str]:
    joints = [n.joint for n in doc.walk() if n.kind == "joint" and n.joint is not None]
    out = [root]
    frontier = [root]
    while frontier:
        b = frontier.pop()
        for j in joints:
            if j.parent == b and j.child not in out:
                out.append(j.child)
                frontier.append(j.child)
    # Motors mounted on these bodies ride along.
    for n in doc.walk():
        if n.robot and n.robot.get("kind") == "motor" and n.robot.get("mounted_on") in out and n.id not in out:
            out.append(n.id)
    return out


def joint_glyph(j: Joint, size: float = 12.0) -> list[tuple[str, object]]:
    """Viewport shapes for a joint: axis line, and a ring (revolute), a
    double arrow (prismatic), a cube (fixed) or a sphere-ish ring pair (ball)."""
    a = v_unit(j.axis)
    color = {"revolute": (1.0, 0.75, 0.2), "continuous": (1.0, 0.55, 0.2), "prismatic": (0.3, 0.9, 1.0), "fixed": (0.7, 0.7, 0.7), "ball": (0.9, 0.4, 0.9)}.get(j.type, (1, 1, 1))
    helper = (0.0, 0.0, 1.0) if abs(a[2]) < 0.9 else (1.0, 0.0, 0.0)
    u = v_unit(v_cross(helper, a))
    v = v_cross(a, u)
    shapes: list[tuple[str, object]] = [("line", (v_sub(j.pivot, v_scale(a, size * 1.5)), v_add(j.pivot, v_scale(a, size * 1.5)), color))]
    if j.type in ("revolute", "continuous", "ball"):
        ring = [v_add(j.pivot, v_add(v_scale(u, size * math.cos(t)), v_scale(v, size * math.sin(t)))) for t in [2 * math.pi * i / 32 for i in range(33)]]
        shapes.append(("poly", (ring, color)))
        if j.type == "ball":
            ring2 = [v_add(j.pivot, v_add(v_scale(a, size * math.cos(t)), v_scale(v, size * math.sin(t)))) for t in [2 * math.pi * i / 32 for i in range(33)]]
            shapes.append(("poly", (ring2, color)))
    elif j.type == "prismatic":
        for s in (1.0, -1.0):
            tip = v_add(j.pivot, v_scale(a, s * size * 1.5))
            back = v_add(j.pivot, v_scale(a, s * size))
            shapes.append(("line", (back, v_add(tip, v_scale(u, 0.0)), color)))
            shapes.append(("line", (tip, v_add(back, v_scale(u, size * 0.3)), color)))
            shapes.append(("line", (tip, v_sub(back, v_scale(u, size * 0.3)), color)))
    else:
        for s in (1.0, -1.0):
            pts = [v_add(j.pivot, v_add(v_scale(u, s * size * 0.5), v_scale(v, size * 0.5))), v_add(j.pivot, v_add(v_scale(u, s * size * 0.5), v_scale(v, -size * 0.5)))]
            shapes.append(("line", (pts[0], pts[1], color)))
        shapes.append(("line", (v_add(j.pivot, v_add(v_scale(u, size * 0.5), v_scale(v, size * 0.5))), v_add(j.pivot, v_add(v_scale(u, -size * 0.5), v_scale(v, size * 0.5))), color)))
        shapes.append(("line", (v_add(j.pivot, v_add(v_scale(u, size * 0.5), v_scale(v, -size * 0.5))), v_add(j.pivot, v_add(v_scale(u, -size * 0.5), v_scale(v, -size * 0.5))), color)))
    shapes.append(("point", (j.pivot, color, 9.0)))
    return shapes
