# Status — where 1.2.1 stands

`1.2.1` is the current Enhanced Mode release of the *Batman: Return of the Joker* Rust/Bevy port (`1.2.0` + playtest fixes for display-scale/CRT render-target resizing, Modern sprite atlas gutters, live D-pad direction reversals, Unlimited SFX same-tick polyphony, and `batman-rotj.log` file logging). Classic Mode remains the default and stays byte-for-byte faithful to the original NES cartridge (boot through ending, all 7 stages, 5 bosses, all 100 actor types, full soundtrack, and every raster split), while Enhanced presentation, audio, and pack-loading features layer cleanly on top.

- Full release history (`0.1.0`–`1.2.1`): [CHANGELOG.md](../CHANGELOG.md)
- Current crate, module, state-machine, and boundary map: [docs/ARCHITECTURE.md](ARCHITECTURE.md)
- Roadmap and `1.3.0+` plans: [docs/PLAN.md](PLAN.md)
- `1.2.0` design specification: [docs/specs/2026-11-25-1.2.0-enhanced-presentation-design.md](specs/2026-11-25-1.2.0-enhanced-presentation-design.md)
- Historical RE lab notebooks: `docs/re/` (`ACTORS.md`, `LEVELDATA.md`, `LEVELS.md`, `MUSIC.md`, `PHYSICS.md`, `RE-NOTES.md`, `SPRITES.md`, `SUBLEVELS.md`)
- Completed `0.x`–`1.1.x` design specs & modernisation tracker: `docs/specs/archive/`

## Verified working at 1.2.1

| Area | Verification & Current State |
| --- | --- |
| Release gates | `.orchestration/release-checks.sh` green (`gates.sh`, `random_play_sweep` 0 divergences, 300-branch `sim::tests::explore` 0 divergences) and `target/tapes/the_loop_matches_the_tas_from_stage_1_to_the_ending.tape` byte-identical to `.orchestration/chain-1.1.14.tape`. |
| Bring your own ROM | Zero ROM bytes tracked (`tools/check-no-rom-data.py`). `crates/rotj-rom` validates PRG+CHR MD5 (`830111df…`) with typed `RomError`; build-time embedding (`ROTJ_ROM` or `raw/`) or runtime search (CLI arg, `ROTJ_ROM`, beside exe); ROM-less CI passes all non-ROM tests and exits 2 with a clear message when run without a ROM. |
| Full-game Classic parity | Whole-TAS lockstep (`sim::tests::the_loop_matches_the_tas_from_stage_1_to_the_ending`) replays frames 351–48026 byte-for-byte (CPU RAM, WRAM, PPU nametables/palette, all 240 scanlines across all 14 FME-7 IRQ raster chains `$D3` 0–`$0D`, and V-flag flow). Boot, intro, title, 14-password screen, attract mode, sound test, pause, cutscenes, and ending match `rotj-oracle`. |
| Audio (Classic & Unlimited SFX) | `rotj-apu` (2A03 + DMC + 90 Hz HPF / 14 kHz LPF) and `rotj-music` (typed driver) match the oracle write-for-write over the whole TAS (`sfx_voices: Classic`, default). `sfx_voices: Unlimited` isolates SFX onto a 16-voice polyphonic APU pool with oldest-voice stealing, 4-frame tail reclamation, `StopAll` silence, and a transparent soft limiter so SFX never steal music channels. |
| Modern picture, High-Refresh & CRT | `picture: Modern` draws all gameplay frames — including split-screen band rooms from the live map and bank-split boss-floor sprites (`0` Classic gameplay fallbacks, 0 differing pixels on headless GPU checks). `frame_rate` (`Classic` / `60..=360` / `Display`) adds fixed-timestep 60 Hz pacing with button latching and display-scale Modern world-space interpolation. `crt` (`Off` / `Scanlines` / `Crt`) applies an embedded WGSL `Material2d` over the game picture + Faithful UI while keeping the Settings Menu crisp on a dedicated overlay camera. |
| Faithful UI & Intro Blend | `ui: Faithful` renders UI-marked sprites via Bevy UI with steady HUD `L`/`E` labels. `intro_blend: true` smoothly blends 50/50 RGB frames strictly during the intro's 79-frame 30 Hz dual-nametable flicker window (`Session::intro_flickering()`). |
| Settings, CLI & Load Pack | Categorized NES-font settings submenus (`DISPLAY`, `GRAPHICS`, `AUDIO`, `LOAD PACK`, `BACK`) with held-key repeat, `1.1.0`-compatible `settings.ron` (with FirebrandX `COMPOSITE DIRECT FBX` credit on `DISPLAY`), `--help`/`--version`, `--pack <dir>`, and session-only `<exe_dir>/packs/` hot-swapping (`Source::User` validated, `PACK INVALID` feedback). |
| Level packs (Tiled) | Export (`rotj export-levels`), per-(area, CHR variant) tilesets (`rotj tilesets` and auto-generated `tilesets/`), custom metatiles (`custom.tsj`), `Source::User` crash-hardening validation, CLI `--pack`, and in-game `LOAD PACK` browser. |
| Disassembly & Nix dev env | `disasm/assemble.sh` rebuilds the ROM byte-identically; `flake.nix` provides a reproducible Rust `1.100.0` + Bevy `0.20.1` + tooling shell (`nix develop`). |

## Next (`1.3.0` onward)

Planned Enhanced releases (each leaving Classic Mode default and byte-exact; see [docs/PLAN.md](PLAN.md)):

- **`1.3.0` — Widescreen:** `> 256 px` view drawn from the live map playfield (building directly on `1.2.0`'s band-room `Piece::Map` foundation), widened actor spawn window, room-edge clamping, and centered 256 px HUD.
- **`1.4.0+` — Content & Modes:** Chapter select & passwords, SNES prototype music, post-game Tiled bonus levels, and verified cut-content restorations (`docs/CUTCONTENT.md`).

## Known limitations & open leads

- **Odd player energy:** only a damage-1 contact leaves an odd energy byte, which makes the original HUD gauge read PPU registers mid-render (refused by the oracle; the port reads bank-14 bytes directly; `docs/re/ACTORS.md` §9t).
- **Fixed-cycle IRQ timing & zero CPU lag:** the 14 raster IRQ chains run at measured mean cycles (`sim/raster.rs`), so jittered mid-line writes resolve to one of the two hardware states; the port takes no CPU time, so hardware lag frames and NMI-off loads take zero frames.
- **Unexercised edge paths:** `$AC16` with no free slot in types `$14`/`$84`/`$90` (`shortcut:` comments in `crates/rotj-core`), `$2002` sprite-0/overflow mid-render bits in the oracle, bank 0 `$9E3C` (variant 8 to `$AC16`), 8 unreferenced sound effects (`docs/CUTCONTENT.md` §5), and `disasm/gen/generate.py`'s `OBSERVED_WINDOW_BANKS` far-call map (`disasm/MAP.md` §11).

## Standing owner decisions

- **Classic parity is permanent:** Classic Mode stays the default and stays verified at every merge and tag (`release-checks.sh` + identical chain tape); Enhanced features layer on top (`docs/specs/archive/2026-10-15-classic-and-enhanced-modes-design.md`).
- **Core vs. ECS boundary:** `rotj-core` stays pure Rust (`Slots` owns Classic simulation state, exposed through `Session`); `crates/app` mirrors drawers read-only into Bevy ECS for Modern rendering, inspection, and future tools.
- **No ROM data in repo:** `github.com/minz1/bevy-rotj` distributes zero cartridge bytes; MIT or Apache-2.0 licensed. FirebrandX's `COMPOSITE DIRECT FBX` palette remains named wherever selectable in the menu.
- **Strict workspace hygiene:** `unsafe_code = "deny"`, `clippy::pedantic` at `-D warnings`, zero `#[allow(...)]` (only inline `#[expect(clippy::needless_pass_by_value, reason = "Bevy system params, rust-clippy#8940")]` on Bevy systems), `cargo deny check`.
