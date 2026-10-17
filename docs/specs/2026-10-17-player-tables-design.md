# Behaviour tables, slice 3: the player's and the weapons' tables (1.1.10)

Status: approved with changes by the Opus stand-in, 2026-10-17 (applied: about 30 more reads
in the inventory; the dispatch and bank-14 metasprite pointers out of scope; `anim < $88`, not
the writers claimed; the unguarded strip reads; `$96DB`'s two readers; a decidable overlay
test; a little-endian `rom_words`). Follows
`docs/specs/2026-10-17-family-tables-design.md` (slice 2, the families) and the storage in
`docs/specs/2026-10-17-behaviour-tables-design.md`. From a read of every `rb`/`rw` in
`player.rs` and every `rom_byte`/`rom_word` in `weapons.rs`.

## What this is, and is not

The player (`player.rs`, bank 4 through `Port::rb`/`rw`, plus bank 14) and the weapons
(`weapons.rs`, bank 5) read about 70 constant tables at run time. They become
`player::Tables` and `weapons::Tables`, fields `prg.player` and `prg.weapons`. Not in scope,
as before: metasprite pointers (`rw(0x9924 + ..)`, `0x9A24`, `0x9A38`, `0x9A40`, `0x9191`,
the weapons' `work.pointer` reads), dispatch pointers (`rw(table + weapon * 2)`, `0x9A75`, the
`$9EB5/$9EC1` table at :936: code addresses matched to handlers), bank 14's `$D1A4`/`$D1D8`
metasprite pointers (the entry poses index `$D1A4`; they never write `anim`), and the one
pointer chase (the weapons' `tables + pose` at :1558 and the reads through it). The HANDOFF's "one `PlayerMotion` set, not 47 tiny
imports" is met by grouping, below; a playable second character (PLAN's Robin) would be a
second `player::Tables`.

## Design

- **A helper first** (the cleanup slice 2 logged): `engine::rom_split_words::<N>(prg, bank,
  hi, lo) -> [u16; N]` for a high-byte table and a low-byte table, and `.map(u16::cast_signed)`
  where the destination is `i16`; slice 2's ~18 hand-rolled copies and `final_boss`'s
  `split_words` move onto it in the same step. A little-endian `rom_words::<N>` too, for the
  interleaved word tables (`$9ECD`).
- **Animation strips** (`player.anim = rb(base + x)`, x bounded by the guard before it):
  each read window is its own array named for the motion it animates (`standing_turn`,
  `walking_turn` — `$96DB` has both, `x < 2` and `x < 3` — `landing`, `long_landing`,
  `crouching_turn`, `jump_rise`, `walking`, `slide_rise`, ...), sized by its guard (`x < 2`,
  `x < 0x11`, `x < 8`). Windows that overlap in the ROM are imported separately; a shared
  byte is read twice. Unguarded reads need their own proof: `$96DD + pose_step` (:1423;
  `pose_step` is also 5 and counts to 6 under LANDING) and `$96FD` (:1800, :1807, and :1822,
  whose index is `melee_block().unwrap_or(pose_timer)`, a slot 0-15 or `$FF`): prove the
  range from the writers or size the array to it, the melee read a separately named
  256-entry window if nothing tighter holds.
  They are grouped in `player::Animations` so `player::Tables` stays a handful of fields.
- **Per-frame properties** (indexed by `anim`): `reach_y` (`$989C`), the overlay
  (`$9705`), the two flash tables (`$97A4`, `$982C`) become one `player::Frame` record per
  `anim`, `frames: [Frame; 0x88]`: the tables sit exactly `$88` apart (`$9705 + $88 = $978D`,
  `$97A4 + $88 = $982C`, `$989C + $88 = $9924`), and `$982C`'s entries past `$70` are
  reach_y's bytes (imported as the bytes they cover). `anim` has bit 7 set at times (room.rs
  writes `$85`), and its writers are the strips, `player.rs`'s literals, room.rs, the
  `flight_steer` family's `walk_frames` and the oracle lift fixtures; a silent test asserts
  every strip and table value and every literal is below `$88`.
- **The overlay's shot frames** (`$978D`/`$9795` by `overlay - $14`, `[u8; 8]` each): a
  silent test that every nonzero overlay in `frames` has `overlay - $14 < 8`; if one fails,
  the window is 256 entries.
- **Offsets by `Motion`** (the muzzle's `$966B`, `$9677`, `$9683`): `[u8; 12]` indexed by
  `motion.byte()` (`Motion` has 12 values, 0-11).
- **Palettes** (`$9662`, `$918D`; the weapons' `$9E10`, `$9D19`, `$9D1D`, `$9D21`): named
  `[u8; 3]`/`[u8; 4]` fields.
- **Bank 14** (`$D174`/`$D177` the entry poses by `held`; `$D26A` the HUD digit tiles, read by
  both `player.rs` and `engine::queue_counters`): the poses in `player::Tables`; the digit
  tiles a `[u8; 10]` field on `Prg` itself (`prg.digit_tiles`), its one non-family reader
  pair sharing it.
- **More player tables** (missed by the first draft): the slot limit by weapon (`$9B08`, every
  `Weapon` value), the launch offsets by motion (`$9ECD`/`$9ED5`/`$9EED + motion * 2`,
  little-endian `[i16; 12]`, the y window 4 entries into the x one), the attributes by side
  (`$9DFF`), the split launch words (hi `$9E03` against lo `$9D48`/`$9D6D`/`$9D9E`/`$9E01`;
  `$9CD3/$9CD6`, `$9E6C/$9E6F`, `$9D0D/$9D1D + fan`), `$9E05/$9E07` as one `[i16; 2]` whose
  high bytes are the `target` reads, `$91A9 + frame >> 1` (`[u8; 12]`) and `$91D6 + x`
  (`[u8; 2]`).
- **Weapons**: the many split-word tables read through `word()` (`$8881-$888D` by direction,
  `$8726-$8762`, `$9F07-$9F43`, `$8CF1-$8D2D`, `$914A-$9186` where `$916E` is the lo table of
  two hi tables, `$9EEF-$9F01`, `$8C71-$8C89`, `$9192/$91A2` by angle), the orbiters' corner steps (`$9E00`-`$9E0C`, by `param`), the frame numbers
  (`$9DF0` by `part`), the homing angles (`$887C`, 3), the attributes by heading (`$8437`, 20
  headings), the splitter's part velocities (`$8A2F`-`$8A3F`, 4 parts), the bubble palette
  (`$9637`, 2). Each index's range per slice 2's four proofs.
- When every bank-4 read that stays is a metasprite or dispatch pointer, `Port::rb` goes
  and `Port::rw` stays.

## Tests

- As slice 2: a throwaway per-index equality check (not committed, nothing printed); silent
  range tests for ROM-sourced indexes (`anim`, the overlay's shot frames); the player's and
  weapons' oracle tests, the Classic suites, the chain, the full set; tapes unchanged.

## Plan

1. `rom_split_words`, `rom_words` and slice 2's sites on them; checks.
2. `weapons::Tables`, `prg.weapons`, `prg.digit_tiles`; checks.
3. `player::Tables` (`Animations`, `frames`, offsets, palettes, entry poses), `prg.player`;
   `Port::rb` removed if unused; checks; grep for leftover reads.
4. Full set, `reviewer`, release 1.1.10. Two agents (`player.rs`, `weapons.rs`) after step 1,
   with the read lists above; the orchestrator reviews and merges.
