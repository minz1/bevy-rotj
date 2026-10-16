# Music: the sound-driver port

Status: **the port matches the game's driver write for write.** Run side by
side with the real 6502 driver in `tools/rotj-oracle` from power-on, it emits the
same register writes on every frame of the whole TAS (40,894 frames, 408,723
driver writes), of `tools/emu/run1` (3,000 frames: title, pause sound, stage
start), and of each of the 48 song-table ids and a fade-then-song sequence
played over stage 1-1 (§7). The stage 1-1 capture acceptance test matches all
**3,468 of 3,468** writes (it was 2,364) and the 600-700 song window 769/769.
Everything below is either proven by a ROM read/disassembly/emulator-trace
evidence triple, or explicitly listed as unknown.

## 1. Where the driver and songs live

The game has one sound driver, in **PRG bank 10, CPU window `$8000`**
(`disasm/bank0A.s`). Its four API entry points are a jump table at `$8000`:

| address | target | purpose |
| --- | --- | --- |
| `$8000` | `$800C` | init: `$4015=$1F`, `$4010=0`, clear `$0600-$06FF`, then fall through into `$8020` |
| `$8003` | `$8020` | reset voices (`$4000/$4004/$400C=$10`, `$4008=0`, `$4015=$0F`, stop channels) |
| `$8006` | `$80B2` | command: a song id (`< $F8`) or a special command (`>= $F8`) |
| `$8009` | `$8217` | the per-frame update |

The NMI handler (bank 15, `$E27D`) maps bank 10 into `$8000`, calls `$8009`,
then restores the window — so the driver ticks once per frame from the NMI.

**Bank select, settled (a first draft of this document got it wrong).** The
FME-7 mapping is the hardware's: command `$08` selects the `$8000` window, `$09`
the `$A000`, `$0A` the `$C000`, `$0B` the `$6000` one (`disasm/MAP.md` §1, and
consistent with the runtime window map read from the NMI shadow RAM `$E8-$EB`).

Evidence, from the *fixed* bank where addresses cannot be ambiguous: the NMI does
`JSR a:$8009` (`disasm/bank0F.s:449`) — so the driver is executed at `$8000`,
which can only be bank 10 if the NMI selected the `$8000` window, i.e. with
command `$08`. It restores the previous window afterwards, which is why the
shadow (`$E8` = `$C0` = bank 0) shows the gameplay mapping rather than bank 10.

So the driver runs at `$8000` for the duration of the NMI call, and its loader
puts the **song bank** in the `$A000` window (command `$09`) so it can read the
song data through `$A000`. Both are the hardware mapping; nothing is reversed.

**Song data** is in PRG banks **6 and 7**. A per-bank pointer table sits at
bank offset `$0000` (bank 6: `34 A1 54 A3 DD A5 C6 A8 ...` = `$A134 $A354 ...`),
and the driver indexes it through `$8B91`/`$8B92` (below).

### The song index table — `$8B91` / `$8B92` (bank 10)

Two interleaved byte arrays the loader reads with `X = song_id * 2`
(ROM bank-10 offsets `$0B91`/`$0B92`, proven by the code at `$80E0`):

- `$8B92[X]` = the song's PRG bank byte. All entries here are `$16`/`$17`
  (banks 6/7 once masked to the cart's 128 KiB).
- `$8B91[X]` = the index of the song's header in that bank's pointer table at
  `$A000`.

Song 4 → index 3, bank 6 → header at `$A8C6`. (The ROM also keeps a 48-entry
table of bank-10 pointers at `$8BD1`, the form the NSF rip uses; the driver does
**not** read it.)

### The song header — at `[bank: index*2]`, CPU `$A000` window

A header is a list of channel records terminated by a byte `>= $80` (proven by
`$810C`):

```
[chan < 8] [voice 0..3] [stream_lo] [stream_hi]     ; a music channel
[chan >= 8] [stream_lo] [stream_hi]                 ; an SFX/DMC channel (chan-8)
```

Stage 1-1's header (`$A8C6`):

```
00 00 DA A8   ch0 voice0 -> $A8DA
01 01 0C AB   ch1 voice1 -> $AB0C
02 02 1F AD   ch2 voice2 -> $AD1F   (triangle)
03 03 FF AD   ch3 voice3 -> $ADFF   (noise)
08 15 AF      SFX ch0    -> $AF15   (the drum/DMC channel)
FF            end
```

## 2. The driver's data model

Per-channel arrays live in RAM from `$0605`, eight bytes each (X = 7..0;
channels 0-3 are the four music voices, 4-7 are unused by this game). The port
mirrors them 1:1:

| base | field | base | field |
| --- | --- | --- | --- |
| `$0605` | flags / active | `$0665` | transpose (`$F9`) |
| `$060D` | stream pointer lo | `$066D` | period low |
| `$0615` | stream pointer hi | `$0675` | period high |
| `$061D` | hardware voice 0..3 | `$067D` | tuning offset (`$F8`) |
| `$0625` | note duration | `$0685` | instrument envelope lo |
| `$062D` | duration reload (`$F6`) | `$068D` | instrument envelope hi |
| `$0635` | loop pointer lo (`$FB`) | `$0695` | envelope step |
| `$063D` | loop pointer hi | `$069D` | envelope output (the register volume) |
| `$0645` | counter (`$FE`) | `$06A5` | envelope decay period |
| `$064D` | counter (`$FD`) | `$06AD` | envelope decay counter |
| `$0655` | volume (`$EE`) | `$06B5` | vibrato macro lo (`$E7`/`$E8`) |
| `$065D` | volume counter (`$F3`) | `$06BD` | vibrato macro hi |
| `$06C5` | vibrato macro offset | `$06CD` | vibrato depth (`$E0`) |
| `$06D5` | vibrato delay | `$06DD` | tremolo flag | |

Globals: `$0600` = current song bank, `$0602`/`$0603`/`$0604` = fade-out
active/step/timer (the port's `Fade`; the volume drops one step per fade step), `$004F-$0052` = per-voice "written this frame" flags (cleared
each frame by `$81EC`, which silences any voice it did not write).

### Stream opcodes (music channels)

The dispatcher at `$827D` reads a byte:

- `$00-$7F` — a note. `$82DD` adds the transpose, looks the period up in
  `$885C` (lo) / `$87FB` (hi), and starts the note.
- `$80-$DF` — a note with a per-instrument record: `Y = (op-$C0)*4`,
  `$8B33+Y..` (triangle) or `$8B4B+Y..`/`$8B4D+Y..` (pulse/noise). The `$C0-$C5`
  records are `[vibrato][env lo][env hi][note]` and `[env lo][period lo]…`.
- `$E0-$FF` — a command, via the tables `$845B`(lo)/`$843B`(hi) indexed by the
  full opcode (so `$E0`'s handler lives at `$853B`). The port matches on the
  opcode: `$E0` vibrato depth, `$E3-$E6` flag clears, `$E7`/`$E8` vibrato
  macro + depth, `$EA` instrument envelope pointer, `$EB`/`$EC` envelope
  cancel, `$ED` envelope decay, `$EE`/`$EF` volume, `$F1` "rest" note,
  `$F3` volume counter, `$F6`/`$F7` duration reload, `$F8` tuning, `$F9`
  transpose, `$FA`/`$FB`/`$FC` jumps and loops, `$FD`/`$FE` counted loops.
- A second table (`$864B`/`$8640`, also indexed by the full opcode) serves
  `$F5-$FF` when one is reached *mid-note* (`$82C4`), so loops and volume
  changes take effect without waiting for the note to end.

The DMC ("sampled bass") is driven by the **SFX engine** (`$88BD`): for each of
two channels it walks `$06E7/$06E9`, and a note makes `$893E` play the four
bytes at `pointer + note*4` into `$4010-$4013`; `$F3` sets that pointer. This is
exactly what `rotj dmc` extracts (`docs/RE-NOTES.md`).

## 3. Extraction and the port

`rotj music` writes two images (the port reads assets, never the ROM):

- `assets/audio/music/driver.bin` — PRG bank 10, CPU `$8000-$9FFF` (the tables).
- `assets/audio/music/song-banks.bin` — PRG banks 6+7, CPU `$A000-$BFFF`.

`crates/rotj-music` reimplements the code (not a CPU emulator): `Music::tick`
runs `$8217` — drain the request queue, advance the tempo, tick channels X=7..0
(as `$824D`), silence untouched voices (`$81EC`), run the SFX engine (`$88BD`) —
and returns the `$4000-$4017` writes in order. It is safe Rust.

## 4. Acceptance test

`cargo test -p rotj-music --test acceptance -- --nocapture` replays
`assets/audio/reference/apu-writes-600-1200.csv`, the game's own driver writes
over stage 1-1 (fceux, `INPUT=physics`; the NMI's `$4014`/`$4016` are filtered
out). It drives the port with `driver-calls-600-1200.csv`: every call that run
made into the driver from power-on, i.e. the `$8000` init at frame 4 and each
`$8009` tick with the `$0040-$0043` bytes it drained. Result:

```
matched 3468/3468 writes (emitted 3468)
song window 600..=700: matched 769/769
```

`rotj-oracle`'s `physics_run_is_the_acceptance_capture` re-derives both files
from the ROM: the oracle's physics run reproduces the capture byte for byte and
makes exactly the recorded calls. So the test needs no ROM and cannot drift from
the game.

### What used to limit the match, and why it is gone

- **Driver RAM left by earlier songs.** The old test faked the pre-capture
  state (play song 1 for a frame, reset) and so could not know `$067A` (song
  `$24`'s noise period high byte, which the header parse never clears). The
  recorded calls replay the real history instead: the title song at 288, the
  start effect `$3B` at 302, the resets at 7, 301, 476 and 590. The ticks also
  have gaps (frames 8-14 and 591-607 run no driver), which the replay keeps.
  With that history the frame-802 bytes `400F=0A`, `4007`, `4006` come out right.
- **The second jump effect.** The game requests `$24` again at 906 (`$0042`,
  bank 4 `$89AC`). Injecting it used to lower the match only because of the
  driver bugs below; with them fixed it matches.
- **The ±1 pulse period bytes** were the `$82DD` carry bug below.

### Port bugs found against `disasm/bank0A.s` (all fixed)

1. **`$E3`/`$E4`/`$E5`/`$E6`/`$E7`/`$E8`/`$E9`/`$EB` had no effect.** The
   handlers change the working flag copy `$44` (`$8564`: `LDA $44 / AND #$FD /
   STA $44`; `$856D` `ORA #$02`; `$8576` `AND #$DF`; `$857F`, `$85F1` `AND
   #$F7`; `$859E` `ORA #$08`; `$85B9` `AND #$F7`; `$85D4` `ORA #$60`), and `$84D5`
   (`LDA $44 / AND #$BF / STA $0605,x`) stores that copy back at the end of the
   channel's frame. The port changed `$0605` directly, so the store-back undid
   it. First visible on the title song at frame 289 (`$E4`: `$4002` written
   without `$4003`).
2. **The SFX fade check read the wrong flags.** `$891B`: `LDA $0602 / BEQ /
   LDA $44 / AND #$01 / BEQ` with `$44` the SFX channel's `$06E5` (copied at
   `$88C8`), which `$FC` sets (`$81C0`: `ORA #$01`) and a header parse clears
   (`$8188`: `$06E5=$80`). The port muted the DMC whenever a fade was past step
   2, so a song started during a fade lost its DMC. Covered by the oracle's
   fade-then-song case.
3. **SFX `$F0` did not stop the channel.** The `$8888`/`$8898` table sends
   `$F0` to `$8998`: `LDA #$00 / STA $06E5,x / JMP $88C3`. The port just ended
   the frame, so the channel stayed active, kept claiming the DMC and re-read
   the `$F0` every 256 frames.
4. **`$82DD` added a stray carry.** After `ADC $0665,x` (transpose) the code
   runs `CPY #$03`, which sets the carry for the `ADC $067D,x` that follows; for
   voices 0-2 it is always clear. The port added the transpose overflow instead,
   so negative transposes played one period step high (the ±1 bytes).
5. **Mid-note `$FD`/`$FE` went to the wrong place.** The mid-note handlers
   (`$87B3`/`$87D7`) jump with `JMP $8357` (keep sounding) and, once the count
   runs out, step past with `JMP $834B`. The port read a new event instead, and
   on exhaustion skipped the stream advance. This is why song `$06`'s pulse
   line measured 134 frames instead of its real 176 (`docs/CUTCONTENT.md`).
6. **The envelope's last frame skipped a decay step.** At the macro end
   (`$8438`) the code falls through into `$8445` (`DEC $06AD`), so the first
   decay count happens on that same frame.
7. **The SFX reader stopped where the ROM keeps reading.** `$F8`-`$FA` with no
   return address (`$89E7`: `BEQ $8A21` -> `JMP $88DE`) and an exhausted
   `$FD`/`$FE` (`$8A24`/`$8A4F`: `INY×3 / JMP $88DE`) continue with the next
   event; `$FC` (`$8A0F`) is a plain jump the port lacked; `$FF` (`$8A7A`) is an
   `RTS` that ends the whole SFX frame; `$F2` (`$89A7`) only changes `$44`,
   which the SFX engine never stores back.
8. **`$800C` init falls through into `$8020`**, so it also writes the voice
   reset. `Music::init` and `Music::reset_voices` now return their writes
   instead of leaving them for the next `tick` to drop.

### What remains unknown

- The FME-7 command/window mapping is still confusing in the prose above: the
  oracle maps CPU `$8000` through command 9 and runs the driver there, and the
  port's offset-based single-bank model matches it; nothing depends on the
  naming.
- The `$80`-`$EF` SFX bytes index no real handler; the port stops the SFX frame
  on them. No song uses them.

### Rejected hypotheses (with evidence)

- *"The frame-802 data lives in a PRG bank the extraction omits."* **Rejected.**
  Song `$24`'s header and streams are in bank 10 offsets `$0C00-$0FFF`, which
  `driver.bin` (bank 10) already contains.
- *"The `3,1,2,0` order at frame 802 is a capture artifact."* **Rejected.** The
  channel loop is strictly descending `X=7..0` (`$824D`/`DEX`) and song `$24`
  maps channels 4,5 to voices 1,3 while channels 0 and 2 keep voices 0,2, so the
  register order is `3,1,2,0`.
- *"The 906 reload needs more state than a plain re-request."* **Rejected.** It
  is a plain `$0042=$24`; the port follows it once the driver bugs are fixed.

## 5. Reproduce

```
rotj music --rom "raw/Batman - Return of the Joker (USA).nes" --out assets/audio/music
cargo test -p rotj-music --test acceptance -- --nocapture
```

## 6. Which event requests which song (decoded 2026-10-14)

The game asks for sound by writing the four zero-page bytes `$0040-$0043`; the
driver drains them once per frame at `$8217` (`LDA $40 / BEQ ... / JSR $80B2`
four times, then clears all four). **The driver itself makes no music/effect
distinction** — every slot runs the same `$80B2` command path (bank 10 CPU
`$8217-$823D`). The split below is the *game's* convention, read from its
writers and confirmed by the traces.

| slot | what the game puts there | evidence |
| --- | --- | --- |
| `$0040` | control: `$FF` (reset/silence) or `$FC` (restart with tempo scaling) | all traced `$40` writes are `$FF`; bank 2 `$98C3`/`$92DA`, bank 13 `$BD4D`, bank 2 `$9433`/`$94F7` (`$FC`) |
| `$0041` | **music** — a song id `0..$1F` | bank 14 `$D973` (stage song); bank 2 `$90BA`=`$08`, `$9465`=`$0E`, `$9A0C`=`$02`; bank 14 `$D159`=`$03`; title routine (below) |
| `$0042` | **sound effect** — an id `>= $20` | damage `$25` (bank 4 `$82C6`), jump `$24` (bank 4 `$04E8`/`$09A8`), many bank 2/4/5/8 writes |
| `$0043` | **sound effect** — an id `>= $20` | bank 4 `$90DC`=`$33`, `$90F4`=`$40`; bank 5 `$8A8D`=`$30`, `$9301`=`$2A` |

The id range is real, not a guess: ids `0-31` use the indexed half of the song
table (`$8B91`/`$8B92[X]`, bank 6/7 headers); ids `32+` mark their entry with
bit 7 and take the header directly from the driver bank (`$8105` path, above).
Every `$41` value seen is `< $20`; every `$42`/`$43` value seen is `>= $20`.

### The stage-start request (proved)

Bank 14 CPU `$D973` (ROM bank 14 offset `$1973`) is the music request:

```
$D973  AD 00 04   LDA a:$0400     ; area index * 2
$D976  4A         LSR
$D977  AA         TAX
$D978  BD 80 D9   LDA a:$D980, x  ; area -> song id
$D97B  30 02      BMI $D97F       ; bit 7 set: no music
$D97D  85 41      STA $41         ; request slot 1 (music)
$D97F  60         RTS
```

The table (bank 14 offset `$1980`, 18 bytes):
`04 04 FF 0B 07 09 09 FF 05 05 FF 0B 07 04 04 FF 0C FF`.

Evidence triple: (a) the ROM read above; (b) trace
`tools/emu/music-frame802/ram-writes.csv` row `609,0041,04,D97F`; (c) the same
trace/dump has `$0400=$00` at frame 609 (`ram-609.bin`), so area 0 selects entry
0 = `$04`. The port's `rotj_music::AREA_SONGS`/`song_for_area` are this table;
`STAGE_1_1_SONG` is entry 0.

The caller is the level load: bank 13 `$BD4D` does `LDA #$FF / STA $40` (stop
sound), `$0402=0`, then `JSR $89FE` (the level loader); the loader reaches
`$D973` later. The port's `audio.rs` now requests `song_for_area(0)`.

### The four request bytes in a traced run

`tools/emu/music-frame802` (`physics` input) and `tools/emu/soundsel-*`
(recorded for this pass) give, with the PC that made each write:

| frame | slot | value | PC | writer | event |
| --- | --- | --- | --- | --- | --- |
| 6 | `$40` | `FF` | `$98C7` | bank 2 `$98C3` | boot/logo scene transition (stop) |
| 287 | `$41` | `01` | `$61B0` | WRAM `$61AE` (see note) | title screen music |
| 300 | `$40` | `FF` | `$6237` | WRAM `$6235` | Start pressed: stop title music |
| 301 | `$42` | `3B` | `$623E` | WRAM `$623C` | Start pressed: transition effect |
| 475 | `$40` | `FF` | `$92DE` | bank 2 `$92DA` | scene blank/transition (stop) |
| 589 | `$40` | `FF` | `$BD51` | bank 13 `$BD4D` | stage load begins (stop) |
| 609 | `$41` | `04` | `$D97F` | bank 14 `$D973` | **stage 1-1 music starts** |
| 751 | `$42` | `24` | `$8508` | bank 4 `$04E8` | **jump** (A at 750) |
| 905 | `$42` | `24` | `$89AC` | bank 4 `$09A8` | **jump** (A at 905) |

Note: the title-screen writer bytes (`$61A4`-`$6240`) are **not in any PRG
bank**; the frame-287/609 WRAM dumps hold them at `$61AE`/`$6235`, so the title
runs a WRAM-resident routine. It is the boot payload, CHR banks `$F0-$F7` copied to
WRAM by bank 2 `$98C3` (`docs/ACTORS.md` §9s).

### `$24` is the jump sound, not a song section

`$24` (id 36) is in the second table half, so it plays through the **music**
channel engine on channels 4 and 5 — but it is a *finite effect*:

- Driven by `rotj-music` it lasts 19 frames: voice 3 (noise) walks
  `$400E` `0E,0D,0C,0B,0A`, `$400C` `16,17,18,19,1B`, `$400F=$08`; voice 1
  (pulse 2) is set but silent (`$4004=$10`, volume 0). Then silence.
- Trigger: a controlled capture. `INPUT=jump` (A at 750/850/950/1050) requests
  `$42=$24` at 751/851/951/1051; `INPUT=walkonly` (walks the whole stretch, no
  jumps) requests nothing; `INPUT=prog`/`physics` (walk + periodic jumps)
  request `$24` at the jump frames. See `tools/emu/soundsel-jump` /
  `soundsel-walkonly` `ram-writes.csv`.

So `$24` reads as the jump (take-off) effect. (The first standstill jump, A at
650, produced no request — the character may still be in the level-entry
descent; every later jump did.)

### The title's song (`$41 = $01`)

The title screen runs a WRAM-resident routine (not in any PRG bank): the trace
`tools/emu/soundsel-attract/ram-writes.csv` has the row `287,0041,01,61B0` — at
frame 287 the title requests song `$01` (the `soundsel-jump`/`-hurt`/`-walkonly`
runs share the same frame/write). The port runs the boot from power-on
(`actor/boot.rs`), so the title's `$01`, the attract mode's `$02` and the stage
start's `$D980[area]` (`$D973`, above) reach the driver as the game's own requests
(`play::tests::start_on_the_title_reaches_gameplay`).

### Firing the default weapon (`$42 = $2D`)

Weapon 0 (`$0476 = 0`) fires through the handler `$9D2D` (the aim-ahead table
`$9EC1[$0476]`), which is `LDA #$2D / STA $42` (`disasm/bank04.s`, `L9D2D`).
The port's `Play::step` returns `$42 = $2D` on the firing frame (app test
`firing_requests_the_weapon_effect`). Other handlers use `$2F`/`$29`/`$43`
(`player.rs::spawn_shot`).

### Damage and death (not traced)

The damage handler (bank 4 `$82C6`) does `$0478 -= $A7` (energy minus the hit
amount), then `LDA #$25 / STA $42` — so **`$25` is the damage effect**. It is
located by ROM read only: no capture reached a collision. Stage 1-1's opening
has no enemy drawn (see `docs/STATUS.md`), and the `walkonly`/`hurt`/`prog`
runs walked 2400+ frames without energy loss. Death/restart was likewise not
reached. Both remain code-located, not traced — but the port's own app test
(`taking_damage_requests_the_damage_effect`) drives a damage-class tile under
Batman and sees `Play::step` return `$42 = $25`.

## Appendix: the `>= $F8` command dispatch (decoded 2026-10-13)

The stream's special commands are dispatched at bank 10 `$80C9`:

    CMP #$F8 / BCC <normal> / SEC / SBC #$F8 / TAX
    high table $80C7: 80 80 80 80 81 80 80 80
    low  table $80CF: 20 20 20 20 A2 4E 80 20
    JMP ($0045)

| command | handler | behaviour (read from `driver.bin`) |
| --- | --- | --- |
| `$F8 $F9 $FA $FB $FF` | `$8020` | song reset: `$4000/$4004/$400C = $10`, `$4008 = 0`, `$0602 = 0`, `$4015 = $0F`, clear the 8 channel bytes `$0605,X`, the 8 voice bytes `$06DD,X` and `$06E5,X` |
| `$FC` | `$81A2` | `$0602 = 1`, `$0604 = 1`, `$0603 = 0`, `ORA #$80` on each `$06DD,X`, `ORA #$01` on each `$06E5,X` - a global voice re-trigger and the only thing that turns the `$84DF` scaling on |
| `$FD` | `$804E` | `$4F..$52 = $40`; for each `$0605,X` with bit 2 set, set bit 7 |
| `$FE` | `$8080` | `$4F..$52 = $40`; for each `$0605,X` with bit 7 set, set bit 2 and clear bit 7 |

Just after the dispatch (`$80D7`) the song table `$8B92` supplies a **PRG bank**
that the driver writes to the mapper with command `$0A` (its shadow `$0600`);
songs 0-31 keep their header table in that bank's `$A000` window. Ids 32 and up
(headers from `$8CF9`) are stored differently — see "What remains unknown / wrong" above.
`$84DF` is the tempo volume scaling, and `$FC` is what makes `$0602` non-zero, so
that scaling is live from a section restart onward.

### Implementation status

All four handlers and the `$84DF` scaling are transcribed in
`crates/rotj-music/src/driver.rs` (`handle_request`, `start_fade`/
`resume_parked`/`park_active`, `Fade::scale`, gated as `$846E` gates it: only
when `$0602 != 0` and `$06DD` bit 7 is set). The run1 capture exercises
`$FE`/`$FD` (the pause, with effect `$3A`) and `$FF`; the oracle's
fade-then-song case exercises `$FC`.

## 7. The oracle comparison

`tools/rotj-oracle/tests/music.rs` runs the game in the cycle-exact oracle and
peeks every driver entry in bank 10 (`$8000`, `$8003`, `$8006`, `$8009`). Each
call is mirrored into `rotj_music::Music` (the `$8009` tick gets the four
request bytes as they were on entry), and each frame's driver writes (PC in
`$8000-$9FFF`) must equal the port's:

| test | what it plays | result |
| --- | --- | --- |
| `physics_run_is_the_acceptance_capture` | frames 1-1005, `INPUT=physics` | equal; also equal to the capture and the recorded calls |
| `port_matches_the_driver_through_run1` | 3,000 frames, `INPUT=plan` | 14,779 writes, equal |
| `port_matches_the_driver_for_every_song_and_effect` | ids `$00-$2F` and `$FC` then `$04`, 480 frames each over stage 1-1 | equal |
| `port_matches_the_driver_through_the_tas` (`--ignored`) | the whole TAS, 40,894 frames | 408,723 writes, equal |

The game never calls `$8006` directly in any of these runs; every request goes
through `$0040-$0043`.
