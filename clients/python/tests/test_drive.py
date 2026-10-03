"""simloop.drive against the golden vectors generated from the Rust kinematics.

The golden file is printed by crates/sim-domain-control/tests/fixtures/
gen_drive_golden.rs, which compiles src/drive/kinematics.rs on its own, so
every case here is the Rust result for the same inputs.  Each section
(scale, limit, step, differential, mecanum) is checked case by case within
the file's ``tolerance``; refusal cases must fail with the same error kind.
"""

import contextlib
import io
import json
import math
import os
import unittest

from simloop import Loop, ProtocolError, drive

REPO = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", ".."))
GOLDEN = os.path.join(REPO, "crates", "sim-domain-control", "tests", "fixtures", "drive_golden.json")


def num(value):
    """A golden number; tolerate non-finite values written as strings."""
    return float(value)


def nums(values):
    return [num(v) for v in values]


def limits_of(case):
    l = case["limits"]
    return drive.Limits(tuple(bool(s) for s in l["supported"]), tuple(nums(l["max_speed"])), tuple(nums(l["max_accel"])))


def deadman_of(case):
    d = case["deadman"]
    on_loss = drive.OnLoss.ramp(nums(d["decel"])) if d["on_loss"] == "ramp" else drive.OnLoss.immediate()
    return drive.Deadman(num(d["timeout_s"]), on_loss)


class GoldenTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        with open(GOLDEN, encoding="utf-8") as f:
            cls.golden = json.load(f)
        cls.tol = float(cls.golden["tolerance"])

    def check(self, section, compute, expected_of):
        cases = self.golden[section]
        self.assertTrue(cases, f"{GOLDEN}: section {section} is empty")
        for case in cases:
            with self.subTest(section=section, name=case["name"]):
                if "error" in case:
                    with self.assertRaises(drive.KinematicsError) as caught:
                        compute(case)
                    self.assertEqual(caught.exception.kind, case["error"], str(caught.exception))
                    continue
                got, want = compute(case), expected_of(case)
                self.assertEqual(len(got), len(want))
                for g, w in zip(got, want):
                    self.assertTrue(math.isclose(g, w, rel_tol=0.0, abs_tol=self.tol) or g == w, f"got {list(got)}, want {want}")

    def test_schema(self):
        self.assertEqual(self.golden["schema"], "sim.drive.golden/1")

    def test_scale(self):
        self.check("scale", lambda c: drive.scale(drive.Axes(*nums(c["axes"])), limits_of(c)), lambda c: nums(c["twist"]))

    def test_limit(self):
        self.check("limit", lambda c: drive.limit(nums(c["previous"]), nums(c["request"]), num(c["dt_s"]), nums(c["max_accel"])), lambda c: nums(c["twist"]))

    def test_step(self):
        def compute(c):
            out = drive.step(nums(c["previous"]), nums(c["request"]), num(c["dt_s"]), num(c["age_s"]), limits_of(c), deadman_of(c))
            return list(out.twist) + [1.0 if out.expired else 0.0]

        self.check("step", compute, lambda c: nums(c["twist"]) + [1.0 if c["expired"] else 0.0])

    def test_differential(self):
        def compute(c):
            return drive.DifferentialDrive(num(c["track_width_m"]), num(c["wheel_radius_m"]), nums(c["signs"])).mix(nums(c["twist"]))

        self.check("differential", compute, lambda c: nums(c["wheels"]))

    def test_mecanum(self):
        def compute(c):
            return drive.Mecanum(num(c["track_width_m"]), num(c["wheelbase_m"]), num(c["wheel_radius_m"]), nums(c["signs"])).mix(nums(c["twist"]))

        self.check("mecanum", compute, lambda c: nums(c["wheels"]))

    def test_unmix_inverts_mix(self):
        # Not golden: the round trip on the cases that mix.
        for c in self.golden["differential"]:
            if "error" not in c:
                d = drive.DifferentialDrive(num(c["track_width_m"]), num(c["wheel_radius_m"]), nums(c["signs"]))
                back = d.unmix(d.mix(nums(c["twist"])))
                for g, w in zip(back, nums(c["twist"])):
                    self.assertAlmostEqual(g, w, delta=1e-9, msg=c["name"])
        for c in self.golden["mecanum"]:
            if "error" not in c:
                m = drive.Mecanum(num(c["track_width_m"]), num(c["wheelbase_m"]), num(c["wheel_radius_m"]), nums(c["signs"]))
                back = m.unmix(m.mix(nums(c["twist"])))
                for g, w in zip(back, nums(c["twist"])):
                    self.assertAlmostEqual(g, w, delta=1e-9, msg=c["name"])


RESOLVED = {
    "schema": "sim.drive.resolved/1", "profile": "robot.drive.json", "profile_sha256": "0" * 64, "kinematics": "differential",
    "geometry": {
        "track_width_m": {"value": 0.12, "provenance": {"kind": "derived", "from": "joints"}}, "wheelbase_m": None,
        "wheel_radius_m": {"value": 0.03, "provenance": {"kind": "derived", "from": "wheel"}},
        "wheels": [{"joint": "left axle", "sign": 1.0, "provenance": {"kind": "derived", "from": "axis"}},
                   {"joint": "right axle", "sign": 1.0, "provenance": {"kind": "derived", "from": "axis"}}]},
    "limits": {"supported": [True, False, True], "max_speed": [0.3, 0.0, 3.0], "max_accel": [0.6, 0.6, 6.0], "stop_decel": [1.2, 1.2, 12.0]},
    "deadman": {"timeout_s": 0.5, "on_loss": "ramp"},
}


class ResolvedDriveTest(unittest.TestCase):
    def test_parses_the_design_example(self):
        r = drive.ResolvedDrive.from_json(json.dumps(RESOLVED))
        self.assertEqual(r.joints, ["left axle", "right axle"])
        self.assertIsInstance(r.mixer(), drive.DifferentialDrive)
        self.assertEqual(r.limits().supported, (True, False, True))
        self.assertEqual(r.deadman(), drive.Deadman(0.5, drive.OnLoss.ramp([1.2, 1.2, 12.0])))

    def test_refusals_name_the_field(self):
        for field, mutate in [
            ("schema", lambda d: d.update(schema="sim.drive.resolved/2")),
            ("geometry.wheels", lambda d: d["geometry"]["wheels"].pop()),
            ("limits.max_speed", lambda d: d["limits"].pop("max_speed")),
            ("deadman.on_loss", lambda d: d["deadman"].update(on_loss="coast")),
            ("kinematics", lambda d: d.update(kinematics=["differential"])),
            ("limits.supported[1]", lambda d: d["limits"]["supported"].__setitem__(1, True)),
            ("limits.max_speed[0]", lambda d: d["limits"]["max_speed"].__setitem__(0, -0.3)),
            ("limits.max_speed[2]", lambda d: d["limits"]["max_speed"].__setitem__(2, float("inf"))),
            ("limits.max_accel[2]", lambda d: d["limits"]["max_accel"].__setitem__(2, 0.0)),
            ("limits.stop_decel[0]", lambda d: d["limits"]["stop_decel"].__setitem__(0, -1.2)),
        ]:
            data = json.loads(json.dumps(RESOLVED))
            mutate(data)
            with self.subTest(field=field), self.assertRaises(drive.ResolvedDriveError) as caught:
                drive.ResolvedDrive.from_json(data)
            self.assertIn(field, str(caught.exception))


class DriveStateTest(unittest.TestCase):
    def state(self, limit_live=False):
        r = drive.ResolvedDrive.from_json(RESOLVED)
        return drive.DriveState(r.limits(), r.deadman(), limit_live=limit_live)

    def test_no_request_before_first_heartbeat(self):
        s = self.state()
        twist, expired = s.update(0.0, [0.3, 0.0, 0.0], 0.0, 0.02)
        self.assertEqual((twist, expired), (drive.ZERO, False))

    def test_fresh_request_ramps_and_stale_one_expires(self):
        s = self.state(limit_live=True)
        twist, expired = s.update(0.0, [0.3, 0.0, 0.0], 1.0, 0.02)
        self.assertFalse(expired)
        self.assertAlmostEqual(twist.forward_m_s, 0.6 * 0.02, delta=1e-15)
        t = 0.0
        for _ in range(24):  # same heartbeat up to t = 0.48 s: still live
            t += 0.02
            twist, expired = s.update(t, [0.3, 0.0, 0.0], 1.0, 0.02)
            self.assertFalse(expired)
        before = twist.forward_m_s
        twist, expired = s.update(0.5, [0.3, 0.0, 0.0], 1.0, 0.02)
        self.assertTrue(expired)
        self.assertAlmostEqual(twist.forward_m_s, before - 1.2 * 0.02, delta=1e-12)
        twist, expired = s.update(0.52, [0.3, 0.0, 0.0], 2.0, 0.02)  # a new request is live again
        self.assertFalse(expired)

    def test_live_twist_passes_through(self):
        s = self.state()
        twist, expired = s.update(0.0, [0.26, 0.0, -1.5], 1.0, 0.02)
        self.assertEqual((twist, expired), (drive.BodyTwist(0.26, 0.0, -1.5), False))

    def test_limit_live_limits(self):
        s = self.state(limit_live=True)
        twist, expired = s.update(0.0, [0.26, 0.0, -1.5], 1.0, 0.02)
        self.assertFalse(expired)
        self.assertEqual(twist, drive.limit(drive.ZERO, [0.26, 0.0, -1.5], 0.02, [0.6, 0.6, 6.0]))

    def test_halt_stops_at_once(self):
        # The run thread zeroes the twist and raises the heartbeat; the controller must not ramp it.
        s = self.state()
        s.update(0.0, [0.26, 0.0, 0.0], 1.0, 0.02)
        twist, expired = s.update(0.02, [0.0, 0.0, 0.0], 2.0, 0.02)
        self.assertEqual((twist, expired), (drive.ZERO, False))

    def expires_at_timeout(self, heartbeats):
        s = self.state()
        s.update(0.0, [0.3, 0.0, 0.0], heartbeats[0], 0.02)
        t, expired = 0.0, False
        for hb in heartbeats[1:]:
            t = round(t + 0.02, 10)
            twist, expired = s.update(t, [0.3, 0.0, 0.0], hb, 0.02)
            if t < 0.5:
                self.assertFalse(expired, f"t={t}")
        self.assertTrue(expired)
        self.assertLess(twist.forward_m_s, 0.3)

    def test_nan_heartbeat_is_never_fresh(self):
        self.expires_at_timeout([1.0] + [float("nan")] * 25)

    def test_decreasing_heartbeat_is_not_fresh(self):
        self.expires_at_timeout([30.0] + [29.0 - i for i in range(25)])

    def test_nan_first_heartbeat_is_no_request(self):
        s = self.state()
        twist, expired = s.update(0.0, [0.3, 0.0, 0.0], float("nan"), 0.02)
        self.assertEqual((twist, expired), (drive.ZERO, False))
        twist, expired = s.update(0.02, [0.3, 0.0, 0.0], 1.0, 0.02)  # a real request is fresh
        self.assertEqual((twist, expired), (drive.BodyTwist(0.3, 0.0, 0.0), False))

    def test_live_request_outside_profile_is_refused(self):
        s = self.state()
        with self.assertRaises(drive.KinematicsError) as caught:
            s.update(0.0, [0.0, 0.1, 0.0], 1.0, 0.02)
        self.assertEqual(caught.exception.kind, "unsupported")


HELLO = '{"type":"hello","element":"rover","period":0.02,"sensors":[{"name":"command.forward","unit":"m/s"}],"actuators":[]}'


class LoopCheckTest(unittest.TestCase):
    def test_check_that_raises_sends_no_ready(self):
        def check(contract):
            raise ProtocolError(f"{contract.element}: the hello lacks ['command.heartbeat']")

        reader, writer, err = io.BytesIO((HELLO + "\n").encode()), io.BytesIO(), io.StringIO()
        with contextlib.redirect_stderr(err), self.assertRaisesRegex(ProtocolError, "command.heartbeat"):
            Loop(reader, writer, check=check)
        self.assertEqual(writer.getvalue(), b"")
        self.assertIn("command.heartbeat", err.getvalue())

    def test_check_that_passes_sends_ready(self):
        seen = []
        loop = Loop(io.BytesIO((HELLO + "\n").encode()), io.BytesIO(), check=seen.append)
        self.assertEqual([c.element for c in seen], ["rover"])
        self.assertEqual(loop._writer.getvalue(), b'{"type":"ready"}\n')


if __name__ == "__main__":
    unittest.main()
