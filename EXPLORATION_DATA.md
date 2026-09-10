# Exploration code integration

The code, robot definitions, controller configurations, tests, documentation and
required CI fixtures from exploration commit `d8aac182` are integrated into `main`.
This is a snapshot integration: the original experimental commit history remains
on the local `physics-gait-exploration` branch, avoiding a multi-gigabyte binary
history in the code push.

Bulk recordings, generated reports and binary archives remain in
`/Users/elliot/physics-simulator-gait-exploration` and its Git history. The exact
excluded paths and Git object IDs are listed in
[`local-exploration-archives.json`](examples/interactive/local-exploration-archives.json).
Historical evidence manifests can refer to these locally retained files. A fresh
remote checkout does not contain the complete evidence archive; the fixtures
required by the configured CI workflows are included separately.

To recover a particular archived file while the original local branch exists:

```sh
git show physics-gait-exploration:path/to/archived-file > /path/to/new-output
```

Do not remove the exploration worktree or branch until its bulk evidence has been
backed up or published to a chosen artifact store. Future code work belongs in
`/Users/elliot/physics-simulator` on `main`.
