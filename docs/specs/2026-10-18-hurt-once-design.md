# The hurt, written once

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: four callers; plain
speeds; the reason stated; the proof names the damage-tile test). HANDOFF item 9's leads ("the hurt rule's copies", "`stun`/`hurt`
names"); MODERNIZE "The hurt rule twice" and "Helper names in `player.rs`". No behaviour change.

## What is there

- The damage-tile response in `player::step_move` (`$82B9-$8310`) and `engine::hurt_player`
  (`$B17F`; four callers: contact `$A9C4`, the boss `$AFD6`, the hurt-only `$AEE3` and the stage 4
boss's slam `$980A`) both start a hurt the same way: the shot
  pose dropped, sound `$25`, a charge dropped (`attack &= CHARGEABLE`), `hurt_timer = $20`,
  `hurt = STUNNED | INVULNERABLE`, a fall turned into a jump, and a knock-back speed of `$20` away
  from where Batman faces. They differ only in what follows: the damage tile also spends energy
  and puts the speed in `work.ax` when he is on foot; `hurt_player` puts it in `vx` when he is on
  foot or aiming up. The writes have no reads between them, so their order is free.
- `player.rs`'s `stun` (`$92E8`) is the frame Batman spends stunned while knocked back: it counts
  the stun down, masks the pads and flickers his facing.

## The change

The reason is MODERNIZE's "one rule": the hurt's start is written once. It saves no lines (the
helper and its doc about equal what it removes).

- `engine::start_hurt(player: &mut Player) -> i16`: the player's part of the shared start (pose,
  timer, the hurt bits, a fall turned into a jump); returns the knock-back speed. Both callers keep
  their own sound and charge lines (the damage tile works on a local copy of the player, so
  `Effects` can't be passed whole) and their own "on foot" rule for where the speed goes. The speed is
  written `-0x20`/`0x20`.
- `stun` becomes `stunned`, its doc "the frame Batman spends stunned while knocked back".
- `hurt` (`$925D`, the `Hurt` motion's handler) keeps its name, like the other motion handlers.

## Proof

`hurt_player_matches_b17f` (the shared start through `hurt_player`); the damage-tile branch
(`PATHS` bit 9) is not in the stage 1-1 move test's coverage, so a counted run of the sweep shows
the lockstep reaching it; the player's oracle tests, the Classic suites, a sweep and a
300-branch exploration unchanged; tapes byte-identical.
