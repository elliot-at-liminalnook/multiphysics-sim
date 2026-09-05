"""Planner-guided PPO on the walk-the-plank environment.

    cd clients/python
    python3 examples/planc/train.py --envs 8 --iterations 300 --save planc.npz

The policy acts as a residual on the LIP planner's joint targets (the
paper's structured references), the reward is the CLF condition on the
LIP error plus progress, and each environment's terrain level follows a
success curriculum. `--play planc.npz` runs a trained policy
deterministically and reports success by level.
"""

from __future__ import annotations

import argparse
import os
import sys
import time

import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))
from simloop import Gym  # noqa: E402

from ppo import PPO  # noqa: E402
from reward import Reward  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..", ".."))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--envs", type=int, default=8)
    ap.add_argument("--iterations", type=int, default=200)
    ap.add_argument("--steps", type=int, default=64, help="policy steps per environment per iteration")
    ap.add_argument("--horizon", type=int, default=400, help="episode length in policy steps (8 s)")
    ap.add_argument("--course", default="flat")
    ap.add_argument("--residual", type=float, default=0.25, help="scale of the policy's residual on the planner's targets (rad)")
    ap.add_argument("--level", type=float, default=0.0, help="starting curriculum level")
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--save", default="")
    ap.add_argument("--play", default="", help="policy file to run deterministically instead of training")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    rng = np.random.default_rng(args.seed)
    gym = Gym.build(ROOT, "walk-the-plank", envs=args.envs, course=args.course)
    ref_cols = [gym.priv_names.index(n) for n in ("ref.l.hip", "ref.l.knee", "ref.r.hip", "ref.r.knee")]
    reward = Reward(gym.priv_names, gym.period)
    ppo = PPO(len(gym.obs_names), len(gym.act_names), rng)
    if args.play:
        ppo.load(args.play)
        play(gym, ppo, ref_cols, args)
        return

    N = args.envs
    levels = np.full(N, args.level)
    episode_seed = np.arange(N) * 7919 + args.seed
    frames = gym.reset(seeds=episode_seed, levels=levels)
    steps_in_episode = np.zeros(N, dtype=int)
    ep_return = np.zeros(N)
    finished = []  # (return, success, level, length)
    started = time.time()
    for it in range(args.iterations):
        obs_buf, act_buf, logp_buf, rew_buf, val_buf, done_buf = [], [], [], [], [], []
        for _ in range(args.steps):
            obs = frames.obs
            a, logp, v = ppo.act(obs)
            target = frames.priv[:, ref_cols] + args.residual * a
            before = frames.priv
            nxt = gym.step(target)
            steps_in_episode += 1
            timeout = steps_in_episode >= args.horizon
            done = nxt.done | timeout
            r = reward(before, nxt.priv, args.residual * a, nxt.done)
            obs_buf.append(obs); act_buf.append(a); logp_buf.append(logp); rew_buf.append(r); val_buf.append(v); done_buf.append(done.astype(float))
            ep_return += r
            if done.any():
                reset_seeds = []
                for k in range(N):
                    if done[k]:
                        success = nxt.priv[k, gym.priv_names.index("success")] > 0.5
                        finished.append((ep_return[k], success, levels[k], steps_in_episode[k]))
                        # The curriculum: up on success, down on a fall.
                        levels[k] = float(np.clip(levels[k] + (0.1 if success else -0.05), 0.0, 1.0))
                        episode_seed[k] += 1
                        ep_return[k] = 0.0
                        steps_in_episode[k] = 0
                        reset_seeds.append(int(episode_seed[k]))
                    else:
                        reset_seeds.append(None)
                nxt = gym.reset(seeds=reset_seeds, levels=levels)
            frames = nxt
        obs_arr = np.asarray(obs_buf)
        ppo.norm.update(obs_arr.reshape(-1, obs_arr.shape[-1]))
        _, _, last_v = ppo.act(frames.obs)
        adv, ret = ppo.gae(np.asarray(rew_buf), np.asarray(val_buf), np.asarray(done_buf), last_v)
        T = args.steps
        stats = ppo.update(obs_arr.reshape(T * N, -1), np.asarray(act_buf).reshape(T * N, -1), np.asarray(logp_buf).reshape(-1), adv.reshape(-1), ret.reshape(-1))
        recent = finished[-N * 4 :]
        if not args.quiet:
            if recent:
                rets = np.array([f[0] for f in recent]); succ = np.array([f[1] for f in recent]); lens = np.array([f[3] for f in recent])
                print(f"it {it:4d} | {time.time() - started:6.0f} s | step reward {np.mean(rew_buf):6.3f} | episodes {len(finished):4d} return {rets.mean():7.2f} success {succ.mean():.2f} length {lens.mean():5.0f} | level {levels.mean():.2f} | std {np.exp(ppo.log_std).mean():.3f} kl {stats['kl']:.4f}", flush=True)
            else:
                print(f"it {it:4d} | {time.time() - started:6.0f} s | step reward {np.mean(rew_buf):6.3f} | level {levels.mean():.2f}", flush=True)
        if args.save and (it + 1) % 10 == 0:
            ppo.save(args.save)
    if args.save:
        ppo.save(args.save)
    gym.close()


def play(gym, ppo, ref_cols, args):
    for level in (0.0, 0.3, 0.6, 1.0):
        wins = 0
        for batch in range(2):
            seeds = [1000 + batch * gym.envs + k for k in range(gym.envs)]
            frames = gym.reset(seeds=seeds, level=level)
            alive = np.ones(gym.envs, dtype=bool)
            for _ in range(args.horizon):
                a, _, _ = ppo.act(frames.obs, deterministic=True)
                frames = gym.step(frames.priv[:, ref_cols] + args.residual * a)
                wins += int(np.sum(alive & frames.done & (frames.priv[:, gym.priv_names.index("success")] > 0.5)))
                alive &= ~frames.done
                if not alive.any():
                    break
        print(f"level {level:.1f}: success {wins}/{2 * gym.envs}")
    gym.close()


if __name__ == "__main__":
    main()
