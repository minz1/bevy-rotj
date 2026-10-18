# The shots' state bits named and their flight written once

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: `PASSES_SHIELDS`, not
`DEFLECTED`; every site listed; the docs of `flags` and `state`; explicit `Flight` variants and
`fly` calling `advance`; `count_down`'s rule. The stand-in's reorder of the splitter's sound
against the timer bit is avoided instead: each handler sets the bit itself, so no write moves). The
survey loop's #6 (MODERNIZE "The survey loop, 2026-10-18"). No behaviour change: the weapons'
oracle tests (each weapon's flight compared with the ROM's RAM frame by frame, both wall touches and
the falling free reached) and the lockstep prove it.

## What is there

- `Projectile::flags` (`$7F30`) holds only bits 7, 6 and 5. Readers: `engine.rs`'s
  `projectile_hits` (`$AA57`: bit 7 skips the shot, bit 5 lets it pass a shield) and
  `boss_contact` (`$AFD6`: bit 7), and `spend` (`$B108`: bit 6 returns early, so a piercing shot
  still deals its damage but is not spent). Writers: `0xE0` (7 in weapons.rs, 1 in player.rs),
  `0x60` (4) and `0x20` (1) in weapons.rs, `= 0` (6 in player.rs, 1 in weapons.rs), and
  `engine.rs`'s `|= 0xE0` (a deflection) and `|= 0x80` (spent). Its doc ("bit 7 spent, bit 5
  deflected") is wrong: the charged pieces and the bubble get bit 5 at spawn, never deflected.
- `Projectile::state` (`$7C80`) holds bits 7 (spent: the end animation plays, also for split parts
  and bubbles) and 6 (falling: gravity, then the timer runs out); weapons.rs reads them and
  `engine.rs` writes `|= 0x40` and `= 0x80`. Bit 1 reaches the probe as `phase << 1`
  (`FLOORS_SOLID`); no port writer sets it.
- "Count down and free": `timer -= 1; if timer & 0x80 != 0 { kind = 0 }`, 8 identical sites
  (weapons.rs); three ignore whether it freed.
- The flight's preamble is written four times (`flying_shot`, `splitter`, `wave`, `three_way`):
  `advance`, then if not spent, a falling shot falls and counts down (returning when freed), else a
  wall touch: the first (timer bit 0 clear) sets the bit, projects and returns; the second is each
  weapon's own (a bounce with its own RNG bit, the split, the three-way's pick). `flying_shot`
  alone returns gravity's overflow on a free and `projection_overflow(..)` on the first touch; the
  splitter sounds `$22` on both touches, before the timer bit; the `seen` markers (`$80CD`, `$80DA`,
  `$9268`, `$9275`) are each copy's own.

## The change

- Beside `Projectile` in `world.rs` (with `orientation`, `pose` and `hurt`), two modules of named
  masks: `shot_flags::{HARMLESS = 0x80, PIERCING = 0x40, PASSES_SHIELDS = 0x20}` and
  `shot_state::{SPENT = 0x80, FALLING = 0x40}`. Writers spell the combinations out (`0xE0` =
  `HARMLESS | PIERCING | PASSES_SHIELDS`, `0x60` = `PIERCING | PASSES_SHIELDS`); every reader and
  writer above uses the names; `flags`' and `state`'s docs point at the modules.
- `Projectile::count_down(&mut self) -> bool` (no `#[must_use]`: three callers ignore it): the timer
  down by one, and the slot freed when bit 7 is set after the decrement (so a timer at `$81` or
  above frees at once); true when freed. The 8 sites call it.
- ```rust
  enum Flight { On, Freed { overflow: bool }, Touched, TouchedAgain }
  fn fly(slot: usize, work: &mut Work, cx: &mut Cx<'_>) -> Flight
  ```
  `fly` runs `advance` and the shared part: the fall and its count-down (`Freed`, carrying
  gravity's overflow), the wall probe (`Touched` when timer bit 0 is clear, `TouchedAgain` when
  set), else `On`. It writes nothing on a touch; each handler's arms keep their writes in order:
  `Touched` sets timer bit 0 and projects (`flying_shot` with its marker and
  `projection_overflow`; the splitter sounds first); `TouchedAgain` keeps each weapon's response
  (the splitter sounding first); `Freed` returns (`flying_shot` its overflow, `wave` true).
  `flying_shot`'s closure parameter `flight` becomes `sprite`.
- Out of scope: the timer's own bits (bit 7 an upward shot, read by `spend`, `bounce_off_wall`,
  the splitter, the three-way, `wave_flight` and `sprite_by_timer`; bit 0 the wall touch), a later
  patch; the charged attacks' state machines (the `*_stage` modules); `param`/`part`.

## Proof

The weapons' oracle tests, the Classic suites, a sweep and a 300-branch exploration unchanged;
tapes byte-identical.
