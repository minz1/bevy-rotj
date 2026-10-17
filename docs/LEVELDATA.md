# Level data format — Batman: Return of the Joker (NES, FME-7)

Status: **partial**. The ROM's *level loader scaffolding* is decoded and tested;
the *streamed background* (the metatile map that becomes the visible nametable)
is located and its renderers are identified, but its compact source and its
auto-tiling generator are not yet decoded. This document states, for every
claim, the address and the evidence, and then lists what is still unknown.

A second pass (§6–§10) added three corrections that matter for the port:

- the **runtime PRG window map** during gameplay is *not* the reset default, so
  the "collision" far calls land in bank 08, not bank 12 (§6);
- those bank-08 routines are the actor's **integration / state dispatcher**, not
  a metatile solidity test (§7);
- the 640-byte `$9204` expansion in `$6000` is ten **8×8 digit glyphs**, not a
  collision map (§8), and the loader it belongs to is the intro/restart path.

**The metatile solidity rule is still not located** — see §11 for the negative
result and exactly what evidence would settle it.

Regenerate the disassembly facts with `python3 disasm/gen/generate.py && sh
disasm/assemble.sh` (must print `OK byte-identical`).

## 0. Where a sublevel comes from

The running game keeps the current position in RAM:

| addr | meaning | evidence |
| --- | --- | --- |
| `$0402` | stage index (0-based) | `disasm/bank0E.s` uses it as the actor-table row index (`LDY $0402` at `$D6xx`, `$DAxx`); frame 700 reads `$0402 = 0` |
| `$0401` | sublevel index within the stage | reset sets `$0402=$01,$0401=$02` (`bank 15 $E35C`); frame 700 reads `$0401 = 8` |
| `$0400` | area/screen index inside the sublevel | incremented by 2 per screen (`bank0E.s` `LDA $0400; ADC #$02; STA $0400`); indexes the per-area tables `$8ECF`/`$8EE1` |

Frame 700 of `assets/levels/stage-1-1/` is therefore stage 0, sublevel 8.

## 1. The level loader — bank 02 `$8F2B`

`disasm/bank02.s` label `L8F2B` (bank 02 file offset `$0F2B`; bank 02 runs at
CPU `$8000`, `disasm/MAP.md` §1). Called during the sublevel-start sequence:
`bank0E.s` `JSR $8F05` at `$D6xx` maps bank 02 into a window and calls the
routine that begins at `$8F05`, whose body falls into `$8F2B`.

It does three things, in order.

### 1a. Nametable patch list at `$91B7`

```
$8F2B: LDA $91B7,x      ; hi byte
$8F2E: BMI $8F4C        ; bit 7 set -> end of list
$8F30: STA $2006
       LDA $91B7,x
       STA $2006
       LDY $91B7,x      ; count
$8F3F: LDA $91B7,x
       STA $2007
       DEY / BNE $8F3F
```

This is exactly the `[hi, lo, count, bytes...]`, `$FF`-terminated write-list
format already decoded in `crates/rotj-core/src/ppulist.rs`. The list at CPU
`$91B7` (bank 02 offset `$11B7`) is 6 entries and ends at `$9203`:

| dest | bytes |
| --- | --- |
| `$20EB` | `04 0A 38 1C 26 38 2C 24 00 20` |
| `$210B` | `05 0B 39 1D 27 39 2D 25 00 21` |
| `$23D0` | `55` x8 (attribute table) |
| `$22EB` | `1A 3A 24 0C 0C 00 0C 1C 22 3A 1C` |
| `$230B` | `1B 3B 25 0D 0D 00 0D 1D 23 3B 1D` |
| `$23E8` | `55` x8 (attribute table) |

A **second** list at CPU `$925A` (offset `$125A`, 4 entries, ends `$12AC`) is
applied later in the same routine (`L9099`); a **third** list at CPU `$93D7`
(offset `$13D7`) is used by the sibling loader `$92AD`.

All three decode cleanly and are exposed by `rotj_core::level` and
`rotj ppulist`.

### 1b. Collision bitmap built into RAM `$6000`

```
$8F4C: LDA #$60 / STA $12     ; dest high byte = $60
       LDA #$00 / STA $10 / STA $11
$8F56: ASL A / ASL A / ASL A / TAX   ; X = chunk*8
       LDA #$07 / STA $13            ; 8 rows
       LDY #$00
$8F60: LDA #$07 / STA $14            ; 8 columns
       LDA $9204,x / STA $15
$8F69: LDA #$00 / ASL $15            ; shift out the top bit
       BCC $8F71 / LDA #$11          ; set bit -> $11
$8F71: STA ($11),y
       INY / DEC $14 / BPL $8F69     ; 8 columns
       INX / DEC $13 / BPL $8F60     ; 8 rows
       LDA $11 / CLC / ADC #$40 / STA $11   ; next chunk 64 bytes on
       INC $10 / CMP #$0A / BCC $8F56
```

So the ROM table at CPU `$9204` (bank 02 offset `$1204`) is **80 bytes**, read
as 10 chunks of 8. Each byte is bit-expanded **MSB first** into 8 output bytes
of `$00` (clear) or `$11` (set), and the 10 chunks land 64 bytes apart in RAM
`$6000-$627F` (640 bytes). `rotj_core::level::collision_bitmap` reproduces this
exactly and is unit-tested.

This is what `disasm/MAP.md` §5 called the "collision/level tilemap"; it is a
*bit* map (one byte per pixel, values `$00`/`$11`), not the tile-id map.

### 1c. Screen loader `$92AD`

`bank0E.s` `JSR $92AD` (from `LD55E`) runs the sibling loader: it reads
`$0400`, clears nametable `$2000-$23FF` (`LDA #$20; STA $2006; LDA #$00; STA
$2006; ... STA $2007`), programs CHR banks from `$E0-$E3`, then executes the
write-list at `$93D7`. So each *screen* has a nametable patch list, applied
over whatever the streamed background has already drawn.

## 2. The streamed background (located, not decoded)

The visible nametable is not stored; it is generated at run time. The renderers
are in bank 02:

- `$8000` (`L8000`): reads a source metatile byte from `($35)` with high byte
  `$6E` (`LDA #$6E; STA $36`), translates it through four 256-byte tables and
  pushes a PPU write-list onto the stack for the display-list executor
  (`bank 0E $C080`, `disasm/MAP.md` §4).
- `$8162` (`L8162`): the same, with high byte `$60` (`ORA #$60; STA $36`).

The four lookup tables are RAM `$7000`, `$70D0`, `$71A0`, `$7270` (CPU `$8000`
window). `bank 0D sub_B351` consumes them:

```
LDA ($20),y      ; source metatile id
TAX
LDA $7000,x / STA $04A0,y     ; top-left of a 2x2 block
LDA $71A0,x / STA $04C0,y     ; bottom-left
LDA $70D0,x / STA $04A1,y     ; top-right
LDA $7270,x / STA $04C1,y     ; bottom-right
```

so each source byte expands to a **2x2 nametable block**. The tables themselves
are *generated per area* from a compact CHR-resident definition: bank 02
`$8B9E` selects CHR banks from the table at `$8ECF` (indexed by `$0400/2`) and
reads a definition via `$2007`, then 16 handlers (jump table `$8C1D`) derive the
four corners by adding small offsets (`+1`, `+$0F`, `+$10`, `+$11`) — classic
auto-tiling. The source metatile map lives in RAM `$6000`/`$6E00` (256 bytes
each, one screen).

## 3. Acceptance test — what it printed

`cargo run -p rotj-tools -- level --rom raw/*.nes --stage 0 --sublevel 8
--dump tools/emu/stage`

```
nametable patches: 10 entries, 128 bytes
collision bitmap: 640 bytes, 268 set ($11)
-- acceptance: first screen vs nametables.bin
   patch bytes in range of the capture: 11/128 agree
   blank-image diff: 1145/2048 bytes differ
   RESULT: NO MATCH
-- acceptance: collision bitmap vs collision.bin
   610/640 bytes differ
-- frame 900 cross-check: 198/2048 nametable bytes differ from frame 700
```

**Interpretation.** The first screen does **not** match, and the evidence says
why rather than papering over it:

1. The ROM lists only touch **128 of the 2048** nametable bytes. The other 1920
   are produced by the `$8000`/`$8162` streamers, which this decode does not
   yet reproduce.
2. Only **11 of those 128** patch bytes agree with the frame-700 capture, and
   the disagreements are systematic, not noise: the ROM patch wants tile ids
   like `04 0A 38 1C` where the capture has `00 4C 5C 00`. The patch is applied
   with a *different CHR bank mapping* and is overwritten by streaming for this
   screen — consistent with the `$91B7` list belonging to the sublevel's
   *intro/other* screen rather than the frame-700 gameplay screen.
3. The captured `$6000` region does not match the loader's 640-byte bitmap at
   all (`610/640` differ). At frame 700 `$6000` holds the *current 256-byte
   metatile screen* plus scratch and level scripts, i.e. it has been overwritten
   since the loader ran; the capture is state, not the loader's output.
4. The frame-900 cross-check shows **198** nametable bytes differ 200 frames
   later, and they cluster in the left columns (`$2000` offsets `0x00,0x20,
   0x40,...`) — the columns the game rewrote as the camera moved. Static ROM
   data cannot produce a frame-dependent nametable; this is the streaming
   renderer's signature.

So: the format's *container* is understood (write-lists + a bit-packed map), the
visible screen is generated, and reproducing it needs the metatile/auto-tile
generator of §2.

## 4. What remains unknown

- **The compact metatile-map source in ROM.** Where the 256-byte screen map
  (`$6000`/`$6E00`) is loaded from, and in what encoding (no plain run of the
  captured bytes exists anywhere in PRG).
- **The auto-tiling generator.** The 16 handlers behind jump table `$8C1D` and
  the exact rule that turns one source byte plus neighbours into the four
  nametable tiles. Needed to reproduce any screen byte-for-byte.
- **The per-area tables.** `$8ECF` (CHR banks) and the `$8EE1/$8EE2` pair
  (a PPU-space "definition" pointer) are indexed by `$0400`, but the table
  extent and the meaning of the definition block (read via `$2007`) are not
  pinned down.
- **The stage/sublevel → area list.** Which `$0400` values belong to stage 0
  sublevel 8, and where that list is stored. `bank0E $0F72` (104 x 16-bit) and
  `bank04 $1924` (153 x 16-bit) are candidate pointer tables (`disasm/MAP.md`
  §5) but neither is confirmed as the per-sublevel screen list.
- **Spawn/scroll metadata.** The bank-0E object system (`$D6xx`/`$DAxx`) reads
  5-byte + 16-byte + 8-byte records from tables indexed by `$0400`/`$0402` and
  then `JMP ($0032)` into the data (embedded scripts). The record layout is
  partly visible but not decoded, and the captured `$6000` contains such script
  bytes (e.g. `$60C0`: `20 85 2E A9 00 85 2D ...`, `$61C0`: `2E A6 2D A0 40
  18 20 C6 C0 ...`).
- **Palette selection per sublevel** — palettes reach the PPU through the same
  display-list executor (`disasm/MAP.md` §6); the per-stage assignment is not
  established.

## 5. API

- `rotj_core::level::SublevelId` — stage/sublevel pair.
- `rotj_core::level::decode_ppu_list(prg, bank, offset)` — patches from a list.
- `rotj_core::level::collision_bitmap(prg)` — the 640-byte `$6000` bitmap.
- `rotj_core::level::LevelData::decode(prg, id)` — patches + collision.
- `rotj_core::level::diff_bytes(a, b)` — `(mismatched, compared)`.
- `rotj level` — writes `assets/levels/stage-1-1/level.bin` (the collision
  bitmap) and `level.txt` (summary), and with `--dump` runs the comparison.

## 6. Runtime PRG window map during gameplay (verified)

The FME-7 keeps a register shadow in RAM `$E0-$EC` and rewrites it every frame
in NMI (`disasm/MAP.md` §1, `$E259`). At gameplay it is not the reset default:

| RAM | cmd | window | bank | frame 700 | frame 850 |
| --- | --- | --- | --- | --- | --- |
| `$E8` | `$08` | `$8000` | 0 | `$C0` | `$C0` |
| `$E9` | `$09` | `$A000` | **8** (`$18`) | `$18` | `$18` |
| `$EA` | `$0A` | `$C000` | 13 | `$1D` | `$1D` |
| `$EB` | `$0B` | `$6000` | 14 | `$1E` | `$1E` |

Evidence: `tools/emu/phys/ramseq-600.bin` holds raw 2 KiB CPU RAM per frame,
frame N at offset `(N-600)*2048`; bytes `$E8-$EB` read `C0 18 1D 1E` at both
frames. Corroborated by `tools/emu/ppuw/mapper-writes.csv`, which logs the
shadow-rewrite writes at frame 700: `cmd $08 <- $C0` at PC `$E25C` and
`cmd $09 <- $18` at PC `$E261`.

**Consequence.** A `JSR a:$A56E` from bank 0 (`disasm/bank00.s`, the actor
update) executes at CPU `$A56E` while bank 8 is at `$A000`, i.e. it is bank 08
file offset **`$056E`** — *not* bank 12 offset `$056E`, which is what
`disasm/`'s nominal per-bank windows assume (`DEFAULT_WINDOWS = {..., $A000:
12}` in `disasm/gen/generate.py`). Same for `$A79B -> $079B`, `$A7DD -> $07DD`,
`$A852 -> $0852`, `$A8E4 -> $08E4`. `disasm/bank08.s` also contains
`JSR a:$A56E` (its line-based listing uses the nominal `$8000` window, so an
in-bank call renders as a "far" numeric call); at runtime that is the same
bank-08 routine. Verification method: read the shadow (authoritative, it is
what NMI writes to the mapper) and cross-check the PC of the shadow rewrite in
the mapper trace; do **not** trust `disasm/` window labels for switchable
banks.

## 7. The far-called routines are the actor's update, not a solidity test

Disassembled from bank 08 (runtime base `$A000`):

- **`$A56E`** — position integrator. Reads velocity/position fields through the
  zero-page pointers `($20)/($22)/($24)/($26)/($2A)` (per-slot 16-bit values at
  `$7Bxx`/`$7Cxx`), adds them, and at `$A59D` calls `$A852`; ends `JMP $CEDE`
  (the bank-0E object dispatcher). It is one entry into a routine that also
  starts at `$A532`.
- **`$A79B`** — state dispatcher: `LDA $7DD0,x; ASL; TAY; LDA $8A68,y ...;
  JMP ($0011)` — a `$8A68` handler table indexed by the actor state `$7DD0`.
- **`$A7DD`** — sets a state timer (`$7E10,x`) from table `$8A6E`, calls `$8A23`.
- **`$A852`** — animation/OAM setup: seeds `$96/$97/$A4/$99` from table `$8A76`
  (indexed by `$F1 & 6`), calls `$C229`/`$C247`, then `JMP $899A`.

None of these tests the metatile map. The hint that they are the solidity test
does not hold; the actual solidity code is elsewhere and was not located (§11).

## 8. The `$9204` expansion is ten digit glyphs, not a collision bitmap

The bank-02 routine at `$8F4C` (CPU `$8F4C` = offset `$0F4C`) expands the 80
bytes at CPU `$9204` (offset `$1204`) into 640 bytes at RAM `$6000-$627F`
(10 chunks of 64). But the source is ten 8×8 glyph rows — the digits **0-9**:

```
$9204: 3C 66 66 66 66 66 66 3C   ; "0"
       18 38 18 18 18 18 18 3C   ; "1"
       3C 66 66 04 08 10 20 7E   ; "2"
       ...                       ; "3".."9"
```

Each bit is expanded MSB-first to an output byte `$00` (clear) or `$11` (set),
i.e. a 1-bpp tile pattern, not a solid/empty map. The expansion does **not**
match the captured WRAM (`610/640` bytes differ), because the region is later
overwritten. So the name "collision bitmap" (§1b) is a misnomer; the routine is
a glyph/pattern builder.

The same routine is the **sublevel intro / restart** path: after the build it
sets CHR banks `$E0-$E3` (`$8FA9-$8FB4`), runs state fragments selected by
`LDA $10; ASL; ...; JMP ($0020)` through `$9254`, waits on timers `$F5`/`$F6`
while writing the `$925A` list to `$2006/$2007`, and ends at `$90F8` with
`JMP ($FFFC)` — the **RESET vector** (`$E2E6`). Its outputs are the transition
screen, which is why its nametable patches do not match the frame-700 gameplay
screen (§3).

## 9. Screens, extent and the area definition

- The renderers read the current metatile screen from WRAM: `$8000` reads with
  high byte `$6E` (`LDA #$6E; STA $36`) and `$8162` with `ORA #$60`
  (`disasm/bank02.s` §2). So the *second* screen is at `$6E00`.
- The map is **static across frames 600-3000** (`tools/emu/walkall/wram-*.bin`:
  `$6000` rows and `$6E00` rows byte-identical). The game streams the
  *nametable* from the map; it does not rewrite the map per screen.
- WRAM `$6000-$6EFF` is 15 pages of 256 bytes, each beginning `71`, consistent
  with 16×16-metatile screen records at `$6000 + n*$100`. The port's
  `SECOND_MAP_OFFSET = $0E00` (`crates/app/src/stage.rs`) therefore selects
  screen **14**, not screen 1; if the sublevel is wider than the two visible
  nametables it should be `$0100`. This is **unverified** — only two nametables
  were captured.
- **Area tables.** `$8ECF,x` with `x = $0400` gives the CHR 1-KiB base bank;
  `$8EE1,x`/`$8EE2,x` give a 16-bit **PPU address** (written high-then-low to
  `$2006`), so `$8EE1` is a normal 16-bit LE table (low bytes at even offsets).
  Area 0 (`$0400 = 0`): base bank `$D9`, PPU address `$0074`. The auto-tiler
  maps CHR banks `base..base+7` and reads the area definition through `$2007`,
  landing at CHR offset `$D9*1024 + $0074 = $36474`, whose first bytes are
  `02 D1 C9 01 02 03 04 05 07 09 0A 0B ...` — the per-area tile/attribute
  source. `$0400` increments by 2 per screen (`bank0E.s`), which is why the
  tables are indexed at even offsets.
- **No plain map in ROM.** Searches for screen 0/1 rows and the repeating
  column pattern across the whole PRG return **0 hits**, so the map is
  generated/compressed. `rotj level` still reproduces only 11/128 patch bytes
  of `nametables.bin` (no improvement; the streamed background is not decoded).

## 10. Still not decoded (this pass)

- **The player's collision routine.** Not located. The code path the brief
  points at (`$A56E`/`$A79B`/`$A7DD`/`$A852`) is the actor update (§7). Searches
  for a `÷16` map-index computation (`4A 4A 4A 4A` in bank 0/8), for a solidity
  table, for reads of `$6000`/`$6E00`, and for `LDA #$6E`/`#$60` map pointers
  found only the auto-tiler and unrelated object code.
- **The metatile solidity predicate.** See §11.

## 11. The collision rule (decoded)

**The rule is a per-metatile class table at WRAM `$7340`, not the geometry.**
The player's movement (bank 4 `$800B`) tests a map position by far-calling bank
14 `$C99D` (body/wall: bank 4 `$80F8`, `$812B`, `$8184`, `$8198`, `$81B7`,
`$8226`, `$823A`) and `$C93B` (head and ground: bank 4 `$847F`, `$8521`, `$8533`,
`$8547`). Both build the map address `$6000 + (y/16)*256 + (x/16)` from the
probe position, read the metatile id, look it up in the 208-byte class table at
`$7340`, and dispatch through a per-class handler table — `$CADC` for `$C99D`,
`$C97D` for `$C93B`, `$CD2E` for the step probe `$CAFE`:

```
$C99D: STA $24              ; the probe direction
       ...                  ; $2B = $60 | (y/16); $2A = (x/16) | $0440[page]
       LDA ($2A),y          ; the metatile id
       TAX
       LDY a:$7340,x        ; the class
       LDA a:$CADC,y        ; -> handler pointer ($2D:$2C)
       JMP ($002C)
$C9DA: LDA $24 / LSR / BCS $CA08 / LSR / BCS $C9F7   ; push back on x
$C9E2: ... STA $93 ...      ; or on y, then zero the velocity
$CA1D: LDA $A1 / STA $A3 / LDA $A0 / STA $A2 / STA $9B,$9C   ; revert + stop
```

`$7340` is filled at sublevel load by bank 02 `$89C1`: a 16-bit pointer at bank
14 `$C000 + area*2` (bank 0 mapped to `$A000` by `$C229`), read as run-length
`(class, run)` pairs and stored as `2*class` (`$89E9 STA $7340,x`, 208 entries).
`crates/rotj-core/src/collision.rs` decodes it (`class_table`) and the two
handler tables into a `Collision { wall, block, hazard }` per class
(`collision_of`); `collision_at` is the whole map lookup, and `crates/app` uses
it for the player's floor/ceiling/wall probes.

The classes, in the game's own words:

| class | `$CADC` (body) | `$C97D` (head/ground) | kind |
| --- | --- | --- | --- |
| 0,1,2,4,5,16 | reverts the position — **wall** | negative — **blocks** | solid |
| 11,13 | reverts | `$00` | wall only |
| 12,14,15 | `$CAA0`/`$CAA3`/`$CAA6` (nothing) | `$C974`: negative only when `$24` bit 2, so **floor only** | one-way (the ground body is class 14) |
| 3,6 | hazard flags only | `$00` | hazard/empty |
| 7,8,9,10 | nothing | `$00` | empty (the sky/backdrop is class 10) |

**What the old derived rule got wrong.** `docs/LEVELDATA.md` previously
described a flood-fill from the ground band over non-empty *tiles*
(`stage.rs::derive_solid_tiles`), because a per-metatile-id rule "cannot
express" the block — the ids the actor walked through and the id he stopped at
"both contain tiles from `{01,02,03}`". That tile reading was the wrong frame:
the collision is per metatile, and the block is a *different* metatile id
(`$76`, class 0) whose body-probe handler pushes the actor out. The flood-fill
also marked the full-width backdrop band at y 72-94 solid (it is metatile class
10, clear), which is why the port needed the 40 px `HEADROOM` workaround.

**Evidence.**

- `tools/rotj-oracle/tests/solids.rs`: replaying the stage-1 route, every one of
  the 1722 `$C99D` probes (179 distinct map cells) dispatches to a `$CADC`
  handler whose push answer equals `collision_of(class_table[id]).wall` — 0
  divergences; the live `$7340` halves equal the ROM decode.
- Replaying `INPUT=physics` through the oracle, the actor's body probe (`$8184`,
  x + 10 px) first reads metatile `$76` (class 0) at probe x 192 on the frame he
  pins (733), and the port's rule reports the same wall; the metatile before it
  (column 11) is clear.
- `tools/emu/phys` (`physics-700-830.csv`): the actor's `$00AC` walks 0 -> 54
  and his x velocity falls to zero at frame 734.
- The capture's `$7340-$740F` (`assets/levels/stage-1-1/collision.bin`) equals
  the decode byte for byte (`rotj_core::collision` tests).

**Still unverified.** The `$C93B`/`$C97D` head/ground answer (`block`) is derived
from the `$C97D` sign table, not observed as a return value; only the floor side
is exercised (the port needs it for the ground). `$0094` reads as the actor's
level x once the camera settles (`$94 = $00AC + 131`) but not before it; the
probe positions come from the game's own `$20-$23` instead.


## 12. Resolved (second pass): the rendered nametable, and where the map lives

This pass decoded the *rendering* end-to-end and verified it byte-for-byte
against the captures. `crates/rotj-core/src/levelmap.rs` implements it and its
acceptance tests pass.

### 12a. The map is column-major in WRAM

`$6000-$6EFF` is **15 pages of 256 bytes, one per metatile ROW**: page `r` is
metatile row `r` and byte `c` is metatile column `c`. It is *not* 15 screens.
Each metatile expands to a 2x2 nametable block through four 256-byte tables:
top-left `$7000`, top-right `$70D0`, bottom-left `$71A0`, bottom-right `$7270`
(indexed by metatile id). Verification: for a scroll position, nametable row
`2r+a`, column `p` = corner table `[a][b]` of `map[r][c]`, with the ring below.

### 12b. The scroll is a single-screen 32-column ring

Both physical nametables are byte-identical in every capture (single-screen
mirroring, `cmd $0C`), so the visible 256 px is one 32-tile-column ring:
nametable address `p` holds level tile column `c` with `c ≡ p (mod 32)` and
`c ∈ [left, left+31]`, `left = scroll_x / 8`. `scroll_x` is RAM `$00AC`
(`tools/emu/pages.py` already used this). RAM `$94` is the actor's own level x;
`$94 - 131` equals `$00AC` once the camera has settled (both are 54 at drive
frames 800+), but at drive frame 700 `$94-131` is 56 while the nametable
reproduces at `$00AC = 50`, so the *nametable* is keyed to `$00AC`.

### 12c. Acceptance (measured)

`cargo test -p rotj-core levelmap` compares the decoded nametable against the
committed captures, requiring 0 mismatches over the 30 visible rows. The
scrolled set is the one the milestone named, `tools/emu/drive` frames 600/700/800:

| capture | camera `$00AC` | sublevel | result |
| --- | --- | --- | --- |
| `assets/.../nametables.bin` (stage frame 700) | 0 | 8 | **960/960** |
| `nametable-drive-600.bin` (drive frame 600) | 0 | 8 | **960/960** |
| `nametable-drive-700.bin` (drive frame 700) | 50 | 8 | **960/960** |
| `nametable-drive-800.bin` (drive frame 800) | 54 | 8 | **960/960** |

Every captured sublevel-8 frame (all 31 `tools/emu/pages/` dumps, scroll 0-54)
also matched 960/960. So the decoder renders the whole **4096 px** map (256
metatile columns), not just the 512 px window the old port assumed.

### 12d. Where the map's bytes come from (ROM source, located not decoded)

The map is **not** in PRG or CHR verbatim (searches for map rows and the four
tables over the whole ROM return 0 hits). The level *payload* is copied into
WRAM `$6000-$7FFF` from **CHR banks `$F8-$FF`** by bank 02 `$99A9` (7776 bytes,
PPU `$0000` → `$6000`), with a 0x1A0-byte tail from bank `$F7` offset `$260` via
`$9925`; bank 02 `$98BD` then `JMP $6000` runs it. That payload is 6502 code
(the port's captures at `$60C0`/`$7F00` disassemble, and it calls the
display-list executor `JSR $C0C6`), so the metatile map at `$6000` is produced
by running it — the compact source and the generator are the remaining unknown.

### 12e. Window-map correction (affects §6/§7)

The runtime map is `$6000` = **WRAM** (`cmd $8 = $C0`, bit 7 = RAM), `$8000` =
bank 8 (`$18`), `$A000` = bank 13 (`$1D`), `$C000` = bank 14 (`$1E`); the FME-7
command→window nibble order is `$8/$9/$A/$B` = `$6000/$8000/$A000/$C000` (see
`docs/ACTORS.md` §0 for the three independent proofs). The "bank 08" attribution
in §6/§7 is therefore bank **13**: `JSR a:$A56E` is bank 13 offset `$056E`.


## The solidity rule (superseded 2026-10-14)

The 2026-10-13 "found" section is retracted. Its rule — a flood-fill from the
ground band over non-empty **tiles** — was a workaround: it happened to block
the capture and to clear most of the sky, but it marked the full-width backdrop
band at y 72-94 solid (the port needed `HEADROOM = 40` to jump through it), and
its premise was wrong. "A per-id rule cannot express it, because the actor walks
through metatiles `01/02/03` at one column and is blocked by `01/02` at another"
mistook *rendered tiles* for the collision unit. The rule is per metatile id
(§11): the actor is blocked by metatile `$76` (class 0) at level x 192, and the
tiles he walks over (metatile `$12`/`$19`, class 10) are a different id.

Kept for the record: the collision response `$CA1D` reverts the position from
its saved copy (`$A0/$A1` -> `$A2/$A3`) and zeroes the velocity (still true), and
the ground line does sit on the 16 px metatile grid — the class table's floor
answer is per metatile.


## 13. The metatile map from the ROM (decoded)

The map is not a ROM blob: the game **decompresses** it from CHR into WRAM
`$6000-$6FFF` at sublevel start. The loader is **bank 02 `$89FE`**, called from
bank 13 `$BD5E` (`JSR $BF51` maps bank 2 into `$8000`, then `JSR $89FE`,
`$8B99`, `$89C1`). Addresses are CPU addresses with bank 2 at `$8000`.

### 13a. Per-area tables (bank 2)

| table | address | shape | area 0 |
| --- | --- | --- | --- |
| CHR base bank | `$8B63` (offset `$0B63`) | **18** bytes, index `$0400/2` | `$D8` |
| stream PPU address | `$8B75`/`$8B76` | **18** x 16-bit LE, index `$0400/2` | `$0000` |

The loader maps CHR 1 KiB banks `base..base+7` to PPU `$0000-$1FFF` and points
`$2006` at the PPU address, then reads the stream by `$2007`.

Both tables are 18 entries, not 16: the loader does `LDA $0400; LSR; TAX; LDA
$8B63,x` and the bytes run to `$8B74` inclusive (`… E7 E7 E8 EA`); the PPU table
runs to `$8B98` (`… 00E7 0217 02F3`) and code resumes at `$8B99`
(`LDA $0400; LSR; TAX; LDA $8ECF,x`). Entries 16/17 (`$E8`/`$EA`, PPU `$0217`/
`$02F3`) decode to non-empty maps (§13c), so they are real streams, not padding.

**There is exactly one copy of this table pair in the ROM** (a byte scan for the
CHR-base signature `D8 D9 D9 DD …` matches only bank 2, and a scan for the
loader prologue `LDA $0400; LSR; TAX; LDA abs,x` finds no CHR-base table
elsewhere). So the same table selects the level for every stage: the "CHR-bank
set" is indexed only by `$0400/2` (the area), and each area is a distinct,
full-width map.

### 13b. The stream format (bank 02 `$89FE`)

1. **Zero** `$6000-$6FFF` (16 x 256 bytes, `$8A38`).
2. **Header**, 6 bytes. `$13 = b0`, `$12 = b1 - 4` (`$8A51-$8A65`), so
   `count = (b0 << 8 | b1) - 4`; `b2-b5` are read and discarded (`$8A67`).
3. **Bit stream** from byte 6 (`$8A73`): 8 bits per byte, **LSB first**; each
   bit selects one token:
   - `1` = **literal**: read one byte, emit it (`$8A9A`);
   - `0` = **copy**: read `lo`, then `b2`; source `$6000 + ((b2>>4)<<8) + lo`,
     length `(b2 & $0F) + 3` (`$8ABF-$8B19`).
4. **Count** decrements once per bit-refill, once for a copy's first byte, and
   once per token; the loop ends on underflow (`$8A86`, `$8AC4`, `$8B1B`).
5. Bytes are emitted to **two** cursors: `$7000+` (the result) and a wrapping
   `$6000-$6FFF` LZ window starting at `$6FEE`; copies read that window.
6. **Transpose** (`$8B2D`): `out[k*256 + p*16 + j] = in[p*256 + k*16 + j]` for
   `k,p,j` in `0..16`, producing the map at `$6000-$6FFF`.

The `$7000-$7FFF` bytes the stream does not reach still hold the level
**payload** (copied to WRAM `$6000-$7E5F` from CHR banks `$F8-$FF` by `$99A9`,
and `$7E60-$7FFF` from bank `$F7` offset `$260` by `$9925`; `$7FC0-$7FFF` are
zero), so the transpose copies payload code into map **columns 160-255**. The
real level is **16 rows x 160 columns**; columns 160-255 are not level data.

### 13c. Acceptance (measured)

`crates/rotj-core/src/levelmap.rs::map_from_rom(chr, area)` implements this and
`rom_decoder_reproduces_the_captured_map` compares it to
`assets/levels/stage-1-1/collision.bin`:

| what | result |
| --- | --- |
| `$6000-$6FFF` (16 x 256) vs the capture, area 0 | **0 / 4096 differ** |
| vs a capture with `$0400 = 2*5` forced at the loader, area 5 | **0 / 4096 differ** |
| vs a capture with `$0400 = 2*16` forced at the loader, area 16 | **0 / 4096 differ** |

The area-5 and area-16 captures (`tools/emu/sublevels-force/wram-a5.bin`,
`wram-a16.bin`) are made by writing `$0400` at the loader's first instruction
(CPU `$89FE`), which is the *same* selector the game uses (§13e); the game then
reads `$8B63[area]`/`$8B75[area]` exactly as for area 0. Committed as
`crates/rotj-core/tests/data/area{5,16}-map.bin` and asserted by
`rom_decoder_matches_a_captured_second_area` /
`rom_decoder_matches_the_extended_area_16`.

`rotj level-rom --rom <rom> --area 0 --out <dir>` (or `--level 0`) writes the
same bytes to `map.bin`. All 18 areas of the fixed table decode to non-empty
maps (`rom_decoder_decodes_every_area`); they form 15 distinct layouts (areas
1=2, 6=7, 14=15 repeat, matching the duplicated table entries).

### 13d. Attributes (decoded)

WRAM **`$7600-$79FF`** holds the whole level's background attribute bytes: 1024
bytes = 16 pages of 64 (one per 256 px). The captured nametable's attribute table
(`$23C0-$23FF`) is page `camera/256`'s 64 bytes (verified at scroll 0:
`collision.bin[$1600..$163F]` equals `$23C0..$23FF`). `crates/rotj-core/src/levelmap.rs::attributes_from_rom`
implements the generator; its acceptance tests pass.

**Output.** The auto-tiler's attribute pass (bank 02 `$8E57`) walks the metatile
map in 2x2 blocks and stores, per byte, four 2-bit palette **classes**: top-left
in bits 0-1, top-right 2-3, bottom-left 4-5, bottom-right 6-7. The `$C6F2`
indirection the disassembly shows is only `(x & 15) << 4` (measured: byte-identical
to a `x & 15` shift of the low class bits).

**The class table `$7B00`** is one 2-bit class per metatile **id** (ids
`0..N-1`). It is filled by bank 02 `$8E1F-$8E43`, which reads `$2007` bytes and
unpacks each into four 2-bit classes, high pair first.

**The CHR area definition.** The auto-tiler maps CHR 1 KiB banks
`$8ECF[area]..+7` (a *different* table from the map stream's `$8B63`) and points
`$2006` at `$8EE2:$8EE1[area]`, then reads the definition through `$2007` (a few
warm-up reads are discarded because of the PPU read buffer's one-byte lag, so the
stored count lands on definition byte 2). Its layout, all offsets relative to the
definition start (area 0: CHR `$D9 * 1024 + $0074`):

| part | bytes | meaning |
| --- | --- | --- |
| header | 0-1 | skipped (warm-up) |
| count | 2 | number of metatile ids `N` (area 0: `$C9` = 201) |
| shape table | 3-17 | 15 bytes filling `$04A0[1..16]`; `$04A0[0]` is forced to `$12` |
| pair nibbles | 18 .. 18+ceil(N/2) | handler shape per metatile: even id = high nibble, odd id = low nibble, indexed through the shape table |
| tile bytes | ..+sum | each id's handler (bank 02 `$8C1D`, a **19**-entry table) reads 1-4 definition bytes; the 19 read counts are `1,1,2,2,2,2,2,2,2,2,3,3,3,3,3,3,3,3,4` |
| class bytes | last ceil(N/4) | 4 classes per byte, high pair first |

Area 0's class bytes start 672 bytes into the definition (CHR `$36714`, verified
by searching for the packed trace image). `attributes_from_rom` walks the parts to
find that offset, so any area's classes can be decoded.

**Measured acceptance.** `crates/rotj-core/src/levelmap.rs::rom_attributes_reproduce_the_captured_attribute_map`
compares the generated `$7600-$79FF` to `assets/levels/stage-1-1/collision.bin`
(the frame-700 `$6000-$7FFF` dump):

| region | result |
| --- | --- |
| the real level (160 metatile cols = pages 0-9, 640 bytes) | **0 / 640 differ** |
| the whole 1024 bytes | 314 / 1024 differ |

The 314 are all in pages 10-15 (metatile columns 160-255), which are the level
*payload*, not level data (§13b); the game's own class bytes past the level's id
count are stale, so those bytes are run-dependent and are not generated. The same
0-over-640 result holds for every committed gameplay capture examined —
`drive` 600/700/800, `pages` 608-1088, `walk`, `run2`, `prog`, `seek`/`seek2`,
`stage`, `sweep` (all sublevel 8, camera ≤ 54). `rom_classes_reproduce_the_emulator_class_table`
also checks area 0's decoded `$7B00` (201 bytes) against an emulator trace
(`crates/rotj-core/tests/data/classes-7b00.bin`, 0/201 differ).

**Other areas.** The `$8ECF`/`$8EE1` tables have 18 entries, like the map
loader's (code resumes at `$8F05`). Against the `$7600` of the forced loads in
§13e (`tools/emu/sublevels-force/wram-a<N>.bin`), counting only 2x2 blocks whose
ids are all below `N`, the generated attributes differ in 0 bytes for 16 of 18
areas. Areas 4 and 12 differ (655/1024, 752/1024). Those maps are one screen
wide and an actor erases the rest (§13e), so the comparison says nothing about
the decoder.
`rom_attributes_reproduce_a_second_area` pins area 16 (1005 blocks, 0 differ;
`tests/data/area16-attrs.bin` is that dump's `$7600`). The `$7B00` page in those
dumps is already overwritten by the actor slot arrays, so it cannot be compared
directly.

**The on-screen `$23C0` at a scrolled camera is streamed (decoded).**

*Where.* The attribute bytes do reach the PPU, through the NMI's VRAM-update
stack buffer. The earlier "no `$2006=$23xx` writes" trace was wrong because
`tools/emu/mapper.lua` logs `memory.readbyte($2006)` in a write callback (its
value column holds `00/20/40/C0`-style bus values, not the stored address
bytes, so the logged addresses were not VRAM addresses). Retraced with the value taken from the store's register
(`tools/emu/attrstream-trace/trace.lua`, opcode at PC-3 = `STA/STX/STY abs`
picks A/X/Y) and the VRAM address tracked: the flusher at CPU `$C09C-$C0A6`
(bank 14, `PLA` records: address high, address low, count, data) writes
`$2006 = $23D8, $23D0, $23C8, $23C0` (`$23D9...` for the next column), two bytes
each (rows 3+7, 2+6, 1+5, 0+4) with `$2000` bit 2 set (+32). Those are the 8
bytes of one attribute column, e.g. frame 680 of `INPUT=drive`: `23D8=00 23F8=0C
23D0=55 23F0=CF 23C8=0A 23E8=7F 23C0=AA 23E0=00`.

*Who builds it.* A write watch on the stack page catches the record being pushed
at PC `$8125/$8128` with `$8000` = 8 KiB bank 2 (`disasm/gen/read_range.py 2
0x0030 0x1c0 --window 0x8000`, CHR/PRG byte search confirms bank 2 offset
`$0120`). The routine (`$8000-$8143`, entered once per frame for the right-scroll
case) pushes the tile column (`$8030-$80AB`) and then the attribute column:

- `y = $20 | (($CC & $0F) >> 1)`: the attribute column, `$CC` being the high
  byte of the 16-bit camera `$CC:$CD` (1/16 px), so `$CC & $0F` is the metatile
  column within the page;
- the carry of that `LSR` (`$CC` bit 0) picks the half: keep mask `$39 = $CC`
  (left quadrants are replaced) or `$33` (right quadrants are replaced), and
  `$3A = ~$39`;
- for each of the 8 rows: `new = (shadow[y] & $39) | (page[y] & $3A)`, stored
  back to the **shadow table `$7530-$756F`** and pushed to the PPU; `page` is
  `$7600 + 64 * (metatile column / 16)` (`$3B/$3C` from `$35/$3F`);
- the shadow is therefore the game's own copy of `$23C0-$23FF`; only two
  quadrants of one column change per call, the other two keep their previous
  bytes. That is the "partial update". The metatile column behind the replaced
  half is the one entering at the right edge (`$35` = 16, 16, 17, 17, 18... at
  frames 712-732 of `tools/emu/walk`).

*Resulting rule.* The routine runs once per 8 px tile step of `$CC:$CD >> 4`.
Each ring slot `p` (nametable column) holds tile column `t(p)` in
`[left, left+31]` with `left = camera/8` (the same ring `render_nametable` uses),
and attribute column `c`, half `h` shows the quadrants of the metatile
`t(4c + 2h) / 2`, copied from its page table. It depends on the camera alone: no
history is needed, as long as the camera did not skip a tile step.
`levelmap::streamed_attributes(pages, camera_x)` implements it.

*Measured acceptance.* The camera is `$CC:$CD >> 4` of the previous frame.
Three scripted runs (`tools/emu/attrstream-trace/trace.lua`, `ROUTE=1`, jump
holds `50/22`, `40/30`, `60/25` after frame 735; the runs walk the full stage 1-1
route and move back and forth) were sampled every frame, 554 frames each, camera
0-512 px (the game stops the camera at 512), including leftward scrolls:

| source | frames | result |
| --- | --- | --- |
| generated vs captured `$23C0-$23FF`, 3 runs, camera = previous-frame `$CC:$CD >> 4` | 1662 | **0 bytes differ on every frame** |
| same, camera = `$00AC` completed with the high bits | 1662 | 24 frames differ, 1-7 bytes each, all at an exact tile-step frame |
| old model (page table of `camera/256`) | | 5-9 of 64 bytes at cameras 22/50/54 |

`$00AC` is `$FD` of the frame before, and `$FD` is `$CC:$CD >> 4` *rounded*
(`inc $FD` when `$CD` bit 3 is set; the store at PC `$8918`), whereas the game
steps on the unrounded value. The port only has the rounded scroll, so a frame
whose scroll lands exactly on a tile boundary may show the update one frame
early or late.

The 1662 frames dedupe to 65 tile steps (`tests/data/attrstream-scroll.bin`: for
each step the captured bytes were identical in every frame and run), and
`levelmap::tests::streamed_attributes_reproduce_every_scrolled_frame` checks all
65 (0 differ). `streamed_attributes_against_earlier_captures` checks twelve
dumps from earlier captures (`tests/data/attrstream-dumps.bin`, camera = `$00AC`
completed from `$CC:$CD`):

| capture, frame | camera | differing bytes |
| --- | --- | --- |
| `drive` 700 / 800, `pages` 720 | 50 / 54 / 22 | 0 / 0 / 0 |
| `route-dense` 760 / 820 / 900 | 73 / 109 / 231 | 0 / 0 / 0 |
| `route-dense` 852 | 152 | **2** (tile-step skew above) |
| `route-step` 1000, `route-oam` 850 | 246 / 148 | 0 / 0 |
| `route-chain` 900 / 1200 | 265 / 512 | 0 / 0 |
| `seek2` 700 | 188 | **14** (the capture writes `$00AC` directly; nothing streamed, the screen shows page 0 exactly) |

`route-chain` frames 1500+ (`$DD = $40`, camera 0) show another sublevel, so a
stage 1-1 page table does not apply (58-61 of 64 differ); not tested.

*Not verified.* (1) Everything is stage 1-1 (area 0). (2) A camera that jumps
more than one tile step per frame, or a level that starts scrolled, would
leave the shadow in a state this model does not predict (the `seek` captures
are the forged example). (3) The routine's left-scroll entry (`$8000` -> `$8162`
and the `$DD` flag bits) is not decoded; the rule above fits the left moves in
the runs (e.g. camera 316 -> 310) but that is a fit, not a decode. (4) Forcing
`$CC:$CD` from Lua (a camera ramp, `tools/emu/attrstream-ramp`) does not
reproduce the model: the game then streams a different lead and wrote the page 0
byte for the new column, so forged cameras say nothing about natural play.

### 13e. The CHR-bank set: one table, indexed by `$0400/2` (decoded)

**Rule.** A level's CHR-bank set is `$8B63[$0400/2]` (CHR 1 KiB banks mapped to
PPU `$0000-$1FFF`) and its map stream is read from PPU `$8B75[$0400/2]` — the
single table pair of §13a. There is no per-stage table and no per-stage loader:
the loader (`$89FE`, bank 2, below) reads the same table for every level, and
the stage selects scripts (actor spawns etc.), not tiles.

Evidence, all with program counters:

- **The loader is bank 2 offset `$09FE`, and it runs at CPU `$89FE` when bank 2
  is mapped to `$8000`.** Frame 558 of `INPUT=drive`:
  `memory.registerexec(0x89FE)` fires, and the byte at `$8A08` is `8E` — bank
  2's byte at offset `$0A08` (unique; no other bank has `AD` at `$09FE` and `8E`
  at `$0A08`), so bank 2 is at `$8000` (`tools/emu/sublevels-load3/exec2.csv`).
  The CHR loop at `$8A08` (`STX $8000; STA $A000`) writes CHR banks
  `$D8,$D9,…,$DF` — base `$D8` for `$0400 = 0`.
  (Window-identity method for reference: at frame 700 the `$8000`/`$A000`/
  `$C000` dumps are byte-identical to banks 8/13/14, `tools/emu/sublevels-win`.)
- **`$0400` selects it**: the loader opens `LDA $0400; LSR; TAX; LDA $8B63,x`
  (bank 2 offset `$09FE`), and `LDA $8B75/$8B76,x` for the PPU address.
- **Forcing `$0400` rebuilds a different level, byte-for-byte.** Writing
  `$0400 = 2*area` at `$89FE` and capturing WRAM 40 frames later reproduces
  `map_from_rom(chr, area)` with **0 / 4096** mismatches for **14 of the 18
  areas**. The exceptions are 4, 10, 12 and 17: their streams are one screen
  long, as explained below (`tools/emu/sublevels-force/`, §13c).

**Verified (2026-10-14): the level -> area table, all 13 rows.**
`$0400` is set at level start by `bank0E:sub_D3D8` (CPU `$D3D8`):
`LDX $20; LDA $D94E,x; STA $0400`, traced at PC `$D3E0` (frame 475 of the
`drive` run but frame 474 here). Bank 14 `$D94E` holds 13 even `$0400` values
(`00 02 06 08 0A 0C 10 12 16 18 1A 1C 20`).

Method (`tools/emu/stagearea-probe/probe.lua`): hook execution of `$D3D8`
itself and write `$20 = n` there; the very next instruction (`LDX $20`) reads
it, so the game's own table runs. Capture WRAM/PPU/RAM/screenshot 40 and 150
frames later. For **every** `n` in 0..12 the trace shows `$D3D8` execute once
(frame 474) and `val-0400.csv` shows exactly two `$0400` writes: the reset at
frame 4 and `$0400 = $D94E[n]` at PC `$D3E0` — so the table row is applied, with
no later re-selection. The transition screen then renders the game's own
`STAGE`-`x-y` banner, and `x = n/2 + 1`, `y = n%2 + 1` exactly (`banners-514.png`).

| `$20` (level) | banner | `$0400` | area (`$0400/2`) | `$8B63[area]` | map vs `map_from_rom(area)` |
| --- | --- | --- | --- | --- | --- |
| 0 | STAGE 1-1 | `00` | 0 | `$D8` | **0 / 4096** |
| 1 | STAGE 1-2 | `02` | 1 | `$D9` | **0 / 4096** |
| 2 | STAGE 2-1 | `06` | 3 | `$DD` | **0 / 4096** |
| 3 | STAGE 2-2 | `08` | 4 | `$E0` | 1632 / 4096 (one-screen map, see below) |
| 4 | STAGE 3-1 | `0A` | 5 | `$E2` | **0 / 4096** |
| 5 | STAGE 3-2 | `0C` | 6 | `$E3` | **0 / 4096** |
| 6 | STAGE 4-1 | `10` | 8 | `$DA` | **0 / 4096** |
| 7 | STAGE 4-2 | `12` | 9 | `$DB` | **0 / 4096** |
| 8 | STAGE 5-1 | `16` | 11 | `$E0` | **0 / 4096** |
| 9 | STAGE 5-2 | `18` | 12 | `$E2` | 1632 / 4096 (one-screen map) |
| 10 | STAGE 6-1 | `1A` | 13 | `$E5` | **0 / 4096** |
| 11 | STAGE 6-2 | `1C` | 14 | `$E7` | **0 / 4096** (frame 624; 8 bytes drift by 744) |
| 12 | STAGE 7-1 | `20` | 16 | `$E8` | **0 / 4096** |

The map column compares `$6000-$6FFF` at frame 624 to
`map_from_rom(area_for_level(n))` (the Rust decoder and
`verify.py`, byte for byte). 11 of 13 levels are **exact**. Levels 3 and 9
(STAGE 2-2 and 5-2, areas 4 and 12) match in the first 16 metatile columns and
in the payload columns 160-255, but not in columns 16-159. Areas 10 and 17 have
**no** level index (they are not in `$D94E`), so `$20` cannot reach them.

**Stages 2-2 and 5-2 are one-screen maps, and an actor clears the rest
(oracle, 2026-10-14).** The load does finish. The oracle forces `$20` at `$D3D8`
the same way the fceux probe does, and reproduces that probe's
`tools/emu/stagearea-probe/ts-3` and `ts-9` dumps exactly. At every dump frame
from 514 to 794 both WRAM and RAM differ in 0 bytes. Watching writes to
`$6000-$6FFF` then shows the whole sequence, with the PC and PRG bank of each
writer:

1. **The stream is one screen long.** Its header count is 136 bytes for area 4
   and 139 for area 12; a full stage has 815-1709. The decompressor (bank 2
   `$8AA1`/`$8AEF`) writes only 256 bytes, which are metatile columns 0-15.
   Areas 10 and 17 are the same kind of stream (179 and 182 bytes).
2. **The transpose fills the rest with leftovers.** The transpose (bank 2
   `$8B3F`, frames 592-594) fills columns 16-255 from whatever `$7100-$7FFF`
   held at load time. `map_from_rom` assumes that is the CHR payload; at this
   moment it is not, which is why columns 16-159 differ. Columns 0-15 equal
   `map_from_rom(area)`.
3. **An actor spawns with the stage.** At frame 601 the spawn routine (bank 14
   `$D7A8`) fills slot 0 with type `$54` and slot 1 with type `$56`.
4. **Actor `$56` clears the rest of the map.** Its handler is bank 11 `$9DF9`
   (`LDA $7DD0,x; CLC; ADC #$10; STA $7DD0,x; STA $11`, then
   `STA ($11),y` at `$9E16` over 16 rows). It writes metatile 0 into 16 columns
   per frame, columns 16-255, during frames 635-649. When `$7DD0` wraps to 0 it
   frees its slot (`STA $7CC0,x`).

So once the stage is running, these two maps are their 16 decoded columns
followed by 240 columns of metatile 0. This comes from the stage's own spawn
data, not from forcing `$20`. Forcing `$0400 = 8` directly (after
`STA $0400` at `$D3E0`, with `$20 = 0`) spawns the same clearing actor, so the
spawn follows the area. A full stage gets no such actor: level 2 (area 3)
equals `map_from_rom(3)` on all 4096 bytes, and nothing writes from `$9E16`.

Areas 10 and 17, forced the same way, keep their decoded columns 0-15 and get
no clearing actor, so columns 16-255 stay as the transpose left them.

What type `$54` is and what the stage looks like is still open. A one-screen
arena is a guess.

Tests: `tools/rotj-oracle/tests/levels.rs`,
`stages_2_2_and_5_2_are_one_screen_maps_the_level_actor_clears` and
`a_full_stage_keeps_its_map`.

**What is still a lead.** The game *entering* `$D3D8` on its own only happens on
the power-on/reset path (bank 15 `$E35C` clears RAM `$00-$FF`, hence `$20 = 0`,
then `JMP $D394`); every captured run has `$20 = 0` at the read. So the table is
proven for its 13 rows, and the banner proves the game treats the index as a
stage-sublevel counter, but no capture yet shows the *progression* that advances
`$20` to another level during play. `$20` is also reused as a zero-page scratch
pointer elsewhere (bank 14 `far_D714` scans actor slots with `INC $20`), so only
its value at `$D3D8` is the level index.

`levelmap::area_for_level` / `map_for_level` encode this table and are tested
against a committed slice of level 1 (`tests/data/level1-map-slice.bin`, STAGE
1-2, area 1) as well as level 0.

**`$0401` is not a sublevel index.** `sub_B40A` (bank 13) builds it from the CHR
bank shadow: `LDA #$00; STA $0401; LDX #$03; loop { LDA $E0,x; ASL; ROL $0401;
… }` (disasm/bank0D.s `$B47F`). `bank0E:sub_D2B0` then reads it as a 4-bit
field to index the 16 actor slots. Stage 1-1's `$0401 = 8` is therefore an
artifact of the CHR banks in use, not a stage-local level number.

**Still unknown:**

- What advances `$20` during normal play (the table above is entered once, on
  the reset path): no capture yet leaves level 0 on its own.
- Whether level 7-2 exists: the table has 13 entries, covering STAGE 1-1..7-1
  only, so the final sublevel (or boss) is loaded by another path.

## 14. Palettes and gameplay CHR banks (decoded)

A level's palette RAM (`$3F00-$3F1F`) and its gameplay CHR banks (FME-7 commands
0-7, shadow `$88-$8F`/`$E0-$E7`) both come from one per-**(area, stage)** record.
`rooms::Room::import` reads the chain's selectors and `actor::room::build_palette` resolves
them; this section records the
addresses and the acceptance.

### 14a. The record

`sub_D3D8` (bank 14, CPU `$D3D8`) sets `$0400 = $D94E[$20]` (§13e) and the level
init calls `sub_B40A` (bank 13, CPU `$B40A`). `sub_B40A` reads two tables:

1. **`$C048`** (bank 14, 18 x 16-bit LE, indexed by `$0400`) points at a
   per-area **stage-pointer table** in bank 0. `LDY $0402; LDA ($20),y; INY;
   LDA ($20),y` (`$B42C`-`$B436`) gives the record address from that table's
   `$0402`-th 16-bit entry.
2. the **7-byte record**: byte 0 is the palette `Y` selector, bytes 1-4 are the
   CHR banks `$E0-$E3` **as stored**, byte 5 is `$DD`, byte 6 extra
   (`$B448`-`$B476`).

The area-0 tables start at bank 0 `$8020` (pointers) and bank 0 `$8060`+
(records). **While `sub_B40A` runs the `$8000` window holds bank 0** — verified
because the `$C048` pointers land on bank 0's data (`tools/emu/palettes-trace/`,
`PALETTE_DEBUG`); the `$E9` shadow at that moment reads `$1C`, whose bank is
therefore a lead, not used here.

### 14b. The palette (`far_C367`)

`sub_B40A` calls `far_C367` (bank 14, CPU `$C367`) with `Y` = record byte 0 and
`X` from a **bank 13** table at `$B5F5` (18 x 16-bit, indexed by `$0400`), at
index `(stage + 1) / 2` (`$B438`-`$B447`). `far_C367`:

- for each of the 8 palettes (x = `$0E, $0C, … $00`), reads a selector byte from
  the table `$C3A6[x]` (pointers to `$C3B6`, `$C3E1`, …), indexed by `X` when
  `x >= 8` and by `Y` otherwise;
- colours 1-3 = `$C4BA[sel]`, `$C522[sel]`, `$C58A[sel]`.

Colour 0 of all 8 palettes is forced to `$0F` (bank 14 `sub_D481`, `$D481`;
bank 13 `$B45B`).

**The gameplay palette is the base palette.** The transition fades it with
`far_C30A` (bank 14, CPU `$C30A`): bank 2 `$9500`-`$9539` starts `$040C = $40`
and decrements it by `$10` every 16 frames, calling `JSR $C30A` (`$952A`) with
`A = $040C` (brightness `$32`) and `X = $FF` (mask `$30`), which replaces high
nibbles downward while the banner runs. At frame 624 of the probes the fade is
still in progress for most levels; by frame 744 every level's PPU palette equals
the base `$04E0`, and the fade at full brightness is the identity, so
`room::build_palette` builds the base.

### 14c. The CHR banks

`sub_B40A` writes record bytes 1-4 to `$E0-$E3` (`$B468`-`$B476`). A stored
value `>= $80` is a **handle**: `$B485`-`$B4C6` reads `$C06C[(handle<<1) & $7F]`
(bank 14) for a group record in bank 0, copies the group's bank list, and
replaces `$E0,x` with the **last** byte of that group. Banks `$E4-$E7` are fixed:
`$E4 = $A0` (bank 13 `$B22A`), `$E5-$E7 = $F5-$F7` (the boot default, bank 2
`$98CB`). `$E5` is swapped by Batman's animation later in play, so the level's
captured set is the frame-624 one.

### 14d. Acceptance (measured)

`actor::oracle::tests::every_level_settles_to_its_captured_chr_and_palette` (the whole-game
chain checks every room load)
decodes all 13 levels and compares against the committed captures
(`tools/emu/stagearea-probe/lvl-<n>/`). For **every** level 0-12:

| what | fixture | result |
| --- | --- | --- |
| palette | `ppu-744.bin` `$3F00-$3F1F` | **0 / 32 differ** |
| CHR banks | `ram-624.bin` `$E0-$E7` | **0 / 8 differ** |

The committed captures are
`crates/rotj-core/tests/data/level-palettes.bin` (13 x 32) and
`level-chr.bin` (13 x 8); the PRG table slices the decoder reads are
`prg-bank00-tables.bin`, `prg-bank0D-tables.bin`, `prg-bank0E-tables.bin`, and
the test places them at their bank offsets in a 128 KiB image.

### 14e. Scope / leads

- Only **stage 0** is decoded: every probe ran with `$0402 = 0` (`sub_D48D`
  writes it; the game's own stage advance is not captured). The per-area tables
  have several stage entries and the `X` selector varies with `$0402`, so the
  stage dependence of the palette is **unverified**.
- The `$E9` shadow `$1C` during `sub_B40A` (which should name the `$8000` bank)
  does not match the bank the pointers resolve in; recorded as a lead above.
- The record's byte 5 (`$DD`) and byte 6 are not decoded (they feed actor/script
  state, §5).

## 15. Building a level from the ROM (the app's `Stage::for_level`)

Stage 1-1 in the app is built from the ROM alone: `Stage::load()` is
`Stage::for_level(0)`. `Stage::for_level(level)` builds any level the ROM holds
from the decoders of §13-§14:

- **map** — `map_for_level` (§13c) gives the metatile map.
- **auto-tile tables** — `rotj_core::levelmap::auto_tile_tables` decodes the
  auto-tiler's tile pass (bank 02 `$8B9E`, jump table `$8C1D`): its 19 handlers
  (`$8C43`-`$8E11`) turn the CHR area definition's per-metatile bytes into the
  four `$7000`/`$70D0`/`$71A0`/`$7270` tables that `render_nametable` needs. The
  walk to each id's bytes is the same one `attributes_from_rom` makes to skip
  them. Reproduced byte-for-byte (ids `< N`) for every level's capture
  (`auto_tiler_reproduces_the_captured_tile_tables`: area 0 vs `collision.bin`,
  area 1 vs `stage-1-2/reference/tile-tables-744.bin`). This decoder lived in the
  app until it moved to `rotj-core` next to `attributes_from_rom`.
- **attributes** — `attributes_from_rom` (§13d).
- **palette and CHR banks** — `rooms::Room` with `room::build_palette`/`room::animate_banks` (§14),
  fed the ROM's PRG (`assets/prg.bin`, written by `rotj prg`); the CHR itself
  is the existing `assets/graphics/chr/pattern-space.bin`.

The four tile tables overlap (stride `$D0`), so they span `$7000-$736F`. Their
entries past an area's id count `N` are not stale from the previous level: the map
loader `$89FE` decompresses into `$7000` before it transposes into `$6000`, so
those entries keep the stream's bytes (`Level::from_rom` reads them back from the
map). The same holds for every `$7000-$7AFF` byte no later pass writes
(`$7510-$752F`, `$7570-$75FF`, `$7A00-$7AFF`), and map cells past the stream's
reach (`levelmap::map_reach`; stage 1-2's stream stops at `$7B00`, so columns
176-255) get the transposed slot arrays `$7B00-$7FFF` (oracle, TAS frame 2150;
`docs/ACTORS.md` §9l).

### 15a. Per-level results (captures `tools/emu/stagearea-probe/lvl-<n>/`)

`for_level(n)`'s nametable/attributes and background, against each capture's
`ppu-744.bin` at the camera in `ram-744.bin` (`$00AC`), 960 tile bytes + 64
attribute bytes; background pixels are the emulator's own `ppu-744.bin` rendered
by `tools/rotj-verify --no-sprites` (the screenshot carries sprites and is
mid-fade, see below).

| lvl | area | nametable vs capture | bg pixels (of 61440) | cause of the mismatch |
| --- | --- | --- | --- | --- |
| 0 | 0 | 0/960 + 0/64 | 5073 | animated top CHR bank only (0 once set to the ROM's load-time bank) |
| 1 | 1 | 59/960 + 7/64 | 2584 | loader nametable patch (§15b) |
| 2 | 3 | 0 | 0 | — |
| 3 | 4 | 209 + 23 | 9681 | capture nametable is at scroll 0, not `$00AC`=88 (0 at left 0) |
| 4 | 5 | 0 | 22 | — |
| 5 | 6 | 0 | 113 | — |
| 6 | 8 | 36 + 4 | 1657 | capture nametable at scroll 0, not `$00AC`=17 (0 at left 0) |
| 7 | 9 | 0 | 0 | — |
| 8 | 11 | 0 | 3862 | animated top CHR bank only (0 once set to the load-time bank) |
| 9 | 12 | 298 + 14 | 11060 | atom above; partial load (§13e) |
| 10 | 13 | 787 + 42 | 34035 | capture nametable at left 128, not `$00AC`=0 (0 at left 128) |
| 11 | 14 | 303 + 9 | 9460 | capture nametable best at left 56 (48 residual), `$00AC`=185 |
| 12 | 16 | 0 | 8625 | animated bank + 1324 px patch |

Levels 0/2/4/5/7/8/12 build and render exactly at their capture's camera.
Levels 3 and 9 never finish loading (their map's columns 16-159 are missing,
§13e), yet the visible 32 columns are intact and match at scroll 0.

### 15b. Leads (reported, not fixed)

- **The animated top CHR bank.** `$E3` (PPU `$0C00-$0FFF`, the background's
  fourth 1 KiB window) is not a level constant: the game cycles it through the
  slots of CHR 8 KiB bank 6 every frame (`tools/emu/drive` `$E3` =
  `$37`/`$36`/`$35`/`$30`/`$32`/`$34` at frames 600-1000; `lvl-8` `$63`->`$62`,
  `lvl-0` `$37`->`$33`). `rooms::Room::chr` holds the ROM record's
  *load-time* bank. The app models the animation (`frame::animate_chr`,
  §16), so a frame compared against a capture is advanced to that capture's own
  counters; the load-time `for_level(0)` differs from the frame-700 capture by
  3394 px, and from an otherwise-identical frame only in that window.
- **The loader's nametable patch.** The emulator's stage 1-2 screen holds cells
  the metatile map cannot explain: the *same* metatile id shows different tiles
  at different columns, so it is neither the map nor the tile tables. The loader
  applies a nametable write-list (bank 02 `$91B7`/`$925A`) that is still
  undecoded; it leaves 59 tiles + 7 attribute bytes different in the level-1
  capture (§15a).
- **`$00AC` is not the probe captures' scroll.** For several forced levels the
  nametable in `ppu-744.bin` matches the map at a *different* left column than
  `$00AC / 8` (level 6/3/9: left 0; level 10: left 128; level 11: left 56). The
  mismatch at `$00AC` is that offset, not the decoder.
- **`lvl-<n>/screen-744.png` is not the settled frame.** Its colours are darker
  than the base palette in `ppu-744.bin` (still fading) and it carries Batman and
  the HUD, so the pixel comparison uses the PPU dump rendered by `rotj-verify`.
- **Level 1-2's derived collision.** The derived rule floods from the ground band,
  so it marks solid tiles high above the actor's start too (x112, y60 is solid for
  level 1-2 but not for 1-1). Whether the game blocks there is unverified; the
  rule is unchanged here.

### 15c. A rendering bug this surfaced

`Stage::compose` passed only the camera's low 3 bits as the renderer's scroll
while handing it the game's 32-column ring. The ring's slot 0 is *not* the
visible left column, so the picture was shifted by `camera / 8` tiles (measured:
6 tiles = 48 px at camera 50, 0 after the fix). Only camera 0 (where coarse and
fine coincide) had been pixel-tested, which hid it. `compose` now passes the
whole camera; stage 1-1's frame matches the emulator exactly at cameras 0, 50 and
54 (`stage::tests::scrolled_frames_match_the_emulator` composes `Stage::load()` at
cameras 50 and 54 with each frame's captured OAM and asserts 0 differing pixels;
reverting the scroll to `camera.x.rem_euclid(8)` fails it at camera 50 with 12470
mismatches).

The scrolled test's reference is the emulator's own frame rendered by
`tools/rotj-verify` with the level's captured CHR banks, rather than the raw
screenshot: at a scrolled camera the screen carries a horizontal split the port
does not model (e.g. `walk` 940's y32-96 band is 32 px off a single-scroll
render), so no raw screenshot past camera 4 reproduces at 0 differences.

### 15d. Stage 1-1 in the app is built from the ROM

`Stage::load()` is now `Stage::for_level(0)`: the app's stage 1-1 picture and
collision come entirely from the ROM decoders (§13-§14, the auto-tiler above).
The captured-asset path for stage 1-1 is retired; nothing under
`assets/levels/stage-1-1/` is read at run time.

Two things the port does not model from ROM are supplied by each test from the
capture it compares against, exactly as `scrolled_screenshots_match_the_emulator`
does:

- **the background CHR animation** — `frame::animate_chr` is driven to the
  capture's own `$043E` tick / `$043F` index (`stage::tests::advance_to`), so the
  rendered `$E3` bank is the one the emulator had (§16);
- **the sprite CHR windows** (`$E4-$E7`, PPU `$1000-$1FFF`) — the port only models
  Batman's window (`SpriteWindow`), so these are set to the capture's latched banks
  (`stage::tests::set_sprite_banks`).

Pixel tests, all exact with the animation state above:

| test | reference | result |
| --- | --- | --- |
| `opening_frame_matches_the_emulator` | `stage` frame 700 (`reference/emulator-700.indices`) | 0 / 57,344 |
| `scrolled_frames_match_the_emulator` | `rotj-verify` render of `stage` frame 700 pattern, cameras 50/54 | 0 |
| `scrolled_screenshots_match_the_emulator` | raw screenshots, six frames (§16d) | unchanged (§16d) |

Captured files kept as decoder/test references (not read at run time):
`collision.bin` and `nametable-drive-*.bin`/`nametables.bin` (map and nametable
decoders), and the `reference/` pixel/OAM/physics fixtures. Removed:
`pattern.bin` and `palette.bin` — their content is covered by `rotj-core`'s
`tests/data/level-chr.bin` / `level-palettes.bin` (level 0, frame 744/624 of
`tools/emu/stagearea-probe/lvl-0/`) and, for the animated window, by the
screenshot test above; nothing read them after `load()` stopped using the
captured stage.

## 16. Parallax: the background CHR animation (decoded)

The "parallax" is **CHR-bank animation**, not a raster split. The top background
band (`$E3`, PPU `$0C00-$0FFF`) is one of the four `$E0-$E3` CHR slots the game
cycles every frame; the pattern in that window moves, so scrolled screenshots
show the band changing on its own. It is *not* an FME-7 IRQ raster program (see
`docs/STATUS.md` "Retracted findings"); no raster model is reintroduced here.

### 16a. The rule (`far_D2B0`, bank 14 CPU `$D2B0`)

Decoded from the disassembly (`disasm/bank0E.s` `far_D2B0`, verified by
`disasm/gen/read_range.py 14 0x12B0 --window 0xC000`) and reproduced by the
oracle (`tools/rotj-oracle/examples/bg_chr.rs`):

- `sub_B40A` (`$B40A`, §14c) writes record bytes 1-4 to `$E0-$E3` and sets
  `$0401`: bit `Y` is `$E0+Y` bit 7 (`LDA $E0,x; ASL; ROL $0401` over `X=3..0`).
- `far_D2B0` runs once per gameplay frame. For each slot `Y` whose `$0401` bit
  is set, with `X = 4*Y`:
  - `INC $0432+X`; if `$0432+X < $0430+X` (period) it does nothing;
  - else `$0432+X = 0`, `INC $0433+X`; if `$0433+X >= $0431+X` (count) it wraps
    to 0;
  - reads `$0410 + 8*Y + $0433+X` and stores it to `$E0+Y`.

So a slot's CHR window is `sequence[frame_index]`, where the index advances once
every `period` frames (its register starts at 0 while the *load* bank is the
sequence's **last** entry, so the first stored bank is `sequence[1]`).

The record itself is the same handle the CHR loader resolves (§14c): byte 0 is
the count, byte 1 the period, bytes 2.. the bank sequence, in the group record at
`$C06C[(handle<<1) & $7F]` (bank 0; `$814A` for stage 1-1's `$E3`). The loader's
resolved bank (`resolve_chr`) is that sequence's last byte.

### 16b. Evidence

- **Oracle, `oracle-route`, PC `$D2EA`** (`STA a:$00E0,y`): `$E3` is written every
  two frames from frame 642, cycling `31 32 33 34 35 36 37 30` (frames 642-656
  then repeat); the load write is `$37` at frame 606. The slot's RAM record reads
  `$043C=02` (period), `$043D=08` (count), `$0428-$042F=30..37` (sequence),
  `$0432/$0433` the tick/index.
- **The captures agree**: every gameplay dump's `ram-*.bin` has the same
  `$0428-$042F`/`$043C-$043F`, and `ppu-*.bin`'s `$0C00-$0FFF` equals
  `pattern-space.bin[sequence[$043F]*1024..]` (`pages` 816, `walk` 940,
  `route-dense` 910-934, scroll 54-246; 0 mismatching bytes).
- **Stage 1-1 (`area 0`)** animates only slot 3: `$0401 = 8` (bit 3), so
  `$E0-$E2` (`$2C/$2D/$2E`) are static and `$E3` cycles period 2, count 8.
  `room::animate_banks` for area 0 loads exactly
  that.

### 16c. The port

`room::animate_banks` decodes the
records. `crates/app/src/stage.rs` keeps the per-slot animation (tick, index) and
`frame::animate_chr` applies the rule above, rewriting the slot's CHR
window from `pattern_space`; the app advances it once per drawn frame
(`draw_stage`). `Stage::background_state(slot)` exposes the game's `$0432/$0433`.

`shortcut:` the game skips `far_D2B0` during its level transition (first call
~35 frames after the load), so the shipped port starts advancing when gameplay
starts and can be a transition's worth of ticks behind until that is modelled.

### 16d. Acceptance — the emulator's real screenshots

`crates/app/src/stage.rs::scrolled_screenshots_match_the_emulator` builds stage
1-1 from the ROM (`Stage::for_level(0)`), advances the background animation to
the captured frame's own counters (`$043E` tick, `$043F` index, read from the
capture's `ram-*.bin`), sets the sprite CHR windows from the frame's latched
banks, and composes with the frame's OAM. Each reference is the emulator's
actual screenshot (`shot-<f>.png` / `screen-<f>.png`) reduced to its NES
colour-index plane (`tools/emu/compare.py`'s mapping), cropped 8 lines top and
bottom like the other pixel tests, under
`assets/levels/stage-1-1/reference/background-<capture>-<frame>.indices`.

The chosen frames are settled-camera (so `$00AC` is the scroll the screen used)
at two cameras from two captures:

| capture | frame | camera `$00AC` | tick/index | port vs screenshot |
| --- | --- | --- | --- | --- |
| `route-dense` | 916 | 246 | 1 / 1 | **557** / 57,344 |
| `route-dense` | 922 | 246 | 1 / 4 | **560** |
| `route-dense` | 926 | 246 | 1 / 6 | **571** |
| `route-dense` | 934 | 246 | 1 / 2 | **552** |
| `pages` | 816 | 54 | 1 / 7 | **388** |
| `walk` | 940 | 54 | 1 / 5 | **389** |

The port's frame equals the emulator's own PPU state rendered by
`tools/rotj-verify` for every one of these (same counts; the background alone is
byte-identical, checked with `--no-sprites`). So the residual is **not** the CHR
animation and **not** a horizontal split: `tools/emu/fit_shift.py` reports best
shift `(0,0)` for both the top (y0-120) and main (y120-224) bands, i.e. no
per-band offset improves it. The differing pixels sit at sprite edges and a few
left-edge/`y64-80` pixels where the emulator's screenshot (the framebuffer, which
lags its PPU state) shows background and a single-scroll render shows content.

**Split code check (negative).** All gameplay writes to the scroll/address
registers `$2000/$2005/$2006` occur in the NMI handler (bank 15 `$E228-$E254`) and
its VRAM flusher (bank 14 `$C094-$C0B6`) — traced with the oracle over
`oracle-route` frames 640-1000 (`tools/rotj-oracle/examples/bg_split.rs`); there
are no main-loop writes. The port's one-scroll-per-frame model therefore matches
the game's own writes, and no raster split is needed to explain these frames.

### 16e. Leads

- **The earlier "y32-96 band is 32 px off" split is not reproducible on these
  frames under a shift** (`fit_shift` best `(0,0)`); the top band's sparse,
  repeating sky makes band-shift fits unreliable (the retracted finding's
  warning). It may be the emulator screenshot's framebuffer lag rather than a
  game-side split.
- **Sprite CHR pairing.** The PPU dump's sprite windows `$E4-$E7` do not always
  equal the RAM shadow: at `route-dense` 916-934, `ram $E6=$F6`/`$E7=$F7` while
  the dump holds `$EF`/`$AA`. The test uses the frame's *latched* windows to
  isolate the background; how the game writes `$E6/$E7` (Batman/enemy animation)
  is the sprite system's task, not this one.
- The animation is proven only for `area 0` / stage 1-1; other areas' records
  decode but are untested against captures.



