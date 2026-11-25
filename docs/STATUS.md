# Status — where 1.1.60 stands

`1.1.60` completes the `1.1.x` modernisation series. Classic Mode remains byte-for-byte faithful to the original NES cartridge (boot through ending, all 7 stages, 5 bosses, all 100 actor types, full soundtrack, and every raster split), while the codebase is hardened, cleanly modularized around `rotj_core::Session` and `rotj_core::map`, and ready for `1.2.0` Enhanced features.

- Full release history (`0.1.0`–`1.1.60`): [CHANGELOG.md](../CHANGELOG.md)
- Current crate, module, state-machine, and boundary map: [docs/ARCHITECTURE.md](ARCHITECTURE.md)
- Roadmap and `1.2.0+` plans: [docs/PLAN.md](PLAN.md)
- Historical RE lab notebooks: `docs/re/` (`ACTORS.md`, `LEVELDATA.md`, `LEVELS.md`, `MUSIC.md`, `PHYSICS.md`, `RE-NOTES.md`, `SPRITES.md`, `SUBLEVELS.md`)
- Completed `0.x`–`1.1.x` design specs & modernisation tracker: `docs/specs/archive/`

## Verified working at 1.1.60

| Area | Verification & Current State |
| --- | --- |
| Release gates | `.orchestration/release-checks.sh` green (`gates.sh`, `random_play_sweep` 0 divergences, 300-branch `sim::tests::explore` 0 divergences) and `target/tapes/the_loop_matches_the_tas_from_stage_1_to_the_ending.tape` byte-identical to `.orchestration/chain-1.1.14.tape`. |
| Bring your own ROM | Zero ROM bytes tracked (`tools/check-no-rom-data.py`). `crates/rotj-rom` validates PRG+CHR MD5 (`830111df…`) with typed `RomError`; build-time embedding (`ROTJ_ROM` or `raw/`) or runtime search (CLI arg, `ROTJ_ROM`, beside exe); ROM-less CI passes all non-ROM tests and exits 2 with a clear message when run without a ROM. |
| Full-game Classic parity | Whole-TAS lockstep (`sim::tests::the_loop_matches_the_tas_from_stage_1_to_the_ending`) replays frames 351–48026 byte-for-byte (CPU RAM, WRAM, PPU nametables/palette, all 240 scanlines across all 14 FME-7 IRQ raster chains `$D3` 0–`$0D`, and V-flag flow). Boot, intro, title, 14-password screen, attract mode, sound test, pause, cutscenes, and ending match `rotj-oracle`. |
| Audio | `rotj-apu` (2A03 + DMC + 90 Hz HPF / 14 kHz LPF) and `rotj-music` (typed driver) match the oracle write-for-write over the whole TAS, wired into `bevy_audio` with frame-0 initialization and 10 ms pause fade. |
| Modern picture & Faithful UI | `picture: Modern` draws gameplay levels and split-screen band/boss rooms via Bevy `TilemapChunk`s + mirrored ECS entities (`mirror.rs`, `modern.rs`), pixel-checked against Classic; `ui: Faithful` renders UI-marked sprites via Bevy UI with steady HUD `L`/`E` labels. |
| Settings & CLI | `settings.ron` beside the exe (`volume`, `overscan`, `palette` with FirebrandX `COMPOSITE DIRECT FBX`, `sprite_limit`, `vsync`, `ui`, `picture`), `--help`/`--version`, `--pack <dir>` (or dropped pack folder/`pack.json`), unknown-flag rejection, flushed startup warnings, and `SAVE FAILED` menu feedback. |
| Level packs (Tiled) | Export (`rotj export-levels`), per-(area, CHR variant) tilesets (`rotj tilesets` and auto-generated `tilesets/`), custom metatiles (`custom.tsj`), `Source::User` crash-hardening validation, and `--pack` playback. |
| Disassembly & Nix dev env | `disasm/assemble.sh` rebuilds the ROM byte-identically; `flake.nix` provides a reproducible Rust `1.100.0` + Bevy `0.20.1` + tooling shell (`nix develop`). |

## Next (`1.2.0` onward)

`1.1.x` is closed. `docs/specs/` is clean for `1.2.0` design specs. Planned Enhanced features (each leaving Classic Mode default and byte-exact; see [docs/PLAN.md](PLAN.md)):

- 1.2.0 prerequisites on record: band-room playfields from the map (`docs/specs/archive/2026-10-30-modern-band-rooms-design.md` "Not here"), plus a "Load pack" menu entry alongside future editor/overworld work.
- Enhanced presentation & audio: interpolated motion + `frame_rate`, CRT filters, widescreen, unlimited-voice SFX, remastered / prototype music, chapter select, bonus levels, and cut-content restorations (`docs/CUTCONTENT.md`).

## Known limitations & open leads

- **Odd player energy:** only a damage-1 contact leaves an odd energy byte, which makes the original HUD gauge read PPU registers mid-render (refused by the oracle; the port reads bank-14 bytes directly; `docs/re/ACTORS.md` §9t).
- **Fixed-cycle IRQ timing & zero CPU lag:** the 14 raster IRQ chains run at measured mean cycles (`sim/raster.rs`), so jittered mid-line writes resolve to one of the two hardware states; the port takes no CPU time, so hardware lag frames and NMI-off loads take zero frames.
- **Unexercised edge paths:** `$AC16` with no free slot in types `$14`/`$84`/`$90` (`shortcut:` comments in `crates/rotj-core`), `$2002` sprite-0/overflow mid-render bits in the oracle, bank 0 `$9E3C` (variant 8 to `$AC16`), 8 unreferenced sound effects (`docs/CUTCONTENT.md` §5), and `disasm/gen/generate.py`'s `OBSERVED_WINDOW_BANKS` far-call map (`disasm/MAP.md` §11).

## Standing owner decisions

- **Classic parity is permanent:** Classic Mode stays the default and stays verified at every merge and tag (`release-checks.sh` + identical chain tape); Enhanced features layer on top (`docs/specs/archive/2026-10-15-classic-and-enhanced-modes-design.md`).
- **Core vs. ECS boundary:** `rotj-core` stays pure Rust (`Slots` owns Classic simulation state, exposed through `Session`); `crates/app` mirrors drawers read-only into Bevy ECS for Modern rendering, inspection, and future tools.
- **No ROM data in repo:** `github.com/minz1/bevy-rotj` distributes zero cartridge bytes; MIT or Apache-2.0 licensed. FirebrandX's `COMPOSITE DIRECT FBX` palette remains named wherever selectable in the menu.
- **Strict workspace hygiene:** `unsafe_code = "deny"`, `clippy::pedantic` at `-D warnings`, zero `#[allow(...)]` (only inline `#[expect(clippy::needless_pass_by_value, reason = "Bevy system params, rust-clippy#8940")]` on Bevy systems), `cargo deny check`.
