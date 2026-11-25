# Architecture

This document describes the structure of the *Batman: Return of the Joker* Rust/Bevy port as of `1.1.60`. All crates deny `unsafe_code`, compile cleanly under `clippy::pedantic` at `-D warnings`, and keep Classic Mode byte-for-byte faithful to the original NES cartridge.

## Workspace Crate Map

| Crate / Tool | Role |
| --- | --- |
| `crates/rotj-rom` | Locates and validates the user's ROM (`Rom::load`, `locate`, `InesHeader`, PRG+CHR MD5 check `830111df…`, typed `RomError`). Zero ROM bytes are tracked in the repo. |
| `crates/rotj-core` | Pure-Rust, Bevy-free simulation, map/terrain/room data, Tiled level-pack exporter/importer, NES software PPU renderer, palettes, and FNV-1a digest helpers. |
| `crates/rotj-apu` | Safe-Rust NES 2A03 APU synthesizer (pulse ×2, triangle, noise, DMC, frame counter, length counters, mixer, and `ApuStream` with 90 Hz HPF / 14 kHz LPF). |
| `crates/rotj-music` | Ported Sunsoft music driver (`Music`, typed `Request`/`ActiveFade`/`VoiceState`/`EnvelopeStep`, bytecode command decoder). Verified write-for-write against the oracle over the entire TAS. |
| `crates/app` | Bevy 0.20 host application (`batman-rotj`): windowing, 60 Hz pacing, keyboard/gamepad input, Classic & Modern renderers, faithful Bevy UI overlay, settings menu, audio output, and ECS mirror. |
| `tools/rotj-oracle` | Cycle/scanline-timed 6502 + FME-7 + PPU + DMC emulator used exclusively by verification tests to lockstep-check `rotj-core` and `rotj-music`. |
| `tools/rotj-tools` | `rotj` CLI for offline inspection, ROM dumps (`header`, `chr`, `level`, `stage`, `sublevels`, `actors`, `music`, `dmc`), and level-pack / tileset export. |
| `tools/rotj-verify` | Headless renderer comparison tool against emulator dumps. |
| `disasm/` | Reproducible ca65/ld65 disassembly (`disasm/assemble.sh` rebuilds the ROM byte-identically from the user's copy). |

## `rotj-core` Module Structure & Host Boundary

`rotj-core` exposes a narrow public facade from `crates/rotj-core/src/lib.rs` (`Buttons`, `Prg`, `Session`, `SoundId`, `Entry`, `WriteList`, plus `chr`, `digest`, `levels`, `map`, `palette`, `ppulist`, `prg`, `render`, and `sim::{session, modern, oam}`). Internal simulation modules (`sim::{boot, button, collision, combat, cutscene, ending, engine, families, frame, pause, player, raster, room, script, scroll, slots, sound_test, spawn, stage, tile_block, weapons, world}`) and `levels::*` implementation modules are `pub(crate)`.

```
crates/rotj-core/src/
├── lib.rs              Public facade re-exports
├── prg.rs              Prg (ROM bytes + imported hitbox/family/player/weapon tables + Levels)
├── chr.rs              2bpp CHR tile decoding (plane_pixel, blit_rgba, byte, tile)
├── palette.rs          Composite Direct (FBX), 2C03 RGB PPU, NTSC decoder, .pal loader
├── ppulist.rs          [hi, lo, count, bytes...] PPU write-list iterator
├── render.rs           Scanline PPU software renderer (Frame, Picture, Line, Mirroring)
├── digest.rs           FNV-1a digest & table helpers for tests
├── map/
│   ├── mod.rs          Map, Cell, Level, Surface, Terrain, CLASS_COUNT, UNREAD
│   ├── room.rs         Room, Start, Raster, Chain, Arrival, ChrBanks, Orientation, Facing
│   └── rom.rs          LZ map/attribute/auto-tiler/collision-class ROM decoders & spawn-table dump readers
├── levels/
│   ├── mod.rs          Levels, Catalog, Source, PackFiles, Place, Corner
│   ├── model.rs        Tiled JSON (.tmj / .tsj / pack.json) schema
│   ├── export.rs       ROM -> Tiled level-pack exporter
│   ├── import.rs       Tiled pack -> Levels importer + Source::User crash-hardening validator
│   ├── tilesets.rs     Per-(area, CHR variant) 256-tile PNG + .tsj generator
│   ├── custom.rs       Custom metatile atlas builder
│   └── folder.rs       Safe filesystem I/O (FolderError, case-insensitive lookup, symlink/depth guard)
└── sim/
    ├── mod.rs          Game loop orchestration, Script enum, actor/second-class passes
    ├── session.rs      Host-facing Session owning (Game, Ppu) + FrameSnapshot + Classic compose
    ├── modern.rs       ModernFrame / View / Run scanline decomposition for Bevy tilemap rendering
    ├── oam.rs          Metasprite decoder, OamBuilder, DrawRecord, Mark (UI/Scene/Actor/etc.), OamEntry
    ├── world.rs        World, Effects, Player, Projectile, Ppu, Rng, SoundId, Motion, Weapon, Transition
    ├── slots.rs        Slots (24 main + 8 second-class), Body, Field aliasing, Counter, EMPTY
    ├── engine.rs       Shared bank-13 movement/visibility/visibility-culling/spawning helpers
    ├── combat.rs       Hitboxes, contact, projectile hits, boss flash/contact/defeat, dying, pickups
    ├── tile_block.rs   Nametable TileBlock queueing ($A0CE) and metatile map patching ($AE0A)
    ├── collision.rs    Runtime wall/floor/ceiling/point probes dispatching on map::Surface
    ├── scroll.rs       Camera tracking, ring buffer, column/row streaming, scroll locks
    ├── spawn.rs        SpawnRecord, SpawnTable, room-row and column spawning
    ├── raster.rs       All 14 FME-7 IRQ raster chains ($D3 0..=$0D) and per-scanline Line builder
    ├── room.rs         Room loader and horizontal/vertical/fade room-change scripts
    ├── stage.rs        StageDescriptor, StageEnd, NextArea, PowerUp, Cutscene/Continue/BossIntro steps
    ├── boot.rs         Power-on, intro, title screen, password entry (14 passwords)
    ├── attract.rs      Attract-mode demo runner
    ├── pause.rs        In-game pause card & password display
    ├── ending.rs       Final boss ending, skyline, credits, final hold
    ├── sound_test.rs   Hidden sound test menu
    ├── script.rs       Shared table-driven Script Runner + Flow
    ├── player/
    │   ├── mod.rs      Port harness, Tables, Animations, draw/overlay/stun/muzzle helpers
    │   ├── physics.rs  Movement integration, wall/floor/platform riding, gravity
    │   ├── state.rs    Per-Motion state handlers (standing, crouching, walking, jumping, sliding, etc.)
    │   └── fire.rs     Weapon firing, melee blocks, target acquisition
    ├── weapons/
    │   ├── mod.rs      Weapon dispatch, Tables, ShotMode, shared flight/projection/draw
    │   ├── homing.rs   Weapon 1 (Batarang) + HomingStage charged attack
    │   ├── splitter.rs Weapon 2 (Splitter) + SplitterStage orbiting blades
    │   ├── wave.rs     Weapon 3 (Wave) + WaveStage weaving spread
    │   ├── three_way.rs Weapon 4 (Three-Way) + ThreeWayStage tethered bubble
    │   └── super_weapon.rs Weapon 5 (Power-Up Super) + SuperStage screen-clear
    ├── families/       All 100 actor handler types ($02-$C8, main + second-class) declared once in families.rs
    ├── oracle.rs       #[cfg(test)] Lockstep harness (lift/project_into, line/IRQ/VRAM/RNG diffs)
    ├── tape.rs         #[cfg(test)] Gzip-compressed per-frame FNV-1a state tape reader/writer
    └── tests.rs        #[cfg(test)] Whole-TAS chain, tape replay, random-play sweep, novelty explorer
```

## The Classic RAM-Projection Seam vs. Host-Side State

Classic Mode's parity guarantee rests on a clean two-tier boundary:

1. **RAM-projected simulation state (`Game`):** `Slots`, `World`, `Schedule`, `Effects` (`Player`, `Projectile`, `Level`, palette, VRAM queue), `OamBuilder`, `Work`, `Scroll`, `Display`, and `Probe`. In `#[cfg(test)]`, `sim::oracle::State::lift` and `project` map these Rust structs bijectively to NES CPU RAM (`$0000-$07FF`) and WRAM (`$6000-$7FFF`) every frame. Classic rules (update order slots `23..=0`, slot 16-23 field aliasing, LFSR cadence, weapon 5's overflow flag) remain explicit, named, and tested.
2. **Host-side state machines (never RAM-projected):**
   - **Core scripts:** `stage::NextArea` (`AreaEntry`, `FadeDirection`, `Step` variants owning `Cutscene`/`Continue`/`BossIntro`/`Intro`), `attract::FirstPhase` & `Step::Done`, `ending::{BurstState, WalkerMode}`, `boot::TitleChoice`, `sound_test::MenuRow`.
   - **Weapons & audio:** `weapons::{ShotMode, HomingStage, SplitterStage, WaveStage, ThreeWayStage, SuperStage}`; `rotj-apu` `FrameCounter` (`SequenceMode`, `Option<PendingWrite>`) and `LengthCounter` (`Option<PendingReload>`); `rotj-music` `Request`, `Option<ActiveFade>`, `VoiceState`, `Option<u16>`/`Option<NonZeroU8>`, `EnvelopeStep`.
   - **App host (`crates/app`):** `Play` wraps `rotj_core::Session` (`step(buttons, prg, chr)`), never accessing `Game` internals directly. Explicit enums own every UI/audio/render transition: `AudioDrive` & `QueueState` (`audio.rs`), `MenuState` & `OverlayView` (`menu.rs`), `ActivePicture` & `BuiltTileset` (`modern.rs`), `PaletteStatus` (`screen.rs`), `LabelHold` & `Recorder` (`play/mod.rs`), `TickState` (`pacing.rs`).

## Rendering, Mirror & UI Pipeline (`crates/app`)

Each 60 Hz tick (`pacing::Step::{Input, Game, Picture}`):
1. **`Step::Input` (`input.rs`, `menu.rs`):** Reads keyboard + gamepads directly into `rotj_core::Buttons` (no bit-reversal) and steers the settings overlay when open (freezing gameplay and fading audio smoothly over 10 ms).
2. **`Step::Game` (`play/mod.rs`):** Calls `Play::step(pad)` -> `Session::step(pad, &prg, &chr)`, forwarding the NMI's `$40-$43` `SoundId` requests to `audio::Music`.
3. **Read-Only ECS Mirror (`mirror.rs`):** Copies `Session::snapshot()` (`DrawerState`, `DrawRecord`s, camera) into stable Bevy entities (`Drawer`, `Kind`, `LevelPosition`, `Draws`) and resources (`MirrorCamera`, `OtherDraws`) for the Modern renderer and future editor/inspector tools.
4. **`Step::Picture` (`screen.rs`, `play/mod.rs`, `modern.rs`, `ui.rs`):**
   - **Classic (`Picture::Classic`, default):** Composes `Session::compose` into `Frame`, skipping work when Modern is active or neither tick nor settings changed. Integer-scaled onto `ScreenCamera` (`main.rs`), hiding 8 px top/bottom overscan and scrolled-line side seams by default.
   - **Modern (`Picture::Modern`):** Renders `ModernFrame` (map playfield + per-split scanline `Run`s) onto Bevy `TilemapChunk`s and mirrored `ModernSprite`s in an off-screen target texture; falls back seamlessly to Classic on non-gameplay frames and bank-straddling boss-floor frames.
   - **Faithful UI (`UiStyle::Faithful`):** Parks UI-marked OAM entries (`Mark::{Hud, Label, Pause, Title, Password}`) and renders them as crisp Bevy `ImageNode`s from CHR atlases, carrying the odd-frame HUD `L`/`E` label across even frames via `LabelHold`.

## Verification Pyramid

| Layer | Command / Mechanism | What it proves |
| --- | --- | --- |
| Static & policy | `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny check`, `tools/check-no-rom-data.py` | Safe Rust (`unsafe_code = "deny"`), pedantic lints, license/advisory compliance, zero ROM bytes tracked. |
| Disassembly | `bash disasm/assemble.sh` | Generated ca65/ld65 source reassembles byte-identically to the user's ROM. |
| Unit & integration (ROM-optional) | `cargo test --workspace` / `ROTJ_REQUIRE_ROM=1 cargo test --workspace` | Unit tests, level-pack round-trip + `Source::User` crash-hardening, APU/music driver, app/UI/Modern tests, and state-tape replay. |
| Whole-TAS lockstep & picture parity | `ROTJ_REQUIRE_ROM=1 cargo test --release -p rotj-core -p rotj-oracle -p batman-rotj -- --ignored` | Replays all 48,100 frames of Aglar's TAS from stage 1-1 through the ending against `rotj-oracle` (RAM, WRAM, PPU VRAM, palette, 240 scanlines, IRQ state, RNG, music writes), records `target/tapes/the_loop_matches_the_tas_from_stage_1_to_the_ending.tape` (checked byte-identical to `.orchestration/chain-1.1.14.tape`), and checks pixel parity against fceux digests and headless Modern screenshots. |
| Off-TAS exploration | `random_play_sweep` (600 runs) + `sim::tests::explore` (300 branches) | Fuzzes off-TAS area starts, weapons 0–5, deaths, and GAME OVER resets against the oracle with 0 divergences. |
| Live boot check | `bash tools/boot-check.sh` | Verifies the built binary creates a window and stays alive for 20 s without panicking (or exits 2 cleanly when ROM-less). |
