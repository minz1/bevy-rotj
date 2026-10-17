# The pad as `Buttons` (1.1.x step B4)

Status: approved with changes by the Opus stand-in, 2026-10-17 (applied: no quirk queries,
a `DPAD` group instead; the any/all conversion rule; the derives and operators; the
forced pads and missed sites named; `walk_gate` typed; the player's pad copies in scope;
the tests trimmed; the roadmap's B4 reworded). Step B4 of
`docs/specs/2026-10-16-modernisation-roadmap-design.md`, from a read-only survey of the
app's input, `Game::nmi`, `World` and every pad reader in the core.

## What this is, and is not

The pad becomes typed game buttons in the core. It is not an action layer: the eight
buttons are the game's actions, their meaning depends on context (A jumps in play and
confirms in menus), and the groups (`ACCEPT`, `FACE`) are the game's. Remapping is the
app's (`KEY_BINDINGS` maps keys to `NesButtons`; a settings step can make it a setting),
and an Enhanced input the NES lacks would sit beside `Buttons`, never inside it. The
roadmap's B4 entry is reworded to say so.

## What the survey found

- The app hands `game_byte()` (the game's `$F6` order: Right `$01` .. A `$80`) to
  `Game::nmi(pad)`, which drops Right when Left is held too and Up when Down is, sets
  `world.pressed = (held ^ world.pad) & pad` (the raw `pad`, kept) and `world.pad = held`.
  `World::pad`/`pressed` are `u8`s mirrored to `$F6`/`$F5`; the recordings and the oracle
  hold bytes.
- About 89 non-test reads and writes in 15 files. The "quirks" are plain tests:
  `trailing_zeros() >= 2` and `is_multiple_of(4)` are `pad & 3 == 0`, `>= 4` is
  `pad & $0F == 0`, `flight_steer`'s `pad & 1` is Right; `boot.rs`'s sound-test equality is
  `pad == A | START`; `attract.rs`'s `HELD_TO_LEAVE` (`$D0`) is `ACCEPT`. Writers: `nmi`, the
  stage-script clear, `stage.rs`'s forced pads (none, Right, Up), `draw_player`'s
  write-back, `player.rs`'s `&= START`, the oracle's pokes. `Player::walk_gate` (`$0470`:
  0, 3 or `$0F`) is only ever ANDed with the pad.

## Design

- `actor::button::Buttons(u8)`, `#[repr(transparent)]`, `#[derive(Debug, Clone, Copy,
  Default, PartialEq, Eq)]` (derived `Eq`, so a `match` on the consts compiles): the bits as
  associated consts typed `Self` (`RIGHT` .. `A`; `NONE`; the groups `SIDEWAYS`,
  `UP_OR_DOWN`, `DPAD = $0F`, `ACCEPT`, `FACE`), `const fn bits()`/`from_bits()`,
  `contains` (all of), `intersects` (any of), and `BitAnd`, `BitOr`, `Not`, `BitAndAssign`,
  `BitOrAssign`. The `button::` consts become these.
- The conversion rule, mechanical: `x & SINGLE != 0` → `x.contains(Buttons::SINGLE)`; any
  multi-bit mask test `x & GROUP != 0` → `x.intersects(Buttons::GROUP)`; `x & m == 0` with
  a group → `!x.intersects(..)` (the `trailing_zeros`/`is_multiple_of` sites become
  `!x.intersects(Buttons::SIDEWAYS)` / `(Buttons::DPAD)`); `x == mask` stays `==`. A plan
  check: no `contains(` takes a group const or a `|` expression.
- `World::pad`/`pressed` and `Player::walk_gate` become `Buttons`; the RAM mirror
  (`world.rs`), `nmi`, and the oracle's byte edges (`player.rs`'s `run_step_draw`) convert
  with `bits()`/`from_bits()`. `Game::nmi(pad: u8)` and `Play::step(pad: u8)` keep the byte:
  the recordings and tapes stay byte-identical. The forced pads are `Buttons::NONE`,
  `RIGHT`, `UP`.
- `player.rs`'s copies (`step_draw`'s `&mut` parameters, `Port`'s `pressed`/`pad`) become
  `Buttons`. Left on `bits()`: `Port::bit` (the 6502 `BIT`'s V flag over many bytes; the pad
  sites call `self.bit(x.bits())`), the rotate in `landing_response`, and `direction >> 1`
  as an index.
- The app's `NesButtons` (shift order) stays separate.

## Tests

- `buttons_are_the_game_bits`: each const's byte (the existing button test moved), and for
  all 256 bytes `!intersects(SIDEWAYS) == (b & 3 == 0)` and `!intersects(DPAD) == (b & $0F
  == 0)`, recording that the rewrite matches the old tests.
- The Classic suites, the tape tests, a 1-seed sweep and a 300-branch exploration; the
  tapes unchanged.

## Plan

1. `Buttons` and its test; the roadmap's B4 wording.
2. `World`, `Player::walk_gate`, `nmi`, the mirror, the oracle edges.
3. The readers, file by file; the `contains` check.
4. Checks, the `reviewer` agent, commit `The pad is Buttons`.
