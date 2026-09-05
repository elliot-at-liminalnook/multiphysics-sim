# Walk the PLANC on the seam

The experiments of Dai et al., *Walk the PLANC: physics-guided RL for agile
humanoid locomotion on constrained footholds*, at planar scale: a point-foot
biped on procedurally generated stepping stones and stairs, a reduced-order
LIP stepping planner supplying terrain-consistent references, and a policy
trained to refine them with a Control Lyapunov Function reward.

Build the environment server once:

    cargo build --release -p sim-phenomena --bin sim-gym

Then, from `clients/python`:

    python3 examples/planc/baseline.py --envs 8            # the planner alone, by level
    python3 examples/planc/train.py --envs 8 --iterations 300 --save planc.npz
    python3 examples/planc/train.py --play planc.npz        # success by level

`train.py` runs PPO (numpy, no framework needed) on `gym.envs` environments
held by one `sim-gym` process and stepped on parallel threads. The policy
outputs a residual on the planner's joint targets (`--residual` scales it);
the reward (`reward.py`) is the CLF condition on the LIP error — a bonus
for small `V`, a penalty for `V̇ + λV > 0` — plus progress, an alive bonus,
a fall penalty and a success bonus. Each environment's terrain level
rises 0.1 on success and falls 0.05 on a fall.

Channels (see `sim_phenomena::scenarios::walk_the_plank`):

- observation: joint angles and speeds, torso pitch, rate and velocity,
  the next three patches relative to the torso;
- privileged: torso pose, feet, foot forces, stance and phase, the
  planner's four joint references, the LIP reference and measured
  states, `clf`, steps, `success`, `failed`.

Throughput is about 50 ms per batched policy step of eight environments
on eight cores, i.e. a few hundred episodes an hour; a run of a few hundred
iterations is an afternoon, not the paper's 4096 GPU environments.
