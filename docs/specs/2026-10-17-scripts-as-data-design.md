# Scripts as data, pilot (1.1.x step A4)

Status: approved with changes by the Opus stand-in, 2026-10-17 (applied: no shared
`Action` enum, only the runner; the survey corrected; Cutscene's hidden state deleted,
not moved; its table written out; one wait rule with the original constants; the runner's
details; the test wording). Step A4 of `docs/specs/2026-10-16-modernisation-roadmap-design.md`
("the `Ending`'s `SCRIPT: &[Action]` table shape generalised"), from a read-only survey of
the eight scripts in `actor/`.

## What the survey found

- No script is lifted from RAM mid-flight: `Game::frame` creates them fresh (from `$BA`,
  `Ending::default()`, `Cutscene::new(scene)`) and the oracle compares the RAM/WRAM
  effects every script frame (`check_script`, scratch masked). A script's step is a private
  field, so a table needs no program counter in RAM; it must only write the same bytes on
  the same frames.
- `Ending` (`ending.rs`) is already a table: `SCRIPT: &[Action]` (32 variants, 73 rows)
  and an interpreter whose `run` returns `Flow::Waited` (the frame ends, `frame` += 1,
  saturating) or `Flow::Done` (`at += 1`, `frame = 0`, the next row runs in the same
  frame); bespoke rows (`Fall`, `Rise`, `Skyline`, `Credits`, `Walk`...) call methods keyed
  on `frame`.
- `Cutscene` (`cutscene.rs`, 10 states) is linear and input-free; its `shown`, `blocks`
  and `hold` fields are step state.
- The helpers are shared (`ScreenClear`, `copy_frame`, `draw_block`, `payload_byte`, also
  used by `Boot`, and `draw_sprite` by `SoundTest` and `Attract`), but the actions mostly
  are not: Cutscene has no fades, split, parallax or mirroring; its banks are indexed by
  scene where Ending's read its buffer overlay; its palette goes through the VRAM queue
  where Ending's writes `effects.palette`; its blocks take count and cursor from
  `$775C`/`$775B[scene]` where Ending's restart at 0; Ending's clear also zeroes its WRAM
  buffer (`$7FC0`). The real overlap (copy, clear, wait, stop music, song, mask) is six
  one-line arms over the helpers.
- `NextArea`, `RoomChange`, `Attract`, `SoundTest`, `Continue` and most of `Boot` branch
  on game state or input; they stay state machines.

## Design

- `actor/script.rs` (new) holds only the runner: `Flow { Waited, Done }`, `Runner { at:
  usize, frame: u16 }` (`frame` saturating: Skyline reaches `$100`, `Hold` sits forever),
  the step loop with `Done`'s same-frame fall-through exactly as `Ending` does it, `true`
  once `at >= table.len()` without indexing, the current row exposed (`Ending::over()`
  reads it), and `fn wait(frame: u16, count: u16) -> Flow` (`Ending`'s `frames` closure:
  `Waited` while `frame < count`). Each script keeps its own action enum and context; the
  shared behaviour stays in the helper functions. A shared, serialisable vocabulary waits
  for authored content scripts, the first consumer that needs one.
- `Ending` moves onto the runner, its table and arms unchanged.
- `Cutscene` becomes a table, its hidden state deleted (`shown` is the second `Clear` row,
  `blocks` and `hold` the runner's `frame`; the context keeps `scene`, `clear` and
  `cursor`), each "set a hold, then pre-decrement" a setter row then `Wait` with the
  original constant, as `Ending`'s `Sound(0x23), Wait(0x21)` already does:
  `CopyPayload` (three frames), `Clear(0)`, `Picture` (one frame), `PictureBlocks` (one
  block a frame until `$775C[scene]`), `Slide` (one frame), `SlideIn` (waits while
  `room[5] | room[6] | room[10] != 0`; on the zero frame `room[0] = 0`, `ppu_ctrl = $A8`),
  `Wait(0x20)`, `GreyPalette`, `Wait(0x21)`, `LightPicture`, `Wait(0x81)`, `StopMusic`,
  `Clear(0)`. Rows are named for what they do. `Cutscene::step` keeps its signature and
  returns `true` on the frame the second clear ends, as today, so `NextArea` is untouched.
- Not here: `Boot`'s linear parts as a table; a `WaitUntil`/`Goto` vocabulary; moving the
  payload helpers out of `cutscene.rs` (five scripts use them; a later cleanup).

## Tests

- `Ending`'s and `Cutscene`'s oracle tests unchanged; the whole-game TAS chain, which runs
  the ending and every cutscene picture; the Classic suites, a 1-seed sweep and a
  300-branch exploration; the tapes unchanged. A wait off by one fails on the first frame
  the following row writes.

## Plan

1. `script.rs` with the runner; `Ending` onto it (tests green).
2. `Cutscene` as a table (tests green).
3. Checks, the `reviewer` agent, commit `Scripts as data: the ending and the cutscene`.
