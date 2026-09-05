"""The planner alone through the seam: the model-based baseline's success
rate by curriculum level, the number a learner has to beat.

    python3 examples/planc/baseline.py --envs 8 --course flat
"""

from __future__ import annotations

import argparse
import os
import sys

import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))
from simloop import Gym  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..", ".."))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--envs", type=int, default=8)
    ap.add_argument("--course", default="flat")
    ap.add_argument("--levels", default="0,0.3,0.6,1.0")
    ap.add_argument("--horizon", type=int, default=400)
    ap.add_argument("--perception-offset", type=float, default=0.0)
    args = ap.parse_args()
    gym = Gym.build(ROOT, "walk-the-plank", envs=args.envs, course=args.course, perception_offset=args.perception_offset)
    ref = [gym.priv_names.index(n) for n in ("ref.l.hip", "ref.l.knee", "ref.r.hip", "ref.r.knee")]
    succ = gym.priv_names.index("success")
    for level in [float(v) for v in args.levels.split(",")]:
        frames = gym.reset(seeds=range(1, gym.envs + 1), level=level)
        wins = 0
        alive = np.ones(gym.envs, dtype=bool)
        for _ in range(args.horizon):
            frames = gym.step(frames.priv[:, ref])
            wins += int(np.sum(alive & frames.done & (frames.priv[:, succ] > 0.5)))
            alive &= ~frames.done
            if not alive.any():
                break
        print(f"level {level:.1f}: planner alone {wins}/{gym.envs}")
    gym.close()


if __name__ == "__main__":
    main()
