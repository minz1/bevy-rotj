# Phase C: the draw-call record, then the picture from entities

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: the drawer is `Mark`;
records hold decoded pieces, not ROM pointers or CHR; records move to a shown list at `finish`;
the replay test from the previous builder plus a drawer check; the oracle's inject keeps
records; the mirror folds into C3; C3's gate is set equality, not a second rasteriser; the
owner questions recommend). **Waiting for the owner** before any implementation (the HANDOFF).
Phase C of `docs/specs/2026-10-16-modernisation-roadmap-design.md`, with the owner's mirror
decision (`docs/specs/2026-10-17-actors-as-entities-decision.md`): `rotj-core` stays plain
Rust; the ECS is a one-way, read-only mirror in the app.

## What this is, and is not

- **C1, the draw-call record** (core, one release): every draw the OAM builder takes is kept,
  with who drew it and its pieces, before it becomes OAM bytes. Classic's picture and RAM are
  untouched.
- **C3, the picture from entities** (app, later): the read-only mirror (actors, player,
  projectiles, camera as entities) built together with its first reader, an Enhanced picture
  whose sprites are Bevy entities from C1's records.

Not here: interpolation, `frame_rate`, CRT, widescreen (built on C3), the inspector and the
editor (they read the mirror later), the authored level format. **Classic's picture does not
change**: the software `Frame` from the PPU state and OAM, byte-checked as today. (C2, a mirror
with no reader, is folded into C3: nothing would read its entities until then.)

## What the survey found

- The picture is one 256x240 `Image` from `Frame::indices()` (`app/screen.rs`); the 8-per-line
  limit is `Frame::render`'s `take(sprites_per_line)`, and `sprite_limit: false` already shows
  every sprite with no flicker.
- `oam::DrawCall` holds the screen position, `attr`, `tile`, `chr_slot` and `bottom`; `Mark`
  (`Scene`, `Hud`, `Label`, `Pause`, `Title`, `Password`) tags each OAM entry with its drawer
  kind, set through `OamBuilder::marking`. `OamBuilder::draw` takes a `&Metasprite`, and some
  are built from RAM or patched bytes (the pause name, the cutscenes' WRAM payloads, the sound
  test's digits), so a ROM pointer is not every metasprite's identity.
- After OAM: the actor and the metasprite are gone, pieces are 8-bit screen coordinates, the
  allocator rotates its start by `$4C` a frame, overflow drops draws, and a piece clipped in x
  keeps the entry's old tile byte.
- `finish` runs at the end of `loop_body`, before `Play` copies the page; `animate_chr` runs
  after `finish`, and the picture's sprite banks come from `Line::sprite_chr`.
- The oracle's `apply` re-lifts `Game` on every inject and keeps only `script` and `marks`.
- `ui.rs` already mirrors OAM into Bevy UI pieces each frame: the pattern C3's sprites follow.
- Bevy 0.20 has `Sprite` with atlases and flips and a `TilemapChunk`; this WSL machine has no
  Vulkan GPU (lavapipe only), so GPU readback is unproven here.

## C1: the draw-call record (core)

- **The drawer is `Mark`**, extended with `Actor(slot)`, `Second(slot)`, `Player` and
  `Projectile(slot)`, set through the existing `marking` by the loops that know it: the actor
  walk (`work.slot`), `draw_second`'s loop, `Game::draw_player` itself (it is called from the
  stages too), the weapons' slot loop. `Scene` stays for anything else in the loop; the scripts
  keep their marks. No `kind`: a handler can change it before it draws. One "who drew" type,
  and the per-entry `marks` gain the slot for free.
- **`oam::DrawRecord { mark, x, y, attr, tile, chr_slot, bottom, pieces }`**, `pieces` the
  metasprite's decoded pieces (the same `Piece`s the builder places), pushed by
  `OamBuilder::draw` for every call, before any piece is clipped or dropped. No CHR banks (the
  picture's come from `Line::sprite_chr`) and no ROM key (add an optional one when art
  replacement needs it).
- **Lifetime**: `draw` pushes into `records`; `finish` (and `clear`) move them into `shown`, the
  records of the page just finished, so `Play` copies `shown` beside the page, and a frame that
  doesn't redraw keeps its records with its page.
- **Not RAM**: outside the projection and the lift, so the oracle can't see them and the tapes
  are unchanged; the oracle's inject keeps `records`/`shown` as it keeps `script` and `marks`.
  `Game`'s `Clone`/`PartialEq` cover them; tape.rs doesn't serialise `Game`.
- **Tests**:
  - *Complete*: over the chain's frames, replaying a frame's `shown` records through a clone of
    the builder taken at the previous `finish` reproduces the page's visible entries and marks.
  - *Right drawer*: every `Actor(slot)`, `Second(slot)` and `Projectile(slot)` record names a
    slot that is occupied that frame.

## C3: the picture from entities (app; after the owner's answers)

- **The mirror, built with its first reader**: 24 actor, 8 second-class and 16 projectile slot
  entities plus the player, spawned once and kept (an empty slot hidden, so ids stay stable for
  the inspector), components copied from `Play` each tick (`Kind`, level position, this frame's
  records by drawer), resources for the camera and the non-entity records. A system ordered
  `.before(Step::Picture)`; no `ResMut<Play>` anywhere in it.
- **Sprites**: each record's pieces become `Sprite` children of its drawer (8x16, an atlas per
  CHR bank set and palette as `ui.rs` builds, flips from the attr), placed from the record now
  and from the level position and camera once interpolation needs it.
- **Background (recommended interim)**: the software `Frame` rendered with sprites off as the
  background layer, sprite entities on top: it keeps the raster splits and behind-background
  priority that a `TilemapChunk` (no per-line scroll, one tileset per chunk) can't. A tilemap
  from the level assets comes with widescreen, once the level's live map is typed.
- **The gate**: set equality, no second rasteriser: per frame, the scene's sprites (position,
  tile, palette, flips) equal the page's visible entries as a set, and the background layer is
  the `Frame` itself. Draw order on overlap is the OAM order (the rotation decides who is on top),
  which the scene keeps.
- **The setting**: a new value or key picks C3's picture; its name is the owner's.

## Questions for the owner

1. **When does C3 land?** Its visible gain today (no flicker, no limit) already ships as
   `sprite_limit: false`; what C3 adds is entities for what follows (interpolation, art, the
   inspector). Recommended: C1 now in 1.1.x; C3 as 1.2.0's first release, with interpolation.
2. **C3's background**: the recommended interim (the software background under sprite entities),
   or wait for a tilemap from the level assets?
3. **The setting's name** (e.g. `picture: Classic | Modern`, beside `ui`), once 1 is answered.
4. **The mirror's timing**: with its first reader (recommended), or on its own before?

## Plan

1. C1 (after the owner's go): `Mark`'s drawer variants and their sites, `DrawRecord`,
   `records`/`shown`, the oracle's inject, the two tests; checks; release.
2. C3 per the owner's answers.
