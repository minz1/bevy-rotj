# The game's screens in Bevy UI (1.1.0, part 2)

Status: approved with changes by the Opus stand-in, 2026-10-16 (changes applied: marks stamped
when an entry is placed and kept through `finish`, marks copied with the OAM, the UI is the
game's own marked sprites rather than a second description from state, art and colours from
each entry's line, atlases dropped when unused, the intended differences stated). The owner chose the look ("faithful
first, modern later"), the setting's name (`ui: Classic | Faithful`, a `Modern` value later)
and the stand-in review before leaving for the evening; the rest was designed autonomously
from a survey of how the game draws each screen.

## Goal

A presentation setting `ui` whose `Faithful` value shows the game's own UI elements as Bevy
UI, in the game's own art, instead of as sprites in the PPU picture.
They look the same, except that what flickered because of the NES no longer does: the HUD's
L/E label, which the game draws only on odd frames (`frame.rs` `draw_hud`, `frame_odd`), shows
every frame. `Classic` (the default) keeps the game's drawing untouched. This is also the
first piece of the 1.1.x modernisation: the UI's look moves out of OAM and into Bevy, where a
later `Modern` layout can change it.

## Scope

In: the stage HUD (energy and meter gauges, the L/E label), the pause card (PAUSE burst,
STAGE x-y, REST n, PASSWORD and the area's four letters), the title's cursor, and the
password screen's entry (the four letters, the cursor mark, the rejected message).

Out, and why:

- The boss status bar: three parts in three layers (nametable rows with baked-in text and
  portraits, a portrait actor among the scene's sprites, raster-selected CHR banks), and two
  of the five boss areas (`$D3` 5 and 7) are not yet understood. It belongs with the GPU
  renderer (1.1.x), where nametable bands become tilemap layers.
- The title's menu line, the password screen's decoration and every other screen built in
  the nametable (stage banners, the boss card, continue, GAME OVER, the attract mode and
  the ending): their text is background tiles, not sprites, and stays the game's picture
  until the GPU renderer.
- Deliberate blinks stay: the meter flashing when full, the energy stub blinking at 0, the
  active password letter's blink, the accepted/rejected verdict blink. Only the label's
  30 Hz alternation, which exists to save sprites, goes. A `Modern` layout later reads the
  state itself (energy, meter); the marks are its seam for hiding the game's drawing.

## Classic stays Classic

Nothing here changes `Game`'s behaviour, its RAM or WRAM, or what the Classic suites
compare: the marks are metadata beside the OAM, and the hiding happens on the app's display
copy of it.
With `ui: Classic` the app shows exactly today's picture.

## Design

### 1. Who drew each sprite (rotj-core)

`OamBuilder` (`actor/oam.rs`) gains `pub marks: [Mark; 64]` (by OAM entry) and
`pub marking: Mark`. `Mark` is `Scene | Hud | Label | Pause | Title | Password`.

- `draw` stamps `marks[entry] = marking` when it advances to the next entry (a piece it
  drops at the edge writes bytes but does not advance, and is not marked).
- `finish` sets `Scene` on the entries it parks; `clear` (which calls it) so resets all 64;
  `lift` starts them `Scene`. `finish` must not reset the drawn entries: it runs after every
  frame's draws, before anything reads them.
- The drawers set `marking` around their own draws and back to `Scene` after:
  `frame::draw_hud` (`Label` around the L/E label, `Hud` around the gauges),
  `pause::draw_screen` (`Pause`), `boot::draw_title`'s cursor (`Title`),
  `boot::draw_password`'s entry metasprite (`Password`; the two decoration metasprites
  stay `Scene`).

These fields are not RAM: `lift`/`project` (`oam.rs`) ignore them and nothing compares two
`Game`s whole, so the lockstep and the tapes cannot see them.

### 2. The UI is the game's own marked sprites (app)

No second description of the screens: the UI shows the OAM entries the game marked, so it
is faithful by construction on every frame, including frames where a script leaves the last
loop frame's sprites up (`Script::StageEnded`, `PowerUp`).

- `Play` copies `game.oam.marks` on the same line as the OAM copy in `step`, so the picture,
  the marks and the UI are always one frame.
- `ui: Faithful`: `Play::compose` parks (y = `$F4`, lines 245 and on, never drawn) every
  entry whose mark is not `Scene` in its display copy, and a new `ui.rs` shows those entries
  as Bevy UI image nodes inside the menu's NES-pixel container (absolute: `x - visible.x`,
  OAM y + 1 - `visible.y`; `Overflow::clip()`, so a piece in the hidden overscan or past the
  right edge crops as the picture does; a `GlobalZIndex` below the menu's overlay).
- The flicker fix: the label (`Label` entries) is shown on a frame when the game drew it
  that frame or the frame before. The game draws it on odd frames, so it shows steadily
  while the HUD is up, and goes at most one frame after the HUD does.
- Each entry's art and colours come from the line it starts on, as the compositor's: its
  tile pair through `lines[y + 1].sprite_chr` and the table its tile number's bit 0 selects
  (8x16 mode ignores `ctrl`'s sprite-table bit), its four colours from palette RAM's sprite
  set (attribute bits 0-1) with `mask`'s greyscale, through the chosen palette table,
  colour 0 transparent; `ImageNode` flips for attribute bits 6-7. An entry on a line with
  sprites off (`mask & 0x10 == 0`) is not shown, as the PPU would not show it.
- Tile atlases: a shared CHR-to-atlas builder (one 4 KiB pattern table as 128 8x16 tiles in
  one 4-colour palette, a 128x128 image), kept for the keys (banks, colours) the shown
  entries use this frame and dropped when no shown entry uses them, so the pause's cycling
  colour does not pile up images. The GPU renderer (1.1.x) reuses the builder.
- The UI nodes are rebuilt when the shown entries, their lines' banks or the colours change.
- The settings menu gets a row `UI` (`CLASSIC` / `FAITHFUL`); the settings file a key
  `ui: Classic | Faithful`, default `Classic`, read key by key like the others. The 1.1.0
  fixture gains it (1.1.0 is not yet tagged).

**Intended differences from Classic in `Faithful`** (presentation only): the label is
steady; parked UI entries no longer take the PPU's per-line sprite slots, so with the limit
on a scene sprite the UI pushed out shows; the UI is always in front of the scene's sprites
(on the NES the lowest OAM index wins, and the game rotates the order each frame). The
residual: if the HUD's only drawn piece on even frames is gone (energy blinking at 0 with
no meter), the label falls back to the game's 30 Hz.

## Testing

In the gates:

1. Classic: the picture tests unchanged; with `ui: Classic` the app's OAM copy is the
   game's (a test).
2. Marks: `draw` marks what it places and not what it drops; `finish` leaves drawn entries'
   marks and makes parked ones `Scene`; `clear` makes all `Scene`.
3. Each drawer marks exactly its own pieces (with the ROM): run the drawer on a clone of the
   frame's `Game` after `oam.clear()`, and the real frame's entries with that mark equal the
   clone's placed entries as a multiset of `(y, tile, attr, x)` — a stage frame (Hud and
   Label, odd and even), a pause frame, the title, a password entry.
4. Over the TAS chain (release, `--ignored`, in the gates' release suite) and the scripted
   pause, title and password sessions: no marked entry has the priority bit (`attr & 0x20`),
   every frame with marked entries is in 8x16 mode (`ctrl & 0x20`), and no marked piece
   spans a change of `sprite_chr` across its 16 rows.
5. App: parked entries are exactly the non-`Scene` ones; the label carry-over; node
   positions with both overscans; the atlas from CHR (pixels of a known tile pair); the
   setting's parse, fixture and menu row.

Real output on the owner's machine (Windows exe, `PrintWindow` captures, the sprite limit
off so freed slots cannot differ): on an odd tick a stage frame with no sprite over the HUD
is pixel-identical to `Classic`'s; on an even tick it differs only by the label, which is
present; the pause card, the title cursor and a password entry captured in both modes are
identical.

## Not here

The `Modern` layout, the boss bar and the nametable-text screens (with the GPU renderer),
interpolation, and any change to the game's own blinks.
