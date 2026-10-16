# Reverse-engineering notes — Batman: Return of the Joker (NES)

Facts verified during research (2026-10-13). Keep sources with every claim so
later sessions don't have to re-derive.

## Cart / ROM

- ROM: `raw/Batman - Return of the Joker (USA).nes`, 393,232 bytes (= 16-byte
  iNES header + 128 KiB PRG + 256 KiB CHR). iNES sha1
  `668a395cf7215f677b6df6c4c8c93e1bb439dc25`, game CRC32 `03EC46AF`, cart ID
  `NES-48-USA`, PCB SAP-E301 (Sunsoft-5A).
- Mapper **69**, FME-7. 8 KiB CPU bank windows: `$6000-$7FFF` switchable
  PRG-RAM/ROM (8 KiB WRAM on cart, battery), `$8000-$9FFF`, `$A000-$BFFF`,
  `$C000-$DFFF` switchable, `$E000-$FFFF` fixed to the last 8 KiB PRG bank.
  CHR windows: `$0000-$07FF` (2 KiB) + `$0800/$1000/$1800` (1 KiB each),
  independent. Writes: command #$00-#$0F to `$8000-$9FFF`, then parameter to
  `$A000-$BFFF`.
  Source: https://www.nesdev.org/wiki/Sunsoft_FME-7
- **Audio: 2A03 only.** The FME-7 board has no expansion audio (unlike the
  FME-7/5B in e.g. Gimmick!). ROTJ's famous deep bass is **DMC sampled bass
  lines**. (Nathan Altice, *I Am Error*.) So `rotj-apu` targets 2A03
  pulse×2/triangle/noise/DMC; there are no Sunsoft-5B channels to synthesize
  for this cart. (The 5B register/audio spec, if ever needed for a JP board
  variant, is at https://www.nesdev.org/wiki/Sunsoft_5B_audio.)
- Graphics are in **uncompressed CHR-ROM** (no documented decompression routine
  this game uses); the 1 KiB CHR banking is used for parallax (mid-frame bank
  swaps for the multi-layer sky). Source: TCRF page + *I Am Error*.

## What exists publicly (and what does not)

- **No public disassembly, no RAM-map document, no level-format notes, and no
  decompilation** exist for this game (checked romhacking.net, Data Crystal,
  TCRF, nesdev forums, GitHub). The `disasm/` tree in this repo is original
  work.
- Partial RAM map from GameHacking.org cheat codes (roles are submitter labels,
  treat as leads to verify): `$00AA` boss HP, `$00F1` boss HP (2), `$009E`
  jump velocity floor, `$00C3` scrolling modifier, `$0466` Batman animation,
  `$046F` walk-behind-scenery flag, `$0477` invincibility ("Super Batman"),
  `$047D` jump-related, `$0482` lives, `$04F2/$04F3` suit palette, `$06D3-$06D8`
  score, `$AB23` sound-engine hook (nensondubois), `$7DD0-$7DD2` item persist.
  Source: https://gamehacking.org/?game=29048
- Reference material for validation:
  - VGMaps full stage maps (RT-55J): https://www.vgmaps.com/Atlas/NES/#BatmanReturnOfTheJoker
  - Spriters Resource rips: https://www.spriters-resource.com/nes/batmanrotj/
  - NSF rip (56 songs, MD5 23c929db5c2f8ce81de53907694864b7): https://ocremix.org/chip/1606
    — useful as a music-data oracle (TCRF: NSF track 6 unused, loops like a stage track).
  - Manual (stage list, weapons, checkpoint rules):
    http://world-of-nintendo.com/manuals/nes/batman_return_the_joker.shtml
- Mapper reference implementation to check semantics against: Mesen
  `SunsoftFme7.h` and TetaNES `tetanes-core` (Rust, MIT, already supports this
  game per its compatibility list).

## Gameplay facts (from the manual)

- 7 stages, sublevels; stage 1 = Gotham City Cathedral, 1-1 Cathedral →
  1-2 City Rooftops, boss "Ace Ranger". Bosses at ends of stages 1,3,4,6,7
  (not 2 or 5).
- A jump, Down+A slide, Up+B aim up; B = wrist projector (hold-to-charge).
  Weapons: Cross Bow, Batarang (homing), Sonic Neutralizer, Shield Star
  (3-way). 8 Backpack Energy Capsules → gold invincible "Super Batman" with
  screen-blast charge.
- Death/continue restarts at the beginning of the sublevel; cannot resume at a
  boss level.
- Physics constants are **not documented anywhere** — derive them from the
  disassembly / Mesen watchpoint study (e.g. RAM `$009E`).

## DMC samples (verified in ROM bytes, 2026-10-13)

- **DMC records come from song streams, not a static copy loop.** Bank 10
  handler `$89C7` (window `$8000`) runs on stream opcode `$F3`: it stores a
  16-bit pointer from the stream into RAM `$06F7/$06F9` (X = voice). The note
  handler at `$893E` computes `Y = note*4` from the note value in `$47`,
  reloads the per-voice pointer and plays the 4-byte record at
  `pointer + note*4`, byte-for-byte into the registers:
  - byte 0 -> `$4010`: bit 7 IRQ enable, bit 6 loop, bits 3..0 rate entry
  - byte 1 -> `$4011`: direct load, 7-bit DAC level
  - byte 2 -> `$4012`: sample address = `$C000 + value * 64`
  - byte 3 -> `$4013`: sample length = `value * 16 + 1` bytes
- **The record tables live per song bank**, addressed through the `$A000`
  window: the song loader `$80DE` selects a bank (cmd `$0A`) and dereferences
  `$A000,x` for the song pointer table; the observed `$F3` pointers are
  `$A06C` (bank 06) and `$A06E` (bank 07), so the table sits at
  `pointer - $A000` inside the command's own bank. Found: **bank 06 file offset
  `$006C`** and **bank 07 file offset `$006E`**, byte-identical 50 x 4-byte
  tables (200 bytes). Bank 10 keeps a **dead, byte-identical copy** at file
  offset `$0C31` (no `$F3` in bank 10 references it). (The cmd-`$0A` window
  point suggests `disasm/MAP.md` §1's `cmd $0A -> $C000` row needs revisiting.)
- All 50 records address the **fixed `$E000-$FFFF` PRG bank (bank 15)**. Nine
  distinct (address,length) instruments, bank-15 file offsets:
  `$E500`/1009, `$E900`/1009, `$ED00`/1009, `$F100`/993, `$F500`/993,
  `$F900`/497 and /1265, `$FB00`/497 and /113 (file offsets `$0500`, `$0900`,
  `$0D00`, `$1100`, `$1500`, `$1900`, `$1B00`).
- Record fields: rate entries run `4..15`; records 0-27 and 48-49 use direct
  load `$7F`, records 28-47 use `$00`; no record sets IRQ or loop.
- Extracted with `rotj dmc` into `assets/audio/dmc/` (`manifest.txt`,
  `sample-bank.bin`, one `.bin` per instrument). Byte statistics look like
  waveforms (e.g. `$E500`: mean `$7E`, 965 transitions over 1009 bytes,
  `$FF`/`$55`/`$AA` dominant), not code.

## Extraction status

- All 32 CHR banks rendered to `assets/graphics/chr/bank00..31.png` by
  `rotj chr` (grayscale preview palette; real per-scene palettes pending
  palette-table extraction from the disassembly).
- Level nametables, sprite/object tables, palette tables, music data: pending
  the disassembly milestones recorded in `docs/PLAN.md`.
