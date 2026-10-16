# The player's motion as a named state (1.1.x step A1)

Status: approved with changes by the Opus stand-in, 2026-10-16 night (applied: 4 is the
slide, 10 a scripted hold, 11 flying, from the handlers; the handler names below; the k50
test's out-of-range poke; the helpers `ground_attack`/`attack_ground` and others renamed). Step A1 of `docs/specs/2026-10-16-modernisation-roadmap-design.md`, the
pilot for the per-family state enums. Built from a survey of every read and write of the
byte.

## Goal

`Player::motion` (RAM `$046E`) is a `u8` that `player::Port::run` dispatches to `m0`..`m11`,
and a dozen places compare it with literals (`== 7`, `< 3`, `!= 0x09`). It becomes an enum
named for what the player is doing, with the handlers named the same way. Behaviour, RAM,
update order and RNG unchanged; the Classic suites prove it.

## The states

| byte | variant | what it is (evidence in the survey: `player.rs` handlers, `docs/ACTORS.md` 9h, 9m, 9n) |
| --- | --- | --- |
| 0 | `Standing` | on the ground, still; resumes a saved motion (`m0_resume`) |
| 1 | `Crouching` | pad down; the idle returns to `Standing` |
| 2 | `Walking` | on the ground with a direction held; the side-jump leaves to `Jumping` |
| 3 | `Jumping` | airborne with x control (the jump's sound and `vy = -$2C` on entry) |
| 4 | `Sliding` | Down+A from a crouch: launched in the facing direction, ends in a crouch or a jump |
| 5 | `Falling` | off a ledge or dropped; B or a direction returns to `Jumping` |
| 6 | `AimingUp` | pad up from standing or walking; the weapons' up-shot geometry |
| 7 | `Dying` | frozen by `engine::level_end`: death, and the boss beaten |
| 8 | `PoweringUp` | held during the power-up, hidden on odd frames |
| 9 | `BossEntry` | lowered into the boss room, the tally counted |
| 10 | `Held` | held by a script (the boss beaten, room `$DA0F`): no handler runs |
| 11 | `Flying` | steered through the air by the pad (actor `$54`, room `$D9FF`), only shooting |

The doc comments that call 3 "walking" and 4 "jumping" are corrected.

## Design

- `actor/world.rs`: `#[repr(u8)] #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] pub
  enum Motion` with the variants above (`Standing` the default), `pub const fn byte(self) ->
  u8` (the RAM value and the index into the game's 12-entry per-motion ROM tables) and
  `pub fn from_byte(byte: u8) -> Option<Motion>`. `Player::motion: Motion`.
- The two places the game stores a motion in another byte keep doing so through `byte()`
  and `from_byte`: the pose's saved motion (`pose & 0x0F`, `player.rs` `m0_resume`) and the
  power-up's `stage_args[1]` (`stage.rs`, restored in `weapons.rs`). `from_byte` there
  `expect`s a valid state, as `run`'s `panic!` on an unknown byte does today.
- The RAM projection (`world.rs` lift/project of `$046E`) converts with `from_byte`/`byte`.
- `Port::run` matches on the variants; the handlers are renamed for their state:
  standing (`standing`, `standing_resume`, `standing_input`, `standing_jump`,
  `standing_shot`, `standing_walk`, `standing_turn`, `standing_idle`, `standing_attack`),
  crouching (`crouching`, `crouching_held`, `crouching_attack`, `crouching_turn`,
  `crouching_release`, `crouching_shot`, `crouching_slide`), walking (`walking`,
  `walking_input`, `walking_stop`, `walking_jump`, `walking_shot`, `walking_step`,
  `walking_pose`), jumping (`jumping`, `jumping_input`, `jumping_shot`), sliding (`sliding`,
  `slide_on_ground`, `slide_launch`, `slide_glide`, `slide_blocked`, `slide_end`,
  `slide_jump`), `falling`, aiming up (`aiming_up`, `aiming_up_jump`, `aiming_up_shot`,
  `aiming_up_turn`, `aiming_up_attack`), `dying`, `powering_up`, `boss_entry`, `flying`;
  and the shared helpers `ground_attack` → `start_crouch` (down pressed: the crouch pose),
  `attack_ground` → `standing_attack`, `jump_probes` → `jump_blocked`,
  `jump_land_response` → `slide_off_ledge`. The 6502 addresses stay in the doc comments.
- Every comparison with a literal becomes a variant comparison (`families/*`, `engine.rs`,
  `frame.rs`, `weapons.rs`, `stage.rs`, `room.rs`); `families/kb6.rs`'s `HELD_MOTION` goes.
  The two "on foot" tests share a method: `Motion::on_foot(self) -> bool` (`Standing |
  Crouching | Walking`, the `< 3` in the player's hurt and `hurt_player`), with
  `|| motion == AimingUp` written out where the game adds it.
- ROM tables indexed by the motion use `u16::from(motion.byte())`.

## Plan

1. Test first: `motion_bytes_round_trip` (every variant's `byte()` through `from_byte`
   is itself; `from_byte(12..=255)` is `None`), `on_foot_is_the_first_three`.
2. Add `Motion`, change the field, convert the projection; fix every compile error by the
   rules above (no `as` casts besides `byte()`'s `self as u8`).
3. Rename the handlers and helpers; correct the doc comments.
4. Run `cargo test -p rotj-core`, the release `--ignored` suites of rotj-core, rotj-oracle and
   the app (the chain, the tapes), and a short seeded random play and exploration run (every
   relift goes through the lift's `from_byte`), clippy. The tapes must be unchanged. The
   raw RAM reads of `$046E` (exploration keys, tests) stay bytes.
5. The `reviewer` agent; then the gates; commit `The player's motion is a named state`.

## Not here

The pose byte's other bits, the player's other fields, the weapons (A2), the families' own
state bytes (A3).
