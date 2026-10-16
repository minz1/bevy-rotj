# The weapons as named states (1.1.x step A2)

Status: approved with changes by the Opus stand-in, 2026-10-16 night (applied: the
projectile struct renamed first; the charge stages as named constants per weapon, not enums,
which keeps weapon 3's `$82..=$FF` range and weapon 2's single stage exactly; weapon 3 is a
weaving `Wave` and weapon 2 a `Splitter`, not a missile and a boomerang; `stored_weapon`
typed too; the overflow-flag call order, the asserts and the names listed). Step
A2 of `docs/specs/2026-10-16-modernisation-roadmap-design.md`, built on A1's pattern
(`docs/specs/2026-10-16-player-motion-design.md`) from a survey of `weapons.rs`.

## Goal

`weapons.rs` (about 2230 lines) dispatches on the selected weapon byte (`effects.weapon`,
`$0476`) to `zero`..`five`, then on each projectile's `kind` byte (`$7BD0`: 0 free, below
`$80` a plain shot, `$80`-`$84` a charged attack's stage) inside each handler, through
functions named for the 6502 (`one`, `three`, `unleashed`, `pairs`, `smite`). It becomes:
a `Weapon` enum for the selection, one enum per weapon for its charged attack's stages, and
functions named by weapon and stage. Behaviour, RAM, update order, RNG and the call order
that carries the CPU's overflow flag unchanged; the Classic suites and `weapons.rs`'s own
oracle tests prove it.

## Scope, and what stays raw

In: the selection byte, the charge-stage bytes (`kind >= $80` within a weapon's handler),
the function names, the comments that say what each weapon is.

Out, deliberately: the projectile fields that mean different things per weapon and stage
(`timer`, `param`, `part`, `part_timer`: a wall marker, a countdown, a vertical flag, a slot
index, an angle), the arithmetic that relies on the game's byte-wise carries (`gravity`,
`velocity`, `pull`, `turn`, `edge`, the boomerang's trim), the `byte`/`set_byte` address
interface (slots 16-23 alias the projectile arrays, `slots::read`/`write`), and the
out-of-range slot writes the game makes. Typing those fields per weapon and stage is a later
step once each stage's payload is named; it goes into `docs/MODERNIZE.md`.

## The weapons

The manual's names were never matched to RAM (`docs/ACTORS.md`), so the variants are named
for what each does in the code; the doc comments carry the likely manual name.

| byte | variant | what it is | its charged attack's stages (`kind`) |
| --- | --- | --- | --- |
| 0 | `Bolt` | the default single shot (bounces off walls, like 3, 4 and 5) | none: `$9A81`'s part flies as a plain shot, `kind` unread |
| 1 | `Homing` | turns toward a target (the manual's homing Batarang) | `GATHER` `$80`, `THROWN` `$81`, `CIRCLING` `$82` (does nothing); `$83`+ panics |
| 2 | `Splitter` | flies on, pushing its speed toward ±`$40`, splits at a wall or when spent | `ORBIT` `$80`; `$81`+ fly the plain path, `kind & $80` picking the thrown sprites |
| 3 | `Wave` | two parts that weave (their velocity's low byte swings `$40`-`$C0`) | `CHARGE` `$80`, `SPREAD` `$81`, `SPREAD_SHOT` `$82..=$FF` |
| 4 | `ThreeWay` | three parts (likely the Shield Star) | `BREW` `$80`, `BUBBLE` `$81`, `TETHER` `$82`; `$83`+ panics |
| 5 | `Super` | the power-up's weapon | `UNLEASH` `$80`, `ORBITER` `$81`, `PAIRS` `$82`, `CLAIMED` `$83`, `CENTRE` `$84` (the last two do nothing); `$85`+ panics |
| 6 | `PowerUpPieces` | not a weapon: the power-up's closing pieces | none |

The `ACTORS.md` §9g claims of a homing missile and a screen-edge bounce for weapon 3 are
corrected with this step.

## Design

- `actor/world.rs`: the projectile slot struct, today also called `Weapon`, becomes
  `Projectile`, and `Effects::weapons` becomes `projectiles` (with `WEAPON_ARRAYS` →
  `PROJECTILE_ARRAYS`): what they are, and it frees the name. Then `#[repr(u8)] pub enum
  Weapon` (the variants above, `Bolt` the default)
  with `byte()` and `from_byte()` as A1's `Motion`; `Effects::weapon: Weapon` and
  `Effects::stored_weapon: Weapon` (copied to and from the selection in six places; it can
  hold `PowerUpPieces` while the pieces run), converted at the RAM projection (test-only;
  `expect` naming `$0476`, as `step`'s panic does past 6; `$0476` only ever holds 0-6: the
  pickup writes 1-4, the rest 0, 5, 6 or the stored value). Where the game derives a shot's
  kind from the weapon (`(weapon + 1) << 1`, `player.rs`), it uses `byte()`. The tests'
  RAM pokes of `$0476` stay bytes. `spawn_shot` and `spawn_melee` keep their asserts, as
  `!= Weapon::PowerUpPieces`.
- `actor/weapons.rs`: per weapon with stages, a module of named constants (`mod homing {
  pub const GATHER: u8 = 0x80; .. }`), matched as today (`wave::SPREAD_SHOT..` keeps the
  range); the projectile's `kind` stays a byte (0 is a free slot everywhere, and the actors
  and the engine free and test it), so no conversion can add or lose a panic. The later
  "kind as a sum type" step takes these names.
- The overflow flag: `unleash` reads the flag the previous slot's `on_screen`, `gravity` or
  `orbiter` left; the dispatch stays as it is, and no `integrate`/`project` call moves out of
  a stage function into a shared prelude.
- Names (`step` keeps its name, as `scroll::step` and `families::step`): `run` →
  `each_projectile`, `zero` → `bolt`, `one` → `homing`, `two` → `splitter`, `three` → `wave`,
  `four` → `three_way`, `five` → `super_weapon`, `unleashed` → `super_attack`, `pieces` →
  `power_up_pieces`, `level_shot` → `flying_shot`, `integrate` → `advance`, `on_screen` →
  `project_to_screen`, `project` → `on_screen_or_free`, `project_kept` → `on_screen_kept`,
  `home` → `homing_steer`, `turn` → `steer_axis`, `retarget` → `homing_retarget`, `gather` →
  `homing_gather`, `throw` → `homing_thrown`, `launch` → `homing_launch`, `spawn_circle` →
  `homing_spawn_circlers`, `circle` → `homing_draw_circlers`, `in_front` →
  `homing_in_front`, `clear_charge` → `homing_clear_charge`, `split` → `splitter_split`,
  `spawn_part` → `splitter_part`, `orbit` → `splitter_orbit`, `tail` → `wave_flight`, `edge`
  → `weave`, `draw_check` → `draw_on_odd_frames`, `charge` → `wave_charge`, `spread` →
  `wave_spread`, `spread_shot` → `wave_spread_shot`, `brew`/`bubble`/`tether`/`let_go`/
  `pull`/`blow` → `three_way_*`, `unleash`/`pairs`/`orbiter`/`smite`/`spring`/`flash`/
  `end_attack` → `super_*`, `by_timer` → `sprite_by_timer`, `draw_parity` →
  `draw_alternate_frames`, `recover` → `bounce_off_wall`, `above` → `above_player`; everything
  else keeps its name. The 6502 addresses stay in the doc comments.
- The literal tests of the selection byte elsewhere (`player.rs`, `stage.rs`, `engine.rs`,
  `room.rs`, families) become variant comparisons. `engine::spend` nearly repeats
  `bounce_off_wall`: noted in `docs/MODERNIZE.md`, not changed here.

## Plan

1. Rename `Weapon` → `Projectile`, `weapons` → `projectiles` alone first (compiles, tests
   unchanged), committed on its own so the rest of the diff reads cleanly.
2. Tests first: `weapon_bytes_round_trip` (0-6, `None` past 6).
3. Add the enums, change `Effects::weapon`, convert the projection; fix every compile error
   by the rules above.
4. Rename; correct the doc comments and `ACTORS.md` §9g to say what each weapon and stage is.
5. `cargo test -p rotj-core`, the release `--ignored` suites of rotj-core, rotj-oracle and the
   app, a 1-seed random play sweep and a 300-branch exploration, clippy. The tapes must be
   unchanged.
6. The `reviewer` agent; the gates; commit `The weapons as named states`.

## Not here

Typing the per-stage projectile fields, the projectile's `kind` as a sum type, the shared
contact code in `engine.rs`, the slot aliasing.
