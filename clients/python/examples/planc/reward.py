"""The CLF reward of "Walk the PLANC" on the seam's privileged channels.

The environment runs the LIP planner itself and reports, per frame, the
reference LIP state (`ref.p`, `ref.pdot`), the measured one (`lip.p`,
`lip.pdot`) and the Lyapunov value `clf = e^T e` on their difference.
The reward asks the learner to keep V small and decreasing — the CLF
condition V̇ + λV ≤ 0 — while making progress and staying up.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np


@dataclass
class Weights:
    alive: float = 0.2
    clf: float = 1.0
    clf_sigma: float = 0.02
    decrease: float = 0.5
    lam: float = 2.0
    progress: float = 5.0
    residual: float = 0.05
    fall: float = 10.0
    success: float = 20.0


class Reward:
    def __init__(self, priv_names, period: float, weights: Weights = Weights()):
        self.i = {n: k for k, n in enumerate(priv_names)}
        self.period = period
        self.w = weights

    def __call__(self, before, after, residual, done):
        """Per-environment reward for the step `before → after`."""
        i, w = self.i, self.w
        v0, v1 = before[:, i["clf"]], after[:, i["clf"]]
        vdot = (v1 - v0) / self.period
        condition = np.maximum(0.0, vdot + w.lam * v1)
        progress = after[:, i["torso.x"]] - before[:, i["torso.x"]]
        success = after[:, i["success"]] > 0.5
        failed = done & ~success
        r = (
            w.alive
            + w.clf * np.exp(-v1 / w.clf_sigma)
            - w.decrease * np.minimum(condition, 5.0)
            + w.progress * progress
            - w.residual * np.sum(residual * residual, axis=1)
            - w.fall * failed
            + w.success * success
        )
        return r
