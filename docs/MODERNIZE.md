# Modernisation candidates for 1.1

Read-only survey (TASK=modaudit) of every crate for places where idiomatic
Rust/Bevy or an ecosystem crate would replace 6502-shaped code, without changing
Classic Mode's behaviour. It proposes; it changes nothing.

**Method.** Every item was read in the source, at the file:line given. Ecosystem
crate facts are from crates.io and the crate's own `Cargo.toml` on 2026-10-15
(the Bevy version in this workspace is 0.20.0). **What proves an item safe is
always the Classic suites** (`docs/specs/2026-10-15-classic-and-enhanced-modes-
design.md`): with every setting at its default the game stays byte-exact, so a
refactor that preserves the per-frame RAM/WRAM projection, the picture and the
sound cannot change Classic. Items I could not close are marked **lead**.

## Ranked (payoff over risk)

| # | item | kind | PLAN phase | payoff | risk | Classic proof | status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `weapons.rs` routine dispatch (`step`→`run`→`zero`…`five`) → one enum state machine | 1 collapse | player as a character | high (2225 lines) | high | weapons oracle tests (`weapons.rs` §tests) + lockstep | verified shape |
| 2 | player motion byte → `Port::m0..m11` (player.rs:502–2520) → an explicit enum state machine | 1 collapse | player as a character | high | high | `player.rs` tests + chain lockstep | verified shape |
| 3 | `Brain::Unported` + `engine::unported`: dead as a handler, but `Unported` is also the `#[default]` empty-slot brain | 1 collapse | actors (family dispatch) | ~30 lines removed | low | chain tape has no `Outside` (`actor.rs:536`); retarget 3 test predicates | verified (lead: test edits) |
| 4 | `Outside` bracket enum + callback plumbing | 1 collapse | actors (family dispatch) | moderate | medium (tape format) | remove the last `Event::Outside` kind; PLAN says steps keep tapes unchanged, so a re-record needs the owner's yes | lead |
| 5 | Bevy tilemaps/sprites replace the software compositor (`render.rs`, `play::compose`/`split`, `screen.rs`) | 2 replace | presentation and UI | high (~700 lines) | medium-high (picture tests read `Frame`) | keep `Frame` as the headless reference; add screenshot diff | lead (`bevy_ecs_tilemap` is Bevy 0.19; `bevy_simple_tilemap` 0.21.0 is Bevy 0.20, unevaluated) |
| 6 | HUD/pause/title/password → Bevy UI | 2 replace | presentation and UI | moderate | medium | HUD state is RAM (lockstep); picture tests | verified shape |
| 7 | `frame::animate_chr` → Bevy texture-atlas animation | 2 replace | presentation and UI | small | low-medium | `$0C00-$0FFF` bank check; picture | verified shape |
| 8 | `Script`/`NextArea`/`RoomChange`/`Cutscene`/`Ending`/`Boot` step loops → data-driven sequences / Bevy `States` | 1 collapse | scripts | high | high | chain lockstep + script tape hashes | verified shape |
| 9 | duplicate `integrate` + four-part walk (k08.rs:57, k92.rs:66) → one `engine` helper | 1 collapse | actors, family by family | ~20 lines | low | k08/k92 oracle tests | verified |
| 10 | palette fade/tint helpers duplicated (`queue_faded` stage.rs:144, `queue_whitened` :859, `queue_dimmed` pause.rs:100, 11× palette `step_by(4)`) | 1 collapse | presentation and UI | moderate | low | picture + lockstep | verified |
| 11 | duplicate PPU write-list decoders (`stage::text` stage.rs:900, `sound_test.rs:171`, `level.rs:149`) → `ppulist::WriteList` | 1 collapse | levels as assets | small | low | `ppulist` + level tests | verified |
| 12 | `Play::compose` per-frame `Vec` allocations (play.rs:138/143/206) → reusable buffers | 1 collapse | presentation and UI | small (perf) | low | picture tests | verified |
| 13 | address-keyed `byte()`/`set_byte()` matches (`Body` slots.rs:212, `Weapon` world.rs:361, `find_owner` slots.rs:89) → an index enum | 1 collapse | actors, family by family | moderate | medium | projection tests | verified shape |
| 14 | `bevy_asset_loader` 0.28.1 + `bevy_common_assets` 0.18.0 replace `read_asset`/`prg`/`load_driver` and manual state gating | 2 replace | levels as assets | small | low | boot/picture tests | verified (crate) |
| 15 | `leafwing-input-manager` 0.22.0 replaces `input.rs` keymap/padmap | 2 replace | presentation and UI | small (~60 lines) | low (adds a dep) | `input.rs` tests | verified (crate) |
| 16 | `bevy_kira_audio` 0.27.1 as the Enhanced-mode audio path (`bevy_seedling` 0.8.0 is Bevy 0.19, blocked) | 2 replace | (Enhanced audio) | n/a | low | n/a (Enhanced, not Classic) | verified (crate) |

Ranking is payoff (lines removed or clarity) over risk, and the two high-payoff
`weapons.rs`/player items head it because they are the PLAN's own stated seams
("the player as a character", "actors family by family"); they are also the
riskiest, so they belong to the pilot the PLAN schedules after 1.0.1.

## 1 — Collapse into idiomatic patterns

### 1.1 Actor family dispatch (PLAN: actors family by family)

- **`Brain::Unported` / `engine::unported` (families.rs:115–118, 433, 449,
  478–479; engine.rs:1010)** — dead as a *handler* (the arm at 478 never runs), but
  the variant is also the `#[default]` brain of an empty slot and the `_` result of
  `SecondBrain::new` (`SecondBrain::Unported` for kind 0 and unknown second-class kinds;
  `step_second` ignores it). So a blanket `panic!` in `Brain::new` or `SecondBrain::new` is wrong: replace it with an
  explicit `Empty` (or `Option<Brain>`) and panic only for a non-zero unknown main kind. **Lead** on that split. All 100 handler types are
  ported (`docs/STATUS.md` 1.0.0 row), and the chain's tape replays with an
  empty `injected` list (`actor.rs:536`, `the_tape_replays_the_chain_without_the_rom`),
  so the `Outside::SecondClass` request never fires. Before/after: the dispatch arm and the `unported` body gone (not the variant, see above). Three test predicates reference the variant (k4c.rs:72,
  oracle.rs:1690, oracle.rs:1728); retarget them to a kind-based check. **Lead**
  on those edits, otherwise verified.
- **`Outside` (actor.rs:111–115, 139, 188–193; oracle.rs:491–500, 629, 654,
  1017, 1203, 1289; tape.rs:24–25, 85, 124, 162, 230)** — the callback and the
  `Event::Outside` tape kind exist to hand the oracle a routine the port does
  not run. With #1.1 landed there is no such routine, so the whole bracket is
  dead. Removing it changes the tape encoding (a version bump), so it is a
  **lead**: fold it into the #1.1 commit once the tape is re-recorded.
- **Duplicate `integrate` (k08.rs:57, k92.rs:66, byte-identical)** and the
  four-part-per-frame walk around it (k08.rs:34–47, k92.rs:39–56). Before/after:
  two copies → one `engine::integrate_part(body, lo, hi, velocity) -> u16`.
  Low risk, pinned by `k08::tests` and `k92::tests`.
- **`weapons.rs` (2225 lines) is routine-shaped** (`docs/PLAN.md` "routine-shaped
  modules"): `step` (weapons.rs:16) dispatches by handler, then the 6502 blocks
  appear as `zero` (222), `one` (228), `two` (925), `three` (1052), `four`
  (1090), `five`/`unleashed` (579–610), `bubble`, `charge`, `spread`, `orbit`.
  Before/after: a match on handler bytes → a `Weapon` state enum whose arms are
  the same blocks. High payoff, high risk; the whole `weapons.rs` oracle suite
  is the proof. Same phase as the player (weapons are his projectiles).
- **Address-keyed byte access (slots.rs:212–287 `Body::byte`/`set_byte`,
  world.rs:361–414 `Weapon::byte`/`set_byte`, slots.rs:85–112 `find_owner`).**
  These are 30-arm matches on `$7B00`-style addresses. Before/after: a
  `#[repr(u8)] enum Field` (or `Struct`/`Slot` index) so the compiler checks
  coverage instead of a `panic!("not a Body array")`. Classic proof: the
  `lift`/`project` tests and the lockstep already exercise every arm.

### 1.2 The player and scripts (PLAN: the player as a character; scripts)

- **`player::Port` motion machine (player.rs:441–2520).** `run` (player.rs:502)
  matches `player.motion` to `m0`…`m11` plus `project`/`draw`/`hurt`/`stun`, each
  a `&mut self` method mutating the same `Port`. This is exactly PLAN's "a motion
  byte `$046E` dispatching to handlers → an explicit enum state machine". #2.
- **The script step loops (`NextArea::step` stage.rs:408, `RoomChange::step`
  room.rs:80, `Cutscene::step` cutscene.rs:87, `Ending::step` ending.rs:239,
  `Boot::frame` boot.rs:332, `Continue::step` stage.rs:989, `Attract::frame`
  attract.rs:106, `SoundTest::step` sound_test.rs:67).** Each is a hand-rolled
  `loop { match self.step { … return/continue } }` reproducing 6502 `JMP`s; the
  `Ending` already goes further with a data `SCRIPT: &[Action]` (ending.rs:77).
  Before/after: the ending's table shape generalised, or one frame per Bevy
  `State`. PLAN's "cutscenes, banners and the ending are data-driven sequences".
  High payoff, high risk; the script tape hashes and the chain lockstep prove it.

### 1.3 Smaller, low-risk collapses

- **Palette fades/tints:** `queue_faded` (stage.rs:144), `queue_whitened`
  (stage.rs:859), `queue_dimmed` (pause.rs:100), and a repeated
  `palette.iter_mut().step_by(4)` loop (11 sites: pause.rs:109, stage.rs:199/837,
  weapons.rs:810/832, attract.rs:345/488, player.rs:2309, room.rs:460,
  boot.rs:389, kb4.rs:277). One `palette::cap(&mut [u8;32], cap)` helper covers
  them. Picture tests prove it.
- **Write-list decoding is done four ways:** `stage::text` (stage.rs:900),
  the inline loop in `sound_test::show_screen` (sound_test.rs:171–179),
  `level::decode_ppu_list` (level.rs:149), and `ppulist::WriteList`
  (ppulist.rs:15). Unify the ad-hoc two on `WriteList`; its tests already pin
  the format.
- **Per-frame allocations in `Play::compose` (play.rs:138 `vec![0u8; 8*1024]`,
  143 `Vec::with_capacity(2048)`, 206 `vec![(0..)]`)** run every frame and are
  scratch. Keep them on `Play` and `clear()`+reuse. Low risk, picture tests.

## 2 — Replace wholesale with existing tech

Only the app/presentation layer; the brief (and PLAN) exclude the game logic, the
oracle, the music driver and the 2A03 synth.

### 2.1 Rendering and UI (PLAN phase 1: presentation and UI)

- **The NES-style compositor.** `Play::compose` (play.rs:136) rebuilds the 8 KiB
  pattern space, the 2 KiB nametables and the raster `Split` (play.rs:197–294),
  draws through the software `Frame` (render.rs:130/169), and `screen.rs:78`
  uploads the RGBA image. Replaced by Bevy `Sprite`s (a CHR `Image` per bank
  with `TextureAtlas` for tiles) plus one `Sprite` per OAM entry, and the band
  split as two/three sprite layers. **Cost:** no new API in the core, but
  `bevy_ecs_tilemap` — the obvious crate — has **no Bevy-0.20 release**
  (0.19.0, 2026-07-04, depends on `bevy ^0.19`; `bevy_ecs_ldtk` 0.15.0 and
  `bevy_ecs_tiled` 0.13.4 are also 0.19). `bevy_simple_tilemap` **0.21.0**
  (2026-10-09, `bevy ^0.20.0`) does exist, but is a minimal crate; unevaluated here,
  so the default is still Bevy's own sprite renderer. **Classic proof:**
  the picture tests compare `Frame::indices()` (render.rs:125), a port-side
  format — so **keep `Frame` as the headless reference the tests read**, and add
  the Bevy path as the shipped picture plus a GPU-readback screenshot diff. This
  is the one item whose Classic proof needs new test tooling, hence **lead**.
- **HUD, pause, title, password, sound test (PLAN phase 1).** `frame::draw_hud`
  (frame.rs:22), `pause::draw_screen` (pause.rs:118), the title
  (boot.rs:960), the password screen and `sound_test` draw NES metasprites and
  nametable runs. In Bevy these become `bevy_ui` nodes/`ImageNode`s with an
  NES-styled font. No new dependency (`bevy_ui` is in Bevy's defaults). The HUD's
  *state* (`$0478`, `$0477`, `$F7`) stays RAM-exact under the lockstep; only the
  drawing moves, so the picture tests are the proof.
- **Background CHR animation (`frame::animate_chr` frame.rs:56)** becomes a Bevy
  `TextureAtlas`/`AnimationPlayer` over the same `Display` records
  (frame.rs:9–19). Verified by the existing rule that `$0C00-$0FFF` equals the
  predicted bank (`docs/STATUS.md` background-animation row) and the picture.
- **Frame pacing / vsync / sprite limit / palette (PLAN 1.1).** Already-planned
  settings (`Time<Fixed>` at main.rs:91, `desired_maximum_frame_latency` at
  main.rs:84); the interpolation snap cases are in the Classic/Enhanced spec.
  No code-shape item.

### 2.2 Assets, input, audio (lower priority)

- **Asset loading.** `read_asset`/`prg()` (play.rs:23–32), `load_driver`
  (audio.rs:47), the pre-run `Screen::new` (main.rs:97) and the hand-rolled
  `GameState` gate all hand-roll what `bevy_asset_loader` **0.28.1** (Bevy 0.20,
  2026-10-13) plus `bevy_common_assets` **0.18.0** (Bevy 0.20, RON/JSON) do.
  Small payoff today, but it is the natural seam for PLAN's "levels as assets"
  (a Tiled/ROM importer behind an `AssetLoader`). Pairs with item 14.
- **Input.** `input.rs` (`KEYMAP` input.rs:67, `PADMAP` :82, `read_buttons` :119) is ~60 lines
  that `leafwing-input-manager` **0.22.0** (Bevy 0.20, 2026-10-15) replaces with
  actions/`InputMap`. Keep `NesButtons::game_byte` (input.rs:41) — the game's own
  `$F6` order is a Classic rule. Low priority: the brief's "never add a
  dependency for a few lines" applies.
- **Audio.** Classic keeps `bevy_audio` + the ported driver/2A03 (PLAN's
  decision; `audio.rs` already uses `add_audio_source`). `bevy_seedling`
  **0.8.0** (2026-08-09) depends on `bevy ^0.19` — **blocked** until a 0.20
  release. `bevy_kira_audio` **0.27.1** (Bevy 0.20, 2026-10-13) is the available
  path, but only for Enhanced-mode audio; Classic sound tests would not use it.

## 3 — Not candidates (already the target shape)

- `families::Brain` is already an enum with per-family data plus a macro dispatch
  (families.rs:115, 470–495) — the shape PLAN asks for, not a byte dispatching to
  functions.
- `Slots`/`Slot`/`Body`/`Weapon` (slots.rs:9, 320; world.rs:313) are already
  structs, not parallel arrays; `lift`/`project` (cfg(test)) are the test-side
  projection PLAN says stays NES-shaped.
- `Rng` (world.rs:419), the `View` (world.rs:18) and the `Settings`-as-one-value
  idea (PLAN 1.1) are already named.
- The oracle (`tools/rotj-oracle`), the 2A03 synth (`crates/rotj-apu`) and the
  music driver (`crates/rotj-music`) are Classic's spec and stay NES-shaped by
  the brief; the oracle's `Cpu::step` (cpu.rs:333) is already an opcode enum, not
  a byte dispatcher.

## Found during 1.1.x (2026-10-16, the modernisation roadmap's step 7)

New candidates, each to be classified (state, flags, table index) before it changes
(`docs/specs/2026-10-16-modernisation-roadmap-design.md`):

- **The player's other flag bytes**: `stage_script` (`$0481`: bit 7 freezes him and clears
  the pads unless bit 5 keeps them, bit 6 skips the projection), `effects.busy` (`$AF`),
  `effects.attack` (`$B8`), `power_up`/`sliding`/`hurt` (`$0472`/`$0473`/`$047A`): named
  bits in `actor/world.rs` (1.1.7).
- **The hurt rule twice**: `player.rs`'s own hurt (the `Falling` → `Jumping` knockback and
  the `on_foot` test) and `engine::hurt_player` repeat the same rule; one function.
- **Runtime ROM tables** (about 390 `rom_byte`/`rom_word` reads in 90 files): velocities,
  hitboxes, metasprite pointers, per-motion offsets. Typed data the level importer
  produces (roadmap A3, B1), which new content and kaizo levels need. Surveyed
  2026-10-17: about 145 metasprite pointers (stay ROM reads: the OAM builder walks the
  bytes, and a pointer's target sits in another bank), about 100 per-family constant
  tables (velocities, offsets, turn and knockback tables; lo and hi often in separate
  tables), 12 hitbox reads (`$ABB2`/`$ABC1`/`$ABD0` back to back, 15 ids each; the boss's
  `$B0FE`/`$B103` 5 apart; `$A0A8` is entry 1 of `$A0A7`), about 52 player and 36 weapon
  tables, about 45 level-ish reads, about 40 text reads and about 30 pointer chases (stay
  ROM reads). First slice: the hitboxes, then the family tables. ROM constants cannot live
  in `World` (liftable from RAM alone), and with the mirror decided (2026-10-17) the core
  keeps no Bevy resources, so typed tables travel beside `prg`: one decision for all the
  groups (e.g. the `prg: &[u8]` threaded through `Game`, `Cx` and the oracle becomes one
  struct holding the PRG and the imported tables), made in the first slice's spec.
  **Decided and slice 1 done (1.1.8, `docs/specs/2026-10-17-behaviour-tables-design.md`):**
  `prg::Prg` owns the image and the imported tables and derefs to the bytes, so only the
  path to `Cx` takes `&Prg`; the hitboxes are `engine::Hitboxes` (the survey's 12 reads plus
  `touch`'s `$AF2A`/`$AF31`, which it missed). Next: the family constants, then the player
  and weapons, each as fields on `Prg`.
  Slice 2 (1.1.9, `docs/specs/2026-10-17-family-tables-design.md`): 29 families' constant
  tables are `<family>::Tables` on `prg.families`; bank 14's `$C5F2/$C6F2` was `x << 4` and
  is a shift. **A lead:** `path_platform`'s tables are sized by their masks (`carries` 256
  records, `jump_positions` 128) because the directions and jumps come from ROM path data
  read through a pointer; a test walking every path's turn bytes would shrink them (the
  directions look like 0-4), and the step's hard-coded `carry` array looks like the same data
  as `$92AF`'s first four records. **Cleanup for slice 3:** about 18 imports hand-roll
  `from_fn(|i| u16::from_be_bytes([hi[i], lo[i]]))` over two `rom_bytes` columns
  (`final_boss::split_words` is one local copy); one `engine` helper for split hi/lo words (and
  one for 4-byte pair records) would make each a line.
  **A lead (slice 3):** the splitter orbit's turn steps (`$8C71/$8C79`, `$8C81/$8C89`, by
  `part + 0/4`) stay ROM reads: type `$80` is shared by every weapon's charge, the `$9AE3` charge
  never writes `part`, and the Super-to-stored weapon switches (`next_area_gate::transition`,
  `room.rs`, `player.rs`) don't free the slots, so a stale slot could reach `splitter_orbit`
  with any `part`. An oracle probe of whether the game ever does would settle the table's size.
- **The overflow flag carried between routines**: `Work::cpu_overflow` mirrors the 6502's V
  for one reader, `$9790` (weapon 5's charge waits while V is set), which sees whatever
  the last V-setting routine left (`bit`'s 52 callers in `player.rs`, the OAM draw, the
  projections). Naming it needs a trace of which setter is last before `$9790` on each
  path; until then `bit` and the setters stay.
- **Helper names in `player.rs`** still shaped by the probe they call: `stun`, `bit`
  (the 6502 `BIT` flag mirror kept for its N/V side effects: name what each caller tests).

- Released as 1.1.3: the families declared once, `SecondBrain`, the frame's steps named
  (`docs/specs/2026-10-17-frame-steps-design.md`), and #13 in substance: the actors' fields
  by name (`slots::Field`, one alias mapping; `docs/specs/2026-10-17-slot-fields-design.md`).
  Still address-keyed on purpose: `next_area_gate`'s read of a projectile array by actor
  slot, `turret::Indexed` (a register index that runs past slot 31), and the WRAM-image
  tooling (`lift`/`project`, `room.rs`'s image), which keep `find_owner`.
- Released as 1.1.4: the pad as `Buttons` (`docs/specs/2026-10-17-buttons-design.md`;
  the core has no `Action` enum, remapping is the app's) and the boot's linear parts as
  script tables (`docs/specs/2026-10-17-boot-tables-design.md`).
- Released as 1.1.5 and 1.1.6: the families' repurposed bytes named by how each is used
  (`docs/specs/2026-10-17-family-fields-design.md`): slot links as locals, flags as value
  consts, 16-bit pairs as named `u16`/`i16` values, the dropper's payload layout in its
  own `arm`; the `hit` byte's engine bits (`slots::hit`).
- Released as 1.1.11: Phase C's C1, the draw-call record (`oam::DrawRecord`, `Mark`'s slot
  drawers, `OamBuilder::shown`; `docs/specs/2026-10-18-phase-c-mirror-design.md`).
- Released as 1.1.12: Phase C's C2, the read-only mirror (`app/src/mirror.rs`).
- **Resolved (2026-10-17):** `stun`'s `held = (attack & $80) << 1` was always 0, and so
  is the ROM's (`$92E8`: `AND #$80`, `ASL`, `STA $047F`, `STA $0470`); the port now writes
  the zeros plainly.
- **A lead (unreached):** a running slot 16+c that writes slot c's aliased bytes through
  `cx` (slot c's narrow fields, `Anim`, `DyingTimer`) has those writes undone by
  `store_aliases` when it finishes; in the game the write stands. No lockstep reaches it
  (the stand-in review of `docs/specs/2026-10-17-slot-fields-design.md`); worth an oracle
  test before any change. Probed 2026-10-17 (a throwaway check around `step_actors`'
  `store_aliases`): slots 16-23 ran about 27,000 handler steps over the TAS, and no step
  in the TAS, the lib suite, the Classic suites, a 1-seed sweep or a 300-branch
  exploration wrote an aliased byte through `cx`. Unreached by the game as shipped; new
  content that places actors differently (kaizo levels) could reach it, so the fix (write
  back only the fields the handler changed) belongs with that content's own tests.

### Done in 1.1.x (2026-10-16, released as 1.1.1)

- #9 the duplicate `integrate` (`k08`, `k92`) is `engine::move_by_velocity`.
- #10 the 12 "every set's first colour" loops are `world::set_first_colours`. The three
  palette helpers it grouped are not duplicates: `queue_faded` lowers bright colours to a
  cap and keeps the first colours, `queue_whitened` raises dark ones to a cap and sets the
  first colours, `queue_dimmed` subtracts `$10`; they stay, `queue_faded` without its `Vec`.
- #11 `stage::text` and the sound test's loop decode with `ppulist::WriteList`, which now
  stops at bit 7 as the game's `BMI` does (the two lists it was verified on are theirs).
- #12 `Play::compose` reads the nametables flat instead of copying 2 KiB a frame.
- New: `engine::spend` nearly repeats `weapons::bounce_off_wall`.
- The family files and `Brain` variants are named for what each actor is (`k08.rs` →
  `harmless_burst.rs`, `Brain::K38` → `Brain::AceRanger`, ...), from two read-only surveys
  that checked the uncertain names against the handlers; the stage number is in a name only
  where it is certain. The main families are listed once (`with_main_families!`).
- The player's motion byte is `world::Motion` and the selected weapon `world::Weapon`, each
  handler named for its state (`docs/specs/2026-10-16-player-motion-design.md`,
  `docs/specs/2026-10-16-weapons-design.md`); the projectile slots are `Projectile`.
- The pad's bits are `actor::button` constants and the pose byte's bits `world::pose`
  constants. Not newtypes: both bytes live in RAM and the game does arithmetic on them,
  so a named mask reads the same and converts nothing.
- Levels as assets, slice 1: an area's terrain is `terrain::Terrain`, imported from the ROM
  and built into a `Level` over the WRAM it finds
  (`docs/specs/2026-10-17-levels-as-assets-design.md`).
- Levels as assets, slice 2: a room's spawn records are typed `SpawnRecord`s, the schedule
  cell bits `CELL_*` (`docs/specs/2026-10-17-spawn-records-design.md`).
- Released as 1.1.2: every family's phase machine is a `Phase` enum (`phases!`), the
  other phase values named constants, the shared state bits `STATE_DROPS_PICKUP` and
  `STATE_UNTARGETED` (`docs/specs/2026-10-17-family-phases-design.md`); levels as assets,
  slices 3-5: `rooms::Room` with the screen record, the orientation bits, and
  `collision::Surface` (`docs/specs/2026-10-17-rooms-design.md`,
  `-screens-design.md`, `-surfaces-design.md`); scripts as data: `actor::script`'s runner
  under the ending and the cutscene (`docs/specs/2026-10-17-scripts-as-data-design.md`).
- The families are declared once (`families!`: `Brain`, `SecondBrain`, both `new`s and the
  dispatch from one table in kind order; `docs/specs/2026-10-17-families-declared-once-design.md`),
  the second-class families in their own `SecondBrain`, so `step_second` is exhaustive
  and the main dispatch has no panic arm
  (`docs/specs/2026-10-17-second-class-brains-design.md`).

## Leads (could not verify)

- **Compositor swap (#5):** the well-known tilemap crates are Bevy 0.19 (only `bevy_simple_tilemap` 0.21.0 targets 0.20, unevaluated), and the picture
  tests read the port's `Frame`; a screenshot-readback comparison is needed before
  the swap is provable. The rest of #5 is read from source only.
- **`Outside` removal (#4):** depends on the tape format change and a
  re-record; not attempted.
- **`bevy_inspector_egui` 0.37.0** is Bevy 0.19 (blocked) and `iyes_perf_ui` is
  stale (0.5.0, 2025) — no dev-overlay candidate for 0.20 was found.
