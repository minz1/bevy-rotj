# Slots 16-23's aliased bytes: write back only what the handler changed

Status: approved by the stand-in (L3) 2026-10-31: approach and both limits accepted, the Body clone
kept for every slot 0-23, the oracle's `in_slot` loads and stores under one `index < 24` guard,
released on its own as 1.1.55. Item 11 of the 2026-10-29 run (MODERNIZE's "A lead (unreached)",
level packs release 6's "Not here").

## What is there

Slots 16-23 have no narrow arrays of their own (`slots::NARROW`, 16 wide): the game indexes
`base + slot`, so slot 16+c's `Contact`, `Age`, `HealthHigh`, the velocity bytes and `RefVx*`
are slot c's next-array bytes (`HealthHigh`, a weapon array, `DyingTimer`, `VxLow`, ...,
`Anim`, `Contact`, `RefVxHigh`). `step_actors` (and the oracle's `Routine::in_slot`, and the
column eraser's oracle test) take the running slot's `Body` out, `load_aliases` fills its
narrow fields from those bytes, the handler runs, and `store_aliases` writes all eleven back.

If the handler writes one of those bytes through `cx` (slot c's field, or the weapon array)
while it runs, `store_aliases` overwrites it with the value it loaded. In the game it is one
byte, so the handler's write stands. The 2026-10-17 probe found no step in the TAS, the
suites, a sweep or an exploration that does this; packs (release 6) can place actors so it
happens.

## Reaching it

Type `$94` (`stage6_lobber_spawner`) hands its child `$96` the highest free main slot in 0-15
and writes the child's `anim`, `contact` and `health` there. With the spawner in slot 23 and
slots 8-15 taken, the child lands in slot 7, and those writes are slot 23's `VyLow`,
`RefVxHigh` and `Contact` aliases: `store_aliases` undoes them.

The oracle test (`stage6_lobber_spawner::tests`) starts from the existing scene (TAS frame
33631, area 1C:00), isolates `$94`, and before the run writes WRAM so that slot 23 is a `$94`
about to spawn (`$7DD0` 0, `$7E90` 0, so the next odd frame spawns) and slots 8-15 are `$94`s
that idle (`$7DD0` = `$80`, the spawned bit; the handler returns before anything else). Slot
1's own `$94` is set to idle too. `assert_actor_phases` then diffs every actor loop against
the ROM; the test also asserts that slot 7 holds a `$96` after the spawn frame, so it cannot
pass by not reaching the case. It is red on `next` and green after the fix. On `next` the
first difference reported is zero page `$0042` (the diff walks RAM before WRAM); slot 7's
`contact` (`$7EE7`) is port 00 against the ROM's `$50`, undone by `store_aliases`; `$7E97`
and `$7EF7` happen to equal the loaded values.

## The fix

`load_aliases` returns the body as loaded (a `Body` clone; nothing for slots 0-15 changes);
`store_aliases` takes it and writes a narrow field back only where the handler's body now
differs from it. The three call sites bind the loaded body and pass it to `store_aliases`.

What stays approximate, stated in the doc comment: a handler that writes the same byte both
ways (its own field and, through `cx`, the alias) keeps its own write whatever the order,
unless its own field ends at the loaded value (then the `cx` write stands), and
a handler that reads its own field after writing the alias through `cx` reads the loaded
value. The test covers neither; nothing in the game or the original pack does either.

## Proof

- The oracle test above, red before and green after.
- Classic unchanged: the tapes, the lockstep suites, the full set (sweep, explore). The probe
  showed no aliased write through `cx` anywhere they reach, so every existing write-back is a
  changed field or an equal value.

## Not here

- Second-class slots (24-31): `step_second` loads no aliases; unchanged.
- Folding the three take/load/step/store sequences into one helper: a cleanup for after this
  ships, if wanted.
