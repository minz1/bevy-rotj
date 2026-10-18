# Port plan — Batman: Return of the Joker → Rust/Bevy

Status: **1.1.17 tagged** (the whole game byte-exact against the oracle, boot to ending, built with the user's own ROM, every frame's picture checked line by line; `docs/STATUS.md`
is the source of truth). **1.0 means full-game parity** (all 7 stages, 5 bosses,
every actor behaviour, the complete soundtrack). Intermediate minor checkpoints are
tagged as they land. After 1.0.0 only Classic Mode is bound by parity
(`docs/specs/2026-10-15-classic-and-enhanced-modes-design.md`).

## Versioning

Strict [Semantic Versioning 2.0.0](https://semver.org): `MAJOR.MINOR.PATCH`,
numbers only, no letter suffixes or labels that look like versions. The tag, the
workspace version in `Cargo.toml` and the in-game version string are the same.

- **Before 1.0.0:** each milestone is a minor release (`0.2.0`, `0.3.0`, …),
  fixes to a released milestone are patch releases (`0.1.1`).
- **From 1.0.0:** new content or features are minor releases, fixes are patch
  releases, and a major release is only for a breaking change to the public
  interface.
- **Public interface (confirmed 2026-10-14):** save data and settings
  files, passwords and chapter-select codes, and the bonus-level file format
  once it ships. A release that cannot read the previous one's saves, settings
  or levels is a major release.
- Every tag must boot (`tools/boot-check.sh`).

## Release ladder

| tag | contents | state |
| --- | --- | --- |
| **0.1.0** | Engine: NES-exact renderer, 2A03 synth (5 channels, verified against the game's own driver), NES input, title screen (attract frame) and a playable stage 1-1 slice with the game's own physics, collision and animation. Both screens verified 0/57,344 pixels vs the emulator. | **tagged** |
| 0.2.0 | Level streaming from the ROM's own map, so stage 1-1 is walkable end to end and scrolled frames are pixel-exact (there is no mid-frame split; see `docs/STATUS.md`) | **tagged** |
| 0.3.0 | Actor/object system, enemies, weapons and HUD — stage 1-1 as a real level | **tagged** |
| 0.4.0 | Stage 1-2 and the stage-1 boss (Ace Ranger), stage progression, death/continue, pause | **tagged** |
| 0.5.0 | Audio output wired up and the music driver/data decoded (music + sound effects in game) | **tagged** |
| 0.6.0 | Stages 2–7: data, actor behaviours and bosses (planned as one minor per stage; they landed together, so one release) | **tagged** |
| **1.0.0** | **Full-game parity**: all stages, bosses, music, HUD, endings | **tagged** |
| 1.0.1 | The "bring your own ROM" pass (below), so the repo can be public. 1.0.x releases keep parity | **tagged** |
| 1.0.2 | The raster splits of all 12 areas (the boss status bars), and every frame's lines checked against the oracle, the oracle's against fceux's pixels | **tagged** |
| 1.0.3 | The ROM embedded at build time: a self-contained exe (the owner's call, 2026-10-16) | **tagged** |
| 1.0.4 | ROM-less builds find the ROM at start-up (argument, `ROTJ_ROM`, beside the exe), and each release carries ROM-less Windows and Linux exes (the owner's design, 2026-10-16) | **tagged** |
| 1.0.5 | The owner's playtest of 1.0.4: integer scaling, no audio pops (the output filter and a queue that follows the device), opposite directions as the game reads them | **tagged** |
| 1.1.0 | The settings file and menu, one picture per tick (`docs/specs/2026-10-16-settings-menu-design.md`), and the game's own HUD, pause card, title cursor and password entry as Bevy UI, the HUD label steady (`docs/specs/2026-10-16-faithful-ui-design.md`) | **tagged** |
| 1.1.x | Modernisation, one patch per step, no behaviour change (owner, 2026-10-16: "replace the NES stuff ALL with modern patterns and modern tech"): the GPU renderer (Bevy tilemaps and sprites; the GPU's readback must equal `Frame`), the game loop on Bevy schedules and `States`, the player as a character (enum state machines, the ECS pilot), actors mirrored to entities for everything that reads them (the Classic simulation stays `Slots`, owner 2026-10-17), levels as assets (an importer run on the player's ROM at build time), scripts as data, input as actions | |
| 1.2.0 onward | Enhanced features, each once its prerequisite has landed: interpolated motion and a `frame_rate` setting (after the GPU renderer and actors as entities), CRT filters (after the renderer), widescreen (after actors and levels), remastered audio, and an audio setting where sound effects play on their own voices instead of taking the music's channels (the owner, 2026-10-17: "modern computers can do unlimited sound") | |
| then | Chapter select and passwords (revisit: the game's own password screen is ported) | |
| then | Prototype music (enhanced mode) | |
| then | Post-game bonus levels authored in Tiled, NES-strict | |
| then | Enhanced look for the bonus levels, prototype bosses, alternate ending, cut-content restorations, overworld map: one minor release each, in the order they land | |
| then | Enhanced effects: native-resolution render upscaled, 2D lighting at the player's resolution (Classic untouched) | |
| then | Community tools: a level editor on the bonus-level format, then level sharing | |
| maybe | A Robin or Nightwing mode: the player becomes a character definition, Batman the verified Classic one; a minor release unless it breaks saves or the level format | |
| 2.0.0 | Reserved for the first breaking change to the public interface (see Versioning) | |

Scope (owner, 2026-10-15): restore what the game cut and give the community tools;
no new original content beyond that (new ideas belong in a new game). After 1.0 the
code is modernised seam by seam, the ones these features cross first (the level
format, actor placement, the player as a character, menus); the stretch goal is a full
re-architecture into a modern game with Classic Mode intact. It is feasible because
every piece of the port's state already projects to the NES's RAM layout
(`lift`/`project_into`), so a modern core can hold its state however it likes and still
be compared byte for byte; each subsystem is replaced under the Classic suites. UI and
the HUD are reimplemented in Bevy UI, NES-styled.

### Re-architecture outline (owner: "all the way", 2026-10-15)

End state: the only NES-shaped code is test-side (the oracle and each subsystem's
projection to the NES RAM layout). Actors are Bevy entities with typed components and
one readable state machine per family; levels load as assets in the documented format
(the ROM decoder becomes an importer); the player is a character definition (moveset,
physics constants, sprites, weapons); cutscenes, banners and the ending are data-driven
sequences; rendering and UI are Bevy tilemaps, sprites and UI, NES-styled in Classic.

What Classic keeps is the game's semantics, accidents included: the update order
(slots 23 down to 0), which free slot a spawn takes, when and how often the RNG steps,
fixed-point wraparound, weapon 5's stale overflow flag. In the modern core each becomes
a named, tested rule instead of a property of the code's shape.

Order (strangler fig, each step under green Classic suites and unchanged tapes):
presentation and UI; levels as assets; the player as a character; actors family by
family (a reviewed pilot first, then agents per family, each proven by its oracle
test); scripts. The steps ride along with 1.x releases rather than gating them. The
spec is written after 1.0.1, starting with the pilot (one actor family and the player).

### Revisit for 1.1 (modernisation candidates, collected as found)

The ranked survey with file:line, risk and the Classic proof for each item is
`docs/MODERNIZE.md` (2026-10-15, verified by a second reviewer: crates checked on crates.io).

- The app's NES-style compositor (`play::compose`, nametables and band splits) ->
  Bevy tilemaps and sprites, checked against Classic's picture.
- Title, HUD and pause screen -> Bevy UI with NES styling.
- The 32 actor slots as fixed arrays (`Slots`) -> a read-only entity mirror for the
  renderer, inspector and editor; `Slots` stays the Classic simulation (owner, 2026-10-17).
- Routine-shaped modules mirroring the 6502 listing -> systems named for what they do (the
  `Outside` brackets went in 1.1.15).
- Settings passed as one value into the core (`docs/specs/2026-10-15-classic-and-enhanced-modes-design.md`).
- Frame pacing (owner, 2026-10-15, a 170 Hz display): the 60 Hz `FixedUpdate` tick is
  not aligned with the display, so input waits 0-1 game frames unevenly and motion
  judders; the interpolation work fixes it, with a vsync setting and the queued-frame
  count (1 since 2026-10-15) exposed. Also a "no sprite limit" setting for the NES's
  8-per-line flicker, and a palette choice (Classic: Composite Direct).
- The player (`actor/player.rs`, a motion byte `$046E` dispatching to handlers) -> an
  explicit enum state machine, as part of the player as a character (owner, 2026-10-15).
- The background CHR animation (`frame::animate_chr`) -> a Bevy texture-atlas animation,
  with the compositor swap.
- From the agents' reports (2026-10-15): `Brain::Unported`/`engine::unported` are
  unreachable now that all 100 types are ported (an unreachable panic, or gone);
  `shot_impact_burst`/`harmless_burst` share the parts loop and `integrate`, `stage3_boss_shot`/`enemy_shot` the step-table
  projectile shape; `player::step_move` works on a copy of the player that the probe
  wrappers sync (work on `cx.effects.player` directly); stale test scaffolding that
  frees `$90`/`$46` children each loop (`stage3_boss`, `stage4_boss`); blending the intro's 30 Hz
  logo/subtitle flicker into one picture (an Enhanced option).
- Audio: `rotj-music` (the game's driver: which notes when) and `rotj-apu` (the 2A03:
  how they sound) stay as Classic's sound and its spec; their shape becomes a
  Bevy-native audio source. Enhanced audio uses modern libraries.

Once the enhancements above have landed, a public write-up of the method (owner, 2026-10-15): the
6502 oracle and the lockstep, whole-game state tapes, digests of the emulator captures, seeded and novelty-guided
exploration, playtest replays and picture parity.

After 1.0.0 parity binds Classic Mode only, which stays the default and stays verified
at every merge and tag; a modern render path and anything that changes the game's
behaviour (the 1/16-pixel fixed point, the camera, one-hit deaths) arrive as Enhanced
Mode settings layered over it (`docs/specs/2026-10-15-classic-and-enhanced-modes-design.md`). `disasm/` stays byte-identical as a baseline check, with ROM patches as
separate artifacts.

**Widescreen (decided 2026-10-14; an Enhanced feature after 1.1.x).** A settings option that widens the
view past the NES's 256 px; classic 256 px stays the default. The extra columns
come from the level map the port already streams, but actors spawn as the level
pages forward (`docs/ACTORS.md` §5a), timed for a 256 px screen, so the spawn
window has to widen with the view or enemies pop in visibly. The HUD stays anchored to the
centre 256 px.

## Bring your own ROM (1.0.1, decided 2026-10-14, done 2026-10-16)

The repo distributes none of the game, so it can go public
(`docs/specs/2026-10-15-bring-your-own-rom-design.md`, `docs/plans/2026-10-15-bring-your-own-rom.md`):
the app, the tests, the oracle and the disassembly read the user's ROM through
`crates/rotj-rom` (MD5-checked); the `.s` listing is generated locally by
`disasm/assemble.sh`; fceux captures are replaced by oracle runs pinned to fceux by
committed digests; the state tape is recorded locally; ROM tables over 32 bytes in the
source are read from the ROM. `tools/check-no-rom-data.py` guards the tree. 1.0.x keeps
parity: every Classic suite runs where a ROM is. Still the owner's call: where CI gets a
ROM and the clean first public commit: decided 2026-10-16 (STATUS "Owner decisions made").

Roadmap for going public (owner, 2026-10-16): the public repository starts from an
orphan commit of a release tree (a history rewrite may follow); CI runs without a ROM
until outside contributors appear, then gets one from a private companion repository
(a read-only token, on pushes to main, tags and manual runs only). Before the orphan
commit: the license files and the `repository` URL (added 2026-10-16; the public repo is
`github.com/minz1/bevy-rotj`), the internal notes stay untracked, and FirebrandX's answer on
the palette (asked; kept with attribution meanwhile).

The TAS input (`tools/emu/tas/`, CC BY 2.0 with attribution) and all original
code and documentation stay.

## Hard requirements (user)

- Full Rust rewrite in **Bevy** (current: 0.20), **no `unsafe`** anywhere
  (workspace lint `unsafe_code = "deny"`; CI enforces).
- **Latest everything** (toolchain, crates, configs).
- Community best-practice repo org, linters, CI present and enforced.
- All assets extracted into `assets/`.
- Buildable, byte-identical 6502 disassembly kept in `disasm/` (we will patch).
- Audio synthesized natively in safe Rust (2A03 + DMC for this cart; see
  `docs/RE-NOTES.md` — ROTJ has no expansion audio).

## Layout

| Path | Purpose |
| --- | --- |
| `raw/` | your ROM in a development checkout (untracked) |
| `disasm/` | ca65/ld65 disassembly; `assemble.sh` must rebuild byte-identical ROM |
| `tools/rotj-tools` | `rotj` CLI: extraction pipeline (CHR, palettes, levels, music) |
| `crates/rotj-core` | shared formats: iNES parsing, CHR decode, level/palette/music data |
| `crates/rotj-apu` | 2A03 synthesis cores (pulse×2, triangle, noise, DMC) |
| `crates/app` | Bevy game: fixed 60 Hz, NES-pixel world, states, player, render |
| `assets/` | local `rotj` extractor output (untracked since 1.0.1; the game reads the ROM) |
| `docs/` | plan + RE notes |

## Milestones

1. **Toolchain & CI** — done: workspace, lints, deny, GitHub Actions.
2. **ROM mapping & CHR extraction** — done: mapper identified, 32 CHR banks
   rendered with tested decoder.
3. **Disassembly bootstrap** — in progress (background agent): per-bank `.s`,
   ld65 config for FME-7 windows, `assemble.sh` verifying byte-identical
   rebuild. Next: deepen bank 15 (reset/NMI/IRQ handlers), recursive descent,
   labels, bank-trace FME-7 writes.
4. **Asset extraction depth** — next after (3):
   - palette tables (bg + sprite per stage) → re-render CHR with real colors
   - nametable/level data per sublevel → `assets/levels/*.ron`-ish
   - sprite/actor tables, enemy definitions
   - music data (note tables, instrument/envelope tables) → `assets/audio/*`
5. **Engine** — Bevy systems mirroring the NES engine:
   - PPU-equivalent: tile background renderer with per-band CHR bank swap
     (parallax), sprite layer with 8×8/8×16 tiles + priority, palette RAM
   - camera / scroll (NES-style per-axis scroll + forced blanking behaviors)
   - player physics mined from disasm + Mesen watchpoints (RAM `$009E`,
     `$00C3`, `$0466`, `$047D` leads)
   - object/actor system (behaviors per enemy table), collision, weapons
   - stage progression, checkpoints, HUD, bosses
6. **Audio** — `rotj-apu` register file + mixer are in place (cores verified
   against Tetanes; triangle DAC-hold bug fixed). Remaining: DMC core (needs the
   disassembly's sample layout), music driver decode, then output wiring.
   **Output decision (2026-10-13): use Bevy's built-in `bevy_audio`** with a
   custom source that pulls samples from `Apu::tick`/`Apu::output` — no new
   dependency and works on Bevy 0.20 today. `bevy_seedling` (Firewheel) is the
   nicer future path but its latest release (0.8.0, 2026-08-09) targets Bevy
   0.19; revisit when a 0.20-compatible release exists.
7. **Verification** — frame-by-frame comparison against real ROM rendering
   (VGMaps + emulator reference), input/physics acceptance tests.

## Post-1.0 content: cherry-picking the 16-bit versions (planned, not ported)

ROMs in `raw/` (untracked): the NES original, the SNES prototype
(`Batman - Revenge of the Joker (USA) (Proto).sfc`, the unreleased ICOM build)
and the Genesis retail port (`… (USA).md`). The 16-bit platforming itself is not
worth porting — only these four things are.

### Prototype music (after the Enhanced features, small)

The prototype carries arrangements of NES tunes that its engine never calls
(title intro ID 01, transition jingles IDs 09/0C).

- **Method: record, don't reimplement.** Force the track IDs in an SNES emulator
  (Mesen2 is already downloaded; the survey's PAR code forces the IDs), capture
  the audio output as the emulator's own mix, and store it under
  `assets/audio/snes/`. Enhanced-mode soundtrack.
- **Verification:** the artifact *is* the emulator's output, so there is nothing
  to re-derive; we check tempo/loop points and that the track is the expected
  arrangement.
- **Caveat:** these are 16-bit sample-based arrangements, not 2A03 renditions.
  They belong in enhanced mode. A faithful-mode version would mean *hand-
  arranging* them for our 2A03 synth (creative work, optional).

### Chapter select and passwords (after the Enhanced features, small)

The Genesis retail build hides a level select behind `xy00` passwords
(stage `x`, sub-area `y`); the Game Boy port has three disabled passwords.

- **Method:** our engine already models stage/sublevel directly, so this is a
  native menu (no ROM logic to extract) that sets those indices, with the
  documented passwords as preset entries.
- **Access (decided):** entries are greyed out until the game is beaten; a
  settings toggle lifts that requirement. The bonus levels appear here
  under the same rule.
- **Verification:** our own UI; parity does not apply.

### Prototype boss encounters (later, large — a project, not an extraction)

The prototype's Joker fights use new AI. This is 65816 code in an *unfinished*
build, so "extracting the routines" is not the right framing:

- **Method:** capture the prototype's fights from an SNES emulator (scripted
  input plus per-frame state: positions, animation, hit timers), then
  reimplement the *observable* behaviour in our actor system and verify it
  frame-by-frame against those captures — the same method that proves the
  renderer today.
- **Prerequisites:** our actor system (0.3 onward), a SNES capture harness
  (Mesen2 scripting), and a SNES asset extractor for boss graphics (4bpp tiles,
  15-bit palettes, tilemaps) — new tooling, moderate.
- **Caveat:** the prototype's AI may be unfinished or buggy; making it playable
  may mean *finishing* it, with no ground truth to match.
- **Scope proposal:** one boss as a proof, then the rest. Deliver as a
  "Prototype Boss" toggle and/or a boss-rush mode.

### Alternate ending (later, moderate)

The prototype has a different ending sequence.

- **Method:** extract its assets with the SNES extractor from the prototype-boss work and
  reimplement the sequence as our own cutscene, gated behind a challenge (e.g.
  clearing without continues, or a difficulty option).
- **Verification:** assets verified against the emulator; the sequence itself is
  our implementation, so it is documented as an enhancement, not parity.

### Bonus levels (after the chapter select and prototype music, decided 2026-10-14)

Post-game levels built from the content the game never used: the unused NES
tileset, and level *shapes* taken from the SNES prototype where its layouts
differ from the NES (the Genesis build is mostly the NES levels redrawn, so it is
a low-value source).

- **Foundation: write the level format, not just read it.** The decoders in
  `rotj-core` (metatile map, attribute classes, spawn records) get encoders, and
  levels are authored in [Tiled](https://www.mapeditor.org/) with an importer
  into our format. One path serves every case: a bonus level, an imported
  layout, and restored cut content such as the stage 3-2 spiked balls (which
  are just spawn records).
- **Unused tileset:** the asset is 8x8 tiles only. Someone groups them into the
  game's 16 px metatiles and draws the map. Before relying on the survey,
  prove the banks are unused: no stage's CHR table selects them (needs the
  stage/sublevel -> CHR-bank rule from `docs/LEVELDATA.md` §13e).
- **Imported layouts:** capture the prototype level's solid/empty grid from an
  SNES emulator (the same capture method this project uses), resample it onto
  the NES 16 px grid, emit a Tiled map, skin it with NES tiles, then hand-tune
  it for NES physics (Batman's jump rises 23 px; 16-bit gaps will not carry
  over as-is).
- **Look:** NES-strict first (NES palette, sprite limits, same renderer as the
  real stages). An enhanced look comes later as a follow-up patch.
- **Access:** post-game, and in the chapter select under its unlock rule.
- **Later, optional:** an in-game level editor. Tiled covers authoring; an
  in-game editor would be a player-facing feature, not a prerequisite.

### Not doing

Running the 16-bit games' engines or graphics, reimplementing the SPC700/DSP or
the Genesis sound hardware. (16-bit level *layouts* are allowed only as design
input for NES-styled bonus levels.)

### How this fits the faithful path

SNES assets cannot go through the parity path: the NES PPU shows 4 colours per
tile from 64, with 8 sprites per scanline, while the SNES is 4bpp/15-bit; and the
SNES audio is sampled, not the 2A03's synth. So everything above lands as an
*enhanced mode* alongside the faithful one, sharing the same game logic.

The settings menu (1.1.0), the modernisation (1.1.x) and the Enhanced features come
first, then the enhancement work above. User's goal,
in their words: "i don't want to leave anything on the cutting room floor".

## Cut content and cross-platform reference (after 1.1.0)

The owner's AI-generated cut-content survey (Gemini, kept locally, not in the repository) lists the
content the retail ROM still carries but never uses: overworld map icons
(blimp, factory), Bat-Signal lighting variants, spiked-ball duplicates in the
Stage 3-2 bank, shattered-rock debris, an alternate sewer-water animation, two
unused flight-stage enemies and an obstacle, unused audio, and unused tracks.

This project is already the right foundation for that work: the disassembly
reassembles byte-identically, so restoring content is a code change in `disasm/`
plus a native implementation in the port, not a hex-patching exercise.

The survey is a lead list, not evidence: it is a model-written synthesis of The
Cutting Room Floor and forum posts, and at least one of its claims (the IRQ
raster parallax) is contradicted by this project's own measurements (see
`docs/STATUS.md` "Retracted findings"). Each item is verified against the ROM
before it is built. By effort:

| effort | content | needs |
| --- | --- | --- |
| provable now | the unused song (NSF track 6; map NSF numbering to the driver's song table first), the debug modes (the Game Genie codes decode to exact ROM addresses) | nothing |
| small, after 1.0 | stage 3-2 spiked balls, stage 3 boss shattered-rock debris, alternate sewer water, Joker password-screen fixes (OAM y-8, hair tiles), Bat-Signal variant | actors and those stages ported; the level encoder (bonus levels) for spawn edits |
| design work | overworld map with the blimp/factory icons, unused flight enemies (no original AI exists), a level for the unknown tileset (bonus levels), the "movement hack" as a modern-controls setting (settings menu) | the level encoder and design decisions |

The **SNES prototype** (an unreleased ICOM build, with altered levels, missing
stages, redesigned bosses and some unused NES tracks) is a *reference* for this
phase rather than a parallel port: it is 65816 + SNES PPU, so none of the NES
tooling (6502 disassembler, FME-7 mapper, 2A03 synth, PPU renderer) carries over.
Treat it as a source of design intent and, for the audio, of alternative
arrangements — not as a second port until this one is finished.


## Open decisions / risks

- Audio output: settled — `bevy_audio` + custom APU source (see milestone 6).
- Byte-identical disassembly: banked code makes single-pass disassembly
  imperfect; the data/code split will be improved iteratively but the build must
  always reassemble to the original ROM.
- Emulator-free palette recovery may need reference screenshots (VGMaps) to
  confirm which palette belongs to which scene.
- APU correctness vs real hardware is not yet proven by waveform comparison;
  the cores are only verified against the reference implementation's semantics.
