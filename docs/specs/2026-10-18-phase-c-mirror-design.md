# Phase C: the draw-call record, then the picture from entities

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: the drawer is `Mark`;
records hold decoded pieces, not ROM pointers or CHR; records move to a shown list at `finish`;
the replay test from the previous builder plus a drawer check; the oracle's inject keeps
records; the mirror folds into C3; C3's gate is set equality, not a second rasteriser; the
owner questions recommend). **The owner answered (2026-10-18)**: C3 lands late in 1.1.x as
groundwork; its background is a tilemap from the level data, so the level's live map is typed
first; the setting is `picture: Classic | Modern`; the mirror is its own release right after C1.
**C1 implemented in 1.1.11**, with one change to the field list: a record holds the `DrawCall`
and the whole decoded `Metasprite` (`mark`, `call`, `sprite`), so an endless metasprite replays
exactly; the replay test runs in every lockstep's loop frames, from the builder the frame began with.
The right-drawer test became two: the records come in the loop's draw order (the HUD, Batman, the
second class, the weapons, the actors, each from the highest slot down), and an actor or
second-class drawer's slot is occupied at the frame's start or end (a projectile can be fired by
Batman's draw step and freed by its handler in one frame, so its slot can be empty at both).
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

## The owner's answers (2026-10-18)

- **C3 in late 1.1.x**, as groundwork for 1.2.0's interpolation and the other Enhanced features.
- **The background from the level data**, not the software `Frame` or the nametables: the level's
  live map (`Effects::level.map`, a streamed, paged buffer today) becomes typed first, in its own
  spec and release, and C3's background is a tilemap over it. Layers, back to front: the
  backdrop colour, behind-background sprites, the tilemap with colour 0 transparent, front
  sprites; the HUD's split strip is its own layer, as the faithful UI draws it.
- **The setting is `picture: Classic | Modern`**, beside `ui`.
- **The mirror is its own release right after C1** (C2 again), so the inspector and editor can
  start against it; its tests are its own (entities match the occupied slots, records split by
  drawer add up to the frame's).

## C2, the mirror: the details C3's text left open (2026-10-18)

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: no `Visibility` or
hidden rule; each position's source named; the axes stated; records, not positions, place
sprites; one capture helper beside the page copy; tests against the game, not the snapshot).

- **The frame on screen.** `Play` copies the OAM page and marks just after the NMI, before the
  next frame runs (`Play::step`, and `Play::running` at construction); one helper does that copy
  and, beside it, takes a `mirror::Snapshot` of the game: `oam.shown` (that page's records), each
  slot's kind and level position, the player's level position and the camera. The records are the
  picture's truth: C3 places sprites from them. Positions and the camera are the game's state at
  the end of that frame (an actor that moved after drawing is not where its sprite is); they serve
  interpolation and the inspector, and no gate is built on them.
- **Entities** (`crates/app/src/mirror.rs`, `MirrorPlugin`): 49, spawned at `Startup` and never
  despawned, each with `Drawer(Mark)` as its identity (`Actor(0..24)`, `Second(0..8)`,
  `Projectile(0..16)`, `Player`), `LevelPosition(Vec2)` and `Draws(Vec<DrawRecord>)` (this frame's
  records by that drawer, in draw order); the slot entities also carry `Kind(u8)` (the slot's type
  byte, 0 for empty). No `Visibility` or `Transform` yet: C3 adds them when it parents sprites.
- **Positions** are level pixels, y down, the game's 1/16 px words divided by 16: `Body::x/y` for
  actor and second-class slots, `Projectile::level_x_hi:lo`/`level_y_hi:lo` for projectiles,
  `Player::x/y` for Batman. Flipping to Bevy's y-up is C3's job.
- **Resources**: `MirrorCamera(Vec2)` (`World::camera_x/y`, the view's top-left, the same units)
  and `OtherDraws(Vec<DrawRecord>)`, the records whose drawer is no entity (`Scene`, `Hud`,
  `Label`, `Pause`, `Title`, `Password`).
- **The system** `mirror::copy` runs in `Update` after `Step::Game` and before `Step::Picture`,
  reading `Option<Res<Play>>` only; it writes components and the two resources, nothing else.
- **Tests** (app, with the ROM):
  - *Capture*: on `Play::start`'s game, every slot's `Kind` equals the game's kind bytes and each
    position the game's word / 16, a projectile's included (a frame with a shot out).
  - *Timing*: over a level's first frames in a headless `App` with the plugin, every visible page
    entry `Play` shows whose mark is a slot drawer or `Player` has a non-empty `Draws` on that
    entity, which a one-frame skew between page and mirror breaks.
  - *Split*: the records over the entities and `OtherDraws` add up, in draw order per drawer, to
    `oam.shown` as `Play` copied it.

## The questions as asked

1. **When does C3 land?** Its visible gain today (no flicker, no limit) already ships as
   `sprite_limit: false`; what C3 adds is entities for what follows (interpolation, art, the
   inspector). Recommended: C1 now in 1.1.x; C3 as 1.2.0's first release, with interpolation.
2. **C3's background**: the recommended interim (the software background under sprite entities),
   or wait for a tilemap from the level assets?
3. **The setting's name** (e.g. `picture: Classic | Modern`, beside `ui`), once 1 is answered.
4. **The mirror's timing**: with its first reader (recommended), or on its own before?

## Plan

1. C1: `Mark`'s drawer variants and their sites, `DrawRecord`, `records`/`shown`, the oracle's
   inject, the two tests; checks; release.
2. C2, the mirror: `MirrorPlugin`, the entities and resources, its tests; release.
3. The level's live map typed: its own spec (the paged `map`, `map_offset`, the streaming), proven
   by the lockstep with the tapes unchanged; release.
4. C3: its own detailed spec (sprites, the level tilemap and its layers, the set-equality gate,
   `picture: Classic | Modern`), reviewed; implementation; release.
