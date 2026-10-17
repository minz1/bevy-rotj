# The player's flag bytes named (1.1.7)

Status: approved with changes by the Opus stand-in, 2026-10-17 (applied: `HIDDEN`,
`FROZEN`, `STUNNED`/`INVULNERABLE`, `CHARGEABLE`, `drain_power_up`, the docs it listed). From a read-only survey of every use;
the MODERNIZE candidates "the player's other flag bytes" and "helper names in `player.rs`".

## What the survey found

Four player-side bytes are tested and written as raw masks, the way `hit` was before
`slots::hit`:
- `stage_script` (`$0481`): only bits 7, 6 and 5, only the values 0, `$80`, `$A0`, `$C0`,
  `$E0`. Bit 7 freezes the player's movement (`step_move`), bit 6 skips the projection
  (`project`), bit 5 keeps the pads: `move_player` clears them only on bit 7 set and bit 5
  clear (`$D40E`, `& $A0 == $80`). Set by the arrivals, the scripts and the deaths.
- `busy` (`$AF`): one bit, `$80`, a scripted sequence holds the game; the pause tests it.
- `attack` (`$B8`): flags only (the charge count is `charge_timer`, `$B7`). Bit 7 the
  weapon charges (every weapon but `Bolt`; it survives the 14 `&= $80` resets), bit 6 a
  charge is building (B held), bit 5 the charged attack is out. Its doc's "a shot is armed"
  for bit 7 is the wrong meaning.
- `flags: [u8; 3]` is three unrelated bytes, `$0472`, `$0473`, `$047A`:
  - `$0472` bit 7: the power-up (the Super weapon) is on: invulnerable, the meter drains,
    weapon pickups ignored. `collision::damage`'s doc calls it "already hurt this frame",
    which is wrong.
  - `$0473` bit 7: sliding; the slide is its only setter, the contacts read it as "the
    player hits actors".
  - `$047A` bit 7: the hurt lockout (controls locked, `stun`/`hurt` timers run); bit 6:
    the invulnerability blink after it.
- `player.rs` helpers: `solid_below` probes the row two above the player (y grows down; it
  is the headroom check for standing from a crouch or a slide) and its doc says "under";
  `draw_flash` is the power-up's meter drain, not a draw (its doc says "hurt drain").

## Design

- The form `slots::hit` and `pose` use: a `pub mod` of `u8` consts per byte, in
  `actor/world.rs` beside the fields; the bytes stay `u8` (they mirror RAM and lift and
  project as bytes). Whole-byte writes compose the consts with `|`.
  - `stage_script::{FROZEN = $80, HIDDEN = $40, KEEP_PADS = $20}` (`HELD` would sit beside
    `Motion::Held`; bit 6 skips `$93A0`'s projection, draw, fall-off death check, blink
    countdown and power-up drain, so the player is not drawn).
  - `busy::SCRIPTED = $80`.
  - `attack::{CHARGEABLE = $80, CHARGING = $40, CHARGED = $20}` (`CHARGING`: B held since
    the press); the `&= $80` resets become `&= attack::CHARGEABLE`.
  - `flags` becomes three fields: `power_up` (`$0472`, `power_up::ON = $80`), `sliding`
    (`$0473`, `sliding::ON = $80`), `hurt` (`$047A`, `hurt::{STUNNED = $80, INVULNERABLE =
    $40}`: bit 6 is what the contacts and the damage tile test, the blink only shows it).
    The lift and projection keep `$0472`/`$0473`/`$047A`; every `&= $7F` on `power_up`
    becomes `&= !power_up::ON`; no read moves past `hurt_player`. The four `let gates = ..flags` copies read the fields they test.
- Renames: `solid_below` → `headroom_blocked`, `draw_flash` → `drain_power_up` (verb-first like
  `spawn_shot`), their docs saying what they do (the drain's end restores the stored weapon
  and `attack`, sets `hurt = INVULNERABLE`, `hurt_timer = $40`, clears the projectiles). The `bit` helper (the 6502 `BIT`'s V mirror) stays: which caller's
  later code reads V is not traced, and `$9790` reads a V that `$B8` did not set; named
  later, after that trace.
- Docs: the fields' docs (`stage_script`'s gains bit 5) and `collision::damage`'s say the
  meanings above; `docs/ACTORS.md` §7a checked against the new names.
- Not here: `attack_timer` (`$046B`, a counter), `bit`'s callers, the hurt rule's two
  copies (they differ: the tile path costs energy, writes `ax` not `vx`, and leaves out
  `AimingUp`; one function would need three parameters for two callers).

## Tests

- Every byte keeps its value: the oracle lockstep, the Classic suites, the tapes
  unchanged, a 1-seed sweep and a 300-branch exploration.
- `solid_below`'s direction checked against the ROM's `$92D1` before the rename.

## Plan

1. The consts and the field split in `world.rs`, every site converted (one agent per
   byte, disjoint files where possible); the doc fixes.
2. The renames.
3. Checks, the `reviewer` agent, commit `The player's flag bytes are named`.
