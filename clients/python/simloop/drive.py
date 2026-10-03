"""Drive kinematics for a robot's controller: the Python port of
``crates/sim-domain-control/src/drive/kinematics.rs``.

The simulator's run thread already limits a teleoperation request with the
shared rule (``kinematics::step``) and hands the controller a body twist plus
a heartbeat.  On that seam the run thread's limited twist is authoritative:
the controller checks it against the profile and passes it through
(:class:`DriveState` with ``limit_live=False``), so a halt stops at once and a
run-thread deadman ramp keeps its own deceleration.  The controller's own
deadman is the guard if the heartbeat stops rising; then it applies the stop
rule from its last output.  A host that sends raw twists asks for
``limit_live=True`` instead.  The twist is then mixed into wheel joint rates
with the robot's kinematic adapter.

Conventions match the Rust code: a twist is ``(forward m/s, lateral m/s,
yaw rad/s)`` in the body frame, lateral positive to the left, yaw positive
counter-clockwise seen from above.  A wheel's rolling rate is positive when it
rolls the body forward; its joint rate is the rolling rate times the wheel's
joint sign.  Arithmetic is written in the Rust order so results agree to the
last bits; ``clients/python/tests/test_drive.py`` checks that against the
golden vectors the Rust file generates.

Standard library only, Python 3.9+.
"""

from __future__ import annotations

import json
import math
from decimal import Decimal
from typing import Any, Dict, List, NamedTuple, Optional, Sequence, Tuple, Union

__all__ = [
    "AXIS_NAMES", "SPEED_UNITS", "ACCEL_UNITS", "RESOLVED_SCHEMA",
    "BodyTwist", "ZERO", "Axes", "Limits", "OnLoss", "Deadman", "Commanded", "KinematicsError",
    "scale", "check_twist", "limit", "deadman_expired", "step",
    "DifferentialDrive", "Mecanum", "ResolvedDrive", "ResolvedDriveError", "DriveState",
]

AXIS_NAMES = ("forward", "lateral", "yaw")
SPEED_UNITS = ("m/s", "m/s", "rad/s")
ACCEL_UNITS = ("m/s^2", "m/s^2", "rad/s^2")
RESOLVED_SCHEMA = "sim.drive.resolved/1"

Triple = Tuple[float, float, float]


class BodyTwist(NamedTuple):
    """A body-frame twist: forward and lateral in m/s, yaw in rad/s."""

    forward_m_s: float = 0.0
    lateral_m_s: float = 0.0
    yaw_rad_s: float = 0.0

    def is_zero(self) -> bool:
        return all(v == 0.0 for v in self)


ZERO = BodyTwist(0.0, 0.0, 0.0)


class Axes(NamedTuple):
    """Normalized axis requests, each in -1..1 (a device binding's output)."""

    forward: float = 0.0
    lateral: float = 0.0
    yaw: float = 0.0


class Limits(NamedTuple):
    """A profile's limits in axis order; an unsupported axis's speed is always zero."""

    supported: Tuple[bool, bool, bool]
    max_speed: Triple
    max_accel: Triple


class OnLoss(NamedTuple):
    """What happens when the deadman expires: ``ramp`` each axis to zero under
    ``decel`` per second, or ``immediate`` zero."""

    kind: str
    decel: Triple = (0.0, 0.0, 0.0)

    @classmethod
    def ramp(cls, decel: Sequence[float]) -> "OnLoss":
        return cls("ramp", (float(decel[0]), float(decel[1]), float(decel[2])))

    @classmethod
    def immediate(cls) -> "OnLoss":
        return cls("immediate")


class Deadman(NamedTuple):
    """A request older than ``timeout_s`` is lost."""

    timeout_s: float
    on_loss: OnLoss


class Commanded(NamedTuple):
    """One limited, deadman-checked command; ``expired`` means the stop rule applied."""

    twist: BodyTwist
    expired: bool


def _rust_float(v: float) -> str:
    """Format like Rust's ``Display`` for f64: shortest digits, no exponent, no trailing ``.0``."""
    if math.isnan(v):
        return "NaN"
    if math.isinf(v):
        return "inf" if v > 0 else "-inf"
    text = format(Decimal(repr(v)), "f")
    if "." in text:
        text = text.rstrip("0").rstrip(".")
    return text


class KinematicsError(ValueError):
    """Why a kinematic request was refused; ``kind`` is one of not_finite,
    out_of_range, unsupported, out_of_profile, bad_parameter (the golden
    file's ``error`` values) and the message matches the Rust Display."""

    def __init__(self, kind: str, message: str):
        super().__init__(message)
        self.kind = kind

    @classmethod
    def not_finite(cls, what: str) -> "KinematicsError":
        return cls("not_finite", f"{what} must be a finite number")

    @classmethod
    def out_of_range(cls, axis: str, value: float) -> "KinematicsError":
        return cls("out_of_range", f"axis `{axis}` = {_rust_float(value)} is outside -1..1")

    @classmethod
    def unsupported(cls, axis: str, value: float) -> "KinematicsError":
        return cls("unsupported", f"axis `{axis}` = {_rust_float(value)} is not supported by this drive")

    @classmethod
    def out_of_profile(cls, axis: str, value: float, max_: float) -> "KinematicsError":
        return cls("out_of_profile", f"`{axis}` speed {_rust_float(value)} exceeds the profile's {_rust_float(max_)}")

    @classmethod
    def bad_parameter(cls, what: str) -> "KinematicsError":
        return cls("bad_parameter", f"{what} must be finite and in range")


def _finite(value: float, what: str) -> float:
    if math.isfinite(value):
        return value
    raise KinematicsError.not_finite(what)


def _check_limits(limits: Limits) -> None:
    for i in range(3):
        if not (math.isfinite(limits.max_speed[i]) and limits.max_speed[i] >= 0.0):
            raise KinematicsError.bad_parameter(f"max_speed.{AXIS_NAMES[i]}")
        if not (math.isfinite(limits.max_accel[i]) and limits.max_accel[i] > 0.0):
            raise KinematicsError.bad_parameter(f"max_accel.{AXIS_NAMES[i]}")


def scale(axes: Sequence[float], limits: Limits) -> BodyTwist:
    """Normalized axes to a twist: each axis times the profile's max speed.
    NaN, an axis outside -1..1 and a nonzero unsupported axis are refused."""
    _check_limits(limits)
    out = [0.0, 0.0, 0.0]
    for i in range(3):
        v = _finite(axes[i], f"axes.{AXIS_NAMES[i]}")
        if not (-1.0 <= v <= 1.0):
            raise KinematicsError.out_of_range(AXIS_NAMES[i], v)
        if not limits.supported[i]:
            if v != 0.0:
                raise KinematicsError.unsupported(AXIS_NAMES[i], v)
            continue
        out[i] = v * limits.max_speed[i]
    return BodyTwist(*out)


def check_twist(twist: Sequence[float], limits: Limits) -> None:
    """Refuse a twist that is not finite, exceeds an axis's max speed, or is
    nonzero on an unsupported axis."""
    _check_limits(limits)
    for i in range(3):
        v = _finite(twist[i], f"twist.{AXIS_NAMES[i]}")
        if not limits.supported[i] and v != 0.0:
            raise KinematicsError.unsupported(AXIS_NAMES[i], v)
        if abs(v) > limits.max_speed[i]:
            raise KinematicsError.out_of_profile(AXIS_NAMES[i], v, limits.max_speed[i])


def limit(previous: Sequence[float], request: Sequence[float], dt_s: float, max_accel: Sequence[float]) -> BodyTwist:
    """Each axis moves from ``previous`` toward ``request`` by at most ``max_accel * dt_s``."""
    if not (math.isfinite(dt_s) and dt_s >= 0.0):
        raise KinematicsError.bad_parameter("dt_s")
    out = [0.0, 0.0, 0.0]
    for i in range(3):
        p = _finite(previous[i], f"previous.{AXIS_NAMES[i]}")
        r = _finite(request[i], f"request.{AXIS_NAMES[i]}")
        if not (math.isfinite(max_accel[i]) and max_accel[i] > 0.0):
            raise KinematicsError.bad_parameter(f"max_accel.{AXIS_NAMES[i]}")
        step_ = max_accel[i] * dt_s
        d = r - p
        # f64::clamp(-step, step): the bound when outside, else the value itself.
        d = -step_ if d < -step_ else step_ if d > step_ else d
        out[i] = p + d
    return BodyTwist(*out)


def deadman_expired(age_s: float, deadman: Deadman) -> bool:
    """True when a request ``age_s`` old is lost; a NaN or negative age counts as lost."""
    return not (age_s >= 0.0 and age_s < deadman.timeout_s)


def step(previous: Sequence[float], request: Sequence[float], dt_s: float, age_s: float, limits: Limits, deadman: Deadman) -> Commanded:
    """One control step of the shared rule: a live request is checked and
    approached under ``max_accel``; a lost one is replaced by the stop rule."""
    if not (math.isfinite(deadman.timeout_s) and deadman.timeout_s > 0.0):
        raise KinematicsError.bad_parameter("deadman.timeout_s")
    if deadman_expired(age_s, deadman):
        if deadman.on_loss.kind == "immediate":
            twist = ZERO
        elif deadman.on_loss.kind == "ramp":
            twist = limit(previous, ZERO, dt_s, deadman.on_loss.decel)
        else:
            raise KinematicsError.bad_parameter(f"deadman.on_loss `{deadman.on_loss.kind}`")
        return Commanded(twist, True)
    check_twist(request, limits)
    return Commanded(limit(previous, request, dt_s, limits.max_accel), False)


def _positive(value: float, what: str) -> float:
    if math.isfinite(value) and value > 0.0:
        return float(value)
    raise KinematicsError.bad_parameter(what)


def _signs(signs: Sequence[float], n: int) -> Tuple[float, ...]:
    if len(signs) != n:
        raise KinematicsError.bad_parameter(f"signs (needs {n})")
    for i, s in enumerate(signs):
        if s != 1.0 and s != -1.0:
            raise KinematicsError.bad_parameter(f"signs[{i}] (must be +1 or -1)")
    return tuple(float(s) for s in signs)


def _finite_twist(twist: Sequence[float]) -> None:
    for i in range(3):
        _finite(twist[i], f"twist.{AXIS_NAMES[i]}")


class DifferentialDrive:
    """A two-wheel differential drive; joint rates are ``[left, right]`` in rad/s."""

    def __init__(self, track_width_m: float, wheel_radius_m: float, signs: Sequence[float]):
        self.track_width_m = _positive(track_width_m, "track_width_m")
        self.wheel_radius_m = _positive(wheel_radius_m, "wheel_radius_m")
        self.signs = _signs(signs, 2)

    def mix(self, twist: Sequence[float]) -> List[float]:
        """Twist to joint rates ``[left, right]``; a nonzero lateral speed is refused."""
        _finite_twist(twist)
        forward, lateral, yaw = twist[0], twist[1], twist[2]
        if lateral != 0.0:
            raise KinematicsError.unsupported("lateral", lateral)
        half = 0.5 * self.track_width_m * yaw
        left = (forward - half) / self.wheel_radius_m
        right = (forward + half) / self.wheel_radius_m
        return [self.signs[0] * left, self.signs[1] * right]

    def unmix(self, joint_rates: Sequence[float]) -> BodyTwist:
        """Joint rates ``[left, right]`` back to the twist they produce (lateral 0)."""
        left = self.signs[0] * joint_rates[0] * self.wheel_radius_m
        right = self.signs[1] * joint_rates[1] * self.wheel_radius_m
        return BodyTwist(0.5 * (left + right), 0.0, (right - left) / self.track_width_m)

    def __repr__(self) -> str:
        return f"DifferentialDrive(track_width_m={self.track_width_m!r}, wheel_radius_m={self.wheel_radius_m!r}, signs={self.signs!r})"


class Mecanum:
    """A four-wheel mecanum drive in the X roller layout; joint rates are
    ``[front_left, front_right, rear_left, rear_right]`` in rad/s."""

    def __init__(self, track_width_m: float, wheelbase_m: float, wheel_radius_m: float, signs: Sequence[float]):
        self.track_width_m = _positive(track_width_m, "track_width_m")
        self.wheelbase_m = _positive(wheelbase_m, "wheelbase_m")
        self.wheel_radius_m = _positive(wheel_radius_m, "wheel_radius_m")
        self.signs = _signs(signs, 4)

    def _lever_m(self) -> float:
        return 0.5 * (self.track_width_m + self.wheelbase_m)

    def mix(self, twist: Sequence[float]) -> List[float]:
        """Twist to joint rates ``[fl, fr, rl, rr]`` (rad/s)."""
        _finite_twist(twist)
        vx, vy, w = twist[0], twist[1], twist[2]
        k = self._lever_m() * w
        r = self.wheel_radius_m
        rolling = [(vx - vy - k) / r, (vx + vy + k) / r, (vx + vy - k) / r, (vx - vy + k) / r]
        return [self.signs[i] * rolling[i] for i in range(4)]

    def unmix(self, joint_rates: Sequence[float]) -> BodyTwist:
        """Joint rates ``[fl, fr, rl, rr]`` back to the twist (least-squares inverse)."""
        fl, fr, rl, rr = (self.signs[i] * joint_rates[i] * self.wheel_radius_m for i in range(4))
        return BodyTwist(0.25 * (fl + fr + rl + rr), 0.25 * (-fl + fr + rl - rr), 0.25 * (-fl + fr - rl + rr) / self._lever_m())

    def __repr__(self) -> str:
        return f"Mecanum(track_width_m={self.track_width_m!r}, wheelbase_m={self.wheelbase_m!r}, wheel_radius_m={self.wheel_radius_m!r}, signs={self.signs!r})"


Mixer = Union[DifferentialDrive, Mecanum]


class ResolvedDriveError(ValueError):
    """A resolved drive that cannot be used; the message names the field."""


class ResolvedDrive:
    """The ``sim.drive.resolved/1`` JSON the host hands the controller as
    ``--drive-json``: the profile's limits and deadman plus the drive geometry
    (derived from the model or declared), each value with its provenance."""

    def __init__(self, data: Dict[str, Any]):
        self.data = data
        self.schema: str = data["schema"]
        self.profile: str = data["profile"]
        self.profile_sha256: str = data["profile_sha256"]
        self.kinematics: str = data["kinematics"]
        geometry = data["geometry"]
        self.track_width_m: float = geometry["track_width_m"]["value"]
        wheelbase = geometry.get("wheelbase_m")
        self.wheelbase_m: Optional[float] = None if wheelbase is None else wheelbase["value"]
        self.wheel_radius_m: float = geometry["wheel_radius_m"]["value"]
        wheels = geometry["wheels"]
        #: Wheel joint names in mixer order ([left, right] or [fl, fr, rl, rr]).
        self.joints: List[str] = [w["joint"] for w in wheels]
        self.signs: List[float] = [w["sign"] for w in wheels]
        limits = data["limits"]
        self.supported: Tuple[bool, bool, bool] = tuple(limits["supported"])  # type: ignore[assignment]
        self.max_speed: Triple = tuple(limits["max_speed"])  # type: ignore[assignment]
        self.max_accel: Triple = tuple(limits["max_accel"])  # type: ignore[assignment]
        self.stop_decel: Triple = tuple(limits["stop_decel"])  # type: ignore[assignment]
        self.timeout_s: float = data["deadman"]["timeout_s"]
        self.on_loss: str = data["deadman"]["on_loss"]

    @classmethod
    def from_json(cls, source: Union[str, bytes, Dict[str, Any]]) -> "ResolvedDrive":
        """Parse and check the resolved JSON (text or an already-parsed dict).
        The schema is checked first; every refusal names the field."""
        if isinstance(source, (str, bytes)):
            try:
                data = json.loads(source)
            except ValueError as e:
                raise ResolvedDriveError(f"resolved drive: not JSON: {e}") from None
        else:
            data = source
        if not isinstance(data, dict):
            raise ResolvedDriveError("resolved drive: expected a JSON object")
        schema = data.get("schema")
        if schema != RESOLVED_SCHEMA:
            raise ResolvedDriveError(f"resolved drive: schema: expected {RESOLVED_SCHEMA!r}, got {schema!r}")
        _check_resolved(data)
        drive = cls(data)
        drive.mixer()  # geometry refusals (bad radius, signs) surface at load, by field
        return drive

    def limits(self) -> Limits:
        return Limits(self.supported, self.max_speed, self.max_accel)

    def deadman(self) -> Deadman:
        on_loss = OnLoss.ramp(self.stop_decel) if self.on_loss == "ramp" else OnLoss.immediate()
        return Deadman(self.timeout_s, on_loss)

    def mixer(self) -> Mixer:
        """The kinematic adapter for this geometry, wheel signs included."""
        try:
            if self.kinematics == "differential":
                return DifferentialDrive(self.track_width_m, self.wheel_radius_m, self.signs)
            if self.wheelbase_m is None:
                raise ResolvedDriveError("resolved drive: geometry.wheelbase_m: a mecanum drive needs a wheelbase")
            return Mecanum(self.track_width_m, self.wheelbase_m, self.wheel_radius_m, self.signs)
        except KinematicsError as e:
            raise ResolvedDriveError(f"resolved drive: geometry: {e}") from None

    def __repr__(self) -> str:
        return f"ResolvedDrive({self.kinematics}, joints={self.joints!r}, profile={self.profile!r})"


def _check_resolved(data: Dict[str, Any]) -> None:
    """Shape checks, naming the dotted field of the first problem."""

    def fail(field: str, message: str) -> None:
        raise ResolvedDriveError(f"resolved drive: {field}: {message}")

    def get(obj: Any, key: str, field: str) -> Any:
        if not isinstance(obj, dict) or key not in obj:
            fail(field, "missing")
        return obj[key]

    def number(value: Any, field: str) -> float:
        if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
            fail(field, f"expected a finite number, got {value!r}")
        return float(value)

    def triple(value: Any, field: str, kind: type) -> None:
        if not isinstance(value, list) or len(value) != 3:
            fail(field, f"expected 3 values, got {value!r}")
        for i, v in enumerate(value):
            if kind is bool:
                if not isinstance(v, bool):
                    fail(f"{field}[{i}]", f"expected true or false, got {v!r}")
            else:
                number(v, f"{field}[{i}]")

    for key in ("profile", "profile_sha256"):
        if not isinstance(get(data, key, key), str):
            fail(key, "expected a string")
    kinematics = get(data, "kinematics", "kinematics")
    wheels_needed = {"differential": 2, "mecanum": 4}
    # isinstance first: a list or dict here is unhashable and would raise TypeError.
    if not isinstance(kinematics, str) or kinematics not in wheels_needed:
        fail("kinematics", f"expected \"differential\" or \"mecanum\", got {kinematics!r}")
    geometry = get(data, "geometry", "geometry")
    for key in ("track_width_m", "wheel_radius_m"):
        number(get(get(geometry, key, f"geometry.{key}"), "value", f"geometry.{key}.value"), f"geometry.{key}.value")
    wheelbase = geometry.get("wheelbase_m")
    if wheelbase is not None:
        number(get(wheelbase, "value", "geometry.wheelbase_m.value"), "geometry.wheelbase_m.value")
    wheels = get(geometry, "wheels", "geometry.wheels")
    if not isinstance(wheels, list) or len(wheels) != wheels_needed[kinematics]:
        fail("geometry.wheels", f"a {kinematics} drive needs {wheels_needed[kinematics]} wheels, got {wheels!r}")
    for i, wheel in enumerate(wheels):
        if not isinstance(get(wheel, "joint", f"geometry.wheels[{i}].joint"), str):
            fail(f"geometry.wheels[{i}].joint", "expected a joint name")
        number(get(wheel, "sign", f"geometry.wheels[{i}].sign"), f"geometry.wheels[{i}].sign")
    limits = get(data, "limits", "limits")
    triple(get(limits, "supported", "limits.supported"), "limits.supported", bool)
    for key in ("max_speed", "max_accel", "stop_decel"):
        triple(get(limits, key, f"limits.{key}"), f"limits.{key}", float)
    # The values the shared rule needs, refused at load rather than at the first step.
    for i, axis in enumerate(AXIS_NAMES):
        if limits["max_speed"][i] < 0.0:
            fail(f"limits.max_speed[{i}]", f"`{axis}` max speed must be finite and >= 0, got {limits['max_speed'][i]!r}")
        for key in ("max_accel", "stop_decel"):
            if limits[key][i] <= 0.0:
                fail(f"limits.{key}[{i}]", f"`{axis}` {key} must be finite and > 0, got {limits[key][i]!r}")
    if kinematics == "differential" and limits["supported"][1]:
        fail("limits.supported[1]", "a differential drive cannot move sideways; the lateral axis must be unsupported (false)")
    deadman = get(data, "deadman", "deadman")
    timeout = number(get(deadman, "timeout_s", "deadman.timeout_s"), "deadman.timeout_s")
    if timeout <= 0.0:
        fail("deadman.timeout_s", f"must be positive, got {timeout!r}")
    on_loss = get(deadman, "on_loss", "deadman.on_loss")
    if on_loss not in ("ramp", "immediate"):
        fail("deadman.on_loss", f"expected \"ramp\" or \"immediate\", got {on_loss!r}")


class DriveState:
    """The controller-side deadman over the seam's twist and heartbeat channels.

    The run thread raises ``command.heartbeat`` by one for every fresh
    request.  A heartbeat is fresh only when it is greater than the last one
    held (the Rust limiter's ``sequence > held``), so a NaN or decreasing
    heartbeat never refreshes the request and the deadman still fires.  The
    age of the request is the sim time since the heartbeat last rose.  The
    first sample's heartbeat is recorded whatever it is (a non-finite one as
    0), and heartbeat 0 means no request has been made yet: the request is
    treated as zero and its age runs from that first sample, so the deadman
    expires after ``timeout_s`` with the robot at rest.

    While the request is live it is checked against the profile
    (:func:`check_twist`) and then, with ``limit_live=False`` (the seam
    default), passed through unchanged: the run thread already limited it
    with the same shared rule, and limiting it again would slow a halt or a
    run-thread deadman ramp to ``max_accel``.  With ``limit_live=True`` (a
    host that sends raw twists, e.g. hardware) it is approached under
    ``max_accel`` with :func:`limit`.  Once the deadman expired the stop rule
    applies from the last output (``step`` with the expired age: a ramp at the
    deadman's deceleration, or zero at once).
    """

    def __init__(self, limits: Limits, deadman: Deadman, limit_live: bool = False):
        self.limits = limits
        self.deadman = deadman
        self.limit_live = limit_live
        #: The twist output last step (the stop rule's and limiter's previous value).
        self.twist = ZERO
        self.heartbeat: Optional[float] = None
        self.changed_t: Optional[float] = None
        self.expired = False

    def update(self, t: float, request: Sequence[float], heartbeat: float, dt: float) -> Tuple[BodyTwist, bool]:
        """One controller step at sim time ``t``: ``(twist to mix, expired)``.
        Raises :class:`KinematicsError` for a live request outside the profile."""
        if self.heartbeat is None:
            self.heartbeat, self.changed_t = (heartbeat if math.isfinite(heartbeat) else 0.0), t
        elif heartbeat > self.heartbeat:  # False for NaN and for a decrease
            self.heartbeat, self.changed_t = heartbeat, t
        if self.heartbeat == 0.0:
            request = ZERO  # no request yet
        age = t - self.changed_t  # type: ignore[operator]
        if deadman_expired(age, self.deadman) or self.limit_live:
            out = step(self.twist, request, dt, age, self.limits, self.deadman)
        else:
            check_twist(request, self.limits)
            out = Commanded(BodyTwist(float(request[0]), float(request[1]), float(request[2])), False)
        self.twist, self.expired = out.twist, out.expired
        return out.twist, out.expired
