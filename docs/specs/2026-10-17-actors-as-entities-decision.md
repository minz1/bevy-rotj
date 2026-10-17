# Actors as entities: the spike's questions, answered (1.1.x step B3)

Status: **decided: the mirror** (the owner, 2026-10-17 morning, after the spike's results
below: Classic's actor model is a fixed array walked in a fixed order, so a core on
`bevy_ecs` is `Slots` with extra steps; the ECS serves what reads the simulation, not the
simulation). The spike, `docs/specs/2026-10-17-ecs-spike.md`, ran 2026-10-17 night on the
branch `ecs-spike`. "Proposed decision" below is the decision. Step B3 of
`docs/specs/2026-10-16-modernisation-roadmap-design.md` ("actors as entities, opening with
a one-family spike ... The fallback, the default unless the spike shows the ECS buys more:
the simulation stays a `Slots` resource and a one-way sync mirrors each slot to an entity
for rendering, the inspector and the editor"), from a read-only survey of the actor walk,
its cross-slot writes, the app and the roadmap's consumers.

## The four points, from the code

1. **A spawn into a lower slot runs in the same pass.** The walk (`step_actors`) goes
   23 down to 0 and reads each slot's `kind` live. Level spawns take the lowest free slot in
   0-15 (`spawn::free_slot`); children take the highest free slot in 16-23
   (`engine::free_child_slot`), so a child of a 0-15 spawner lands above it and waits a
   frame, and the same-pass case arises when a 16-23 slot spawns into a lower one. The
   write-back guard (`slot.kind != 0 || main[index].kind == running`) handles "freed, then
   re-spawned".
   An ECS walk keeps this only as an exclusive system over 24 persistent entities with
   immediate writes, which is the `Slots` walk with entity lookups added.
2. **Handlers read and write other slots mid-pass**: about 215 non-test lines matching
   `cx.slots.main[`, `cx.byte(`, `cx.set_byte(` or `free_child_slot` (`final_boss.rs`
   alone 44; the roadmap's "about 98" counted writes only): child spawns, a parent or child
   by stored index, target scans. Each would become a world lookup through an adapter, with
   no behaviour to show for it.
3. **Slots 16-23 alias other slots' fields**: `load_aliases`/`store_aliases` copy 11
   narrow arrays around each step, and for 10 of them `index + 16` lands in a field of
   slots 0-7 (`$7EE0 + 16` is `$7EF0`, slot 0's health high byte; `$7E30 + 16` is `$7E40`,
   slot 0's vx low byte); only `$7BA0` (age) reaches a projectile array (`$7BB0`), so the
   roadmap's "weapon arrays" is corrected. Under entities, slot 17's components would be
   slot 1's fields.
4. **`Work` is scratch carried between handlers**: it stays a plain struct either way.

## What entities would serve

- The app does not read slots: it renders from the OAM builder and the software frame.
- Phase C's Enhanced sprites read what the actors draw (read-only); Classic stays on the
  OAM builder (its 8-per-line drop and rotation are the game's). They cannot come from a
  `Body` mirror: an actor's look is computed inside its step and reaches
  `cx.oam.draw(sprite, DrawCall)`, and `Body.sprite` is not uniformly a frame (`$44`/`$C0`
  keep a parent slot there). The likely source is a per-draw-call record beside the OAM
  page (metasprite, `DrawCall`, slot), the pattern `oam::Mark` already follows.
- The inspector is not built; the editor places actors from the level assets
  (`SpawnRecord`), not from live slots.
- Interpolation (1.2.0) needs each actor's previous and current position: read-only.

Every consumer reads, over all 32 slots, the projectiles and the player; none needs the
simulation in the ECS, and all of them are in the app, which already runs a Bevy `World`.

## Proposed decision

- `Slots` stays the simulation's owner and the walk stays as it is; no spike is built,
  since the code answers its four questions.
- The ECS sits at the app boundary: Phase C's first step (still in 1.1.x) adds the
  draw-call record and a read-only app-side mirror of what Phase C's renderer, the
  inspector or interpolation read; its contents are Phase C's spec's to decide. The core
  keeps no Bevy dependency unless that spec shows a core test needs one.
- B2's slice 2 (the loop as a schedule in the core) is dropped, or folded into Phase C if
  an app-side schedule appears there: one hand-ordered chain over `Game` gains nothing from
  a schedule while no system outside the simulation shares it.

## The owner's question

"The roadmap says that by the end of 1.1.x the core is on Bevy's ECS with actors as
entities. For Classic the ECS walk would be the same 23-to-0 walk with entity lookups,
and the actor state is already typed Rust (`Slot`, `Body`, `Brain`, the families' named
phases). Do you accept a plain-Rust core with the ECS at the app boundary (a read-only
mirror built for Phase C's renderer) as meeting 'modern patterns and modern tech', or do
you want the core itself on `bevy_ecs` (resources and a schedule) even where it buys
nothing measurable?" The stand-in recommends the first. The roadmap's goal sentence, its
Phase B and PLAN's 1.1.x row change only with the answer.

## What the next modernisation steps are instead

These hold whatever the answer: typed cross-slot access (`docs/MODERNIZE.md` #13: one
accessor over a field enum instead of the address-keyed `byte`/`set_byte`/`find_owner`,
the 16-23 aliasing handled in that one place; it removes most raw `$7xxx + index` sites and
makes any later entity move mechanical); B4 (input as actions); `Boot`'s linear parts as a
script table; per-family names for the `Body` bytes; then Phase C's spec.

## The spike's results (2026-10-17)

Branch `ecs-spike` (kept for reference, not merged): the 24 main and 8 second-class slots
as 32 entities in a `bevy_ecs` `World` inside `Slots` (`default-features = false,
features = ["std"]`; no new packages, bevy 0.20 already locked), the walk unchanged
(23 down to 0, over the entities), and `bobber_spawner`'s child drop rewritten as a query.

- **Byte-exact first time:** the debug lib tests (552), the release `--ignored` suites
  (27, the whole-game chain included), a 1-seed sweep: all clean, no fix needed. A test
  pins the query version against the indexed one over six free-slot patterns.
- **Build:** a clean release build of `rotj-core` takes **4 s** today and **15 s** with
  the spike (`bevy_ecs` and its macros).
- **Run:** the whole-game chain test in release, two runs each on an idle machine:
  **18.5 s / 19.0 s** today, **21.9 s / 21.6 s** with the spike (about 15% slower: entity
  lookups on every slot access).
- **Code:** 133 call sites in 48 files changed, 122 of them only by an accessor
  (`slots.main[i]` becomes `slots.main()[i]` or `slots.main_mut()[i]`), 11 in shape
  (slices like `main[..16]` become `iter().take(16)`, loops over the arrays become
  `iter_mut()`); `Slots` gains about 200 lines of plumbing (index views over the entities,
  hand-written `Clone`, `PartialEq`, `Debug` and `Default`, which a `World` lacks, and a
  `SlotIndex` component).
- **The idiomatic family:** today's
  `(0..16).rev().find(|&slot| cx.slots.kind(slot) == 0)` becomes a query over
  `(Entity, &SlotIndex, &Slot)` filtered by index and kind, then `max_by_key` on the
  index: a query's order is not the slot order, so the game's scan order (which is the
  behaviour) has to be rebuilt from data instead of being visible in the loop. The field
  writes after it are the same.

**What it shows:** the ECS can hold the Classic simulation byte-exactly, at a cost of
about 4x the core's clean build, about 15% run time, noisier call sites, and a family that
is no clearer, because the lockstep's fixed order rules out what ECS is good at (queries
in any order, parallel systems). Nothing in the simulation got simpler. The read-only
mirror for the app's consumers remains the proposed decision; the owner chooses.
