VERIFICATION PASS, scheduled by the coordinator every {every} commits and before an epic is completed.

The {count} commits since the last pass ({since}..{head}) were written without builds or tests. This turn is the exception to the 10-second rule: build, find what's broken, and fix it. No time limit applies to the commands below. Add no features.

1. **Build.** Run `cargo check --workspace --all-targets` once; it compiles everything, including tests, without code generation. Then `cargo build` the binaries these commits touched (usually `-p sim-spatial`, sometimes `-p sim-app`). Run long builds in the background and read the full error output.
2. **Fix every compile error and every new warning** these commits introduced. Rerun only the builds that failed.
3. **Hunt for bugs by reading.** Go through `git diff {since}..{head}` crate by crate, looking for:
   - wrong units or signs, off-by-one errors
   - unhandled errors, broken invariants
   - callers left on an old path
   - missing cleanup or cancellation
   - UI state that can't be reached

   Fix what you find.
4. **Run tests once** for the crates whose logic changed substantively, filtered to the relevant modules (`cargo test -p <crate> --lib <module::>`). Fix the failures.
5. **Capture each viewer mode these commits changed** once with ui_capture, and look at the images. Fix what's visibly wrong.
6. **Commit the fixes in logical groups.** Each message says how the bug was found (compiler, test, capture or reading) and what it broke.

In your report, list:
- every command, with its duration and result
- each bug found: how, where, and what it broke
- anything still broken, and why

Commits in range:
{subjects}
