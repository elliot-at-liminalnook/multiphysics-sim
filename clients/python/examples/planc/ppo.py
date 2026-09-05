"""A small PPO in numpy: Gaussian policy and value MLPs, GAE, clipped
objective, Adam. Enough to train the residual policy on a few environments;
swap in a framework for anything bigger."""

from __future__ import annotations

import numpy as np


class MLP:
    """Two tanh hidden layers, linear output, with a hand-written backward."""

    def __init__(self, sizes, rng, out_scale=1.0):
        self.params = []
        for k in range(len(sizes) - 1):
            scale = np.sqrt(2.0 / sizes[k]) * (out_scale if k == len(sizes) - 2 else 1.0)
            self.params.append(rng.normal(0.0, scale, size=(sizes[k], sizes[k + 1])))
            self.params.append(np.zeros(sizes[k + 1]))
        self.adam = [np.zeros_like(p) for p in self.params], [np.zeros_like(p) for p in self.params]
        self.t = 0

    def forward(self, x):
        cache = [x]
        h = x
        n = len(self.params) // 2
        for k in range(n):
            z = h @ self.params[2 * k] + self.params[2 * k + 1]
            h = np.tanh(z) if k < n - 1 else z
            cache.append(h)
        return h, cache

    def backward(self, cache, grad_out):
        """Gradients of sum(out * grad_out) with respect to the parameters."""
        grads = [None] * len(self.params)
        n = len(self.params) // 2
        g = grad_out
        for k in reversed(range(n)):
            h_in = cache[k]
            if k < n - 1:
                g = g * (1.0 - cache[k + 1] ** 2)
            grads[2 * k] = h_in.T @ g
            grads[2 * k + 1] = g.sum(axis=0)
            g = g @ self.params[2 * k].T
        return grads

    def step(self, grads, lr, beta1=0.9, beta2=0.999, eps=1e-8, clip=0.5):
        norm = np.sqrt(sum(float((g * g).sum()) for g in grads))
        if norm > clip:
            grads = [g * (clip / norm) for g in grads]
        self.t += 1
        m, v = self.adam
        for k, (p, g) in enumerate(zip(self.params, grads)):
            m[k] = beta1 * m[k] + (1 - beta1) * g
            v[k] = beta2 * v[k] + (1 - beta2) * g * g
            mh = m[k] / (1 - beta1**self.t)
            vh = v[k] / (1 - beta2**self.t)
            p -= lr * mh / (np.sqrt(vh) + eps)


class Normaliser:
    def __init__(self, dim):
        self.n = 1e-4
        self.mean = np.zeros(dim)
        self.var = np.ones(dim)

    def update(self, x):
        b = len(x)
        bm, bv = x.mean(axis=0), x.var(axis=0)
        delta = bm - self.mean
        total = self.n + b
        self.mean = self.mean + delta * b / total
        self.var = (self.var * self.n + bv * b + delta**2 * self.n * b / total) / total
        self.n = total

    def __call__(self, x):
        return np.clip((x - self.mean) / np.sqrt(self.var + 1e-8), -10.0, 10.0)


class PPO:
    def __init__(self, obs_dim, act_dim, rng, hidden=(64, 64), lr=3e-4, clip=0.2, epochs=4, minibatches=4, gamma=0.99, lam=0.95, entropy=0.0, init_std=0.1):
        self.policy = MLP((obs_dim, *hidden, act_dim), rng, out_scale=0.01)
        self.value = MLP((obs_dim, *hidden, 1), rng)
        self.log_std = np.full(act_dim, np.log(init_std))
        self.log_std_adam = (np.zeros(act_dim), np.zeros(act_dim), 0)
        self.norm = Normaliser(obs_dim)
        self.rng = rng
        self.lr, self.clip, self.epochs, self.minibatches, self.gamma, self.lam, self.entropy = lr, clip, epochs, minibatches, gamma, lam, entropy

    def act(self, obs, deterministic=False):
        x = self.norm(obs)
        mean, _ = self.policy.forward(x)
        std = np.exp(self.log_std)
        a = mean if deterministic else mean + std * self.rng.normal(size=mean.shape)
        logp = self.log_prob(mean, a)
        v, _ = self.value.forward(x)
        return a, logp, v[:, 0]

    def log_prob(self, mean, a):
        std = np.exp(self.log_std)
        z = (a - mean) / std
        return -0.5 * np.sum(z * z, axis=1) - np.sum(self.log_std) - 0.5 * a.shape[1] * np.log(2 * np.pi)

    def gae(self, rewards, values, dones, last_value):
        T, N = rewards.shape
        adv = np.zeros((T, N))
        last = np.zeros(N)
        for t in reversed(range(T)):
            next_value = last_value if t == T - 1 else values[t + 1]
            nonterminal = 1.0 - dones[t]
            delta = rewards[t] + self.gamma * next_value * nonterminal - values[t]
            last = delta + self.gamma * self.lam * nonterminal * last
            adv[t] = last
        return adv, adv + values

    def update(self, obs, actions, logp_old, adv, returns):
        B = len(obs)
        x = self.norm(obs)
        adv = (adv - adv.mean()) / (adv.std() + 1e-8)
        stats = {"policy_loss": 0.0, "value_loss": 0.0, "kl": 0.0, "clipfrac": 0.0}
        count = 0
        for _ in range(self.epochs):
            order = self.rng.permutation(B)
            for start in range(0, B, max(1, B // self.minibatches)):
                idx = order[start : start + max(1, B // self.minibatches)]
                xb, ab, lpb, advb, retb = x[idx], actions[idx], logp_old[idx], adv[idx], returns[idx]
                mean, cache = self.policy.forward(xb)
                std = np.exp(self.log_std)
                z = (ab - mean) / std
                logp = -0.5 * np.sum(z * z, axis=1) - np.sum(self.log_std) - 0.5 * ab.shape[1] * np.log(2 * np.pi)
                ratio = np.exp(logp - lpb)
                clipped = np.clip(ratio, 1 - self.clip, 1 + self.clip)
                use_clipped = (clipped * advb < ratio * advb)
                # d(loss)/d(logp): loss = -mean(min(ratio·A, clipped·A))
                dl_dlogp = np.where(use_clipped, 0.0, -ratio * advb) / len(idx)
                dlogp_dmean = z / std  # d logp / d mean
                grad_mean = dl_dlogp[:, None] * dlogp_dmean
                grads = self.policy.backward(cache, grad_mean)
                self.policy.step(grads, self.lr)
                # log_std: d logp / d log_std = z² − 1; entropy bonus pushes it up.
                grad_log_std = (dl_dlogp[:, None] * (z * z - 1.0)).sum(axis=0) - self.entropy
                m, v, t = self.log_std_adam
                t += 1
                m = 0.9 * m + 0.1 * grad_log_std
                v = 0.999 * v + 0.001 * grad_log_std**2
                self.log_std -= self.lr * (m / (1 - 0.9**t)) / (np.sqrt(v / (1 - 0.999**t)) + 1e-8)
                self.log_std = np.clip(self.log_std, np.log(0.02), np.log(1.0))
                self.log_std_adam = (m, v, t)
                # value
                val, vcache = self.value.forward(xb)
                err = val[:, 0] - retb
                vgrads = self.value.backward(vcache, (2.0 * err / len(idx))[:, None])
                self.value.step(vgrads, self.lr)
                stats["policy_loss"] += float(-np.mean(np.minimum(ratio * advb, clipped * advb)))
                stats["value_loss"] += float(np.mean(err * err))
                stats["kl"] += float(np.mean(lpb - logp))
                stats["clipfrac"] += float(np.mean(np.abs(ratio - 1.0) > self.clip))
                count += 1
        return {k: v / max(count, 1) for k, v in stats.items()}

    def save(self, path):
        np.savez(path, log_std=self.log_std, norm_mean=self.norm.mean, norm_var=self.norm.var, norm_n=self.norm.n, *self.policy.params, **{f"value{k}": p for k, p in enumerate(self.value.params)})

    def load(self, path):
        d = np.load(path)
        self.log_std = d["log_std"]
        self.norm.mean, self.norm.var, self.norm.n = d["norm_mean"], d["norm_var"], float(d["norm_n"])
        for k in range(len(self.policy.params)):
            self.policy.params[k] = d[f"arr_{k}"]
        for k in range(len(self.value.params)):
            self.value.params[k] = d[f"value{k}"]
