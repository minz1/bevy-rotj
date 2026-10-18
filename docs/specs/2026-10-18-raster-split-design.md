# The raster split, typed

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: `rooms::Raster` reused
for the header; no new `chain`/`step` names; the lift's `$D0` assert; the delay's byte order and
the handlers' delay constants as words; `bands` from 0 with the literal indexes listed; the boot
test through the projection; the chains the proof reaches). The survey loop's #3 (MODERNIZE "The survey loop, 2026-10-18"). No
behaviour change: the lockstep (which compares every frame's 240 lines and the IRQs' RAM) proves
it.

## What is there

`Scroll::room: [u8; 13]` is `$D0-$DC`, indexed raw about 150 times (raster.rs most, then the
scripts: cutscene, boot, ending, stage, room, attract; `actor::modern` and the app's `play.rs`
read `room[0] & 0x80`). The bytes:
- `$D0` (`room[0]`): bit 7 makes the NMI arm the first IRQ (`$E1E3`);
- `$D1:$D2` (`room[1]`, `room[2]`): the first IRQ's delay in CPU cycles after the NMI;
- `$D3` (`room[3]`): which handler chain runs (0-`$0D`);
- `$D4` (`room[4]`): which handler of the chain the next IRQ runs (the chain's step);
- `$D5-$DC` (`room[5..=12]`): the chain's own bytes (band scrolls, bounce flags, counters),
  each kind using them its own way.

## The change

- `rooms::Raster` (`armed: bool`, `first_irq_cycles: u16`, `handler`, `next_irq`), already the type
  of `$D0-$D4` in a room's screen record, becomes the live header too: `Scroll::room` becomes
  `Scroll::split: Raster` and `Scroll::bands: [u8; 8]` (`$D5-$DC`, `bands[0]` is `$D5`). A room's
  `parameters` then sets `scroll.split = raster; scroll.bands = [0; 8]` instead of copying bytes.
  Its names stay (raster.rs already has a `chain` and a `step`); a doc says `next_irq` is the index
  of the IRQ the chain runs next, back to 0 when the chain ends.
- `first_irq_cycles` is `$D1` low, `$D2` high; the handlers' mid-chain delay pairs become words
  (`[0xA0, 0x2C]` is `0x2CA0`; likewise `0x31A0`, `0x39C0`, `0x1800`, `0x1640`).
- The off-by-five risk is in literal indexes a grep for `room[` misses, each moved by hand:
  `step(scroll, 6, ..)`, `bounce(scroll, frame, 7, 8)` and `(.., 5, 8)`, the credits'
  `[(5, ..), (7, ..), (9, ..)]` pairs, the intro's `room[5 + band]`, `room[5..=10].fill`
  (`bands[..6]`), and docs naming an index (they name the address, `$D5+i`, instead).
- The lift turns `$D0` into the bool with `debug_assert!(matches!(ram[0xD0], 0 | 0x80))`, as
  `rooms.rs`'s own decode does; the projection writes it back. The boot test that compares all of
  `$D0-$DC` with the oracle's compares through the projection.
- `bands`' per-chain meaning is a later slice, one chain at a time.

## Proof

The release lockstep compares every frame's 240 lines and the IRQs' RAM over the whole TAS, which
runs every chain the game's rooms and scripts use (STATUS 1.0.2); a counted run lists the `$D3`
values the lockstep reaches, and any chain it doesn't reach gets its band indexes checked by hand
or a raster test. Plus the boot's and `raster.rs`'s tests, a sweep and a 300-branch exploration;
tapes byte-identical.
