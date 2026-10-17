# The 1.1.x modernisation: order and shape

Status: approved with changes by the Opus stand-in, 2026-10-16 night (applied: levels as
assets before the ECS; actors as entities opens with a spike whose four rules are named, with
a named fallback; the lockstep's snapshot is the RAM projection; the script stays a resource,
not Bevy `States`; each byte classified before it changes; the player as a character and the
behaviour tables restored; Classic's picture stays `Frame`; the `Outside` hook named). The owner approved moving
the GPU renderer after the actors and levels, and gave standing approval to reorder further
as understanding grows ("use new patterns everywhere possible, because we can"; the port
should be a touchpoint for NES software, maybe for kaizo ROTJ levels). This replaces the
1.1.x row's order in `docs/PLAN.md` and the "Re-architecture outline".

## Goal

By the end of 1.1.x the shipped game is idiomatic Rust, with Bevy's ECS at the app boundary
(decided 2026-10-17, Phase B): named states instead
of state bytes, actors as entities, levels as assets, scripts as data, rendering from the
entities. The only NES-shaped code left is test-side (the 6502 oracle, each subsystem's
projection to the NES RAM layout, the tapes) and Classic's rules, written as named, tested
rules (update order, free-slot choice, RNG steps, the fixed point, the OAM allocator, whose
flags the game reads back). Every step is a patch release that changes no behaviour: the
Classic suites (lockstep chain, tapes, random play, exploration, picture parity, playtests)
stay green and the tapes unchanged.

## Why this order

A step is cheaper and safer when what it builds on is already in its final shape.

- A renderer of the NES's own state (nametables, OAM, per-line raster state) on the GPU
  would have to keep the 8-sprites-per-line rule exact, which sprite entities cannot do
  without slicing sprites into rows; and it would be replaced once actors are entities and
  levels are assets. So rendering comes last, from the entities, while Classic's picture
  stays the software `Frame` (the reference the picture tests already read) until then.
- Moving the core into the ECS is easiest once its pieces are idiomatic: a named state
  machine moves into a system as is; a byte dispatching to 6502-shaped routines does not.
- Levels as assets needs no ECS and is the payoff the owner named (the editor, kaizo
  levels), so it comes before the riskiest step, not after it.
- If the port is to be a touchpoint for NES software, the transferable asset is the method
  (the oracle, lift and project, the tapes) and a core that stays plain Rust behind a thin
  Bevy layer; another reason the actors' ECS move needs a spike that shows its worth.
- Each early step is small, independent and proven by the lockstep alone, so the fleet can
  take the family-by-family work in parallel.

## Phase A: idiomatic Rust in the core (no new framework)

Each byte is first classified: a state (an enum, `#[repr(u8)]`, converted at the RAM
projection, so the lockstep compares exactly what it compares today; its `from_byte` panics
exactly where today's dispatch does), a set of flags (a newtype with named bits), or a table
index (a typed index). Many are more than one: the motion is a state that also indexes the
game's per-motion tables and is saved as a raw byte, so it keeps a `byte()`. Each routine-
shaped dispatch becomes a `match`, each magic number a named constant or a field. Behaviour,
update order and RNG steps unchanged.

1. **The player's motion** (`Player::motion`, `$046E`, `player::Port::run`'s `m0..m11`):
   the pilot, one reviewed spec, the template for the rest. Then **the player as a
   character**: his other bytes classified (the pose's flags and saved motion, the attack
   and stage-script flags), his physics constants and tables named, so a later character
   (PLAN's Robin or Nightwing) is a different definition of the same type.
2. **Weapons** (`weapons.rs`, 2231 lines, a handler byte dispatching `zero`..`five`).
3. **Actor families**, family by family (`families/*`, their state bytes), in parallel once
   the pilot's pattern is set; each family's oracle tests and the chain prove it. Each
   family's behaviour tables (velocities, hitboxes, metasprite pointers: the handlers read
   the ROM at run time in about 390 places) become typed data that the level importer
   (below) produces, since new content and kaizo levels need them as much as layouts.
4. **Scripts as data** (`Boot`, `NextArea`, `RoomChange`, `Cutscene`, `Ending`, `Attract`,
   `SoundTest`, `Continue`: hand-rolled step loops reproducing 6502 jumps): the `Ending`'s
   `SCRIPT: &[Action]` table shape generalised.
5. Small collapses the survey found (`docs/MODERNIZE.md`): the palette fade helpers, the
   PPU write-list decoders, the duplicate `integrate`, address-keyed `byte()`/`set_byte()`
   matches as an index enum, the per-frame allocations in `compose`.

6. **Names** (the owner: "everything readable, variable names that make sense"): modules,
   types, functions and fields named for what they are in the game, not for the 6502
   (`families/k92.rs` becomes the actor's name with its kind byte a constant, `m0..m11`
   the motion states, `$C247`-shaped helpers what they do); the addresses stay in the doc
   comments, where they connect the code to the listing. Done alongside each step above,
   and as a sweep of what remains.
7. **Keep looking.** New candidates go into `docs/MODERNIZE.md` as they are found, ranked;
   each step's review asks what else its area could lose or simplify, and builds its
   pattern for the steps after it (a state enum that can become an ECS component, an
   importer that serves the level editor).

Parked for the owner: removing `Outside`/`Brain::Unported` changes the tape format.

## Phase B: levels as assets, then the core on Bevy's ECS

Decided 2026-10-17 (the owner, after B3's spike;
`docs/specs/2026-10-17-actors-as-entities-decision.md`): the Classic simulation stays plain
Rust (`Slots` owns the actors, `Game::frame` walks them), and the ECS sits at the app
boundary as a one-way, read-only mirror for the renderer, the inspector, the editor and the
Enhanced picture. B2's schedule and B3's entity move below are superseded by that; a
simulation free of Classic's parity, if ever wanted, is a separate mode with its own tests.

Reordered 2026-10-17 (`docs/specs/2026-10-17-frame-steps-design.md`): B2 opens with its
first slice (the frame's steps named, no new dependency); B3's one-family spike then comes
before B2's resources and schedule, because the spike decides the actor walk's shape, which
fixes `Cx` and the systems' signatures. The spike brings in `bevy_ecs` and records the
clean-build and chain-test times.

1. **Levels as assets** (before the ECS: it needs none, and it is what the editor, bonus
   levels and the community's own, kaizo, levels need): the ROM decoders become an importer
   run on the player's ROM at build time (like the embedded ROM, nothing ROM-derived
   committed) into a documented, Tiled-compatible format the game loads as Bevy assets,
   feeding today's `World` and `Schedule` state. The lockstep alone proves it.
2. **The loop as a schedule**: `Game`'s pieces become resources (`World`, `Effects`,
   `Scroll`, the OAM builder, the script) and `Game::frame`'s steps systems in a fixed,
   chained order in a dedicated schedule run once per tick, with `bevy_ecs` alone so the
   core's tests still run without an app. The script is a resource the schedule branches
   on, not Bevy `States`: a new script's first step runs in the frame it starts, and a
   finished pause or boot falls through into the loop body the same frame, which `NextState`
   would shift. The lockstep's snapshot is the RAM/WRAM projection (`project_into`), and a
   lift writes the resources and components in place (a `World` is neither `Clone` nor
   `Eq`); the spike records the chain test's time and the clean build's before and after.
   Until the owner approves removing `Outside` (a tape format change), the schedule keeps an
   exclusive-system hook for its callback.
3. **Actors as entities**, opening with a one-family spike whose spec answers the four
   points where an ECS fights Classic: a spawn into a lower slot runs in the same pass
   (Bevy's `Commands` defer it); handlers write other slots mid-pass (about 98 sites); slots
   16-23 alias the weapon arrays; `Work` is scratch carried between handlers. The rules to
   pin: slot entities are spawned once and never despawned (kind 0 is empty), the walk is an
   exclusive system with immediate writes, the aliasing and `Work` are named rules. The
   fallback, the default unless the spike shows the ECS buys more: the simulation stays a
   `Slots` resource and a one-way sync mirrors each slot to an entity for rendering, the
   inspector and the editor.
4. **Input as typed buttons**: the pad becomes `Buttons`, the game's own buttons with
   its groups, and the `$F6` byte only at the Classic boundary (recordings, the oracle).
   Remapping and any action layer are the app's (a settings step); the core has no
   `Action` enum (`docs/specs/2026-10-17-buttons-design.md`).

## Phase C: rendering from the entities

Tilemaps from the level assets, sprites from the actor entities, the HUD from state (the
`Modern` UI): the Enhanced picture, and the natural home of no sprite limit. Classic's
picture stays the software `Frame` (or its sprites come from the OAM builder's output),
because sprite entities cannot keep the 8-per-line drop and the allocator's rotation any
better than a GPU renderer of the NES's state could; a GPU readback check, if any, needs a
headless adapter (WSL, CI), which C's spec settles. Then the Enhanced features that need it
(interpolation and `frame_rate`, CRT filters, native-resolution rendering), as 1.2.0 onward,
with widescreen after levels and actors.

## How each step is proven

The Classic suites at every merge; a step's own oracle tests; the tapes unchanged
(re-recording one is a reviewed change); the gates on the exact commit before a push. A
step that cannot keep the tapes unchanged stops and asks.

## Not decided here

Each step's details (its own spec when it is large: A1, A2, B1, B2, B3, C), the `Modern` UI
layout, the level format's schema.
