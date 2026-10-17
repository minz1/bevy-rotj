# The families' repurposed bytes named (1.1.x, after A3), pilot

Status: approved with changes by the Opus stand-in, 2026-10-17 (applied: the form chosen
by how a byte is used, not accessor pairs everywhere; the bosses out of the pilot, their
phase docs instead; the dropper's writes into its payload named in the payload's file;
`Body`'s per-family doc exceptions moved into the families; an explicit exit). From a read-only survey of the 105 family files; the step the roadmap's
"keep looking" and the owner's "variable names that make sense" ask for.

## What the survey found

`Body` mirrors the game's per-slot WRAM arrays, so a field means what its family makes of
it: `anim` is a child's slot in `dropper` and a parent's in `boss_shell` and
`stage4_boss_slam`, a "released" flag plus a step in `dropper_payload`; `sprite` a parent's
slot in `final_boss_bouncer` and `stage4_boss_shot`, a child's variant in six spawners;
`subtype` a parent's slot in `dropper_payload`, a `$80` flag in `ace_ranger`, half a
velocity in `blast_walker`; `counter` a slot in `stage6_lobber`, a bitfield in
`ace_ranger`, a lifetime in `boss_shell`; `hit` an accumulator in `crumbling_block`;
`scratch` a toggle in `final_boss`. Four forms already name such a byte: a local
`const X: Field` for half of a 16-bit field (the platforms' `TRAVEL_LOW`, the bursts'
`LIFETIME`), a local binding at the use (`let parent = usize::from(body.anim);` in
`boss_shell`), a family doc line (`dropper.rs`'s "keeps the child's slot in `$7E90`"),
and value consts (`CHILD_SPRITE`, `THROWN`). The bosses' bytes change meaning by phase
(`final_boss`'s `anim` is a step, a countdown, a CHR ramp with a flag, a count).
`Body` must stay the WRAM mirror the oracle compares.

## Design

The form follows how the byte is used:
- **A slot link**: a local at the use (`let child = usize::from(body.anim);`); one private
  `fn child_slot(body: &Body) -> usize` only where a file reads it three or more times.
- **A 16-bit pair**: a named local or a `u16` getter (`let landing_y =
  u16::from_be_bytes([body.counter, body.timer]);`).
- **Flag bits**: value consts (`const LANDING: u8 = 0x80;`), as `THROWN` already is.
- **Half of a 16-bit field**: the existing `const X: Field`.
- **A phase-local counter**: the phase's doc says what the byte holds there; no rename.
- Free functions, not `Body` methods: methods from 105 families would share one namespace
  on `Body` (two families' `child_slot` would clash).
- A family doc line lists its mapping; `Body`'s own field docs go back to the WRAM meaning
  (its per-family exceptions, `sprite`'s "`$44` and `$C0` store a parent slot" and
  `scratch`'s list, move into those families).
- Pilot: `crumbling_block` (`hit` and `behaviour` a 16-bit fall speed, `state` a step
  counter with a `$80` flag; its `clear` copy of `hit` stays a raw byte), then `dropper`
  with `dropper_payload`: the dropper's writes into the payload's bytes (`subtype` the
  parent slot, `timer`/`counter` the landing height, `anim` the step) become one
  `pub(crate) fn arm(child: &mut Body, parent: u8, landing_y: u16, ..)` in
  `dropper_payload.rs`, so the payload's layout lives in its own file; `anim & 0x06` and
  `>= END` stay on the raw byte.
- The bosses (`final_boss`, `stage3_boss`): only a check that each `Phase` variant's doc
  says what `anim`/`counter`/`scratch` hold in it.
- Exit: if `crumbling_block` does not read better, stop at doc lines.

## Tests

- The families' oracle tests, the Classic suites, a 1-seed sweep and a 300-branch
  exploration; the tapes unchanged.

## Plan

1. `crumbling_block`; read the result; stop at doc lines if it reads no better.
2. `dropper`/`dropper_payload` (with `arm`); the bosses' phase docs; `Body`'s docs.
3. Checks, the `reviewer` agent, commit `The families' repurposed bytes are named: the
   pilot`.
