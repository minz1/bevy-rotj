# Autopilot rules: batmanROTJ

## Checks
Run from `/home/minz1/projects/batmanROTJ` or the item's worktree. Check `$?` of every command; never `cmd | tail && git commit`. Before a long run: `df -h /` and `df -h /mnt/c`; under 50 GB free on `/`, `rm -rf target/debug` in the main checkout and recheck; still low: exit STOP.
- Item: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the tests of the crates it touches (`cargo test -p <crate>`). A bug fix adds a test that fails without the fix.
- Release or batch: `bash .orchestration/fullset.sh <worktree> <outdir>` (fmt, ignored suites, workspace, sweep, explore; summary in `<outdir>/summary`), then `cmp <worktree>/target/tapes/the_loop_matches_the_tas_from_stage_1_to_the_ending.tape .orchestration/chain-1.1.14.tape`. It outlives the session only when detached: `setsid -f bash .orchestration/fullset.sh <worktree> <outdir> </dev/null >"<outdir>.log" 2>&1`, record it in the Handoff, exit `WAIT 60`. Running: `pgrep -f "fullset.sh <worktree>"` matches. Done: no match and `<outdir>/summary` has an `explore exit` line. Never `run_in_background` or a bare `&`. Use `/home/minz1/projects/rotj-map` as the worktree (`git switch --detach <commit>` first).
- Final item: `bash .orchestration/release-checks.sh` on main HEAD (gates, sweep, explore). Green is `GATES GREEN at <commit>` in `.orchestration/rel/summary`, `sweep ... diverges 0`, `explore ... 0 divergences`.

## May happen mid-run
Only after the checks above pass on the exact commit, one step at a time:
- Release: in `/home/minz1/projects/rotj-rel` (worktree on `next`) cherry-pick the branch's commits, update STATUS (title, a row, Next paragraph), PLAN's status line, MODERNIZE's item and the spec's "Implemented in 1.1.N"; `sed -i 's/version = "1\.1\.M"/version = "1.1.N"/' Cargo.toml crates/*/Cargo.toml tools/*/Cargo.toml && cargo update -q -w --offline`; commit `Release 1.1.N`.
- In the main checkout: `git merge --ff-only next`, `bash .orchestration/release-checks.sh`, `git tag -a 1.1.N -m "1.1.N: <what>"`, `git push --no-tags origin main`, `git push origin refs/tags/1.1.N`, then `gh run list --workflow release.yml --limit 3` until it finishes.
- Rebuild the Windows exe before fast-forwarding main to the next release: `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc CARGO_TARGET_DIR=target/win cargo build --release -p batman-rotj --target x86_64-pc-windows-gnu`, then `x86_64-w64-mingw32-strip -o /mnt/c/Users/emery/Documents/batman-rotj/batman-rotj.exe target/win/x86_64-pc-windows-gnu/release/batman-rotj.exe`.
- Commit subjects are one line, no Co-Authored-By trailer. Batch small same-kind changes into one release; keep risky ones separate.
Nothing else leaves the machine.

## Reviewer stand-in
- Every spec and design decision: a `general-purpose` agent with `model: opus`; apply its changes and note them in the spec's Status line.
- Every implementation: the `reviewer` agent; fix its findings before release.
- At most 2 Opus agents at once. Tell subagents not to run the heavy suites and to check `$?`. Opus for risky equivalence work, Sonnet for constant substitution. Delete ROM-derived listings a subagent writes to its scratchpad.
- Decide everything you can with the stand-in; if it is critical, park it in the Parking lot for a later patch.

## Fleet
- Launch: `KIT_CONFIG=opencode-go.json MODEL=opencode-go/deepseek-v4.1-flash BASE=<branch> bash .orchestration/launch.sh <task>...` (brief `.orchestration/briefs/<task>.md`, worktree `/home/minz1/projects/batmanROTJ-wt/<task>`, branch `agent/<task>`). It returns at once. Main must be clean. `fleet-ok.sh` caps at 6 agents; "fleet full" means wait, not fail.
- Log: `.orchestration/logs/agent-<task>.log`. The log is for humans. Running: `pgrep -f -- "title rotj-<task>( |\$)"` matches. Otherwise read `.orchestration/logs/agent-<task>.exit`: it holds the exit code (0 done, 124 hit the 3h timeout, anything else failed; a non-zero code is a failed attempt). If pgrep is empty and the file is absent, wait 5 s and repeat both once; still neither means the agent died (a failed attempt). Result: `/home/minz1/projects/batmanROTJ-wt/<task>/REPORT.md`.
- Continue an agent: `bash .orchestration/resume.sh <task> <message-file>` (moves the old log and `.exit` to `.<n>.log`/`.<n>.exit`); restart a stopped one in its existing worktree: `bash .orchestration/adopt.sh <task>`. Both refuse while the task is running. Merge a finished one: review its diff with `reviewer`, then `bash .orchestration/merge.sh <task> '<subject>'`, then `bash .orchestration/retire.sh <task>`.
- Record every running agent with its log in the Handoff and exit `WAIT`; do not poll.

## Stop list
Exit `STOP <reason>` and park the item for:
- Enhanced features (1.2.0).
- Re-recording a tape, or any change that cannot keep the tapes unchanged.
- New public names beyond the approved ones, or a change to the approved level-format schema.
- Level packs release 3 (waits on "Owner decisions pending" in `docs/STATUS.md`).
- Destructive or hard-to-reverse steps: deleting a branch or worktree with unmerged work, `git push --force`, `git reset --hard` on shared branches.
- Under 50 GB free on `/` after cleaning `target/debug`; a red gate that two attempts did not fix.

## Where to look for more work
Mandate: keep releasing 1.1.x patches until the codebase is modernised, except what is strictly needed to prove Classic (the oracle and its harness, the RAM/WRAM projection and lift, the tapes, Classic's named rules).
- `.orchestration/survey2.md`: ranked survey items (first is child and shot spawning written once).
- `docs/MODERNIZE.md`, section "The owner's review of the passed-on candidates (2026-10-18)": the owner's review list.
- `docs/STATUS.md`: "Next, in order" and "Leads not yet verified".
- `HANDOFF.md`, "Next, in order" item 2: the small items already found.
