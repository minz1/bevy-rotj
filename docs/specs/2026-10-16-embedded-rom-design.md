# The ROM embedded at build time (1.0.3)

Status: approved 2026-10-16 by a stand-in reviewer under the owner's delegation (approve
with changes; all six applied: empty files instead of a cfg, an absolute `ROTJ_ROM`, `raw/`
watched only when it exists, `build.rs`'s crate doc, the boot check led by the exe's own
exit, the stale `Rom::load` references). The owner's call is in `docs/STATUS.md` "Owner
decisions made". Implemented in 1.0.3.

## Goal

A self-contained exe: the app's build script reads the user's ROM, checks it and compiles
it in. 1.0.1's start-up search (argument, `ROTJ_ROM`, cache, beside the exe, `raw/`) and
its cache go. The exe built this way contains the game and is never shared; anyone else
builds their own with their own ROM.

## Design

1. `rotj_rom::locate() -> Result<Option<(PathBuf, Rom)>, String>`: `ROTJ_ROM` (a non-empty
   value, which must be an absolute path: the build script, each test crate and the
   user's shell run in different directories) if set, else the workspace's `raw/Batman - Return of the Joker (USA).nes` if it
   exists, else `Ok(None)`. A file that is named or present but missing, unreadable or not
   the game (PRG+CHR MD5 `830111df…`) is `Err` naming the file. `for_test()` and the build
   script both use it, so tests and the exe pick the same ROM by the same rule.
2. `crates/app/build.rs` (`rotj-rom` as a `[build-dependencies]`, std-only):
   - always: `cargo::rerun-if-env-changed=ROTJ_ROM`, `cargo::rerun-if-changed=build.rs`;
   - `Err`: panic with the message, so the build fails (a named but wrong `ROTJ_ROM`, or a
     wrong `raw/` file, never builds a ROM-less exe);
   - `Ok(Some)`: write PRG and CHR to `OUT_DIR/prg.bin` and `chr.bin`,
     `cargo::rerun-if-changed=<path>`;
   - `Ok(None)`: write both files empty, `cargo::warning=built without a ROM ...`, and
     watch `raw/` if the directory exists (a missing path would make cargo rerun the
     script on every build); otherwise adding the ROM needs `touch crates/app/build.rs`.
3. `crates/app/src/main.rs` always `include_bytes!` both files: one code path, linted
   with and without a ROM. Empty PRG means no ROM: it prints "built without a ROM: set
   ROTJ_ROM or put it in raw/ and rebuild" (and, on Windows, waits for Enter as 1.0.1
   did) and exits 2. The app ignores its arguments.
4. Deleted: `Rom::load`, `search`, `Source`, `cache_dir`, `remember`, `rom_or_exit` and
   their tests (search order, bad cache, remember). Kept: `parse_ines`, `from_ines`,
   `for_test`, the parse/MD5 tests; new tests for `locate`'s rule (the inner function takes
   the env value and the raw path).
5. `tools/boot-check.sh` runs the exe once and goes by what the build made: status 2
   with the "built without a ROM" message passes (fails under `ROTJ_REQUIRE_ROM`);
   anything else must be the game still running when killed. A failed build prints its
   tail. The ROM-less exe is exercised by CI's boot job (no ROM) and by the release's
   fresh-clone gate; the old `ROTJ_ROM=/nonexistent` runtime step goes (the runtime has no
   ROM search to fail). The CI boot job's comment says so.
6. Docs: README ("You need your own ROM": build with it; the exe contains the game, don't
   share it), STATUS (1.0.3 row, decisions), PLAN (1.0.3 row), HANDOFF.md, and the BYOR
   spec's status (section 3's start-up search superseded by this spec).

## Checks

- With the ROM: `.orchestration/gates.sh` green.
- A fresh clone without a ROM: `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` and `cargo build -p batman-rotj` pass; its exe prints the
  message and exits 2.
- `ROTJ_ROM=/nonexistent cargo build -p batman-rotj` fails, naming the file.
- The Windows exe boots from the embedded ROM with the old cache moved aside.

## Not done

A first-run importer (a shareable exe) stays possible later (1.1's "ROM decoder becomes an
importer"). The CI build job's artifact stays a ROM-less exe.
