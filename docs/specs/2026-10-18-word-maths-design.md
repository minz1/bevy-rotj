# Byte-wise 16-bit arithmetic as word maths

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: collision's `push` and
`settle` are not plain adds; the launch offset's V flag is the signed word overflow; the muzzle is
8-bit maths, out; `move_by_velocity` and the burst parts converted; two engine sites named exactly;
the splitter's negate converted with its carry named; `word` and weapons' ROM `word` deleted; four
exhaustive tests instead of six; one release in four commits). The survey loop's #8 (MODERNIZE
"The survey loop, 2026-10-18"). No behaviour change: each converted form is checked exhaustively
against the byte-wise code it replaces, and the lockstep proves the whole.

## What is there

`player.rs`, `weapons.rs`, `engine.rs` and `collision.rs` do 16-bit position and address maths a
byte at a time (`overflowing_add` on the low byte, the carry into the high byte, `from_be_bytes`
round trips) where the values are already `u16` words, at about 40 sites; `collision.rs` has its
own `hi`/`lo`/`word`/`adc` helpers, `slots.rs` a second `hi`/`lo`, `weapons.rs` a ROM `word`. Some
sites are not plain adds: a negate whose carry leaks into the next add (`push`), a flag the 6502
takes from the high byte's add (the launch offset's V), adds to the low byte alone (the nametable
row wraps). About 45 more `overflowing_*` lines in 29 other files (families, scroll, room, raster,
attract, levelmap) are out of this release.

## The change

In four commits, each passing the lockstep before the next: collision (with the helper move),
engine, weapons, player. Sites are named by function; the line numbers move.

- **Helpers.** One `hi`/`lo` pair, `pub(crate)` in `engine.rs`; `collision.rs`'s and `slots.rs`'s
  copies go. `word` goes: its uses become masks or `from_be_bytes` (crumble `(probe.x & 0xFE00) |
  0xF0`, `(probe.y & 0xFF00) | 0xF0`; `stand_on_screen` `work.y & 0x0FFF`, `(work.y & 0xFF00) |
  (probe.y & 0xFF)`; `stand` `i16::from_be_bytes([behaviour, hit])`). `adc` goes once `push` is
  converted. `weapons.rs`'s ROM `word` goes for the existing `rom_split_words`. The test-only
  `layout::word`s and `engine::low_byte(i32)` stay (different things).
- **collision.** `push`: one small fn, `back(a) = u16::from(a.wrapping_neg()) + u16::from(a == 0)`
  (the ROM's negate, whose carry leaks into the next add, doc'd in a line); leftward and vertical
  `w + back(p)`, rightward `w + 0xFF00 + back(a)` then `| 0x0F` (`a = lo(probe.x) + 0x10`).
  `settle` is `(y & 0xFF00).wrapping_sub(1)`; `stand`'s `word(hi - 1, lo)` is `wrapping_sub(0x100)`.
  `column`'s `lo - 1` stays byte-wise: "the low byte only, no borrow".
- **engine.** The nametable step `(addr + 0x40) & 0x23FF`; under its guard (`lo >= 0xC0`) the
  row wrap is `addr & 0x243F`, doc'd "rows 30-31 wrap to rows 0-1"; `quad.wrapping_add(0xC0)` is
  `quad | 0xC0` (`quad` is at most `$3F`). The add to the low byte at the next site stays byte-wise:
  "adds to the low byte only, as the ROM does". `move_by_velocity`: the word from `body.get(hi)`
  and `body.get(lo)`, `.wrapping_add_signed(i16::from(v.cast_signed()))`, set back once. The burst
  parts read `rom_word(cx.prg, 13, data)` and `data + 2`.
- **weapons.** The plain word adds as `u16` maths. The splitter's left branch: `back =
  vx.wrapping_neg()`, then `x.wrapping_add(u16::from(vx == 0))`, doc'd "Classic adds the negate's
  carry". Gravity's V comes from the old low byte and stays computed before the word update.
- **player.** The plain word adds and subs as `u16` maths (the `mag == 0` site is
  `x.wrapping_add(0xFF00 | u16::from(neg))`); the launch offset becomes
  `offset.cast_signed().overflowing_add(pos.cast_signed())`, V from the aimed axis's result, as
  `slide_launch` already does (the `add` closure and its tuple go); `i16::from_le_bytes(
  probe.landing)` for the landing; the V taken from the old low byte stays computed first. The
  muzzle's screen-byte maths is 8-bit and stays.

## Proof

Four tests, each exhaustive over its small domain against the byte-wise code copied once inside the
test:
- **engine** `byte_carry_is_word_maths`: hi in {`$00`, `$7F`, `$80`, `$FF`} x every lo x every
  operand byte: add with carry = `wrapping_add(u16::from(b))`; sub with borrow = `wrapping_sub`; the
  sign-extended byte = `wrapping_add_signed(i16::from(b.cast_signed()))`; a `$FF` high byte =
  `wrapping_add(0xFF00 | b)`; and word + word over every pair of high bytes x low pairs {(0,0),
  (`$FF`,1), (`$80`,`$80`)}: the sum and `adc_overflows` = `i16::overflowing_add`'s flag.
- **collision**: the real `push` for every probe low byte x every work low byte x hi {`$00`, `$FF`},
  all three directions, against the copied `adc` chain, with the edges named (leftward `p == 0`
  gives `w + 1`, rightward `a == 0` gives `w - 0xFF`).
- **weapons**: all 65,536 `vx`, `(wrapping_neg, vx == 0)` against the `!lo + 1` / `!hi + carry`
  chain.
- **engine**: all 65,536 addresses for the nametable step, and the guarded domain for the row wrap.

Then the weapons' and player's oracle tests, the Classic suites, a sweep and a 300-branch
exploration unchanged, tapes byte-identical; and a grep that no low-byte `overflowing_*` is left in
the four files but the two named sites.
