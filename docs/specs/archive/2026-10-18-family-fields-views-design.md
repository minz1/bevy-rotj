# Typed per-family state: views over `Body`

Status: **deferred** by the Opus stand-in, 2026-10-18. In the three first-slice families no
handler reads better with a decoded `Fields` copy (the stomper's `counter`/`anim` change meaning
by phase, the crumbling block's fall speed is clamped by its low byte, the payload's `arm` writes
engine fields too), and a copy held across `engine::load`/`integrate`/`end_frame` could undo their
writes, a bug class that does not exist today; the enum dispatch and the mirror component would
be scaffolding for an inspector that does not exist. 1.1.5's names and `arm` stay the form. The
real consumer, typed spawn properties (`side`, `path`, `drop`), goes with the level format's next
version, as a codec over `SpawnRecord`; if an inspector arrives, start with a read-only
`describe(&Body)` per family. The survey below is kept for then. HANDOFF item 8 (`Body` beyond the WRAM layout: typed per-family state,
carried by the RAM projection and the mirror; the level format's later typed actor properties).
No behaviour change: the lockstep proves it, tapes unchanged.

## What the survey found

- `Body` is 23 fields over 30 WRAM arrays. Every one of the 105 families' `State`s is a unit
  struct: all per-family state lives in `Body`'s bytes.
- The engine reads some fields for every actor (position, `flags`, velocities, `hit`'s bits,
  `dying_timer`, `health`, `contact`, `state`'s two shared bits, `cell`); 78 families give
  `timer`, `counter`, `anim`, `sprite`, `scratch`, `behaviour` or `subtype` (and a few `hit` or
  `vx_drift`) their own meaning, which 1.1.5/1.1.6 named only as locals, consts and docs.
- Moving those bytes into the `Brain` payload breaks on residue (a freed slot's bytes seed the
  next spawn), on about 40 families' cross-slot writes made before or without the child's
  `Brain`, on the narrow fields' aliasing between slots, and on the oracle's mid-run lift. So
  `Body` stays the WRAM mirror.

## The design

- **A view per family**: in a family's module, `Fields`, a plain `Copy` struct of named, typed
  values (enums, `u16`s, slot indexes), with `Fields::of(&Body) -> Fields` and
  `Fields::store(self, &mut Body)`, which writes back only the bytes it decodes. Handlers use it
  where it replaces today's locals and consts, not everywhere at once.
- **One dispatch**: `families::fields(kind, &Body) -> Option<families::Fields>`, an enum over the
  families done so far (`_ => None`), hand-written until every family has a view; then it joins
  `families!`.
- **The mirror**: `mirror::DrawerState` gains `fields: Option<families::Fields>` and the slot
  entities a `FamilyFields` component, set on change beside `Kind`, for the inspector (its `Debug`
  output) and later the editor.
- **First slice** (one release): `stomper` (a self-contained three-phase machine whose
  spawn-seeded map cell `[subtype, behaviour]` is the level format's case), `crumbling_block` (a
  second-class actor on wide arrays only, the one engine field reused as data: its fall speed
  `[behaviour, hit]`), and `dropper_payload` (the parent-writes-child case: `dropper`'s `arm`
  becomes `Fields { .. }.store(child)`).
- **Later slices** take families a few at a time; too entangled for now (each its own design when
  reached): `final_boss`, `stage3_boss`, `stage4_boss` and its slam, `ace_ranger`, the stage 6
  boss and its lobbers and spawners, `spinner_spawner`, `falling_enemy_spawner`, `turret`,
  `next_area_gate`, `column_eraser`, `boss_defeated_script`, the platforms that keep a parameter
  in an aliased narrow byte (`sinking_platform`, `vertical_shuttle_platform`, `path_platform`),
  `blast_walker`, the two-kind modules, and `dropper` itself.

## Proof

- Per view, over many patterned bodies: `store(of(b))` leaves `b` unchanged, and changing one
  decoded field changes only its declared bytes (compared through `Field::ALL`).
- The three families' oracle tests, the Classic suites, a sweep and a 300-branch exploration
  unchanged; the tapes byte-identical.
- A mirror test that a snapshot's `fields` equals `families::fields(kind, &body)`.

## Not here

Typed spawn properties in the level format (a view over `SpawnRecord` bytes in the same modules,
with the level packs once the owner answers); the entangled families above.
