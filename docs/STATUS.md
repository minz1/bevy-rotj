# Status — where 1.1.21 stands

1.0.0 means **full-game parity**: all 7 stages, 5 bosses, every actor behaviour and
the complete soundtrack (release ladder in `docs/PLAN.md`). This file is the
single source of truth for what is done and what is next; the other `docs/*.md`
hold the evidence. Update it in the same commit as the work it describes.

Last reconciled 2026-10-17 (agent merges are recorded here as they land).

## Milestones

| tag | state |
| --- | --- |
| 0.1.0 | **done**, tag `0.1.1` (`0.1.0` cannot run: see "Bugs the checks missed") |
| 0.2.0 streaming | **done, tag `0.2.0`** (2026-10-14): stage 1-1 builds from the ROM alone (`Stage::load = for_level(0)`; map, attributes, auto-tiler tile tables, palette, CHR banks and the background animation all decoded), is walkable across its 4096 px, and scrolled frames match the emulator's PPU state exactly (0 px at cameras 0/50/54); `tools/boot-check.sh` passes. Left: the ~1% screenshot residual lead and the game's real solid-tile rule |
| 0.3.0 actors | **done, tag `0.3.0`** (2026-10-15): the port's whole game loop replays the published TAS byte for byte (RAM/WRAM at every loop frame) from the stage 1-1 start (frame 351) through stage 1-2 and the stage 1 boss fight to frame 6770, with only the area-2/4 raster IRQ and the area loads' leftover bytes injected (`actor::tests::the_loop_matches_the_tas_through_stage_1_and_its_boss`, since extended into stage 2); the app runs `Game` (stage 1-1 into 1-2, firing, death, continue, game over), `tools/boot-check.sh` passes. Actor families: 72 of ~96 types ported. Left for 0.4.0: pause, the banner cutscene engine `$98BD` (the stage 1 -> 2 transition), weapon 1 and the charged attacks of weapons 4/5 |
| 0.4.0 | **done, tag `0.4.0`** (2026-10-15): the banner cutscene `$98BD`, the pause `$9C37`, the `$DD` bit-7 room change (fade path `$BA8B`, second page draw, `$B586` attribute raise) and two-nametable scrolling (`$85F7`/`$86D9`/`$83C4`/`$824F`) are ported and verified against the oracle (`docs/ACTORS.md` §9o-§9q). The whole-loop chain `actor::tests::the_loop_matches_the_tas_from_stage_1_into_stage_4` replays the TAS byte for byte from frame 351 to 15720: stages 1, 2 and 3 with their bosses and cutscenes, into stage 4-1 (12753 loop frames, 1030 cutscene NMI waits), injecting **only the raster IRQ**: the area loads' leftover bytes are now built by the port's own loader image (`room::load_level`). Every actor type in the TAS inventory (§8b) is ported (2026-10-15). The firecheck pass fixed the weapon screen-edge bounce, the walk tail's stale X after a charge, weapon 4's bubble count and weapon 0's endless-metasprite freeze (which hangs the real game too, and now the port), and seeded random play of stage 1 (13 pinned runs, 24,659 loop frames) matches the oracle, weapon 5 included since its charge carries the CPU's overflow flag (`docs/ACTORS.md` §9g). The harness injects lag-frame NMIs like IRQs |
| 0.5.0 audio | **done, tag `0.5.0`** (2026-10-15): the ported driver (write-exact against the game's over the whole TAS) plays every song and effect the game requests: the app hands each frame's `$40-$43` requests from the game's NMI to it. App tests pin the jump (`$24`, bank 4 `$8508`, with the driver's noise-channel writes), weapon fire (`$2D`, `$9D2D`) and damage (`$25`, `$82C6`) effects, each failing if requests are dropped; the title requests its own song `$01` (traced: frame 287, PC `$61B0`) and a stage start `$D980[area]` (`audio::tests::scenes_request_the_games_own_songs`) |
| 0.6.0 stages 2-7 | **done, tag `0.6.0`** (2026-10-15; one release for all six stages, owner's call, see "Owner decisions made"): the whole-loop chain `actor::tests::the_loop_matches_the_tas_from_stage_1_to_the_ending` replays the TAS byte for byte from frame 351 to 41177: all 7 stages and 5 bosses, up to the loop frame type `$B8` arms the ending (`$BA = 6`): 35299 loop frames, 12 room changes, 17 next-area loads, 2060 script frames, injecting only the raster IRQ and lag-frame NMIs. Fixes on the way: riding `$CE3E`, crumbling `$CC6D`, second-class types `$06/$0C/$0E/$10`, type `$B8`, `$96`'s own-slot read, `$84`'s `$0401`, `$AC`'s off-screen X (`$A852`), area 17's class blob, spikes `$CAA9`/`$CD09` (`$CD09` unexercised). App tests play stages 4-7 through their rooms to each boss (`play.rs`) |
| 1.0.0 | **done, tag `1.0.0`** (2026-10-15, after the owner's playtest): the ending is ported and in the chain (2026-10-15): type `$B8` sets `$BA = 6`, `$D967` copies the WRAM payload (`$99A9`) and jumps to `$6135`, which never returns: title, skyline, 28 credit lines, the last screen `$6656` (TAS 47938) and its hold; every frame matches the oracle to TAS 48026 (`actor/ending.rs`, `docs/SUBLEVELS.md` §7e), and the app plays it (`play::tests::beating_the_final_boss_plays_the_ending`). Weapon 5's charge waits on the CPU's real overflow flag (2026-10-15: V carried out of `$84AB`'s motion tails, `$C247` and `$DC87`, compared with the oracle's V every loop frame over the whole chain; `docs/ACTORS.md` §9g). Every one of the game's 100 handler types (`$02`-`$C8`) is ported (2026-10-15): the last three, `$46` (the `$42` boss's child), `$90` and `$92` (the `$8E` boss's burst), which the TAS never spawns, match the oracle in scenes where only the TAS's input changes (the pad released at the boss), so a slower player reaches them (`docs/CUTCONTENT.md` §3). The `rotj-core` readability pass is merged (slots, families, player, effects, weapons, the ending). The boot is ported (2026-10-15): power-on, the intro and its raster split (`$D3 = 9`), the title, the password screen (14 passwords, `$6E6C`), the attract mode and the sound test match the oracle at every NMI wait from power-on (`boot::tests`); the app powers on and plays them, every song from the game's own requests (`docs/ACTORS.md` §9s). Left: off-TAS random play now matches the oracle (2026-10-15): the sweep's 600 runs (every area start, weapons 0-5, seeds 1-3, a death run and a GAME OVER run per start) show no divergence after seven root-cause fixes (`docs/ACTORS.md` §9t: a spawn's no-free-slot exit, type `$54`'s weapon clear, spikes' `$A8` lost to a stale player copy, type `$1E`'s turn, motion 2's blocked side-jump and its third probe column, a lag-frame sound byte in the harness, and the oracle's PPU register reads); 126 runs plus the GAME OVER resets are pinned per stage (31 s in release). Known gap: an odd energy (only from a damage-1 block contact) makes the HUD read PPU registers mid-render, which neither the oracle nor the port models. Left: and the novelty-guided explorer (2000 branches, seed 1) reached 295 cells and 125 `(type, phase)` pairs, 116 cells and 5 pairs beyond the TAS's 589 and 246, with no divergence. The owner's playtest signed it off. The "bring your own ROM" pass follows as 1.0.1 (`docs/PLAN.md`) |
| 1.0.1 | **done, tag `1.0.1`** (2026-10-16): bring your own ROM. The tracked tree carries none of the game (`tools/check-no-rom-data.py`: no 32-byte run of PRG or CHR, raw, hex or as words); the app and every test read the user's ROM through `crates/rotj-rom` (PRG+CHR MD5 `830111df…`; argument, `ROTJ_ROM`, cache, beside the exe, `raw/` in debug builds; status 2 and a message without one); `disasm/assemble.sh` generates the listing from it, still `OK byte-identical`; fceux captures became oracle runs pinned by committed digests (`*/digests.txt`), the tape is recorded under `target/tapes/`, and the ROM tables over 32 bytes in `collision.rs`/`levelmap.rs` are read from the ROM. Every Classic suite green with the ROM (670 tests, the release suite's 23), and a fresh clone without a ROM passes `cargo test --workspace` (the ROM tests skip). The Windows build ships as the exe alone |
| 1.0.2 | **done, tag `1.0.2`** (2026-10-16): the picture. The owner's playtest of 1.0.1 showed garbage in the stage 1 boss's status bar and warping at the top of stage 1. The game runs FME-7 raster IRQ chains (`$E2A9`, a handler per `$D3`) in 12 areas and the port modelled four; every check compared RAM, nametables and palette, never the lines the PPU draws. Now `actor/raster.rs` runs all of them (`$D3` 0-`$0D`) and builds the 240 line states of every frame; the oracle records the same per line, pinned to fceux's pixels on all 48,100 TAS frames (`tools/rotj-oracle/tests/picture.rs`, 0 differing); every lockstep (the chain, random play, the sweep, the explorer, playtests) compares the port's lines with the oracle's each frame, and the app's own composition is compared pixel for pixel over the boot, title and attract (`play::tests::the_app_draws_like_the_oracle_from_power_on`). The PPU mask shadow `$FE` is modelled (the continue screen and GAME OVER blanked nothing before). Off the TAS, with lines compared (2026-10-16): the sweep's 600 runs show no divergence after one lockstep fix (an NMI that ran past the first IRQ's counter let that IRQ in before the loop woke, and the lockstep injected its bytes as the NMI's, so the port's own IRQ model ran it a second time; its bytes now stay with the frame's IRQs; stage 5's `28821` seed 3 weapon 4 is pinned), and three runs freeze as the game does (weapon 0's charge, `docs/ACTORS.md` §9t), the oracle with them; the explorer (2000 branches, seed 1) reached the same 295 cells and 125 pairs with no divergence |
| 1.0.3 | **done, tag `1.0.3`** (2026-10-16): the ROM embedded at build time (the owner's call). `crates/app/build.rs` finds the ROM by `rotj_rom::locate` (`ROTJ_ROM`, which must be an absolute path, else `raw/`), checks its PRG+CHR MD5 and compiles PRG and CHR into the exe, which then runs on its own; a named but missing or wrong ROM fails the build. Without a ROM the workspace still builds, lints and tests (public CI), and that exe says it was built without one and exits 2. 1.0.1's start-up search, its cache and the exe's argument are gone with their tests; the tests read the ROM by the same rule as the build. An exe built this way contains the game and is never shared (`docs/specs/2026-10-16-embedded-rom-design.md`) |
| 1.0.4 | **done, tag `1.0.4`** (2026-10-16): downloads. The build-time embedding stays as 1.0.3 has it; an exe built without a ROM now finds it at start-up (`rotj_rom::Rom::load`): the first argument (a file dropped on the exe), then `ROTJ_ROM`, then a `.nes` beside the exe. A named file that is missing or wrong is an error; a bad file beside the exe is noted and the search goes on; with none the exe says where it looked and exits 2 (waiting for Enter on Windows). No cache and no `raw/` at start-up: nothing is written to the player's machine. `.github/workflows/release.yml` builds the ROM-less Linux and Windows (mingw cross-build) exes on a pushed tag, strips them and attaches them to the GitHub release; 1.0.4 is the first release with binaries (`docs/specs/2026-10-16-embedded-rom-design.md`, "Amendment") |
| 1.0.5 | **done, tag `1.0.5`** (2026-10-16): the owner's playtest of 1.0.4. The picture letterboxes at the largest whole multiple of 256x240 that fits the window, on whole pixels (`main.rs` `integer_view`, `fit_camera`); `AutoMin` had stretched it to about 5.7x, so pixels were 5 or 6 screen pixels wide and dithered backgrounds shimmered as the camera scrolled. The pops: the app made 734 samples per 60 Hz frame (44,040/s) for a device playing 44,100/s, and every underrun dropped a signal that sat near 0.5 of full scale (no output filter) to zero; now `ApuStream` has the console's 90 Hz high-pass and 14 kHz low-pass (the NES-001's 440 Hz high-pass is left out by the owner's ear: it takes the bass), the frame makes 735 samples nudged by 2 to hold three frames queued, and the source waits for the queue and holds its last sample when it runs dry. Left+Right (or Up+Down) held together: `Game::nmi` now ANDs the new presses with the raw read as `$C1DB` does (the owner's playtest replays clean) |
| 1.1.0 | **done, tag `1.1.0`** (2026-10-16): the settings and the faithful UI. Spec 1 (`docs/specs/2026-10-16-settings-menu-design.md`): `settings.ron` beside the exe with five presentation settings (`volume`, `overscan`, `palette`, `sprite_limit`, `vsync`; public names, the 1.1.0 file `crates/app/settings-1.1.0.ron` parsed by a test in every release); the menu (Esc, F1, a pad's Guide or right stick anywhere, Select on the game's pause; the game's own 8x16 font from CHR `$CC-$CF`; the game frozen, nothing recorded and the sound faded over 10 ms while it is open; the buttons held at its close hidden from the game until released); one picture per 60 Hz tick, winit waiting on its fixed-rate schedule and handing over the OS's input just before the tick (`pacing.rs`; on the owner's idle RTX 4070 machine the interval's mean is 16.667 ms, standard deviation 0.19-0.21 ms, worst 17.5 ms; a first sleep-and-spin limiter held 0.00 ms but read the keyboard a frame late, a reviewer's catch); overscan hidden by default (lines 8-231 at the integer fit for 224 lines, and black 8-pixel sides on lines scrolled sideways, where the column the game is still writing shows: the owner's playtest found that seam at the right edge, and a plain side crop cut the title's full-width text; the tests still check all 256x240); palettes Composite Direct, the 2C03's DAC levels, the NESdev wiki's NTSC decoder and a player's `palette.pal`; the per-line sprite limit as an option (`Frame::render`'s `sprites_per_line`). Measured on the owner's machine (the Windows exe, `PrintWindow` captures of a 1280x720 client): the picture is 768x672 at 3x and centred with overscan hidden, 768x720 shown; every pixel run a multiple of 3; every colour from the chosen table (Composite Direct, then the 2C03's); the menu and the pause's `SELECT  SETTINGS` hint on the same grid, the hint clear of the pause's password. The sprite limit's review over the TAS: 16,025 of 48,100 frames drop sprites on some line; every sprite the limit hides is an ordinary actor, projectile or explosion piece, and only two drop patterns hold still for 10 or more frames (8075-8090 and 13965-13976, Batman's own pieces in an overflow), so the game masks nothing on purpose. Spec 2 (`docs/specs/2026-10-16-faithful-ui-design.md`, designed and reviewed by an Opus stand-in while the owner was away): `ui: Faithful` shows the game's own UI sprites as Bevy UI. `OamBuilder` marks which drawer placed each OAM entry (`Mark`: HUD, label, pause card, title cursor, password entry; metadata beside the page, not RAM, carried across the lockstep's relift); the app parks the marked entries in its display copy and shows them as image nodes on tile atlases built from the player's CHR, each in its line's banks and colours; the HUD's L/E label, which the game draws on odd frames only, is carried one step, so it holds still. Every lockstep frame of the whole TAS checks that each marked sprite is drawable as UI exactly as the PPU draws it (in front, 8x16, one bank set, clear of the sides; `oam::undrawable`), and the chain saw HUD and label marks; app sessions check the title shows only its cursor, the password entry 9, 6 or no pieces, the pause card is all UI. Captured on the owner's machine: the title and a password frame pixel-identical in both modes, the pause card the same, the label present in all six Faithful shots and two of six Classic ones |
| 1.1.1 | **done, tag `1.1.1`** (2026-10-17): the first modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged). The player's motion and the selected weapon are enums with their handlers named for each state; the projectile slots are `Projectile`; the pad's buttons and the pose byte's bits are named; every actor family file and `Brain` variant is named for what the actor is; small collapses (`engine::move_by_velocity`, `world::set_first_colours`, write-lists that stop where the game's `BMI` does); levels as assets, slices 1 and 2: an area's terrain is a `Terrain` imported from the ROM and built into a `Level`, a room's spawns typed `SpawnRecord`s and the schedule's bits named (`docs/MODERNIZE.md` "Done in 1.1.x") |
| 1.1.2 | **done, tag `1.1.2`** (2026-10-17): the second modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged). Every actor family's phase machine is a named `Phase` enum (`phases!`, 26 families; the other phase values named constants), the actor state bits `STATE_DROPS_PICKUP`/`STATE_UNTARGETED`; levels as assets: a room's layout and screen record are a `rooms::Room` (palette selectors, CHR banks, orientation with named bits, page order, start record, raster split, fixed rows and quads, the player's arrival), a metatile class's behaviour a `collision::Surface` the probes dispatch on (pinned to the ROM's handler tables); scripts as data: a shared runner (`actor::script`) under the ending and the cutscene, which is now a table; dead duplicate decoders deleted (`palettes.rs`, `actors.rs`'s screen records, `collision`'s summary types) |
| 1.1.3 | **done, tag `1.1.3`** (2026-10-17): the third modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged). The actor families are declared once (`families!`: `Brain`, `SecondBrain` for the second-class families, both `new`s and the dispatch from one kind-ordered table); `Game::frame` reads as its steps (`ScriptFrame`, `script_frame`, `loop_body`, `move_player`...); the actors' fields by name: a `slots::Field` per array with the slot aliasing in one mapping, the spawners', parents' and platforms' address-keyed writes typed (`cx.get`/`set`, `Slots::kind`, `platform_parameter`, `pressed`), the part tables through `Field::from_array`; left address-keyed on purpose: `next_area_gate`'s projectile read by actor slot and `turret`'s register-indexed bytes |
| 1.1.4 | **done, tag `1.1.4`** (2026-10-17): the fourth modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged). The pad is `Buttons` in the core (named buttons and groups, `contains` for one button and `intersects` for any of a group; the byte stays at the Classic boundary: `Game::nmi`, the recordings, the oracle); the boot's linear parts are script tables on the shared runner (power-on, the intro, the title set-up, the slide-in and its flicker, the title's chosen flash), deleting its hidden counters |
| 1.1.5 | **done, tag `1.1.5`** (2026-10-17): the fifth modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged). The families' repurposed bytes named where it reads better (flag consts, 16-bit pairs as named values, the dropper's writes into its payload as the payload's own `arm`, the bosses' phase docs saying what each byte holds), `Body`'s docs back to the WRAM meaning; the stun's always-zero shift written as zeros (the ROM's `$92E8` agrees); the ECS spike's results for the owner (`docs/specs/2026-10-17-actors-as-entities-decision.md`: byte-exact, about 4x the core's clean build, about 15% slower chain test, no simpler code) |
| 1.1.6 | **done, tag `1.1.6`** (2026-10-17): the sixth modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged). The named-bytes pattern extended: the spawners', links' and flags' repurposed bytes named in their families; the `hit` byte's engine bits named (`slots::hit`: `DYING`, `FLASHING`, `STRUCK`); the families' 16-bit pairs (the slam's fall speed, the shot's and the bouncer's velocities, the dropper part's offset, the shell's life) as named 16-bit values, their byte-wise carries shown equivalent to 16-bit arithmetic for every input |
| 1.1.7 | **done, tag `1.1.7`** (2026-10-17): the seventh modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged). The player's flag bytes named (`docs/specs/2026-10-17-player-flags-design.md`): `stage_script` (`FROZEN`, `HIDDEN`, `KEEP_PADS`), `busy::SCRIPTED`, `attack` (`CHARGEABLE`, `CHARGING`, `CHARGED`), and the three unrelated bytes that were `flags[0..3]` split into `power_up`, `sliding` and `hurt` (`STUNNED`, `INVULNERABLE`); `solid_below` (it probes above) is `headroom_blocked`, `draw_flash` is `drain_power_up`; `collision::damage`'s doc no longer calls the power-up "already hurt" |
| 1.1.8 | **done, tag `1.1.8`** (2026-10-17): the eighth modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged). The behaviour tables' storage decided (`docs/specs/2026-10-17-behaviour-tables-design.md`): `prg::Prg` owns the PRG image and the tables imported from it once, and derefs to the bytes, so only the path to `Cx` takes it; slice 1, the hitboxes, is `engine::Hitboxes` (the actors', the bosses', `touch`'s hazards, the room exits' reach and the second-class platforms' widths), every index's range read from its writes |
| 1.1.9 | **done, tag `1.1.9`** (2026-10-17): the ninth modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged). The behaviour tables' slice 2 (`docs/specs/2026-10-17-family-tables-design.md`): 38 families' constant tables (velocities, offsets, timers, turn and knockback tables, spawn positions, CHR and palette bytes) are typed `Tables` on `prg.families`, lo/hi pairs merged into 16-bit values, every index's range proven (silent tests for indexes read from the ROM or spawn records); bank 14's `$C5F2/$C6F2` table is `x << 4` and became a shift |
| 1.1.10 | **done, tag `1.1.10`** (2026-10-18): the tenth modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged). The behaviour tables' slice 3 (`docs/specs/2026-10-17-player-tables-design.md`): the player's tables (`player::Tables`: the animation strips by motion, one `Frame` record per `anim`, the muzzle and launch offsets by motion, the slot limits by weapon, the palettes) and the weapons' (`weapons::Tables`) are typed on `Prg`, with `prg.digit_tiles` for the HUD; `engine::rom_split_words` reads split hi/lo tables. Phase C's spec is written and waits for the owner |
| 1.1.11 | **done, tag `1.1.11`** (2026-10-18): the eleventh modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged). Phase C's C1, the draw-call record (`docs/specs/2026-10-18-phase-c-mirror-design.md`): every draw the OAM builder takes is kept as an `oam::DrawRecord` (who drew it, the call, the decoded metasprite) before any piece becomes an OAM byte; `Mark` names the drawer down to the slot (`Actor`, `Second`, `Player`, `Projectile`) and `Mark::is_ui` picks the game's own UI drawers; `finish` moves the frame's records to `shown`, beside the page. Not RAM: the tapes are unchanged. Every lockstep loop frame (the chain, the sweep, the explorer, the tape replay) replays its records on the builder it started from and gets its page and marks back, its records come in the loop's draw order, and every actor and second-class drawer's slot was occupied at the frame's start or end |
| 1.1.12 | **done, tag `1.1.12`** (2026-10-18): the twelfth modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged). Phase C's C2, the read-only mirror (`docs/specs/2026-10-18-phase-c-mirror-design.md`, "C2"): `crates/app/src/mirror.rs` keeps 49 entities for the game's drawers (24 actor, 8 second-class and 16 projectile slots, Batman), each with `Drawer(Mark)`, `LevelPosition` in level pixels and its `Draws` for the frame on screen, the slots also their `Kind`, plus `MirrorCamera` and `OtherDraws`; `Play` takes a `mirror::Snapshot` with the page it shows, so records, page and positions are the same frame's, and the copy reads `Play` only and writes on change. Tested against the game (kinds and positions, a shot's included) and over a level's first 300 frames in a headless app (every mirrored sprite shown has its drawer's records; the records split add up); a snapshot taken a frame late fails it |
| 1.1.13 | **done, tag `1.1.13`** (2026-10-18): the thirteenth modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged, the chain's tape byte-identical). The level's live map typed (`docs/specs/2026-10-18-live-map-design.md`): `levelmap::Map` holds the 16 rows of 256 metatile ids and is indexed by `levelmap::Cell { row, column }` (`Cell::at` the probes' rule that was `collision::map_offset`, `from_address` for the `$6000-$6FFF` addresses ROM tables and spawn records hold, `next` and `in_page` carrying past column `$FF` into the next row as the flat buffer does), with `page_index` the one page rule the probes and `engine::map_page` share; no map site builds a flat offset or subtracts `$6000` any more. The page table `Schedule::rows` is `page_columns` (it holds each page's first column), and the four stage actors that edit it are `page_table_pair_patch`, `page_table_tail_set`, `page_table_tail_clear` and `page_column_reset` |
| 1.1.14 | **done, tag `1.1.14`** (2026-10-18): Phase C's C3, the `Modern` picture (`docs/specs/2026-10-18-modern-picture-design.md`), a new setting `picture: Classic | Modern` (default `Classic`, unchanged), groundwork for 1.2.0; the Classic suites, a random play sweep and a 300-branch exploration unchanged. In play, in rooms with no raster split (and in their pause, power-up and stage-end frames), Modern draws the level's background as four Bevy `TilemapChunk`s over the typed live map (one per background palette, the tileset the 256 tiles of the frame's banks in that palette's colours) and the game's sprites as Bevy sprites from the draw records, children of their drawers' mirror entities, layered by their priority bit and in draw order, with no per-line limit or flicker; every other frame (the script screens, the split rooms) shows the Classic picture. Modern renders into an image the screen quad shows, so scaling, overscan and the faithful UI are unchanged. Proven without a second rasteriser in every lockstep: on the 8,830 Modern frames of the whole chain, every TV-visible background cell from the live map equals the nametables' (left out: blank tiles' palettes and the 8 px edges a scroll streams into; one game difference allowed and counted: in a vertical room, a page's row 14 showing its row 15's palette, which the game streams into row 14's slot as its scroll locks at the page's end, 185 cells on the chain), and every loop frame's visible OAM entries are its records' sprites (equal below 64 placed without an endless metasprite; the builder's two cases counted: a clipped piece's half-written entry, 0 on the chain, and the one entry `finish` leaves unparked after exactly 63, 8). A headless render of stage 1-1 on this machine's software Vulkan matches the Classic picture pixel for pixel over the TV-visible area (`modern::tests::stage_1_renders_modern`) |
| 1.1.15 | **done, tag `1.1.15`** (2026-10-18): the fifteenth modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged). `Outside` removed (`docs/specs/2026-10-18-outside-removal-design.md`, the owner's approved release of its own): `Game::frame` no longer takes a callback to hand the oracle an unported second-class update (every type is ported since 1.0.0), and with it went the lockstep's re-lift, its on-demand peeks and `GameRun::relifted`; a test build asserts in the loop that no live second-class type is unported. **The tape format's change**: the `Relift` and `Outside` events and their wire tags (1, 4) are retired, not reused, so the other events keep their encoding; the 1.1.14 chain tape replays unmodified on the new code, and the tape the new lockstep records is byte-identical to it, so nothing was re-recorded in substance |
| 1.1.16 | **done, tag `1.1.16`** (2026-10-18): the sixteenth modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged, the chain's tape byte-identical). Weapon 5's charge and the 6502 overflow flag as data flow (`docs/specs/2026-10-18-overflow-flow-design.md`): a trace over a sweep, an exploration and a held-B probe found the charge waits, in play, on the flag Batman's frame leaves (B held about 90% of the time, invulnerable standing, a mid-jump turn or a crouch release), which a live second-class actor's draw usually clears; so `Work::cpu_overflow` is gone, `player::step_draw` returns the flag `$84AB` leaves, `draw_second` and `weapons::step` take it and thread it slot to slot, and `super_unleash` takes it with the Classic rule written on it; `bit` is `set_overflow_from`. New tests pin the threading (the charge waits on the incoming flag; a shot above that goes off the screen clears it; `draw_second` with no live actor passes it through) |
| 1.1.17 | **done, tag `1.1.17`** (2026-10-18): the seventeenth modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged, the chain's tape byte-identical). The hurt written once (`docs/specs/2026-10-18-hurt-once-design.md`): the damage tile's response and `engine::hurt_player` share `engine::start_hurt` (the shot pose dropped, stunned and invulnerable for 32 frames, a fall turned into a jump; the knock-back speed away from where Batman faces), each keeping its own rule for where the speed goes; the stunned frame's handler is `stunned` |
| 1.1.18 | **done, tag `1.1.18`** (2026-10-18): the eighteenth modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged, the chain's tape byte-identical). The sprite attribute bits named (`docs/specs/2026-10-18-attribute-bits-design.md`, the survey loop's first item): `oam::attr` (`FLIP_X`, `FLIP_Y`, `BEHIND`, `PALETTE`) replaces the raw masks on the attribute bytes in 31 files, the Classic and Modern pictures decoding the byte with the same names; `Player::faces_left` serves every facing test; `breakable_block::KEEPS_PALETTES` names that family's own switch; the landing's turn test is named as the Classic rule it is (the ROM at `$8427` reads only the facing byte's bit 7, never the pad) |
| 1.1.19 | **done, tag `1.1.19`** (2026-10-18): the nineteenth modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged, the chain's tape byte-identical). The collision probes' results typed (`docs/specs/2026-10-18-probe-results-design.md`, the survey loop's #2): the hit probes (`wall`, `floor`, `feet`, `head`, `ahead`, `column`, `corners`) return true when they hit and `solid`/`below` true when solid, where both had answered in bit 7 with opposite meanings; `collision::probe` names the flag bits the handlers read (`RIGHTWARD`, `LEFTWARD`, `FLOORS_SOLID`); the probes' oracle tests still compare the ROM's exact answer (the A byte for the hit probes, the N flag for `solid`) |
| 1.1.20 | **done, tag `1.1.20`** (2026-10-18): the twentieth modernisation patch, no behaviour change (the Classic suites, a random play sweep and a 300-branch exploration unchanged, the chain's tape byte-identical). The raster split typed (`docs/specs/2026-10-18-raster-split-design.md`, the survey loop's #3): `Scroll::room`'s 13 raw bytes (`$D0-$DC`) are `Scroll::split`, the `rooms::Raster` header a room's screen record already used (armed, the first IRQ's delay as a word, the handler chain, the next IRQ's index), and `Scroll::bands` (`$D5-$DC`), the chain's own bytes; the handlers' delays are words; the chain lockstep reaches every handler chain but the intro's (`$09`, proven by its own oracle test) and `$0B` (checked by hand) |
| 1.1.21 | **done, tag `1.1.21`** (2026-10-18): level packs, release 1 (`docs/specs/2026-10-18-level-packs-release-1-design.md`, on the owner-approved format and the owner's answers): `rotj_core::levels` holds the Tiled JSON model (maps, layers, objects, typed and list properties, tileset references, the `.world`, `pack.json`, unknown fields kept), the exporter (`rotj export-levels --rom <file> --out <dir>` writes the original game as one world: a stage per area named by its banner, a room per Tiled map, one tileset name per (area, CHR) variant, the stage's page map, actors as objects classed by family with `variant`s and `triggers`, scroll-lock markers, `player-start` with facing and scroll mode and lock) and the importer, which refuses malformed packs by name. Proven against the ROM: `import(export(rom))` equals the game's assets in everything the game reads (terrain, rooms, the 590 spawn records the schedule can name, the schedule's triggers and locks), with three owner questions skipped by name and pinned (two rooms' first spawn column, bit 6 of 24 pending cells, the dropped residue), and `export(import(export(rom)))` is byte-equal file by file. The game does not load packs yet (release 3); nothing it does changed (the Classic suites, a sweep and a 300-branch exploration unchanged, the chain's tape byte-identical) |

## Verified working

| area | evidence |
| --- | --- |
| Gates | `.orchestration/gates.sh` (since 1.0.1, with the ROM in `raw/` or `ROTJ_ROM`, which since 1.0.3 the app's build embeds): fmt, clippy pedantic at zero warnings, the release `--ignored` suite of rotj-core, rotj-oracle and the app (the lockstep chain, 19 s, which records the state tape under `target/tapes/` and since 1.0.2 compares every frame's lines; the oracle's whole-game tests; the oracle's picture against fceux's on all 48,100 TAS frames; the app's pixels against the oracle's from power-on), `ROTJ_REQUIRE_ROM=1 cargo test --workspace` (676 tests at 1.0.3, 682 at 1.0.4, 687 at 1.0.5), `tools/check-no-rom-data.py` (no tracked file carries 32 bytes of the ROM), `cargo deny check`, `disasm/assemble.sh` (generates the listing from the ROM, `OK byte-identical`), `tools/boot-check.sh`. Without a ROM `cargo test --workspace` passes with the ROM tests skipped, which is all public CI can run, and the exe built and run without one exits 2 with its "no usable ROM found" message (CI's boot job; each release's manual checks also take a fresh clone without a ROM, whose exe must then boot from `ROTJ_ROM`, the first argument and a ROM beside it). fceux captures the tests once read are local (git-ignored); the tests produce the same bytes with the oracle and pin them to fceux by the committed digests in `*/digests.txt` (`tools/emu/digest.py`). `ROTJ_REQUIRE_ROM=1 ROTJ_RECORD_TAPE=1` is gone: the lockstep always writes the tape |
| Playtest replays and picture parity | the app records a session's pads (`ROTJ_RECORD_INPUT=<dir> cargo run --release -p batman-rotj`, one `<millis>-level<N>.pads` per stage start); `ROTJ_REQUIRE_ROM=1 ROTJ_PLAYTEST=<file> cargo test --release -p rotj-core the_playtest_matches_the_oracle -- --ignored --nocapture` replays it in lockstep from power-on (`oracle::assert_playtest`: one pad per NMI wait, the RNG pinned to the app's, `ROTJ_LEVEL` as the game's own `$D3D8` level poke). Every lockstep also runs the app's own `Game::nmi` on a clone (its RNG, pads and counters must equal the game's) and applies its VRAM writes to a port-side `Ppu` compared with the oracle's nametables and palette at every NMI: on over the whole chain, no divergence. All 13 level starts match for 300 frames. Fixed on the way: the VRAM queue drains last-pushed first (`$C084`), the boss card's attribute row is read from `$D93E`, a level's first load sits over the boot payload (CHR `$F0-$F7`, not the ending's `$F8-$FF`) with the intro's raster bytes. Since 1.0.2 every lockstep also compares the 240 lines each frame is drawn with (`Lockstep::compare_lines`: scroll, `$2000`, `$2001`, the CHR banks per line, split lines either state), asserts the port's own raster IRQs leave the bytes the game's did (`compare_irq_ram`), and fails on a mid-frame PPU write the line model cannot show. Replays inject the oracle's lag NMIs: all 12 of the owner's recorded sessions (2026-10-15/16, up to 11 lag NMIs in one) replay clean at 1.0.5, including the 1.0.1-era one that once diverged at 119 s (most likely the Left+Right `pressed` bug fixed in 1.0.5). Gaps: the port takes no CPU time, so lag frames and loads with the NMI off take no frames in the app; VRAM writes by injected routines |
| Boot | `tools/boot-check.sh` (and CI) must print `OK: still running after Ns`, or for an exe built and run without a ROM (CI) `OK: built and run without a ROM` |
| Title screen | the title the port boots into (reset, intro, title) renders the emulator's frame 300 with 0/57,344 pixels different (`play::tests::the_title_draws_the_emulators_frame`); the boot, intro, title, passwords, attract mode and sound test match the oracle at every NMI wait from power-on (`boot::tests`) |
| Level map | decompressed from the ROM (bank 02 `$89FE` LZ stream over CHR), reproducing the captured map 4096/4096 bytes (`rotj_core::levelmap::map_from_rom`, `docs/LEVELDATA.md` §13). The loader picks its CHR set by area `$0400/2` alone from two 18-entry tables (`$8B63`, `$8B75`); forcing `$0400` at `$89FE` gives 0/4096 differences for 14 of 18 areas. Areas 4/10/12/17 are one-screen maps (their streams decode 16 metatile columns); in stages 2-2 and 5-2 (areas 4, 12) a stage actor, type `$56` (bank 11 `$9DF9`), clears the leftover columns after the load. Found with the oracle, which matches fceux's dumps there byte for byte (§13e, `rotj-oracle/tests/levels.rs`) |
| Background animation | the "parallax" is CHR-bank animation: bank 14 `$D2B0` steps each animated slot `$E0-$E3` through a per-area sequence (stage 1-1: `$E3` cycles `$30-$37` every two frames), decoded from the CHR loader's records and ported (`room::animate_banks` at load, `frame::animate_chr` each frame); every gameplay capture's `$0C00-$0FFF` equals the predicted bank, and all gameplay `$2000/$2005/$2006` writes are in the NMI, so there is no raster split (§16) |
| Attributes | generated from the CHR area definition (auto-tiler `$8B9E`/`$8E57`, class tables `$8ECF`/`$8EE1`): 0/640 vs the captured `$7600` for stage 1-1, and 0 differences on level blocks for 16 of 18 forced loads (`attributes_from_rom`, §13d). The port renders every page from it; the captured-attribute shortcut is gone. While scrolling, the NMI flusher `$C09C` writes one attribute column per 8 px step, rebuilt by bank 02 `$8000-$8143` as `(shadow & mask) | (page & ~mask)` over the entering metatile column; `streamed_attributes` reproduces every frame of three 554-frame scrolling runs (§13d) |
| Stage 1-1 rendering | the whole sublevel (16 pages, 4096 px) from the decoded map; opening frame 0/57,344; rendered nametables match the captures at cameras 0/50/54 (`stage::tests`) |
| Camera | `camera = player x - 112` (feet terms), clamped at the level edges, measured from captures (level x 56/109/185 -> scroll 0/0/54) |
| Physics | the game's own fixed point: the y velocity is a signed byte in 1/16 px (`$046A`, sign `$0469`), gravity +6 per frame clamped to [-128, 127] (bank 4 `$8311`-`$8339`); jump impulse -43 and a held-A thrust of -22 on eligible frames (bank 4 `$8B78`: every other frame by `$F7` bit 7, window `$047D` < 4 held / < 2 released), so a tap rises 23 px and a hold 37 or 54 px; top speed 8704; the walk ramp `v += (target - v + 1) >> 1` gated by the six-frame timer `$047C`/`$A6`; airborne y = ground line + truncated displacement. Standstill and three hold lengths replay exactly through landing (`player.rs` tests, `docs/PHYSICS.md`). Down+A on flat ground is the same jump |
| Collision | the game's rule: metatile id -> class `$7340[id]` (decoded per area from bank 14 `$C000` and bank 0 blobs, equal to the captured table) -> the `$C99D` wall, `$CAFE` floor and `$C93B` point-test handlers, named per class by `collision::Surface` (pinned to the ROM's tables by `surfaces_match_the_handler_tables`; the probes' oracle tests and the lockstep check them); the walk pins on the class-0 block at x 192 as captured |
| Batman | real sprites, the decoded 42-state animation table, OAM byte-for-byte vs the capture; crouch (Down, `$04`) and aim-up (Up+B, `$46`) from captures |
| HUD | live: energy `$0478`, meter `$0477`, icon `$F7` bit 7, verified against captured sprite sets and pixels |
| Input | keyboard + gamepad, handed to the game in its own `$F6` order (Right bit 0, A bit 7; `NesButtons::game_byte`); until 2026-10-15 the app passed the controller's shift order, so Right jumped and A walked (found by the playtest replay's app-NMI check) |
| Audio | 2A03 + DMC synth on the game clock through `bevy_audio`, driven by the ported music driver (`crates/rotj-music`): 769/769 writes for frames 600-700 at a unique offset; 2364 of 3468 for the whole capture. Ids 32+ take the loader's `$8105` path (header pointer in the driver bank); the frame-802 request is the jump effect `$24` from bank 4 `$84FD` (`LDA $046E / CMP #$03 / ... / STA $0042`), and the capture's `3,1,2,0` voice order is the driver's own (`docs/MUSIC.md`) |
| Actors (data only) | spawn records decoded (`rotj actors`; `$D714` 12-byte records, pending schedule `$A2D4[$0400]` -> `$7410`); the slot window: 32 slots scanned as 0-15 and 24-31, `$7CC0` occupied/type, `$7CE0` schedule slot, `$7D50` state flags (bit 7 active, bit 6 done, dispatch through `ASL`), traced at frame 607 (`actors::live_slots`, `docs/ACTORS.md` §7d). The first capture with enemies drawn (`tools/emu/route-dense`, `INPUT=route`): the spawn routine (PC `$8879`-`$88F5`) fills slot 0 from record 7 (`$ACDC`, the soldier on the crate) at frame 846 and slot 3 from record 4 (`$ACB8`, a green enemy) at 894 (§7f, `actors::tests`) |
| Actor core | `rotj_core::actor` (`docs/ACTORS.md` §9): every `$C247`/`$C145` call of route frames 841-999 plus poked clip, wrap, bottom-anchor and overflow cases reproduce the OAM page (`oam::tests`); column and room spawns reproduce every byte they change (`spawn::tests`); type `$0A` matches the oracle's actor loop byte for byte on route frames 894-995 (`k0a::tests::matches_the_oracle`). The harness brackets a routine with oracle peeks and compares every RAM/WRAM byte either side changed |
| Oracle | `tools/rotj-oracle`: a 2A03 CPU, FME-7 (live IRQ), register-level PPU and DMC fetch model timed exactly like fceux 2.6.5's old PPU core; from power-on with `run1`'s recorded input it reproduces fceux's CPU RAM byte for byte for all 3000 frames, including the intro's IRQ chain and a DMC sample, and all 14786 APU writes; a gameplay capture (`oracle-route`, recorded input, no RAM pokes) replays from power-on to frame 900 with RAM, OAM and 3443 APU writes exact (`tests/run1.rs`, `tests/route.rs`; spec `docs/specs/2026-10-14-oracle-design.md` "Validation results"). Runs locally (needs `raw/`); write-only PPU registers and `$2004` read fceux 2.6.5's latch (checked against an fceux capture), and a `$2007` read during rendering is refused by name |
| Whole-game input | the published TAS ([TASVideos 1087M](https://tasvideos.org/1087M), Aglar, FCEU 0.98.16, power-on start, ROM MD5 matches ours) replays in the oracle in sync: the area index `$0400` steps through all 18 areas (`$00`-`$22`) with no life lost, and at frame 2100 (stage 1-2) the oracle's RAM equals fceux's byte for byte. `tools/emu/fetch-tas.sh` fetches and converts it (git-ignored); `rotj-oracle/tests/tas.rs` pins stage 1-1 -> 1-2 and, with `--ignored`, every area (10 s in release). Every level, room and boss is now reachable for captures and oracle diffs |
| Disassembly | recursive descent iterated to a fixed point, far calls resolved through the observed window map, and the game's 48 indirect-jump dispatch tables followed (reached code 4086 -> 9832 instructions; `disasm/MAP.md` §11). Observed register-write PCs, now attributed with the right window map and per-write state, are all instruction starts (`disasm/gen/check_observed.py`) and extend bank 0D (+33). Banks 02/04/05/0A keep the linear-sweep listing: seeding 02/0A from those PCs alone dropped the auto-tiler and the sound driver to data |
| Cut content | song `$06` is defined, never requested (stage table bank 14 `$D980`, sound test bank 12 `$9C85`, inline requests) and loops every 176 frames; the survey's two Game Genie codes belong to other games (`docs/CUTCONTENT.md`) |

## Next, in order

1. **The 1.0 release candidate.** Every gate green on main, the novelty-guided
   explorer's run clean, the owner's playtest signed off: **tagged `1.0.0`**
   (2026-10-15).
2. **1.0.1, bring your own ROM**: **tagged `1.0.1`** (2026-10-16).
   **1.0.2, the raster splits and per-line picture checks**: **tagged `1.0.2`** (2026-10-16).
   **1.0.3, the ROM embedded at build time**: **tagged `1.0.3`** (2026-10-16).
   **1.0.4, the start-up fallback and release downloads**: **tagged `1.0.4`** (2026-10-16).
   **1.0.5, integer scaling, the pops and opposite directions**: **tagged `1.0.5`** (2026-10-16).
3. **1.1** per `docs/PLAN.md` (roadmap agreed with the owner 2026-10-16): 1.1.0 is the
   settings menu and the game's screens in NES-styled Bevy UI (**tagged `1.1.0`**); 1.1.x
   patches then replace every NES-shaped subsystem with modern Bevy code under
   the Classic suites (`docs/specs/2026-10-15-classic-and-enhanced-modes-design.md`); the
   Enhanced features (interpolation, CRT filters, widescreen, remastered audio) follow
   from 1.2.0. **1.1.1, the first modernisation patch: tagged `1.1.1`** (2026-10-17).
   **1.1.2, the second: tagged `1.1.2`** (2026-10-17). **1.1.3, the third: tagged
   `1.1.3`** (2026-10-17). **1.1.4, the fourth: tagged `1.1.4`** (2026-10-17). **1.1.5,
   the fifth: tagged `1.1.5`** (2026-10-17). **1.1.6, the sixth: tagged `1.1.6`**
   (2026-10-17). **1.1.7, the seventh: tagged `1.1.7`** (2026-10-17). **1.1.8, the eighth: tagged
   `1.1.8`** (2026-10-17). **1.1.9, the ninth: tagged `1.1.9`** (2026-10-17). **1.1.10, the tenth: tagged
   `1.1.10`** (2026-10-18). **1.1.11, C1 the draw-call record: tagged `1.1.11`** (2026-10-18).
   **1.1.12, C2 the mirror: tagged `1.1.12`** (2026-10-18). **1.1.13, the live map typed: tagged
   `1.1.13`** (2026-10-18). **1.1.14, C3 the `Modern` picture: tagged `1.1.14`** (2026-10-18).
   The level packs wait for the owner's answers on the schema ("Owner decisions pending").
   **1.1.15, `Outside` removed: tagged `1.1.15`** (2026-10-18). **1.1.16, the overflow flag as data
   flow: tagged `1.1.16`** (2026-10-18). `Body`'s typed per-family state is deferred
   (`docs/specs/2026-10-18-family-fields-views-design.md`). **1.1.17, the hurt written once:
   tagged `1.1.17`** (2026-10-18). **1.1.18, the attribute bits named: tagged `1.1.18`**
   (2026-10-18). **1.1.19, the probe results typed: tagged `1.1.19`** (2026-10-18). **1.1.20, the
   raster split typed: tagged `1.1.20`** (2026-10-18). **1.1.21, level packs release 1: tagged
   `1.1.21`** (2026-10-18). Next: level packs release 2 (the stage descriptor) and the survey
   loop's list (the empty slot named, the PPU names, the projectile, ...).

Settled since the 0.3.0-era list that stood here (2026-10-15): the whole game runs on
the port's own loop and collision (the app's legacy path is deleted), every level and
room is built from the ROM and verified in the whole-game chain, and sound effects come
from the game's own requests.

## Leads not yet verified

- **Odd player energy.** Only a damage-1 contact (a `$04`/`$06` block whose `$7EE0` has
  bit 6 clear; whether any live record does is open) leaves an odd energy, which makes
  the HUD gauge (`$DB9D`, table `$DC0F`) read PPU registers mid-render. The oracle
  refuses a `$2007` read while rendering and the port draws the gauge from bank-14 bytes
  (`docs/ACTORS.md` §9t).
- **`$AC16` with no free slot in types `$14`, `$84` and `$90`** leaves a stale Y the
  `shortcut:` comments say no scene reaches; no sweep run reaches them.
- **`$2002`'s sprite-0 and overflow bits mid-render** are not modelled by the oracle.
- **Bank 0 `$9E3C`** (variant 8 to `$AC16`) looks like dead code; whether another
  `$AC16` caller passes 8 is open (`docs/CUTCONTENT.md` §3).
- **Eight sound effects** with no request site found (`docs/CUTCONTENT.md` §5).
- **`generate.py`'s far-call map** `OBSERVED_WINDOW_BANKS` still uses the
  pre-correction window attribution (`disasm/MAP.md` §11).

## Owner decisions pending

- **Level packs, `first_spawn_column` in two rooms** (`docs/specs/2026-10-18-level-packs-release-1-design.md`):
  area 11 rooms 2 and 3 (`rooms/5-1-3.tmj`, `5-1-4.tmj`) start their spawn schedule at a column
  no approved field carries and no rule derives (the rule fits the other 34 rooms). Release 1 ships
  with the derived value and its proof skips those two fields by name; release 3 (the game loading
  packs) waits for the answer: a `player-start` property, or accept the derived column there.
- **Level packs, bit 6 of 24 pending schedule cells** (stage 7-2, area 17): they read `$C0|id`;
  the format carries the trigger, not that bit, so they import as `$80|id`. The spawner reads
  only bit 7 and the id, so play is the same; release 3's lockstep would see the byte at
  `$7410`. Release 1's proof compares those cells on bit 7 and the id and pins the count.
- **Level packs, `no_handler` in user packs** (`docs/specs/2026-10-18-empty-slot-design.md`): the
  original game's pack names three second-class kinds the ROM has no handler for (ids past area 17
  room 0's records, never reached in play); should the importer refuse them in other packs, since
  placing one would stop the game once it loads packs?
- **Level packs, release 2's knobs** (the survey in `docs/specs/2026-10-18-stage-descriptor-design.md`):
  the engine reads per stage its song, banner/pause digits, the picture before the banner, whether
  it is a continue checkpoint and its password (the pause screen's four letters), and what clearing
  it does. New public names to approve for `pack.json`: `password` (on checkpoints), `cutscene`,
  and the values
  `song-xx`/`keep` (songs are shared between stages, so `"music": "stage-1"` can't name them),
  `picture-xx`, `ending`. The approved `sprite_palettes` has nothing to carry (no sprite palette is
  read by area; rooms carry theirs): drop it? The approved `boss` knob: the boss intro comes from
  the next-area gate's spawn record (stage 3-1 mixes plain and intro gates), so a stage-level
  `boss` can't express it; drop it or keep it as an editor-wizard field (recommended), with typed
  gate properties later. The engine side of release 2 goes ahead without these.
- **Level packs, the schedule residue in release 3**: dropping the residue (the owner's answer)
  changes WRAM bytes the lockstep compares (`$7410-$750F`); release 3's spec will propose how the
  proof treats them.

## Owner decisions made

- **Level packs, release 1 (2026-10-18)**, answering the schema questions in
  `docs/specs/2026-10-18-level-packs-design.md`: a stage carries its area's physical page map
  (an ordered list of the area's pages, each a room region or empty); actors carry a `triggers`
  list (schedule columns) and rooms a `scroll-lock` marker, the schedule's unread residue
  dropped; `player-start` carries `facing`, `scroll_mode` and `scroll_lock`; the equivalence
  proof covers what the game reads; one generated tileset per (area, CHR) variant (27); rooms
  named by the stage's banner, `rooms/<stage>-<room>.tmj` (`rooms/3-2-1.tmj`); one class per
  family, with a typed `variant` enum property (a per-family dropdown in Tiled and the editor)
  where a family has more than one kind.
- **The music driver may be modernised (2026-10-18)**: lifting MODERNIZE §2's rule; the song
  bytecode as a command enum decoded once and the APU registers named, proven by the driver's
  write log against the oracle as today.

- **Phase C (2026-10-18)**, answering `docs/specs/2026-10-18-phase-c-mirror-design.md`: C3 (the
  `Modern` picture from entities) lands late in 1.1.x as groundwork; its background is a tilemap
  from the level data, so the level's live map is typed first; the setting is
  `picture: Classic | Modern`; the read-only mirror is its own release right after C1.

- **The core on Bevy's ECS: the mirror (2026-10-17 morning).** After the spike's results,
  the owner chose the mirror: `Slots` stays the Classic simulation's owner, and the ECS is a
  one-way, read-only mirror at the app boundary for Phase C's renderer, the inspector, the
  editor and the Enhanced picture. In their words, the ECS and Classic's parity are at odds;
  a simulation free of parity would be a separate mode with its own tests.

- **The core on Bevy's ECS: spike first (2026-10-17 night).** Asked whether the core
  should move to `bevy_ecs` or stay plain Rust with a read-only ECS mirror in the app
  (`docs/specs/2026-10-17-actors-as-entities-decision.md`), the owner chose the middle
  option: a throwaway one-family spike in lockstep that measures the build and chain-test
  times and shows an idiomatic ECS family beside today's (`docs/specs/2026-10-17-ecs-spike.md`,
  branch `ecs-spike`), so the choice is made on data. The owner also played 1.1.3 and
  found it the same game ("it looks great").

- **1.0.5 (2026-10-16, after the owner's playtest of 1.0.4).** The picture letterboxes at
  a whole multiple (over filling the window with a sharp-bilinear stretch). The output
  filter is the 90 Hz high-pass and 14 kHz low-pass only: with the NES-001's 440 Hz
  high-pass the music sounded thin and high to the owner. Overscan (lines 0-7 and
  232-239, which a TV hides) is shown as the PPU draws it until 1.1's settings.
- **1.0.4: ROM-less builds find the ROM at start-up, and releases carry downloads
  (2026-10-16, the owner's design).** One rule: an embedded ROM wins; without one, the exe
  tries the first argument, `ROTJ_ROM`, then a `.nes` beside the exe. No cache and no
  `raw/` at start-up. A tag-triggered workflow attaches ROM-less Linux and Windows exes
  to the release (built-in `GITHUB_TOKEN`, no new secrets); 1.0.3 gets none, since its
  ROM-less exe can't play.
- **Reviews go back to the owner (2026-10-16).** Specs and plans: spec -> owner review ->
  plan -> owner review -> execute, with no stand-in reviewers. Code review and test runs
  stay with the `reviewer` agent.
- **1.0.3: the ROM is embedded at build time (2026-10-16; decided as 1.0.2, which the
  picture fix took).** The owner wants a
  self-contained exe: `build.rs` reads the ROM (`ROTJ_ROM` or `raw/`), checks its MD5 and
  compiles it in; 1.0.1's start-up search and cache go. A build without a ROM still
  compiles (public CI) and says so when run. Consequence accepted: an exe built this way
  contains the game and is not shared; others build it with their own ROM. A first-run
  importer (shareable exe) stays possible later, with 1.1's "ROM decoder becomes an importer".
- **The public repository is `github.com/minz1/bevy-rotj` (2026-10-16)**: one orphan
  commit of the 1.0.1 tree (`6d948c6`, tag `1.0.1`); the development history up to it is
  kept locally only (branch `private-history`, tags 0.1.1-1.0.1). Licenses: MIT or
  Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`).

- **1.0.1's open calls (2026-10-16)**, prepared in `docs/specs/2026-10-15-bring-your-own-rom-design.md`
  section 9:
  - **CI gets no ROM for now** (option C): parity stays the local `gates.sh` at every
    merge and tag; public CI runs the ROM-less checks. A private companion ROM repo for CI
    (option A) goes on the roadmap for when outside contributors appear (`docs/PLAN.md`).
  - **The public repo starts from an orphan commit** of a release tree (option A); this
    repo stays private with the history and the tags. Rewriting the history
    (`git filter-repo`, option B) may follow later.
  - **The palette stays, attributed** (`assets/graphics/palette-sources/ATTRIBUTION.md`,
    README); the owner is asking FirebrandX.
  - **The Gemini cut-content survey is not committed** (untracked, git-ignored, kept
    locally); `docs/CUTCONTENT.md` holds what was verified from it.
  - **The lost fceux evidence is accepted**: four of twelve attribute-stream dump records
    and route2's room-change bytes, whose inputs were never recorded.
  - **1.0.1 is tagged once every gate, the reviewer pass and the Windows check are green.**

- **1.0 playtest signed off (2026-10-15).** The owner played the Windows build of main
  (`7188ed0`): boot, title, passwords, stages, deaths, GAME OVER, pause, the app: "it feels
  right"; 1.0.0 follows once every gate and the explorer's run are clean on the release commit.

- **Classic Mode stays verified after 1.0 (2026-10-15).** NES parity becomes the
  default "Classic Mode", guarded by every Classic suite at every merge and tag while
  the code is re-architected; Enhanced Mode settings (frame rate, widescreen, ...) get
  their own tests (`docs/specs/2026-10-15-classic-and-enhanced-modes-design.md`).

- **1.0.0 waits for the owner's playtest (2026-10-15).** When the 1.0 work is done
  and every gate passes, main stops at a release candidate; the owner playtests
  (recording with `ROTJ_RECORD_INPUT`, replayed against the oracle) and the 1.0.0
  release commit and tag follow their sign-off.
- **Stages 2-7 ship as one release, `0.6.0` (2026-10-15).** They landed in one
  merge (`cb07b36`); the PLAN's one-minor-per-stage ladder (0.6.0-0.11.0) is
  collapsed into this tag, and the next release is 1.0.0.

- **Lints and readability (2026-10-14).** Everything is readable by 1.0, the
  music driver included. The app crate opts into the workspace lints and the
  in-code allow blocks go; the one permitted suppression is
  `clippy::needless_pass_by_value` inline on Bevy systems (rust-clippy#8940).
  A post-1.0 refactor pass slims the code further.

- **The TAS input is committed** with attribution (`tools/emu/tas/`, CC BY
  2.0), so CI can run the whole-game tests (2026-10-14).
- **Shipped randomness:** the game's own LFSR (`$C21C`), stepped a fixed number
  of times per frame, the median measured along the TAS; diff tests keep
  injecting the oracle's bytes (2026-10-14). It goes through
  `actor::world::Rng::next_frame`.
- **0.2.0** tagged by the orchestrator with the owner's approval (2026-10-14).

## Decided autonomously (2026-10-14, owner may veto)

- **The embedded ROM's design (2026-10-16, 1.0.3).** Spec
  (`docs/specs/2026-10-16-embedded-rom-design.md`) approved by a stand-in reviewer, all
  six changes applied. The build always writes `prg.bin` and `chr.bin` (empty without a
  ROM) instead of setting a cfg, so one code path is linted with and without a ROM;
  `ROTJ_ROM` must be absolute (the build script, the tests and the shell resolve a
  relative path in three directories); `raw/` is watched only when it exists, so a
  ROM-less build doesn't rerun the script every time (adding the ROM to a new `raw/`
  needs `touch crates/app/build.rs`); `tools/boot-check.sh` goes by the exe's own exit
  instead of guessing whether the build had a ROM.

- **Picture parity's tolerances (2026-10-16, 1.0.2).** Spec and plan
  (`docs/specs/2026-10-16-picture-parity-design.md`, `docs/plans/2026-10-16-picture-parity.md`)
  approved by a stand-in reviewer. The port's lines must equal the oracle's except where
  the real game itself varies frame to frame: a line whose IRQ write lands within the
  IRQ's 10-20 cycle wander of the line's end shows a mix of its neighbours' states, and a
  byte the loop writes in the frame an IRQ reads it (the pause's `$B6`) may be read before
  or after. Those lines are accepted and counted (pinned over the TAS); everything else is
  exact. The IRQ timing is the oracle's mean per handler, not modelled cycle by cycle.

- **Bring your own ROM (2026-10-16, 1.0.1).** Spec and plan
  (`docs/specs/2026-10-15-bring-your-own-rom-design.md`, `docs/plans/2026-10-15-bring-your-own-rom.md`)
  approved by stand-in reviewers, all required changes applied (the MD5 kept, written by
  hand rather than a new crate). Nothing is extracted to disk: the app slices the ROM in
  memory; the cache holds only the remembered ROM. A bad cached ROM is skipped rather than
  fatal. The tape moved to `target/tapes/`. Found on the way: without a ROM, `cargo test
  --workspace` was red at 1.0.0 (19 tests counted 0 oracle calls as a failure, and three
  `expect`ed the ROM); CI never ran, so nothing showed it.

- The oracle spec and plan (`docs/specs/2026-10-14-oracle-design.md`,
  `docs/plans/2026-10-14-oracle.md`) were approved under the owner's delegation
  after a reviewer-agent pass, not by the owner.
- Release order stays stage by stage (every tag playable); the actor handlers
  are ported system by system against the oracle, then stages are assembled.
- Observed register-write PCs seed only banks that already have an entry
  point (`disasm/MAP.md` §11).
- The actor design and plan (`docs/specs/2026-10-14-actors-design.md`,
  `docs/plans/2026-10-14-actors.md`) were approved by a stand-in reviewer under
  the owner's delegation; the core was built to them.
- The actor design and its follow-ups (slots 16-23 sharing, the second-class
  system, the "level edits" change that moves the level map out of read-only
  state) were approved by the orchestrator under the delegation.
- (Superseded by 1.0.1: the app reads the user's ROM.) The app gets the PRG as a committed asset (`assets/prg.bin`), like the CHR
  and music data already are; reproducible with the `rotj` extractor and loaded
  through one function. The 1.0.1 "bring your own ROM" pass replaces all such
  assets with a runtime read of the user's ROM.
- **State tapes (2026-10-15; since 1.0.1 recorded locally, not committed).** Whole-TAS parity moves to a committed tape the
  port replays without the oracle (CI covers it), checked by per-frame FNV-1a
  state hashes; the lockstep stays the ground truth and checks the tape is fresh
  (`docs/specs/2026-10-15-state-tape-design.md`, `docs/plans/2026-10-15-state-tape.md`).
  Spec and plan approved by stand-in reviewers (all changes applied). Implemented
  2026-10-15; the tape is stored gzip-compressed (`flate2`, a dev-dependency already
  in the lock).
- **Off-TAS verification (2026-10-15).** Spec and plan
  (`docs/specs/2026-10-15-off-tas-verification-design.md`,
  `docs/plans/2026-10-15-off-tas-verification.md`) approved by stand-in reviewers,
  all required changes applied: seeded random play from every area start with every
  weapon, and playtest replays from a fresh stage start with the RNG pinned to the
  app's and the app's own NMI checked.
  Implemented: the fresh start pins the title-dependent OAM page and `$FB` on the
  oracle side to the app's values, like the RNG (they depend only on when Start was
  pressed on the title, which the app's own title does not reproduce); picture parity
  (spec section 4) added after a stand-in review.
- **The lockstep chain's place in the gate (2026-10-15).** With the chain at the
  ending it takes 8-10 min in a debug build, so it is `#[ignore]` and the gates run
  it in release with `--ignored` (19 s) at every merge and tag; the state tape
  covers the same run in every plain `cargo test` and CI. The oracle stays the
  ground truth: the tape only replays what the lockstep recorded.
- Claude subagents (Sonnet by default, Opus for foundation code) were used
  alongside DeepSeek until the owner's Claude usage resets.

## Known shortcuts

Each is marked in the code with its upgrade path: `grep -rn 'shortcut:' crates`.
The ones that affect behaviour: the title and a `ROTJ_LEVEL` start play area
0's song (`audio.rs`; later songs come from the game's own `$41` requests); the spread's FME-7 latch write with no free slot is left out as unobservable (`weapons.rs`), not a shortcut; the raster IRQs (all 12 areas since 1.0.2)
run at fixed cycles measured from the oracle (`raster.rs`), so a line where the real
game's write wanders across the line's end shows one of the two states the game shows.
The port takes no CPU time: no lag frames, and a load with the NMI off takes no frames
(the real game blanks 3-4 frames longer at the attract's loads). The app's old
`ROTJ_LEGACY` path is deleted (2026-10-15).

## Bugs the checks missed

Found by the owner's first playtests (2026-10-15, and 1.0.1's and 1.0.4's on 2026-10-16);
each is now pinned by a test.

- **The picture shimmered and the sound popped (1.0.4).** Both sat outside every check:
  the pixel tests compare the 256x240 frame, not how the window scales it (`AutoMin`, about
  5.7x on the owner's screen), and the audio tests compare register writes and the synth,
  not the queue feeding the device (44,040 samples a second against 44,100, each underrun a
  step of about half full scale). Pinned by `the_picture_scales_by_whole_multiples_on_whole_pixels`,
  `a_held_level_settles_to_zero`, `the_frame_size_follows_the_queue` and
  `an_underrun_holds_the_last_sample_and_refills`. The same playtest's replay found the
  port's `pressed` wrong with Left and Right held together
  (`opposite_directions_press_from_the_raw_read`).

- **The boss status bar was garbage, and stage 1 warped at the top (1.0.1).** The game
  splits the screen with FME-7 IRQ chains in 12 areas (per-band scroll, a `$2006` jump to
  another nametable row, the status bar's CHR banks); the port ran 4, and the app drew one
  band with the top's banks. Every lockstep injected the IRQs' RAM bytes, so the core was
  byte-exact while the picture was wrong, and the one pixel test was the title. Fixed by
  porting every handler (`actor/raster.rs`) and checking lines in every lockstep, the
  oracle's lines against fceux's pixels on the whole TAS, and the app's pixels against the
  oracle's from power-on. The same checks found the continue screen and GAME OVER never
  blanking (`$FE`), and the sweep found the pause and the `$D3` 6 and 8 bands, whose IRQs
  read RAM the frame's own loop has just written (the lines now come from the loop's end).

- **No sound on any real device since 0.5.0.** The app registered its synth source
  with `init_asset` only, so Bevy never scheduled the system that plays an
  `AudioPlayer<MusicSource>`; and the source returned `None` when its queue ran dry,
  which ends a rodio source for good. Every test checked requests reaching the driver,
  none a device (WSL has none). Fixed with `add_audio_source` and silence on a starved
  queue (`audio::tests::bevy_audio_plays_the_music_source`, `decoder_drains_the_queue`).
- **The synth played everything an octave high.** `ApuStream` clocked `Apu::tick`
  (one APU cycle, two CPU cycles) at the CPU rate; the driver tests compare register
  writes, which were right (`stream::tests::pulse_period_253_is_a440`, and the
  triangle's).
- **The wrong colour table.** The renderer used Nestopia's RGB-PPU palette (the
  PlayChoice/Vs. look); Classic now uses FirebrandX's Composite Direct, measured from a
  NES's composite output (`palette::tests::the_table_is_the_source_palette`). The pixel
  tests compare palette indices, so they could not see it; a palette choice is a 1.1
  setting.
- **No bass or drum samples.** The app's synth never got the DMC sample bank
  (`set_sample_rom`, the fixed `$E000-$FFFF` window), so the DMC read zeros: the
  driver test had it, the app did not (`audio::tests::the_dmc_reads_the_games_samples`).
- **Start held from the title paused the stage at once.** The app skipped the game's
  stage-start sequence, so a held Start became a new press; resolved by porting the
  boot, title and stage start (the app's interim input mask is gone).
- **Scrambled controls in the app.** The pad reached the game in the controller's shift
  order (Right jumped, A walked); fixed with the playtest replay's app-NMI check
  (`NesButtons::game_byte`).
- **A packaged build could not find its assets.** The app read `assets/` through a
  path fixed at compile time; it now looks beside the executable first
  (`assets_dir`), which the Windows build needs.
- **0.1.0 could not run** (tagged 0.1.1).

## Corrected facts

- "The first area load sits over the ending's payload (`payload_image`, CHR
  `$F8-$FF`)": after boot it is the intro/title payload, CHR `$F0-$F7` copied by bank 2
  `$98FB` (`levelmap::boot_image`); `$F8-$FF` is the cutscene and ending payload.
  `Level::from_rom`'s doc still says otherwise (lead, to fix).

- "The `INC $20` at WRAM `$63D0` is the stage advance": it is the ending's skyline
  loop counter (`$6361` runs 256 times, then `$63D7`); and `$D58F` (`$BA = 5`) is the
  full-meter power-up, not the path into the ending (`docs/SUBLEVELS.md` §7e).

- `tools/emu/run1` is not a no-input capture: it used `dump.lua`'s
  `INPUT=plan` (Start pulses from frame 100, then Right/A/B). The oracle
  replays that input.
- The 15360 "fall cap" was the standstill jump's landing velocity, not a cap;
  the game clamps the velocity byte to 127 (1/16 px).

- `ppu-writes.csv` from `tools/emu/mapper.lua` logged `memory.readbyte(addr)`
  for PPU register writes, which is the bus value, not what was written
  (fixed 2026-10-14). Which writes happened and their PCs were right; the
  value column of earlier captures is not.

- "The auto-tile table entries past an area's id count are stale from the
  previous level": they hold the map stream's bytes, which `$89FE` writes to
  `$7000` before transposing them into the map (`docs/LEVELDATA.md` §15).
- The `$9204` bit-packed table expands to ten digit glyphs, not a collision map.
- `$06D3-$06D8` is the sound driver's page, not a score.
- Lives (`$0482`) are not in the gameplay HUD.
- The second screen's map is at `$6100` (screens are 256-byte pages from
  `$6000`), not `$0E00`.
- B does nothing at the level start; a differential capture shows no sprites
  added. `$7DD0-$7DD2` are per-slot actor bytes (bank 8 `$8BBD` and others write
  them; the room change clears them), not item bytes as first recorded. How
  Batman gets a weapon is unknown (bank 5 `$8000` dispatches on `$0476`).
- "The parallax is an FME-7 IRQ raster program, 21 arms per frame": wrong for
  gameplay (the mapper shadow was misread; no gameplay capture arms the IRQ, and
  `MAP.md` §3's handler addresses never execute). The IRQ *is* used in the
  intro: the NMI writes `$81` to command `$D` in `run1` frames 60-95, fceux
  enters the handler `$E2A9` 480 times in 3000 frames, and the chain at bank 2
  `$95C9` writes `$2000/$2006` (`$08/$09 = $96A1` from frame 61).
- "The y32-96 band is offset 32 px": a sparse-sky fitting artifact. Frames
  800-802 write nothing at all to `$2000/$2005/$2006`.
- "Everything in a dump belongs to the next frame": per component — CHR banks
  yes (NMI rewrites them in vblank), OAM/palette/nametables no
  (`tools/emu/check-frame.py` encodes the measured pairing).
- "Seeding the descent with observed execution collapsed coverage (3274 -> 566
  in bank 0)": an artifact of the harvest's backwards window map and
  frame-final bank state (`disasm/MAP.md` §11). Corrected, the seeds are clean
  and kept for banks that already have an entry.
- "Physics runs in 1/16 px with launch -44 then -60": the game keeps 1/4096 px
  and the launch is -43/-59 in 1/16 terms (`49f1c48`, `498c5ed`).
- "Only metatile `$FF` is solid" and "the camera is a dead-zone follow": both
  replaced by measured rules (`447e6a8`, `11672af`).
