# Batman: Return of the Joker (USA) — disassembly map

Hard-won facts from the recursive-descent / FME-7 work in `gen/generate.py`.
Every address here was read out of the actual ROM bytes; claims that are only
inferred are marked **(inferred)** or **(lead, unverified)**. `$XXXX` in the
switchable windows (`$8000`/`$A000`/`$C000`) is a CPU address *as seen when the
named bank is mapped there*; the bank file / offset is given alongside.

Regenerate with `python3 gen/generate.py && sh assemble.sh`.

## 1. Bank map

Cart: mapper 69, Sunsoft FME-7, 128 KiB PRG (16 x 8 KiB), 256 KiB CHR.

FME-7 command/parameter ports: write command to `$8000-$9FFF` (low nibble
selects), then the parameter to `$A000-$BFFF`.

| cmd | meaning | evidence |
| --- | --- | --- |
| `$0-$7` | CHR 1 KiB banks for PPU `$0000/$0400/.../$1C00` | NMI CHR loop at `$E204`: `LDX #$07; STX $8000; LDA $88,x; STA $A000; DEX; BPL` — writes 8 banks from RAM `$88-$8F` |
| `$8` | PRG bank at `$8000` | reset `$E337` writes cmd `$08` = `$1B` |
| `$9` | PRG bank at `$A000` | NMI `$E28C` writes cmd `$09` from RAM `$E9` |
| `$A` | PRG bank at `$C000` | NMI `$E297` writes cmd `$0A` from RAM `$EA` |
| `$B` | PRG bank at `$6000` | reset `$E337` loop |
| `$C` | mirroring | reset `$E35C`; per-frame loop `$E259` writes `$EC` |
| `$D` | IRQ control (`$81` = enable) | NMI `$E1F8`, helper `$E1A5` |
| `$E`/`$F` | IRQ counter low/high | helper `$E1C0` |
| `$10`? | (none; high nibble ignored) | — |

The game keeps a **mapper-register shadow in RAM `$E0-$EC`** and rewrites all
of cmd `$0..$C` every frame in the NMI "slow" path (`$E259-$E26A`):
`LDA $E0,x; STA $A000` for `X=$0C..$00`, with `STA $88,x` copying `$E0-$E7`
into the fast CHR shadow `$88-$8F`.

### Default (reset) runtime windows

Observed at reset `$E337` (bank 15 offset `$0337`):

```
cmd $08 -> $8000 = PRG bank 11 (0x0B)     cmd $09 -> $A000 = PRG bank 12 (0x0C)
cmd $0A -> $C000 = PRG bank 13 (0x0D)     cmd $0B -> $6000 = PRG bank 14 (0x0E)
```

The values are stored to RAM `$E8-$EB` and re-applied by NMI. (Bank number =
written value `& $0F`, since a 128 KiB ROM connects only 4 bank lines.)

`$E000-$FFFF` is fixed to the last bank (15 / `0x0F`) — reset/NMI/IRQ live there.

### Per-bank code window (run space)

`rotj.cfg` gives each PRG bank a "run" window so in-bank labels resolve. The
table below is derived from each bank's own in-window absolute references
(a self `JMP/JSR` whose target is an instruction boundary inside the same bank);
it is a naming choice, the FME-7 places banks wherever the stage tables say.

| bank | window | decisive evidence |
| --- | --- | --- |
| 00 | `$8000` | many in-bank `$80xx` refs |
| 01 | `$A000` | in-bank `$Axxx` refs |
| 02 | `$8000` | `JMP $8F2B` at offset `$0F49` = the loader loop at `$0F2B` (self) |
| 04,05 | `$8000` | strong in-bank `$80xx` refs |
| 08 | `$8000` | strong in-bank `$80xx` refs |
| 0A | `$8000` | audio API `JMP $800C/$8020/$80B2/$8217` all self; `LDA $8B4D,y` self |
| 0B | `$8000` | dispatcher at `$8009` reads table `$81D7/$81DF` (self) |
| 0C | `$8000` | strong in-bank `$80xx` refs |
| 0D | `$A000` | `JMP $B29D`/`JSR $B2BA` at offsets `$129D`/`$12BA` (self) |
| 0E | `$C000` | strong in-bank `$Cxxx` refs |
| 0F | `$E000` | fixed bank |

Banks 03/06/07/09 have no decisive self-reference; they keep the historic
`bank % 3` rotation. **(inferred)**

### Observed switch sites

Literal writes to the switch ports found by scanning (command, then parameter):

- `$E337-$E346` (bank 15, reset): loop sets `$08..$0B` = `$1B..$1E`.
- `$E204` (NMI): CHR banks `7..0` from `$88-$8F`.
- `$E259` (NMI): cmd `$0..$0C` from `$E0-$EC` (mapper shadow).
- `$E27C` (NMI): `LDA #$0A; LDX #$09` → maps bank 10 into `$A000`, then
  `JSR $8009` (bank 11 dispatcher), then restores `$A000 = $E9`.
- `$E28C`/`$E297` (NMI): restore `$A000`/`$C000` from `$E9`/`$EA`.
- bank 0A `$A0E8`/`$A23F`: `LDA #$0A; STA $8000` (cmd `$0A` → `$C000`) +
  `LDA $0600; STA $A000` — maps a stage-selected bank; used for object/level
  pointer tables.
- bank 02 `$8A06`/`$8BA1`: `LDX #$00; STX $8000; STA $A000` — select CHR 1 KiB
  bank 0 = `A` (loaded from a table at `$8B63,x`).
- bank 02 `$98AD`: cmd `$0E` (IRQ counter).
- bank 02 `$8F30`, bank 0E `$C02B`, others: cmd `$0E/$0F` IRQ counter.

Stage-specific window values are loaded from RAM tables and **were not fully
traced** (see §9).

## 2. Interrupts

Vectors at `$FFFA` (bank 15 offset `$1FFA`):

```
$FFFA NMI   = $E1D0    $FFFC RESET = $E2E6    $FFFE IRQ = $E2A9
```

### RESET — `$E2E6`

`SEI; CLD; LDX #$FF; TXS; INX; STX $2000`; two-vblank PPU warmup; disable APU
DMC/`$4016`; `$4015=$0F`, `$4017=$C0`; clear PPUCTRL/PPUMASK; `$FC/$FD=0`;
program FME-7 (`$0D=0`, mirroring `$80`, `$0D=0`); `CLI`; zero page `$00-$FF`
clear loop; **set default windows** (`$E337`); `JSR $C117` (bank 13);
`$0402 = $01`, `$0401 = $02` (sublevel/stage); mirroring cmd `$0C` = `$03`
(single-screen); `$F9/$FA = $49/$2F`; `$0180/$0181 = $FF/$7F` (stack-restore
pointer used by the display-list executor); `JMP $D394` (bank 13 main loop).

### NMI — `$E1D0`

1. Save `A/X/Y` to `$F2/$F3/$F4`.
2. Disable FME-7 IRQ (cmd `$0D` = 0).
3. If `$D0` bit7 set: program IRQ counter low `$D1`, high `$D2` (cmd `$0E/$0F`)
   and enable IRQ (cmd `$0D` = `$81`).
4. If `$F0` bit7 **clear** (render path): NMI re-arms the split IRQ, writes CHR
   banks `$88-$8F` (cmd `$7..$0`), then `PPUSCROLL = $AC,$AD`, `PPUCTRL = $AB`,
   `JMP $E27D`. If `$F0` bit7 **set**: OAM DMA (`$2003=0; $4014=2`),
   `PPUCTRL=$FF`, `PPUMASK=$FE`, `JSR $C084` (bank 13), `INC $F1`,
   `EOR #$80; STA $F7` (blink/anim toggle), status read,
   `PPUSCROLL = $FD,$FC`, `PPUCTRL=$FF`, rewrite mapper shadow `$E0-$EC`,
   `JSR $C1DB` (bank 13), then `$AC=$FD; $AD=$FC; $AB=$FF`, `$F0=0`.
5. `$E27D`: `LDA #$0A; LDX #$09; STA $A000` (temporarily map bank 10 to
   `$A000`), `JSR $8009` (bank 11 object dispatcher), restore `$A000=$E9`,
   `$C000=$EA`.
6. `$E29D`: `LDA $DF; STA $8000` (pre-set command latch), restore `A/X/Y`, `RTI`.

### IRQ — `$E2A9`

```
save A/X/Y; disable IRQ ($0D=0)
LDA $D3; ASL; TAX; LDA $E2CA,x -> $08; LDA $E2CB,x -> $09; JMP ($0008)
```

`$E2CA` is a **split dispatch table** (14 x 16-bit): `DDF4 DE8F DF1D DF3D
DFE4 E001 E07E E0EE E16B 95C9 9731 97C9 97DD 9720`. The `$DDxx/$DFxx` entries
are in **bank 13** (window `$C000`), `$E001/$E07E/$E0EE/$E16B` in the fixed bank
and `$95C9…` in the `$8000` window (bank 11 by default). Only the entries that
begin on a clean instruction boundary are real handlers; `$DFE4` points at `$FF`
padding and the `$DF3D` entry is mid-code, so not every slot is used.

## 3. Mid-frame scroll / CHR "parallax" mechanism

The game fakes parallax with an **FME-7 IRQ counter** armed in NMI. The counter
fires mid-frame and the IRQ handler:

- **bank 15 `$E001`**: `LDA $D4; ASL; TAY; ...; JMP ($0008)` through table
  `$E076` (`E012 E02C E046 E057`). These entries set scroll/PPUCTRL and re-arm
  the IRQ counter via `$E1C0` (`LDA #$00; LDY #nn`) and `$E1A5` (`$D4=A`, cmd
  `$0D`=`$81` enable).
- **bank 15 `$E07E`**: same but through table `$E0EA` (`E08F E0B0`):
  - `$E08F`: `PPUADDR = $2620`, `PPUSCROLL = $FD,0`, `LDY #$07; JSR $E1C0`,
    re-arm with handler `$01`.
  - `$E0B0`: `PPUCTRL |= $02`, adjust vertical scroll accumulator `$D5` by the
    direction bit in `$D6.bit7`, `$FC = $D5+$50`, re-arm with handler `$00`.
- **bank 15 `$E0EE`**: sets `PPUSCROLL` from `$D6`, re-arms the counter.
- **bank 13 `$DDF4`**: fill `$04E0-$04FF` with `$0F` (scroll/OAM staging).
- **bank 13 `$DE8F`**: sets `$FE` (`PPUMASK`), `JMP $C232` (camera math).
- **bank 13 `$DE94`**: clear nametable `$2000..$23FF` via `$2006/$2007` (X=0,Y=8).
- **bank 13 `$DF1D`**: fill `$04E0-$04FF` = `$30`, `JMP $C34B`.
- **bank 13 `$DF28`**: copy/merge `$04E0-$04FF` into `$04A0-$04BF` with a
  scroll offset `$30` derived from `$0460` (per-band scroll buffer), `JMP $C33C`.

IRQ control helpers (fixed bank):
`$E1A5`: `STA $D4; LDX #$0D; LDA #$81; STX $8000; STA $A000; JMP $E29D` — arm
split with handler index in `A`.
`$E1C0`: `LDX #$0E; STX $8000; STA $A000; INX; STX $8000; STY $A000; RTS` —
set IRQ counter low `A`, high `Y`.
`$E1B4`: `LDX $2002; STA $2005; LDA #$00; STA $2005; RTS` — set scroll X=A,Y=0.

IRQ state RAM: `$D0` (bit7 = arm split), `$D1`/`$D2` (counter), `$D3` (handler
index), `$D4` (sub-handler index), `$D5`/`$D6` (vertical accumulator/dir).

## 4. Video / PPU update engine

- **Generic VRAM display-list executor** — bank 0E `$C080-$C0C5` (window
  `$C000`): pops `[hi,lo,count,bytes...]` off the stack and writes `$2006`/
  `$2007`; special-cases `$3F` (palette) by re-writing the address. Entry
  `$C0B9`; push helper `$C0C6`. This is how nametables **and palettes** reach
  the PPU during vblank.
- **Per-scene PPU write lists** (same `[hi,lo,count,bytes..]` format,
  `$FF`-terminated), executed directly:
  - bank 02 `$91B7` (offset `$11B7`) — nametable list used by the level loader
    (6 entries, ends `$9203`). `; tbl_91b7_ppulist` in `bank02.s`.
  - bank 0C `$9BEB` (offset `$1BEB`) — nametable list (8 entries, ends `$9C35`)
    with the routine at `$9B59`. `; tbl_9beb_ppulist`.

## 5. Level / nametable data

- **Level loader**: bank 02 offset `$0F2B` (`$8F2B` when bank 02 is at
  `$8000`). It walks the compressed PPU list at `$91B7` (`LDA $91B7,x` ->
  `STA $2006` x2 -> run-length `$2007` copy, terminate on bit7), then builds the
  **collision/level tilemap in RAM `$6000`** from tables at `$9204` (offset
  `$1204`) and `$925F`/`$926B` (row/column tile definitions). See
  `bank02.s` around `$8F2B` and `$9155`.
- Tile-id staging buffers `$7000`, `$70D0`, `$71A0`, `$7270` (copied to PPU by
  the tilemap renderer in bank 0D `$D2BA`).
- Candidate per-sublevel tables also exist at bank 0E offset `$0F72` (104 x
  16-bit pointers into bank-window `$8000`) and bank 04 offset `$1924`
  (153 x 16-bit pointers into `$A000`). **(inferred pointer tables)**

## 6. Palette tables

Palettes reach the PPU through the display-list executor (§4), so there is not
one obvious "palette loader". Palette-shaped data blocks found (all bytes
`<$40`, several `$0F`):

| location (bank file offset) | size/shape | note |
| --- | --- | --- |
| bank 0C offset `$1C36` (`$9C36`) | 32 B, `0F 0F 30 30 ...` | candidate palette block next to the `$9BEB` list |
| bank 0C offset `$1C6A` (`$9C6A`) | 60 B ramp `22 32 04 14 24 34 ...` | colour ramp / conversion table |
| bank 08 offset `$1DC2` (`$9DC2`) | 16 B, `0F 0F 30 30 0F 0F 27 30` | candidate palette |
| bank 0E offset `$04B8` (`$C4B8`) | 128 B, palette-valued | candidate multi-palette block |
| bank 02 `$D155` (offset `$1155`) | routine | copies 32 B from `($20),y` to `$7000,x` — palette/attribute buffer fill |

Per-stage assignment **not proven** — needs the stage table (see §9).

## 7. Sprites / OAM

- OAM DMA each frame from page `$02`: NMI `$E22B` (`$2003=0; $4014=2`).
- Object/actor working arrays in RAM `$7B00-$7EFF` (e.g. `$7B00`/`$7B20`/`$7C00`
  positions, `$7D90` state, `$7DB0` type index, `$7DD0` animation, `$7E40`
  timer, `$7E70`/`$7E80` velocities) — bank 00/02/0B/0D.
- Sprite pattern tables are CHR-ROM, selected by the 1 KiB CHR banks (`$88-$8F`
  shadow, cmd `$0-$7`). Title/attract and stage-1 sprite tiles come from those
  banks; per-scene CHR bank *lists* were not isolated.

## 8. Music / sound

- **Sound driver is PRG bank 10 (`0x0A`), window `$8000`.** API jump table at
  `$8000`: `JMP $800C` (init: `$4015=$1F`, clear `$0600-$06FF`), `JMP $8020`
  (reset voices: `$4000/$4004/$400C=$10`, `$4008=0`, `$4015=$0F`),
  `JMP $80B2`, `JMP $8217`.
- **Song pointer table** at `$8BD1` (bank 10 offset `$0BD1`): 48 x 16-bit
  pointers, `; tbl_song_ptr` in `bank0A.s`. Entry 48 is code, so the table is
  48 entries. (The NSF rip lists 56 tracks — the extra 8 are likely SFX or
  NSF track splitting; **unverified**.)
- APU register writes seen at bank 10 offsets `$000E` (`$4015`), `$0022`
  (`$4000`), `$0025` (`$4004`), `$0028` (`$400C`), `$002D` (`$4008`).
- The GameHacking lead "`$AB23` sound-engine hook" maps to window `$A000`, not
  bank 10 at `$8000`; **not reproduced** here (lead, unverified).
- **DMC sample records (§8 detail).** Stream opcode `$F3` in handler `$89C7`
  stores a 16-bit pointer into RAM `$06F7/$06F9` (per voice). The note handler
  `$893E` uses `Y = note*4` (note in `$47`) to play the four-byte record at
  `pointer + note*4` straight into `$4010`/`$4011`/`$4012`/`$4013` (control /
  direct-load / address `$C000+v*64` / length `v*16+1`). The song loader
  `$80DE` selects a song bank (`cmd $0A`) and reads its per-song pointer table
  at `$A000`, so the `$F3` pointers `$A06C`/`$A06E` resolve to file offset
  `$006C`/`$006E` of bank 06/07: two byte-identical **50 x 4-byte** tables
  (200 bytes). Bank 10 holds a dead, identical copy at file offset `$0C31`.
  All records address the fixed bank 15 (`$E000-$FFFF`): nine distinct
  (address,length) samples at bank-15 offsets `$0500`(1009), `$0900`(1009),
  `$0D00`(1009), `$1100`(993), `$1500`(993), `$1900`(497/1265), `$1B00`(497/113).
  Extracted by `rotj dmc` to `assets/audio/dmc/` (see `docs/RE-NOTES.md`).

## 9. Uncertainties / not established

- **Stage/sublevel table**: the code that picks per-stage PRG window banks, the
  palette, and the level list from a stage index (`$0401`/`$0402` candidates)
  was not located. This is the main missing piece for automated extraction.
- Full per-bank runtime window assignment for banks 03/06/07/09 and the
  "second" window a bank might use.
- The exact `$D3` handler-index range (table has 14 entries, only 8 are known
  to be reachable).
- Palette-to-scene mapping (needs the stage table).
- Attract-mode / title-screen code entry (bank 10's `$80B2`/`$8217` audio
  entries are known; the video/attract state machine is in the bank-13 main
  loop `$D394` but was not split out).
- No plain ASCII strings exist in PRG (an ASCII sweep found only incidental
  byte runs); all text is CHR tiles.

## 10. RAM map (observed in the reached code)

Zero page:

| addr | use | evidence |
| --- | --- | --- |
| `$08/$09` | indirect `JMP ($0008)` vector | `$E001`, `$E07E`, `$E2A9` |
| `$20-$25,$2B-$2F` | camera/level math | `$C0E0`-`$C260` |
| `$30-$3F` | tile/scroll math | `$D2BA`, `$C0E0` |
| `$45/$46/$47` | pointer/index | bank 0A `$A10C` |
| `$88-$8F` | CHR 1 KiB bank shadow (cmd `$0-$7`) | `$E204` |
| `$90,$91` | level pointer low/high | bank 11 `$8049` |
| `$AB,$AC,$AD` | PPUCTRL / scroll Y / scroll X | `$E21E`-`$E223` |
| `$B1,$B6` | flags/dir | bank 13 `$DF43`, `$E1A5` |
| `$CA-$CE` | camera | `$C146`-`$C14C` |
| `$D0-$D4` | IRQ split state (arm/counter/handler/sub) | §3 |
| `$D5/$D6` | vertical scroll accumulator + direction | `$E0B0` |
| `$DD` | flags (bit6-7 scroll), `$DF` preset cmd | `$D2A6`, `$E29D` |
| `$E0-$EC` | FME-7 register shadow (cmd `$0-$C`) | `$E259` |
| `$E8-$EB` | PRG window banks `$8000/$A000/$C000/$6000` | `$E337` |
| `$F0` | bit7 = simple vs full NMI path | `$E200` |
| `$F1` | frame counter | `$E23D` |
| `$F2-$F4` | NMI/IRQ register save | `$E1D0` |
| `$F7` | anim/blink toggle | `$E240` |
| `$F9/$FA` | `$49/$2F` init | `$E362` |
| `$FC/$FD` | scroll mirror X/Y | `$E24A` |
| `$FE/$FF` | PPUMASK / PPUCTRL shadow | `$E235`, `$E230` |

Pages:

| addr | use |
| --- | --- |
| `$0180/$0181` | stack-restore pointer (`$FF/$7F`) for the display-list executor |
| `$0401/$0402` | stage / sublevel (candidates) |
| `$0440-$047F` | scroll/camera working values (`$0460`,`$0462`,`$0463`) |
| `$04A0-$04BF` | secondary per-band scroll buffer |
| `$04E0-$04FF` | scroll/OAM staging, filled by IRQ handlers |
| `$0600-$06FF` | sound-driver state; `$0600` also a stage bank id |
| `$6000-$63FF` | collision/level tilemap (built by the bank-02 loader) |
| `$7000-$72FF` | nametable/tile staging buffers |
| `$7B00-$7EFF` | object/actor arrays |

GameHacking leads from `docs/RE-NOTES.md` (`$009E`, `$00AA`, `$00C3`, `$00F1`,
`$0466`, `$046F`, `$0477`, `$047D`, `$0482`, `$04F2/$04F3`, `$06D3-$06D8`,
`$7DD0-$7DD2`) were **not** individually verified here. **(lead, unverified)**

## 11. Indirect-jump dispatch tables

The game dispatches per actor / state through one indirect-jump idiom, now
followed by the descent (`gen/generate.py` `dispatch_targets`):

```
LDA <index>,x       ; index in RAM ($7DD0 / $7D50 / $7CC0 / $D4 / $0476 / ...)
ASL; TAY            ; 16-bit table index
LDA <table>,y       ; pointer low
STA <zp>
LDA <table+1>,y     ; pointer high
STA <zp+1>
JMP (<zp>)
```

The last thirteen bytes always have the shape
`B9 lo hi 85 zz B9 lo+1 hi 85 zz+1 6C zz 00`; nothing else in PRG matches it
(operand-consistency over eight bytes), so the sites are found by a raw-byte
scan even where the surrounding code is not reached yet. The table is always in
the site's own naming window, so its entries are read from that bank. A table is
followed while each entry is in `$8000-$FFFF` and its target decodes as official
code; the first entry that is not ends the table (the bytes after a table are the
next routine's instructions and never decode as a valid pointer). An entry in
the site's window is a handler in the same bank; an entry outside it is resolved
through the FME-7 window map like a far call. A `PHA/PHA/RTS` jump-table form
does **not** occur (the one `PHA;RTS` pair, bank 3 `$171F`, is not a dispatch).

Tables followed (anchor address, resolved handler-entry count); banks 0, 8, 11,
12, 13, 14, 15 are descent banks:

| bank | window | tables |
| --- | --- | --- |
| 00 | `$8000` | `$8C74`(4) `$9022`(4) `$9243`(7) `$975A`(3) `$9D91`(6) |
| 08 | `$8000` | `$875B`(8) `$8A68`(3) `$8EBB`(4) `$927D`(8) `$980B`(4) `$9C27`(4) |
| 11 | `$8000` | `$8514`(3) `$8860`(5) `$8D0B`(2) `$910D`(6) `$940A`(3) `$96FD`(3) |
| 12 | `$8000` | `$81C0`(3) `$83C7`(2) `$87C5`(3) `$893B`(2) `$8B8C`(3) `$9100`(7) `$9324`(7) `$94AC`(2) `$987F`(2) `$9A22`(2) |
| 13 | `$A000` | `$ABA8`(5) |
| 14 | `$C000` | `$CADC`(17) `$CD2E`(18) `$CE79`(2) `$CEC3`(1) `$CF72`(1) `$D16C`(4) `$DE89`(3) `$DF15`(4) `$DFDE`(3) |
| 15 | `$E000` | `$E076`(4) `$E0EA`(2) `$E163`(4) `$E1A1`(2) |

Two of the bank-15 anchors are the known split handlers of `MAP.md` §3 (`$E076`,
`$E0EA`); `$E163`/`$E1A1` are the same shape and new. Bank 13 `$ABA8` decodes
`$AB5A` (four states) and `$AB69`, matching `docs/ACTORS.md` §7c; bank 0 `$8C74`,
bank 8 `$8A68` and bank 12 `$81C0` are the per-type tables named there. Banks
02/04/05 also contain tables. They kept the linear-sweep fallback because a
table alone does not say which bank runs where, so their anchors were
intentionally **not** followed; the observed-execution pass below does not give them one either.

After this pass the reached set grows from 4086 to 9832 instructions (8800 to
21767 bytes): bank 00 230→1274, 08 459→1563, 11 190→1616, 12 685→1711,
13 1219→1270, 14 1032→2015, 15 271→383 instructions.

### Observed-execution seeds (retracted finding re-tested)

`tools/emu/observed-code.py` turns the captures' register-write program counters
into entry seeds. An earlier attempt was retracted as "seeding the descent with
observed execution collapsed coverage (3274 → 566 in bank 0)"; that was an
artefact of two bugs in the harvest, both fixed:

- the FME-7 window map was read backwards (`$8→$8000 …`), so every PC was
  attributed to the wrong bank. The corrected map (`docs/ACTORS.md` §0) is
  `$8→$6000`, `$9→$8000`, `$A→$A000`, `$B→$C000`, with cmd `$8` bit 7 = WRAM.
- the PC was attributed from the frame's *final* window state, but the game
  re-maps windows several times per frame (e.g. bank 10 runs at `$8000` for one
  dispatch and is restored) and its direct writes bypass the RAM shadow, which
  then lags. The PC must be read against the register state at its own write.

Each logged PC is the instruction *after* the store (fceux reports PC inside the
write callback), so it is a valid instruction start -- e.g. the NMI's
`STX $8000` at `$E259` logs PC `$E25C`. `disasm/gen/check_observed.py` checks
that: of the 90 corrected PCs, 84 fall inside the baseline descent's decoded
regions and all 84 are instruction starts (0 mid-instruction hits); the other 6
are in still-undecoded code (bank 0D `$1272/$1277/$1E7F-$1E88`). Independently,
all 90 pass a byte-level test: the three bytes at `pc-3` are an absolute store
to its own register window (`8D/8E/8C 00 80`, `… 00 A0`, or `… 0x 20`).

Seeding the descent with these PCs is kept, but only to extend banks the static
seeds already reach (`collect_entries`). Measured reached instructions: 9832 ->
9865 (bank 0D +33; the rest unchanged). Seeding banks 02 and 0A from these PCs
alone was tried and rejected: their linear-sweep listings became descents of
1459/606 instructions that dropped known code to `.byte` data (the auto-tiler
`$8B9E`/`$8E57`, the level loader `$8F2B`, the whole sound driver `$80DE`-`$88BD`,
all of which exec traces show running). Register-write sites are a poor entry
set; those banks need entry points from exec traces. No register write was
observed while banks 04/05 were mapped at `$8000`. The old "collapse" compared a
linear sweep against a descent; bank 0, where it was measured, has no observed
seed and is unchanged.

`generate.py`'s far-call `OBSERVED_WINDOW_BANKS` is still the pre-correction map
(`$8000` -> banks 0/11 and so on); whether resolving far calls through the
corrected map changes coverage is **a lead, unverified**.


