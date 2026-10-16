# Settings, the settings menu and frame pacing (1.1.0, part 1)

Status: approved by the owner, 2026-10-16, after designing it with them in chat the same
day (sections 1-4 approved one by one).

## Scope

1.1.0 is two specs (owner, 2026-10-16):

1. **This one:** the settings file, an in-game settings menu in NES-styled Bevy UI, five
   presentation settings (volume, overscan, palette, sprite limit, vsync) and frame pacing
   locked to the 60 Hz tick.
2. **Next:** the game's HUD, pause, title and password screens redrawn in Bevy UI as an
   Enhanced presentation setting (Classic keeps the game's own drawing); its pause screen
   gets a SETTINGS row opening this menu.

Later 1.x, not here: the compositor as Bevy sprites/tilemaps and interpolated motion at the
display's rate (then a `frame_rate` setting, `60` | `Display`), widescreen (the first
behaviour setting, with the core-side `Settings` value and input-only enhanced tapes), CRT
filters. PLAN's ladder is renumbered accordingly when this lands.

Every setting here is presentation: none reaches `Game`, so the Classic suites (lockstep
chain, state tape, random play, exploration, picture parity, playtest replays) are
untouched and run as they do today. Defaults are the NES's behaviour where the NES had one
(overscan hidden, the 8-sprite limit, Composite Direct), otherwise today's.

## The settings file

`settings.ron` beside the exe (`std::env::current_exe()`'s directory), owner's choice:
portable, like the exe itself. Module `crates/app/src/settings.rs`, a `Settings` resource:

```ron
(
    volume: 100,              // 0-100, steps of 5
    overscan: Hidden,         // Hidden | Shown
    palette: CompositeDirect, // CompositeDirect | RgbPpu | Ntsc | File
    sprite_limit: true,       // true: 8 per line, the NES's | false: no limit
    vsync: true,
)
```

These names and values are public interface from 1.1.0 (PLAN "Versioning"): a later
release must read this file. So every field is `#[serde(default)]` (a missing key is its
default), unknown fields are ignored (an older release reads a newer file), and a fixture
`crates/app/settings-1.1.0.ron` is parsed by a test in every release.

- Read once at start-up. No file: defaults, nothing written until the menu closes.
- A file that does not parse: a warning, defaults, and the file is renamed
  `settings.ron.bak` before the next save overwrites it.
- Saved when the menu closes, written to `settings.ron.tmp` and renamed over the file.
- A folder that cannot be written: one warning, the settings live in memory for the session.
- The functions take the directory, so tests use a temporary one.

## The menu

Module `crates/app/src/menu.rs`.

**Ways in.** Esc or F1 on the keyboard, Guide (`GamepadButton::Mode`) or the right stick's
click (`RightThumb`, for when Steam or the Game Bar takes Guide) on a pad, from anywhere:
boot, intro, title, cutscenes, gameplay. Also Select while the game's own pause is up
(`Script::Pause`): the game's pause is not a menu (Start alone ends it, `pause.rs`), so the
app draws a one-line hint `SELECT  SETTINGS` in the game's font inside the visible 224
lines below the pause sprites (the exact row is picked in the plan against a capture of the
pause screen). The hint is a Bevy UI node over the picture, not in the game's frame.
Taking Select away from the game while paused is safe for Classic: the game is still the
game for the pad it receives, and the recorder records what it received.

**Ways out.** Esc, F1, Guide, right stick, B, or A/Enter on BACK.

**While open.**

- The game does not step: no `Play::step`, so no tick, no recorded pad byte (replays stay
  valid) and no music driver frame.
- The sound fades to silence over 10 ms (441 samples) and the queue is kept at its target
  with zeros: `advance_music` still runs every frame, pushing the fade and then zeros
  instead of running the driver and synth. On close it fades back in over 10 ms. Holding the last sample (`QueueSource`'s dry rule) would
  leave a DC level, which 1.0.5 removed.
- The last game frame stays on screen behind the menu, redrawn live as settings change, so
  palette, overscan and sprite limit show at once.

**After close.** A button still held when the menu closes is masked from the game until it
is released, so the A or B that closed the menu is not a jump or a shot.

**Look.** A bordered box centred on the picture: black (`$0F`) fill, white (`$30`) text and
border, the cursor row white and the others grey (`$10`; the font has no arrow), colours
from the active palette. Bevy UI laid out in NES pixels and
scaled by the picture's whole multiple (`UiScale`, from `integer_view`), so a menu pixel is
a game pixel; the box always lies within the 224 lines visible with overscan hidden.

```
        SETTINGS
    VOLUME          100   (the cursor row: white; the rest grey)
    OVERSCAN        HIDE
    PALETTE         COMPOSITE
    SPRITE LIMIT    ON
    VSYNC           ON
    BACK
```

Up/Down moves the cursor (wrapping), Left/Right changes the value and applies it at once
(volume by 5, stopping at 0 and 100; the others cycle). With `File` chosen and no usable `palette.pal`, the value
reads `NO FILE` and the picture uses Composite Direct.

**Font.** The game's own: CHR bank `$CC` (the font the sound test and the pause load) holds
an 8x16 uppercase set, A-Z, 0-9 and `! ? - .`, each glyph a tile pair (`2n` over `2n+1`;
the sound test's "SOUND TEST" is `0C 0A 2C 38 14 00 1C 24 0C 1C`). At start-up the app
builds a glyph atlas `Image` from the player's CHR (embedded or loaded, so nothing
ROM-derived is committed) and a fixed character-to-tile table in `menu.rs`. A cell is 8x16,
so 14 text rows fit in 224 lines. Spec 2 reuses the atlas.

## The settings

**volume.** `Music::advance_frame` scales each pushed sample by `(volume / 100)²` (a
squared curve, so the steps sound even), the same place the menu's fades apply. One master
volume: the 2A03 mixes music and effects on the same channels.

**overscan.** `Hidden` shows lines 8-231: the screen quad's `Sprite::rect` crops the image
and `integer_view` fits 256x224 (a larger whole multiple in the same window). `Shown` is
today's 240 lines. The game's frame, the tests and the replays always draw and check all
240 lines.

**palette.** Applied in the app: `screen::upload_frame` maps `Frame::indices()` (already
greyscale-masked) through the chosen 64-colour table; `Frame::rgba()` keeps Classic's
colours for the tests. In `rotj_core::palette`:

- `NES_PALETTE` (Composite Direct, FirebrandX), the default, unchanged;
- `RGB_PPU`: the 2C03 RGB PPU (PlayChoice, Vs. System), its 3-bit-per-channel levels as
  the NESdev wiki documents them; hardware data, no palette file;
- `ntsc()`: computed from the composite signal: each colour's 12-phase square wave at the
  2C02's documented voltage levels, demodulated to YUV and RGB as the NESdev wiki's
  "NTSC video" decoder does, with no gamma
  step (the signal is already gamma-encoded for a TV, as sRGB is) and fixed parameters
  (no knobs);
- `File`: `palette.pal` beside the exe, read at start-up and when chosen; 192 bytes, or
  1536 (the emphasis variants, of which the first 192 are used); any other size is
  `NO FILE`. Players bring their own, so the repo ships no third-party palette beyond
  Composite Direct (whose permission request is still pending, see its `ATTRIBUTION.md`).

**sprite_limit.** `Frame::render(&mut self, picture, sprites_per_line: usize)`, the only
change to `rotj-core`'s API: the app passes 8 or 64; every existing caller passes
`MAX_SPRITES_PER_LINE`. Off removes the flicker of the PPU's per-line cap (the game rotates
its OAM start each frame, `$C145`, so a different sprite drops each frame). It does not
remove the game's own drops when its 63-entry page fills (`OamBuilder::draw`, even frames
stop, odd frames wrap): that is game state. Overlapping sprites still swap front and back
as the rotation moves the lowest OAM index.

**vsync.** `Window::present_mode`: on is `AutoVsync` (Fifo; under VRR a frame below the
refresh rate presents at once), off is `AutoNoVsync`. Applied live.

## Frame pacing

No setting: the app always shows one picture per tick, the NES's own behaviour. Measured
against today's (Bevy at the display rate, the tick in `FixedUpdate` from wall time):
on a VRR display (the owner's 170 Hz, G-Sync on, windowed) every picture lasts 16.67 ms
instead of the 3-3-3-3-3-2 refresh cadence; on fixed-rate displays it is no worse, and
input latency is the same.

- Module `crates/app/src/pacing.rs`. `tick` (`play.rs`) and `advance_music` (`audio.rs`)
  move from `FixedUpdate` to `Update`, once per frame in that order; `tick` does not run
  while the menu is open (`advance_music` then pushes silence, see "While open").
  `Time::<Fixed>` is no longer used.
- A limiter system in `First` (before `PreUpdate` reads the pad) waits until its deadline:
  `thread::sleep` to about 1 ms short, then a spin. `next_deadline(deadline, now, period)`
  advances by 1/60 s; more than one period late, it resets to `now` (no bursts of catch-up
  ticks). So a frame is: wait, read the pad, tick, draw, present; Bevy's pipelined rendering
  draws it while the main thread waits for the next deadline.
- `desired_maximum_frame_latency` stays 1 (wgpu's lowest-latency value; 0 is not
  expressible in Bevy and DX12 takes 1-16).
- The tick stays 60.0 Hz; the audio queue's two-sample nudge absorbs drift as today.
- `ROTJ_FRAME_LOG=1` prints, every 5 s, the frame interval's mean, standard deviation and
  worst case, for measuring the real output.

## Testing

Classic: the suites run as today at every merge and tag (`.orchestration/gates.sh`). The
picture tests pass `MAX_SPRITES_PER_LINE`, so they also prove the one core change.

In the gates (unit and headless app tests):

1. The app's upload with Composite Direct equals `Frame::rgba()` byte for byte over the
   power-on picture test: moving the palette into the app changes nothing in Classic.
2. Settings: defaults; round trip; a missing key is its default; an unknown key is ignored;
   a bad file gives defaults and becomes `.bak`; an unwritable directory warns once and
   does not panic; the 1.1.0 fixture parses.
3. Sprite limit: a scene with 9 sprites on one line draws 8 with the limit, 9 without.
4. Palettes: `RGB_PPU` pinned to its levels; `ntsc()` pinned by digest, with neutral greys,
   `$0F` black and `$30` the brightest; `.pal` accepted at 192 and 1536 bytes, rejected
   otherwise.
5. Menu (Bevy `App` with the menu's systems, driving `NesInput`): Esc, F1 and Select during
   the game's pause open it; while open the game does not step (no tick, no recorded byte);
   Left/Right changes `Settings`; a button held at close is masked until released; at
   open and close no sample-to-sample step exceeds the last sample's level / 441 (the
   10 ms ramp's own step); the glyph table maps
   "SOUND TEST" to the sound test's tile bytes (with the ROM).
6. Pacing: `next_deadline` on time, slightly late, more than a period late; an `--ignored`
   real-time test of 600 frames with the 99th percentile wake error under 0.5 ms.

Real output (the 1.0 lessons: check what the player sees and hears), on the owner's
machine with the Windows exe launched from WSL:

7. Pacing: `ROTJ_FRAME_LOG=1`, frame interval 16.67 ms ± 0.5.
8. Pixels: a PowerShell capture of the window, measured in Python: with overscan hidden
   the picture is 224 lines times the whole multiple, shown 240; sampled RGB equals the
   chosen palette's table; the menu's pixels lie on the same grid.
9. The owner's playtest on the 170 Hz G-Sync display: no judder; the menu by keyboard and
   pad; even volume steps.

Before the tag: the fresh-clone ROM-less check as in 1.0.5 (the font comes from whatever
ROM the game runs with, so nothing changes there) and the Windows exe in
`Documents\batman-rotj` updated.

Also, as a review step rather than a test: list the TAS frames with lines of more than 8
sprites, look at a sample without the limit for any deliberate masking by the game, and
record the finding in STATUS.

## Not here

The `frame_rate` setting and interpolation, queued frames as a setting, a fullscreen
toggle, separate music and effects volumes, palette knobs (hue, saturation), key
rebinding, widescreen, CRT filters, the game's screens in Bevy UI (spec 2).
