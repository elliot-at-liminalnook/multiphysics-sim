"""Environment mode of the seam: the learner is the master.

    from simloop import Gym
    gym = Gym.spawn(["target/release/sim-gym", "--task", "walk-the-plank", "--envs", "8"])
    frames = gym.reset(seeds=range(8), level=0.0)
    while True:
        frames = gym.step(actions)          # one policy period each
        ...

Every request is batched over the environments the server holds; the
server steps them on parallel threads. See `sim_couple::environment`
for the protocol.
"""

from __future__ import annotations

import json
import os
import subprocess
from typing import Any, Dict, Iterable, List, Optional, Sequence

import numpy as np


class GymError(Exception):
    pass


class Frames:
    """A batch of frames: arrays shaped `(envs, ...)`."""

    def __init__(self, reply: Dict[str, Any]):
        self.obs = np.asarray(reply["obs"], dtype=np.float64)
        self.priv = np.asarray(reply["priv"], dtype=np.float64)
        self.t = np.asarray(reply["t"], dtype=np.float64)
        self.done = np.asarray(reply["done"], dtype=bool)
        terrain = reply.get("terrain")
        self.terrain: List[Optional[np.ndarray]] = [None if p is None else np.asarray(p, dtype=np.float64) for p in terrain] if terrain else [None] * len(self.t)

    def __len__(self) -> int:
        return len(self.t)


class Gym:
    def __init__(self, process: subprocess.Popen):
        self._process = process
        self._reader = process.stdout
        self._writer = process.stdin
        hello = self._read()
        if "hello" not in hello:
            raise GymError(f"expected hello, got {hello}")
        h = hello["hello"]
        self.envs: int = h["envs"]
        self.period: float = h["period"]
        self.obs_names: List[str] = h["obs"]
        self.priv_names: List[str] = h["priv"]
        self.act_names: List[str] = h["act"]

    @classmethod
    def spawn(cls, command: Sequence[str], cwd: Optional[str] = None) -> "Gym":
        process = subprocess.Popen(list(command), stdin=subprocess.PIPE, stdout=subprocess.PIPE, cwd=cwd, text=True, bufsize=1)
        return cls(process)

    @classmethod
    def build(cls, root: str, task: str, envs: int = 1, **flags: Any) -> "Gym":
        """Spawn `target/release/sim-gym` from a checkout root."""
        exe = os.path.join(root, "target", "release", "sim-gym")
        if not os.path.exists(exe):
            raise GymError(f"{exe} not built: cargo build --release -p sim-phenomena --bin sim-gym")
        command = [exe, "--task", task, "--envs", str(envs)]
        for name, value in flags.items():
            command += [f"--{name.replace('_', '-')}", str(value)]
        return cls.spawn(command, cwd=root)

    # -- protocol --------------------------------------------------------
    def _write(self, request: Dict[str, Any]) -> None:
        self._writer.write(json.dumps(request) + "\n")
        self._writer.flush()

    def _read(self) -> Dict[str, Any]:
        line = self._reader.readline()
        if not line:
            raise GymError("environment server exited")
        reply = json.loads(line)
        if "error" in reply:
            raise GymError(reply["error"])
        return reply

    def reset(self, seeds: Optional[Iterable[Optional[int]]] = None, level: float = 0.0, levels: Optional[Sequence[float]] = None) -> Frames:
        """Reset the environments whose seed is not None (all, by default)."""
        seeds = list(range(self.envs)) if seeds is None else list(seeds)
        request = []
        for k, seed in enumerate(seeds):
            lvl = levels[k] if levels is not None else level
            request.append(None if seed is None else {"seed": int(seed), "level": float(lvl)})
        self._write({"reset": request})
        return Frames(self._read())

    def step(self, actions: np.ndarray) -> Frames:
        actions = np.asarray(actions, dtype=np.float64)
        self._write({"step": actions.tolist()})
        return Frames(self._read())

    def snapshot(self) -> List[List[float]]:
        self._write({"snapshot": None})
        return self._read()["snapshot"]

    def restore(self, snapshots: Sequence[Sequence[float]]) -> Frames:
        self._write({"restore": [list(map(float, s)) for s in snapshots]})
        return Frames(self._read())

    def close(self) -> None:
        try:
            self._write({"close": None})
        except Exception:
            pass
        self._process.wait(timeout=5)

    def __enter__(self) -> "Gym":
        return self

    def __exit__(self, *_: Any) -> None:
        self.close()
