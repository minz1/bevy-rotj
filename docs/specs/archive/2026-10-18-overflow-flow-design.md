# Weapon 5's charge and the overflow flag, as data flow

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: the routine test's
incoming V stays on the test side; each dead writer's static reason; `&mut bool` through weapon
5's chain, `draw_second` passing V through; `bit` a `Port` method on a `Port` field; no
`charge_waits`; the rule's wording; the tests). Implemented in 1.1.16. HANDOFF item 7 (`Work` and `cpu_overflow` as explicit data flow);
MODERNIZE "The overflow flag carried between routines". No behaviour change: the lockstep's
checks of V at `$D42F`, `$D43B` and `$9790` and the Classic suites prove it.

## What the trace found

`Work::cpu_overflow` mirrors the 6502's V flag across routines for one reader: type `$80`
(`$9790`, `weapons::super_unleash`) tests `LDA $B8; BVC`, and `LDA` leaves V as the code before
left it, so the charge waits while V is set. About 30 sites write the field. Instrumented over a
sweep (430 reads), a 300-branch exploration (253) and a held-B probe (159), the last writer
before `$9790` was always the player's `$84AB` (motion handlers' `BIT`s) or, when a second-class
actor is live, `draw_second` (which then almost always clears V); the charge sat in slot 15 every
time, so no other projectile's handler was ever last. Writers ever last with V set: `melee_block`'s
`BIT $F6` (B held, about 90% of all reads), `standing`'s `BIT $047A` (invulnerable while knocked
back), and `jumping_input`/`crouching_release`'s `BIT $047F` (the shot pose). The intended rule
(`BIT $B8`, bit 6 = charging) would be "the charge waits while B is held"; Classic adds the quirks.

Every other writer is dead for `$9790`, by the code, not only the trace:
- the HUD, `step_second`, `step_move` and the lift: `$84AB` writes V first on every path
  (`Port::project`'s `BIT $0481`), so nothing before it reaches `$9790` in a whole frame;
- the other weapons' handlers and `power_up_pieces`: `weapons::step` runs one weapon per call,
  and the pieces switch to weapon 5 for the next call;
- every actor family's draw and the pause's: they run after `$9790`.
The `$D444` routine test lifts V from the oracle's CPU and hands it to `weapons::step`; that
stays, on the test side. One live path the trace never ran: `spawn_melee` takes the first free
slot from 15 down, so with a shot still flying the charge lands in slot 14 and the shot's handler
leaves the V; the threading covers it, and a unit test pins the port's model of it (the ROM
lockstep has not met it).

## The design

- **No shared flag.** `Work::cpu_overflow` goes:
  - `player::step_draw` (`$84AB`) returns the V it leaves. `Port` keeps an `overflow` field (it
    may start false: `project` writes it first); its `bit` becomes `set_overflow_from(value)`
    ("`BIT`'s overflow: the value's bit 6"), a sed over its callers, and the ADC and probe sites
    write the field.
  - `engine::draw`/`draw_metasprite` return `OamBuilder::draw`'s bool instead of storing it; the
    callers dead for `$9790` drop it.
  - `draw_second(.., overflow: bool) -> bool`: the V its last live slot leaves, the input passed
    through when none is live.
  - `weapons::step(work, cx, overflow: bool)`: `each_projectile` keeps a local and passes
    `&mut overflow` down weapon 5's chain (`super_weapon`, `super_attack`, `super_orbiter`, which
    already return a "keep going" bool); `flying_shot` sets it from gravity, the projection
    (derived inline as `on_screen && sub_overflows16(work.y, camera_y)`, so `project_to_screen`
    and its 20 callers carry nothing) or its draw; `super_orbiter` clears it; `super_unleash`
    takes the value.
- **The Classic rule**, on `super_unleash`'s doc and its `overflow` parameter: weapon 5's charge
  waits while the 6502's overflow flag is set, which `LDA $B8` leaves as the code before left it.
  In play that is the last writer among: Batman's frame (`$84AB`: set while B is held, while he
  stands invulnerable, or turning mid-jump or rising from a crouch), then the last live
  second-class actor's projection and draw (which usually clear it), then a shot in a higher slot.
- **The lockstep** compares the threaded values where it compared the field (`$D42F` after the
  player's draw, `$D43B` after `draw_second`, `$9790` in `super_unleash`).
- **Tests**: ROM-gated in `weapons`: weapon Super, the charge (type `$80`) in slot 14, meter 2:
  with V coming in set it stays `$80`, clear it becomes the pairs; with a live off-screen shot in
  slot 15, V set coming in still unleashes (the shot's projection clears it). ROM-free:
  `draw_second` with no live slot returns its input.
- **Not here**: the rest of `Work` (its per-call fields such as `pointer`, `x`/`y` in the weapons
  and `probe_flags` are locals in disguise; `anchor`/`tile`/`chr_slot` carry between slots) is the
  next survey item.

## Proof

The Classic suites (the chain, the random-play stages, the weapon-5 charge test), a sweep and a
300-branch exploration unchanged; the tapes byte-identical.
