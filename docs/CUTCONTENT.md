# Cut-content audit (TASK=cutaudit)

Two claims from an AI-generated survey of the game's cut content (the owner's, made with
Gemini; kept locally, not in the repository) checked against the
ROM, `disasm/`, the ported music driver (`crates/rotj-music`) and emulator
captures. Each finding gives its evidence; anything not proven is marked a lead.

| claim | verdict |
| --- | --- |
| An unused, looping song ("NSF track 6") exists | **confirmed** — song id `$06`; no game code requests it |
| `GEKUVOGK` / `LAXVTNNY` enable Batman debug modes | **not supported** — both codes come from other games in the cited cheat database |

Captures used: `tools/emu/cutaudit-songs/` (`WATCH=40,41,42,43`,
`INPUT=prog`, 4000 frames) and `tools/emu/cutaudit-exec/`
(`EXEC=80DE`). The NSF rip is the Zophar file
`Batman - Return of The Joker (EMU).zophar.zip` (fetched to `/tmp`, not kept).

---

## 1. The unused song is song id `$06` (NSF "track 6")

### 1a. Where songs live in the driver

The driver is PRG bank 10 (`disasm/bank0A.s`), CPU window `$8000` while it
ticks. Song requests are the four bytes `$0040-$0043`: the per-frame update
`$8217` reads each, calls `$80B2` if non-zero, then clears them
(`disasm/bank0A.s:282-303`). `$80B2` treats a value `< $F8` as a song id and
`>= $F8` as a command.

A song id `id` is looked up with `X = id * 2` in two adjacent byte arrays
(`$80DE-$8101`, `disasm/bank0A.s:127-148`):

* `$8B92[X]` = bank byte. If bit 7 is clear it is the PRG bank of the song;
  the loader maps it into the `$A000` window (FME-7 command `$0A`).
* `$8B91[X]` = index into that bank's `$A000` header-pointer table.

Reading the table (bank 10 offset `$0B91`) gives:

| id | `$8B91`/`$8B92` | song header | kind |
| --- | --- | --- | --- |
| `$00` | `00 16` | bank 6 idx 0, `$A134` | duplicate of `$01` |
| `$01`..`$07` | `00..06`, `16` | bank 6 idx 0..6 ($A134, $A354, $A5DD, $A8C6, $B07E, $B5D1, $B8E5) | **music** (channels 0-3 + one SFX channel) |
| `$08`..`$0F` | `00..07`, `17` | bank 7 idx 0..7 ($A136, $A225, $A56E, $A638, $AA15, $AFFB, $B543, $B609) | **music** |
| `$10`..`$1F` | `00 17` | bank 7 idx 0, `$A136` | duplicate of `$08` |
| `$20`..`$4F` | lo/hi with hi `>= $80` | direct bank-10 addresses `$8CF9`..`$9608` | **sound effects** (channels 4-7) |

So the driver's *music* songs are ids **`$01`..`$0F`** (15 songs, `$00` is id
`$01` again), and ids `$20`..`$48` are sound effects, not songs. (Headers were
parsed from `song-banks.bin`/`driver.bin`; every music header is
`ch0..3 -> voice 0..3` plus one SFX channel, every `$20+` header is
channels 4-7.)

### 1b. The NSF track-number mapping (the part the survey could not pin down)

"NSF track 6" is a *track number*, not a song id. The rip resolves it with its
own stub: the NSF's init address is `$9700` in its patched bank-10 image, and
the player calls it with `A = track_number - 1`. The stub (bytes read from the
NSF; CPU `$8000` window):

```
$9700: AA        TAX                 ; A = track-1
$9701: E0 07     CPX #$07
$9703: 90 0D     BCC $9712           ; low tracks and high tracks use different NSF bankswitches
$9705: A9 0A     LDA #$0A            ; NSF bankswitch register values (rip-specific)
...
$9712: A9 08     LDA #$08
...
$971C: BD 30 97  LDA $9730,X         ; track -> song id
$971F: 4C 06 80  JMP $8006           ; driver's command entry = request the id
```

The track table `$9730` is `01 02 03 04 05 06 07 08 09 0A 0B 0C 0D 0E 0F 20 21
22 ... 48` (56 entries). So **NSF track 6 -> song id `$06`**, and the rip's
`.m3u` names that track `Unused [Track 6]`. (The stub and table are the rip's,
not retail ROM bytes; the identification of *track 6* rests on them agreeing
with the id-space analysis below.)

### 1c. Which song ids the game code requests

Only two tables of music ids exist (found by scanning the bank-window-space for
`LDA abs,x; STA $40-$43`):

* **Stage music** — bank 14 (CPU `$C000` window) `$D980`, indexed by
  `$0400 >> 1` (the area index). `$D973`:

  ```
  $D973  lda $0400
  $D976  lsr
  $D977  tax
  $D978  lda $D980,x
  $D97B  bmi $D97F     ; bit 7 set = no request
  $D97D  sta $41
  $D97F  rts
  ```

  Table bytes (bank 14 offset `$1980`):
  `04 04 FF 0B 07 09 09 FF 05 05 FF 0B 07 04 04 FF 0C FF` ->
  requests `{04,05,07,09,0B,0C}`.

* **Sound-test music list** — bank 12 (CPU `$8000` window) `$9C85`, used by
  the sound-test screen: entered from bank 14 `$D3D5` when `$F6 == $90`
  (`disasm/bank0E.s:4309-4316`, `JMP $9A36`; the code at `$9A36` is the menu
  setup/loop, `$9ABD`):

  ```
  $9ABD  lda $9C85,x
  $9AC0  sta $41
  ```

  Table bytes (bank 12 offset `$1C85`):
  `01 02 03 04 05 07 08 09 0A 0B 0C 0D 0F` -> the sound test offers
  `{01..0F}` minus `{06,0E}`.

Adding every inline `LDA #id; STA $40-$43` in the ROM gives the music-range ids
`{02,03,08,0A,0E}` (e.g. `$BB59 LDA #$0A; STA $41`; bank 1 end-of-stage
codes); `$01` is requested from the title screen (frame 288 in the capture
below) and is in the sound-test list. **Union of all three sources =**
`{01,02,03,04,05,07,08,09,0A,0B,0C,0D,0E,0F}`. The only music id in
`$01..$0F` never requested is **`$06`** (`$00` is a duplicate of `$01`; a
full-ROM scan finds no `A9 06 85 4x` and no music-id table containing `$06`).

Emulator check (second source): `tools/emu/cutaudit-songs/ram-writes.csv` logged
every write to `$40-$43` over 4000 frames of attract + stage 1-1; the non-zero
values were `$FF` (reset), `$01`, `$04`, `$24`, `$3B` — never `$06` and no
other music id. `tools/emu/cutaudit-exec/ram-exec.csv` (`EXEC $80DE`) shows the
song loads `A=$01` (frame 288, title), `$3B` (302), `$04` (610, stage 1-1).
Caveat: `EXEC $80DE` alone over-counts, because `$80DE` is also the target of
the driver's alternate entry `$80D7` (`CMP #$FE; BNE $80DE`) — the `$40-$43`
WATCH is the authoritative list.

### 1d. The port plays id `$06` — it is a real, looping song

`crates/rotj-music/tests/unused_song.rs` loads the extracted assets, requests id
`$06`, and ticks the driver. Result (`cargo test -p rotj-music --test
unused_song -- --nocapture`):

```
song id $06 over 600 frames:
    pulse1: 1812 writes
    pulse2: 1306 writes
  triangle: 1242 writes
     noise: 1294 writes
       dmc: 24 writes
    status: 20 writes
pulse 1 loop: period 176 frames (2.93s), >=98% of the last 6000 frames repeat
```

The song data also loops explicitly. Song id `$06` is bank 6 header index 5,
header `$B5D1`:

```
00 00 E5 B5   ch0 voice0 -> $B5E5
01 01 9F B6   ch1 voice1 -> $B69F
02 02 5C B7   ch2 voice2 -> $B75C
03 03 8D B7   ch3 voice3 -> $B78D
08 CF B7      SFX ch0    -> $B7CF
FF            end
```

Walking stream `ch0` (CPU addresses in the `$A000` window, bank 6): it starts
`$B5E5 $F9` (transpose), then `$B5E7 $FB $87 $B6` — opcode `$FB` stores the loop
point `$B5EA` (the address after the instruction) and jumps to `$B687`; the
body runs to `$B695 $FA`, and `$FA` jumps back to `$B5EA`. (`$FA` =
"jump to the stored loop point" at handler `$86A2`; `$FB` = "set loop point,
then jump" at `$86BA`; `$FC` = plain jump at `$86CA`.) Channels 1-3 carry the
same `$FB/$FC` structure. The pulse-1 line is a 176-frame ostinato, matching the
test's measured period. (An earlier port measured 134 frames; it mishandled
`$FD`/`$FE` loops reached mid-note. `rotj-oracle`'s `music` test now plays `$06`
in the game's own driver and the port matches it write for write.)

### 1e. Proven vs lead

* **Proven**: `$06` is defined by the driver's table and is never requested —
  no inline write, not in the stage table, not in the sound-test table, and
  absent from a 4000-frame `$40-$43` capture. It is a real song, not a stub
  (the port emits thousands of writes; the stream loops).
* **Proven**: the NSF's own track table maps track 6 -> id `$06` and labels it
  "Unused [Track 6]".
* **Lead (unverified)**: whether some *computed* (not table/inline) request path
  could ever produce `$06`. No such path was found, but the scan is not a proof
  of absence. The `$80D7` alternate driver entry is noted above.

---

## 2. The two Game Genie codes do not belong to this ROM

Decoded with the standard NES Game Genie algorithm (alphabet
`APZLGITYEOXUKSVN`; validated against the reference example `yeuzugaa` ->
`$ACB3`/`$07`/`$00` from `qalle2/nes-util`, which follows
`nesdev.org/nesgg.txt`):

| code | CPU address | value | compare |
| --- | --- | --- | --- |
| `GEKUVOGK` | `$B9CE` | `$0C` | `$4C` |
| `LAXVTNNY` | `$E72E` | `$03` | `$FF` |

An 8-letter Game Genie code only applies where the byte on the bus equals its
**compare** value. Checking the Batman ROM at each address under every possible
FME-7 bank mapping:

* `$B9CE` is in the `$A000` window (bank 13 in the disassembly's naming
  convention): at offset `$19CE` the byte is `$C9` in bank 13, and **no** of the
  16 PRG banks has `$4C` there. For reference, bank 13 offset `$19CE` decodes as
  `CMP #$D0` (`disasm/bank0D.s`), so `GEKUVOGK` does not patch an instruction
  here.
* `$E72E` is the fixed bank `$E000-$FFFF` (bank 15), offset `$072E`. That byte
  is `$00`, not the compare `$FF`; the region decodes as data, not code.

The reason is the provenance: both codes come from **other games in the same
cited cheat list** (CaH4e3, "Mostly Unpublished NES Cheats",
`cheatsbase_b.php`):

* `GEKUVOGK` is under the section header **"Banana Prince, Bananan Ouji no
  Daibouken"** — "Re-enables ingame debug handler. Pause the game, press A to
  change the weapon, press B to max it power."
* `LAXVTNNY` is under **"Baseball Simulator 1.000, Choujin - Ultra Baseball"** —
  "Re-enables a complex debug mode cheat. Hold A + B + Select then press Reset,
  to enter it, press Select + Start to exit back to the game." (its Japanese
  twin is `LAXVANNY`).

The page's actual **"Batman - Return of the Joker, Dynamite Batman"** section
lists only `(GEN) Sound test: At the title screen, hold A + Start` — no Game
Genie codes. The survey (and its ref 18) attached the two other games' codes to
Batman.

### 2a. Proven vs lead

* **Proven**: the two codes decode to addresses whose compare bytes do not match
  this ROM under any banking, so they do not patch Batman; the cited source
  attributes both codes to other games; the Batman section of that source has
  no such codes.
* **Consequence**: the survey's "debug mode" behaviours cannot be confirmed for
  this ROM and should be treated as **retracted**. What the codes do in their
  real games was not checked (those ROMs are not here).

### 2b. Related, and checkable: the sound test is real

The same source's sound-test cheat ("hold A + Start at the title") matches the
ROM: bank 14 `$D3D5` jumps to the sound-test screen at bank 12 `$9A36` when the
input flag `$F6 == $90`, and that screen's music list is `$9C85` (§1c). So the
survey's *sound-test* claim is supported; only its attribution of track 6 to
"not present in the sound test" needs the id mapping above (`$06` is one of the
two ids absent from `$9C85`; the other, `$0E`, *is* requested elsewhere).

The port runs it (`actor/sound_test.rs`, `docs/ACTORS.md` §9s). "At the title" is
loose: `$D3C6` reads `$F6` once, after the title's choice has flashed and faded and
the screen is cleared (or a password is accepted), so A and Start must still be held
then, and nothing else. In lockstep with the oracle
(`boot::tests::the_sound_test_plays_and_resets_like_the_oracle`) it plays songs and
effects, and A on its last row resets the game.

---

## Reproduce

```
cd /home/minz1/projects/batmanROTJ-wt/cutaudit
cargo test -p rotj-music --test unused_song -- --nocapture
# captures:
DUMP_DIR="$PWD/tools/emu/cutaudit-songs" WATCH="40,41,42,43" INPUT=prog UPTIME=4000 \
  DUMPS=600 RAMSEQ=1,1 timeout -k 3 600 fceux --sound 0 \
  --loadlua "$PWD/tools/emu/dump.lua" "$PWD/raw/Batman - Return of the Joker (USA).nes"
```

---

# Cut-content audit 2 (TASK=cutaudit2, 2026-10-15)

The first audit (§1-§2) covered the songs and the Game Genie codes. This pass
sweeps the whole game now that it is ported and verified (`STATUS.md`): the actor
dispatch, the level tables, the sprite/animation tables, the ending payload, the
sound driver and CHR. Each finding gives its address and the method that produced
it; anything not proven is marked **lead**. Scratch scripts live in
`tools/emu/cutaudit2-scratch/` and (the scratch oracle program) `/tmp`.

## Summary

| # | area | finding | verdict |
| --- | --- | --- | --- |
| 3 | actor types | all 100 handlers `$02`-`$C8` have a spawn write; the TAS spawns 94; `$3A $3C $46 $90 $92 $B8` are spawnable only off the TAS's path | **proven** (spawn sites); `$46`/`$90`/`$92` are reached by input alone (the pad released at the boss), so nothing is cut; the `$90`'s `$9E3C` arm is dead code |
| 4 | rooms/records | the TAS reaches 30 `(area,room)` pairs; 96 of 312 bank-3 spawn records sit at ids no area schedule selects, so `$D714` can never read them | **proven** (dead ids); reachable rooms beyond the TAS's sequence = **lead** |
| 5 | sound | song `$06` unused (confirms §1); effects `$27 $32 $36 $39 $42 $45 $46 $49` are defined but no request site or TAS write names them | song **proven**; effects **lead** |
| 6 | player animation | 41 states written by code, 46 seen on the TAS; every state is used | **proven** (player); actor metasprite/state tables not swept = **lead** |
| 7 | ending payload | the script uses every draw list (its block counts equal the parsed counts) and all 28 credit lines | **proven** (lists/lines); an orphan table/string sweep was not finished = **lead** |
| 8 | CHR | 33 of the 256 1 KiB CHR banks are never mapped by the TAS, the ending/cutscene lists or the map-loader table | **proven** (method); untraced branches = **lead** |

---

## 3. Actor types — every handler has a spawn site; six sit off the TAS's path

**The dispatch table is full.** Bank 14 `$CF0D` selects the handler's PRG bank by
`type / 2` and `$CF72` is the 16-bit handler pointer indexed by the occupied byte
(`docs/ACTORS.md` §8a, code at `$CED5`). All 101 even entries `$00`-`$C8` are
non-zero. `$00`'s entry `$CED9` is the actor loop's own `LDY $7CC0,x` — the free
branch — not a handler, so the real handler set is the **100 types `$02`-`$C8`**
(`tools/emu/cutaudit2-scratch/audit_actors.py`).

**The TAS spawns 94 of them.** `cargo run -p rotj-oracle --release --example
inventory_tas` (whole-game TAS, frames 1-40896) lists 94 distinct `$7CC0` values.

**The six the TAS never spawns each have a ROM spawn write.** The child spawns
are ``LDA #type; STA $7CC0/$7CD0`` sites (`$7CD0` = `$7CC0 + $10`, the second
slot bank 16-31). Searching the PRG for that idiom (byte `A9 xx
9D/99 C0/D0 7C`) found:

| type | LDA site (bank, offset) | who runs it |
| --- | --- | --- |
| `$3A` | bank 11 `$105C` | the stage-1 boss `$38` (bank 11 `$8D1F`); its phase-1 part (port `k3a`) |
| `$3C` | bank 11 `$10DB` | the same boss, phase-2 shell (port `k3a`) |
| `$46` | bank 11 `$15AE` | the stage-4 boss `$42` (bank 11 `$947A`), its `$46` child (port `k42`) |
| `$90` | bank 0 `$1D70` | `sub_9D13`, called at `$9B85` by the `$8E` handler (bank 0 `$993A`) when its counter `$7E90` reaches `$50` |
| `$92` | bank 0 `$1E47` | the `$90` handler (bank 0 `$9DAD`) spawns it |
| `$B8` | bank 12 `$0F47` | the stage-7 boss controller `$B4` (bank 12 `$8C59`) on its death, after the TAS's last input (inventory stops at 40894; `$B8` appears at TAS 40921, `docs/ACTORS.md` §8b.1 / port `kb8`) |

**Conclusion.** There is **no orphan handler**: every one of the 100 types has a
spawn write. The six off-TAS types are reached only through branches the TAS's
input does not take (boss phases 1/2, the `$8E` counter, the post-input death).
The port implements all 100 (`crates/rotj-core/src/actor/families.rs`; `k46`,
`k90` and `k92` were the last, 2026-10-15).

**Reached by input alone.** All three off-TAS types appear in the oracle when
the TAS's input is replaced by a released pad at the boss, with no RAM poke
(the `matches_the_oracle` scenes of `k46`/`k90`/`k92`, `docs/ACTORS.md` §9d):

- `$90`/`$92`: in the `$8E`'s phase 3 (`$9B11`) `$7E90` is zeroed when the
  20-frame wind-up ends, then counts +1 a frame: the burst at `$50`, phase 1
  at `$80`. On the TAS the boss enters phase 3 at
  14971, ends the wind-up at 14991 and dies at 15050 (`$A9:$AA` = 0) with the
  count at `$3B`, 21 frames short. From `Scene::tas(14672)` with the pad
  released, the count reaches `$50` and the burst fires at 15083; the first
  `$92` lands at 15093. A player slower than the TAS sees the burst.
- `$46`: from `Scene::tas(26808)` with the pad released, the unhit `$42`
  charges `$7E10` to `$C0` and spawns the `$46` at 27208. The TAS's fire keeps
  draining the counter (6 per hit, `$9616`).

**Dead code.** The `$90`'s wall burst picks `$AC16` variant 7 at `$9E40`; the
arm before it, `$9E3C` (`LDA #$08 / BNE $9E42`, variant 8), follows a
`JMP $9E1C` and no branch, jump or pointer names it (a scan of bank 0's
branches and `JMP`/`JSR`, and of the whole PRG for the bytes `3C 9E`). The
variant table has a ninth entry for it (`$ACD6` = `$AD18`, variant 2's parts,
flags `$ACC5` = `$01`); whether another `$AC16` caller passes 8 was not
checked (lead).

**Worth restoring.** Nothing is cut here: the six types are conditional content,
not removed content. An enhanced "show every boss phase" mode (forcing the
boss's `$7DD0` through phases 1/2) would surface `$3A`/`$3C`; `$46`/`$90`/`$92` need
only a player who does not kill the boss as fast as the TAS.

---

## 4. Rooms, areas and spawn records

**The TAS reaches 30 `(area, room)` pairs, all 18 areas** (`$0400`, `$0402`):

```
00:00 00:02 00:04 | 02:00 | 04:00 | 06:00 06:02 06:04 | 08:00 | 0A:00 |
0C:00 0C:02 | 0E:00 | 10:00 | 12:00 12:02 12:04 12:06 | 14:00 | 16:00 |
18:00 | 1A:00 1A:02 1A:04 | 1C:00 | 1E:00 | 20:00 20:02 20:04 | 22:00
```

**The bank-3 record pool holds 312 records over 63 types.** `$A000[area]`
selects a per-area stage table, `[stage]` a record table, `[id*2]` a 12-byte
record (`$D714`/`$D765`; `docs/ACTORS.md` §7b). The pool of record pointers spans
`$AC88`-`$BB1C`.

**96 of the 312 records are dead.** The id the spawner reads is `(schedule byte &
$3F)` from the per-area schedule `$A2D4[area]` -> RAM `$7410` (`$D765`'s
`ASL/AND #$7F`). The schedules of the 18 areas mark **only ids `0..12`, `16` and
`63`** pending (union); every record that appears only at some other id, in every
table that holds it, can never be selected by the level-start spawner. That is 96
records (31%), across 22 types (`$1E`×10, `$36`×13, `$7C`×9, `$8A`×8, `$80`×7, …).
Method: `tools/emu/cutaudit2-scratch/audit_actors.py`.

**The TAS's own rooms select 47 records.** The 30 rooms' pending ids resolve to
47 distinct record addresses (each room picks 1-6; records are heavily reused
across rooms). The bank-2 column/row spawners (`$87C5`/`$8779`) choose which
schedule slots to check by the `$0440` column mapping and then read the *same*
record table by the *same* schedule id (`disasm/bank02.s:1124`-`1186`), so every
spawn path is bounded by the id set above.

**Lead.** "Rooms the game defines but the TAS never reaches" cannot be listed
statically: the per-area stage-pointer pool is shared and non-monotonic
(`$A000`'s area pointers overlap — area 0 `$A024`, area 1 `$A02A`, area 8 `$A02E`
< area 7 `$A04C`), so there is no per-area room count to enumerate and the game
advances rooms by level geometry, not a table.

**Worth restoring.** The 96 dead records are inert data; an enhanced mode cannot
show them without a new schedule byte pointing at their id.

---

## 5. Sound — one unused song (confirms §1) and eight unused effects

Method: the driver's id tables `$8B91`/`$8B92` (bank 10, `X = id*2`) give the
defined ids; the request sites are every ``LDA #imm; STA $40-$43`` in the PRG,
the two id tables (`$D980` stage songs, `$9C85` sound test), the ending/cutscene
script (`ending.rs`, `cutscene.rs`), and the whole-game TAS trace of `$40-$43`
(scratch oracle program). Script: `tools/emu/cutaudit2-scratch/audit_sound.py`.

* **Songs.** Defined distinct: `$01`-`$0F` (`$00` = `$01`; `$10`-`$1F` = `$08`).
  Requested: `$00`-`$05`, `$07`-`$0F`. **`$06` is never requested** — the §1
  finding, now re-confirmed with the whole-game trace in addition to the static
  scan. (`$10`-`$1F` are aliases, not content.)
* **Effects.** Defined distinct headers `$20`-`$49` (`$4A`-`$4F` all alias `$49`
  = `$9608`). Requested: `$20`-`$26`, `$28`-`$31`, `$33`-`$35`, `$37`, `$38`,
  `$3A`-`$3F`, `$40`, `$41`, `$43`, `$44`, `$47`, `$48`.
  **No request site or TAS write names `$27 $32 $36 $39 $42 $45 $46 $49`**
  (**lead**: a register- or table-driven writer could still produce one; the scan
  reads immediates and the two id tables only).

**Worth restoring.** The eight effect headers are complete programs; an enhanced
"sound test all effects" screen (the retail sound test offers songs only) would
play them, and would also expose `$4A`-`$4F` as the `$49` alias.

---

## 6. Player animation states — none unused

`$0466` is Batman's animation state. The static scan
(`tools/emu/sprites.py::states_written_by_code`, run here as a scratch copy)
finds **41 states** written by code: the immediates plus the bank-4 tables
`$96DB/$96DD/$96E0/$96F1/$96F3/$96FD/$979D/$979F` (their lengths in
`TABLE_LENGTHS`). The whole-game TAS writes **46 distinct** `$0466` values; the
five extra (`$42 $43 $85 $86 $87`) are table-driven states the restricted scan
misses. **Every statically-found state appears on the TAS**, so no player
animation state is unused.

**Lead.** The player metasprite pointer tables (`$9924`/`$9A24`) and the
per-actor metasprite tables (bank 1 via `$84CA`, bank 9 via `$81C6`) were **not**
swept for entries no handler selects. Method to finish: enumerate each table's
indices and intersect with the index set every handler can compute (the port's
`families/*` handlers mirror those indices; the oracle's `inventory_tas`
`phases` column already records which `$7DD0` values each type reaches).

**Worth restoring.** Nothing found; the animation table is fully exercised.

---

## 7. Ending payload — every draw list and credit line is used

The ending script (`crates/rotj-core/src/actor/ending.rs`, `SCRIPT`) draws from
five list addresses: `$6720` (5 blocks at the title, 2 at the skyline), `$68A5`
(9), `$6B02` (5), `$70C3` (2), `$7147` (3). The draw-block format (`$7E69`,
`draw_block`) is `[n entries][entry x n]`, entry = `count, len, flags, src_lo,
src_hi, addr_lo, addr_hi`. Parsing each list
(`tools/emu/cutaudit2-scratch/audit_ending.py`) yields exactly **5/9/5/2/3
blocks** — the script's counts equal the lists' lengths, so **no draw block in
any list is skipped**.

The credits read `$6D12` (28 line offsets, `CREDITS = $1C`) into the list `$6D2E`;
all 28 offsets are used by the loop, and each points at a well-formed block
(several offsets are reused by short lines). **No credit line is unused.**

**Lead.** A full sweep of the payload (`$6000`-`$7FFF`, copied from CHR `$F8-$FF`
by `$99A9`) for tables or text never referenced is not finished. A scan for byte
runs inside the credits' font-tile range (`$C0`-`$FE`) found runs at `$6A2D`
(64 bytes), `$717A`-`$71C0` and `$7608`-`$7650` outside the credit strings; their
roles (skyline sign, last-screen text, banner text) are unverified.

**Worth restoring.** Nothing found in the lists; a bigger payload may hold
unshown text (see the lead).

---

## 8. CHR — 33 of the 256 1 KiB banks are never mapped

Method: a scratch oracle program replays the whole-game TAS and records (a) every
value written to the CHR shadow `$E0`-`$E7` and (b) every direct FME-7 CHR write
(reconstructed from `$8000` selecting a command `0-7` and the `$A000` value it
points). To that union `audit_chr.py` adds the ending's and the banner's bank
lists (`$67EC`/`$6AAD`/`$6CEA`/`$7103`, `$76D7`, read from the payload) and the
level map-loader bases (`$8B63`, 8 consecutive 1 KiB banks each, `docs/LEVELDATA.md`
§13e). Result: **223 of 256 banks mapped, 33 never**.

The 33 never-mapped 1 KiB banks, grouped by 8 KiB CHR bank (`KiB = bank*8 + k`):

```
bank  0: 7
bank  2: 4 6 7
bank  3: 2 3 4 5 6 7
bank  4: 0 2 3
bank 12: 4 5 6 7
bank 13: 4 5 6
bank 15: 0 1 2 3 4 5 6
bank 18: 0
bank 20: 4
bank 21: 1
bank 23: 6 7
bank 26: 5
```

(1 KiB: `07 14 16 17 1A-1F 20 22 23 64-67 6C-6E 78-7E 90 A4 A9 BE BF D5`.)

All 33 hold real tile data, not padding: each has 333-896 nonzero bytes of 1024
(the same scratch check counts them).

**Lead.** The ending runs past the TAS's last input, so its banks come only from
the static lists, and a CHR bank could be mapped by an untaken branch (e.g. the
`$90`/`$92` actors' tiles). The 33 are candidates, not a proof of unused art.

**Worth restoring.** A CHR-bank viewer (or an enhanced mode that pages through
`pattern-space.bin`) would show whether the 33 banks hold art at all; those that
do are the most likely cut graphics.

---

## Reproduce (cutaudit2)

```
# every script reads raw/... and/or assets/graphics/chr/pattern-space.bin
python3 tools/emu/cutaudit2-scratch/audit_actors.py    # §3, §4
python3 tools/emu/cutaudit2-scratch/audit_256.py       # §4 (screen tables), §5/§6 raw
python3 tools/emu/cutaudit2-scratch/audit_sound.py     # §5
python3 tools/emu/cutaudit2-scratch/audit_ending.py    # §7
python3 tools/emu/cutaudit2-scratch/audit_chr.py       # §8 (needs trace3.out below)
# whole-game TAS traces (need raw/ and tools/emu/tas/inputs.csv):
cargo run -p rotj-oracle --release --example inventory_tas   # §3, §4 type/room inventory
# scratch oracle program (outside the repo) writes the $E0-$E7 / $0466 / $40-$43 / FME-7 trace:
(cd /tmp/cutaudit2-trace && CARGO_TARGET_DIR=$PWD/../../../target cargo run --release) > trace3.out
```
