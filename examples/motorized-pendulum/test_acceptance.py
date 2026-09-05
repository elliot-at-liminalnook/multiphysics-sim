"""Corrupt real benchmark artifacts to prove that the acceptance gate fails.

Run run.py first. PENDULUM_RESULTS selects a non-default artifact directory.
No extra CAD exports or simulations are needed for these tests.
"""
import copy
import json
import os
from pathlib import Path

import pytest
from run import HERE, ROOT, read, verify


@pytest.fixture
def artifacts():
    output = Path(os.environ.get("PENDULUM_RESULTS", ROOT / "runs/motorized-pendulum"))
    if not (output / "acceptance.json").is_file():
        pytest.fail("run examples/motorized-pendulum/run.py before these artifact regression tests")
    report = read(output / "acceptance.json")
    names = ["nominal", "repeat", "half_step", "hold"]
    return [read(output / "pendulum.simrobot.json"), read(output / "reference.json"),
            {n: read(output / (n + ".simresult.json")) for n in names},
            {n: [json.loads(s) for s in (output / (n + ".controller.jsonl")).read_text().splitlines()] for n in names},
            read(HERE / "expectations.json"), report["elapsed_s"]]


def failed(data):
    checks = verify(*data)
    # Failure reports must remain writable even with non-finite input.
    json.dumps(checks, allow_nan=False)
    return [c["name"] for c in checks if not c["passed"]]


def test_original_artifacts_pass(artifacts):
    assert not failed(artifacts)


def test_wrong_cad_units_fail(artifacts):
    bar = next(l for l in artifacts[0]["links"] if l["name"] == "pendulum")
    bar["inertia"][1][1] *= 1e6  # mm² accidentally used instead of m²
    assert "CAD inertia relative error" in failed(artifacts)


def test_frozen_plant_fails(artifacts):
    artifacts[2]["nominal"]["trace"]["joints"]["pivot"] = [0.0] * 320
    assert "nominal settled tracking at 0.3 rad" in failed(artifacts)


def test_unpowered_motor_fails(artifacts):
    artifacts[2]["nominal"]["trace"]["motors"]["servo"]["current"] = [0.0] * 320
    assert "nominal motor energized" in failed(artifacts)


def test_missing_torque_balance_fails(artifacts):
    artifacts[2]["nominal"]["trace"]["motors"]["servo"]["torque_nm"] = [0.0] * 320
    assert "nominal mean static torque at 0.3 rad" in failed(artifacts)


def test_repeat_drift_fails(artifacts):
    artifacts[2]["repeat"]["trace"]["joints"]["pivot"][100] += 0.0001
    assert "repeat angle agreement" in failed(artifacts)


def test_fake_falsifier_fails(artifacts):
    artifacts[2]["hold"]["trace"] = copy.deepcopy(artifacts[2]["nominal"]["trace"])
    assert "hold fails the tracking criterion" in failed(artifacts)


@pytest.mark.parametrize("kind", ["nan", "inf"])
def test_nonfinite_sample_fails(artifacts, kind):
    artifacts[2]["nominal"]["trace"]["joints"]["pivot"][100] = float(kind)
    assert "nominal finite results" in failed(artifacts)


def test_controller_cannot_skip_a_sample(artifacts):
    artifacts[3]["nominal"].pop(30)
    assert "nominal controller sequence" in failed(artifacts)


def test_controller_cannot_send_wrong_reference(artifacts):
    artifacts[3]["nominal"][30]["target_rad"] = 0.0
    assert "nominal controller reference" in failed(artifacts)


def test_performance_regression_fails(artifacts):
    artifacts[5]["nominal"] = artifacts[4]["run_budget_s"] + 1
    assert "nominal wall budget" in failed(artifacts)


def test_truncated_result_fails(artifacts):
    artifacts[2]["half_step"]["trace"]["t"].pop()
    with pytest.raises(ValueError, match="different lengths"):
        verify(*artifacts)


@pytest.fixture
def cli():
    output = Path(os.environ.get("PENDULUM_RESULTS", ROOT / "runs/motorized-pendulum"))
    report = read(output / "acceptance.json")
    return report["commands"][0][0], output / "pendulum.simrobot.json"


def test_invalid_model_does_not_start_controller(cli, tmp_path):
    import subprocess
    import sys
    sim, _ = cli
    model = tmp_path / "invalid.simrobot.json"
    model.write_text('{"version":3,"links":[]}')
    marker = tmp_path / "started"
    script = tmp_path / "controller with spaces.py"
    script.write_text("from pathlib import Path\nPath(" + repr(str(marker)) + ").touch()\n")
    result = subprocess.run([sim, "run", str(model), "--controller", str(script), "--python", sys.executable],
                            capture_output=True, text=True, timeout=10)
    assert result.returncode != 0
    assert not marker.exists(), "controller started before validating the plant"


def test_controller_failure_fails_simulator(cli, tmp_path):
    import subprocess
    import sys
    sim, model = cli
    script = tmp_path / "failed.py"
    script.write_text("raise SystemExit(7)\n")
    result = subprocess.run([sim, "run", str(model), "--seconds", "0.02", "--no-flex", "--no-contact",
                             "--controller", str(script), "--python", sys.executable,
                             "--out", str(tmp_path / "failed.simresult.json")],
                            capture_output=True, text=True, timeout=10)
    assert result.returncode != 0
    assert "controller" in result.stderr.lower()


def test_nonpositive_step_is_rejected(cli, tmp_path):
    import subprocess
    sim, model = cli
    result = subprocess.run([sim, "run", str(model), "--step", "0", "--out", str(tmp_path / "bad.json")],
                            capture_output=True, text=True, timeout=10)
    assert result.returncode != 0
    assert "positive and finite" in result.stderr
