VERIFICATION PASS, scheduled by the coordinator every {every} commits and before an epic is completed.

The {count} commits since the last pass ({since}..{head}) were written without builds or tests. This turn is the exception to the 10-second rule: build, find what's broken, and fix it. Shell commands have no 10-second cap in this pass, and background commands work again. Add no features.

**The first pass after 2026-10-02 rebuilds the whole workspace once** (the dev profile now keeps line tables only, and the old cache was cleared): expect 20 to 30 minutes for step 1, and don't clean or change profiles to speed it up.

**Builds run one at a time, from this session only.** `cargo` on your PATH queues commands so they don't fight over the build directory; never start cargo from subagents or several at once. Each kind of build (check, test, build) compiles the code differently, so don't mix them: follow this order and reuse its output.

1. **Build.** Run `cargo test --workspace --no-run --message-format short` once: it compiles every library and test target with code generation, so the test runs below start at once. Then `cargo build -p sim-spatial` (and any other binary these commits touched); it reuses those libraries. Don't add a separate `cargo check`. Run long builds in the background and read the full error output.
2. **Fix every compile error and every new warning** these commits introduced. Rerun only the builds that failed.
3. **Hunt for bugs by reading,** split across `pair-reviewer` subagents started together in one message (one per crate or area). Reviewers read; they never run cargo. Go through `git diff {since}..{head}` crate by crate, looking for:
   - wrong units or signs, off-by-one errors
   - unhandled errors, broken invariants
   - callers left on an old path
   - missing cleanup or cancellation
   - UI state that can't be reached

   Fix what you find.
4. **Run tests** for the crates whose logic changed, one command per crate, filtered where you can (`cargo test -p <crate> --lib <module::>`). They are already built by step 1. Fix the failures, then rerun only those.
6. **Commit the fixes in logical groups.** Each message says how the bug was found (compiler, test or reading) and what it broke.

5b. **Run the checklist steps these commits touch, in the freshly built viewer.**
   - For hardware steps, start `hx_virtual_bench`, then `serve_actuator_calibration` with a copy of `server.json` pointing at the pseudo-terminal it printed. Never the real device.
   - Launch the viewer as the checklist does, and run the step scripts (REST, `system_ui` for clicks; `ui_capture.py` for screenshots). Open the screenshots with Read.
   - Check the viewer's output for panics and "Encountered an error" warnings: each one is a bug to fix, including ones in the code around your work.
   - Stop every process you started.

In your report, list:
- each checklist step you ran: pass or fail, with its screenshot path
- every command, with its duration and result
- each bug found: how, where, and what it broke
- anything still broken, and why

Commits in range:
{subjects}
