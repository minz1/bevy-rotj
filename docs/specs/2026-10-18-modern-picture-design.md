# C3: the Modern picture

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: the model from the
snapshot, the gate at the NMI's state; the stale edge columns, the final boss's death tiles and the
left 8 px named; four 8x8 tilemaps, one per palette, tile byte = layer; sprites by draw order;
sprite sets, not multisets, with "no overflow" defined; one pure per-piece placement; the rule on
the live `$D0` and its coverage stated; the camera off on Classic frames, the clear colour).
Phase C's C3 (`docs/specs/2026-10-18-phase-c-mirror-design.md`, the owner's answers): the picture
from entities, groundwork for 1.2.0's interpolation and widescreen. Built on C1's draw records,
C2's mirror and the typed live map (`docs/specs/2026-10-18-live-map-design.md`). The owner
approved releasing it unseen.

## What the survey found

- **The raster splits** (`actor/raster.rs`, `$D3` per room): `$D3` 0, 1, 3, 5, 6, 7 and 8 rooms
  scroll bands of the screen on their own (parallax bands, swinging rows, a boss floor) over fixed
  nametable rows drawn once at room entry, not from the camera's window; `$D3` 2 shows a band from a
  nametable address. `$D3` 4 (the bosses of areas `$0E $14 $1E $22`) keeps an ordinary playfield on
  lines 0-191 and switches CHR banks for the boss bar below, whose tiles are nametable rows 24-29
  from ROM and whose digits are VRAM-only. `$D3` 9-`$0D` are the boot's, the cutscenes' and the
  ending's.
- **Not the map**: the boot's screens, the ending, the room-change slide, most of the next-area
  load (fades, banner, cutscene, boss card, continue). The map stays valid in a pause, a power-up,
  a stage end and with the background masked off.
- **The HUD is all sprites** (`frame::draw_hud`); the only background strip in play is the boss bar.
- **Background CHR**: banks 0-3 (`oam.chr[0..4]`, the room's), the same on every line outside the
  bar; animated groups step a bank every 2-8 frames (`frame::animate_chr`). Palettes change with
  fades, the power-up flash, the pause dim and the bosses' flashes.
- **Attributes**: `Level::attributes` is in the level's layout (the streamed VRAM copy merges and,
  on vertical levels, rotates it); horizontal two-page rooms raise the level's own attribute rows
  (`room::raise_attributes`). Actors that change tiles change the map too (`breakable_block`,
  `crumbling_block`, `collapsing_floor_controller`, `stomper`), except the final boss's death tiles,
  the bar and its digits, which are VRAM-only.
- **Bevy 0.20's `TilemapChunk`**: one array-texture tileset (a `u16` index per cell, alpha
  discard, flips, a tint, no palette lookup); the layer limit is the adapter's (wgpu's floor 256).
- `ui.rs` has the sprite atlases (`Key`: 4 banks and a palette; `tiles` one 8x16 atlas per key,
  colour 0 clear) and the rule picking a sprite's key from its line.

## The design

### When Modern draws (`rotj_core::modern::shows`)

One rule in the core, read by the app (through the mirror's snapshot) and by the gate: a frame is
drawn Modern when
- the game is in play: no script, or a pause, a power-up or a stage end;
- the background is shown (`$2001` bit 3 on every line) with the same background banks on every
  line;
- no raster split is armed (the live `$D0` bit 7, as `Play::step` reads it, not the room record);
- the final boss's death tiles (`final_boss_part`'s VRAM-only writes) have not been written since
  the room began.

Every other frame shows the Classic picture whole, with the Modern camera off. That covers the
rooms with no screen record (22 of the 36 the survey listed) and the pause, power-up and stage-end
frames in them. The split rooms move to Modern one release at a time after this one: first `$D3` 4
(four boss rooms, whose bar is the only background strip in play, so the HUD-strip layer the owner
asked for lands there, from the bar's nametable rows and banks), then the band rooms. The HUD
itself is sprites and is in this release.

### What is on screen, and when

`Play::step` runs the NMI and then the next frame, so the game it holds is a frame ahead of the
picture. The Modern view is computed in `mirror::Snapshot::of`, with the page and the records (C2),
and the gate uses the same state: the lockstep's game at the NMI that shows that page.

### The background: four tilemaps over the live map

- **The model in the core** (`rotj_core::modern`): `tile_at(game, level_x, level_y) -> (tile,
  palette)`, the live map's cell for a level pixel (`Cell::at` with the page table and the
  orientation), the metatile's corner tile (`Level::tiles`) and its palette from
  `Level::attributes` (the 32x32 px quadrant of its page's byte); and the view's top-left in level
  pixels: the camera plus `scroll_x - round(camera_x)` as a signed byte (it carries the final
  boss's shake).
- **The app** (`crates/app/src/modern.rs`): four `TilemapChunk`s, one per background palette, each
  the size of the view plus a tile each way (33x31 tiles of 8x8 px), a cell visible in the chunk
  of its palette; the tile data rebuilt each frame from the model and placed at the view's offset
  within a tile. Each chunk's tileset is an array texture of the 256 tiles of banks 0-3 (the
  frame's `Line::chr`), layer = tile byte, in that palette's colours through `Colours`, colour 0
  transparent; rebuilt only when the banks, the background palette or the colours change.
- **Modern draws the full width**: the PPU's left-8 clip and Classic's black edge columns are
  not drawn.
- **The backdrop** is the palette's colour 0 through `Colours` (greyscale as `ui.rs` applies it),
  the Modern camera's clear colour.

### Sprites: from the records on the mirror's entities

- **One placement, shared**: a pure `oam::place(call, piece) -> Option<(y, tile, attr, x)>` with
  `x` an `i16`, the builder's rule for one piece (y from `piece_y`, x flipped and offset), returning
  the placement even when x is off the visible width, because `draw` writes y, attribute and x
  before its width test. `OamBuilder::draw` uses it and keeps the overflow-flag sums and the
  allocator; `DrawRecord::sprites` applies the width test and gives one lap of an endless
  metasprite. C1's replay test still guards `draw`.
- **The app**: each drawer entity's `Draws` (and `OtherDraws`, without the UI marks when
  `ui: Faithful` shows them) become `Sprite` children, 8x16 from `ui.rs`'s atlases (its key rule
  pulled out as a function both use), flips from the attribute, at their screen position.
- **Layers**, back to front: the backdrop; sprites with the priority bit set; the tilemaps; the
  other sprites. Within a layer, draw order decides overlap: an earlier draw is in front, as if the
  allocator never rotated, so overlapping sprites hold still. Not kept: the PPU's rule that a
  behind-background sprite with a lower OAM index hides a front sprite's pixels.
- **No per-line limit and no flicker in Modern**: every record's sprites show, those an overflow
  dropped included; `sprite_limit` does not apply.

### The view and the setting

- Modern draws into a 256x240 image with its own 2D camera and render layer, the camera on only on
  Modern frames; the existing screen quad shows that image instead of the software one on Modern
  frames, so the integer scaling, overscan and the faithful UI stay as they are.
- **`picture: Classic | Modern`** in `settings.ron` (default `Classic`) and the menu, beside `ui`;
  the 1.1.0 file still parses.

## The gates (core, every lockstep)

- **Sprites** (every loop frame, beside C1's checks): the page's visible entries (y below the
  parked lines) are a subset of the frame's records' visible sprites, and equal to them as sets when
  the page did not overflow, that is when the records place at most the 64 pieces the page holds
  (odd frames wrap and overwrite instead of setting `$BC`). An entry a piece clipped in x left
  half-written (its new y, attribute and x over the entry's old tile, when no later piece took the
  entry) is the one exception, named in the check.
- **Background** (every frame `modern::shows` holds, at the NMI's state, in the lockstep where the
  port-side `Ppu` is): for each visible 8x8 cell of each line, `tile_at` equals the nametables' tile
  and attribute at that line's scroll, except the 8 px edge columns a scrolled line shows stale
  (`screen.rs`'s rule, moved to the core). This proves, over the whole chain, the sweep and the
  explorer, that the tilemaps show what the PPU shows, without a second rasteriser.
- **The app**: a test over a level's first frames that every Modern frame's tile data and sprite
  children match the core's model and the records (no GPU needed); and a screenshot of a Modern
  frame for the owner, if this machine's software Vulkan can take one.

## Not here

The split rooms (their own releases, above); interpolation, widescreen and the other Enhanced
features (1.2.0); art beyond the game's CHR.
