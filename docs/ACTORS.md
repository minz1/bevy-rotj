# Object / actor system — Batman: Return of the Joker (NES, FME-7)

Status: **partial, evidence-first**. The slot layout, the collision scan, the
per-slot state dispatch and the level-script spawn path are located and read
from the ROM bytes; the per-field *semantics* are only partly established. Every
claim below gives the address it was read from, and inference is marked
**(inferred)**. This pass also corrects the runtime window map, which had the
even-bank attributions off by one bank.

## 0. Runtime PRG window map (corrected)

The FME-7 command in `$8000` selects, by its low nibble, the 8 KiB window
selected by command `$8/$9/$A/$B` = `$6000/$8000/$A000/$C000` (nesdev
*Sunsoft FME-7*; `docs/RE-NOTES.md` already flagged the old table via the audio
loader's `$A000,x` after command `$0A`). The NMI shadow at `$E8-$EB` is
`C0 18 1D 1E` at gameplay, so:

| cmd | window | value | bank |
| --- | --- | --- | --- |
| `$8` | `$6000` | `$C0` | **WRAM** (bit 7 set) |
| `$9` | `$8000` | `$18` | 8 |
| `$A` | `$A000` | `$1D` | 13 |
| `$B` | `$C000` | `$1E` | 14 |

Evidence: (a) the level map the renderer reads at `$6000/$6E00` is byte-identical
to a WRAM dump, and appears nowhere in PRG — so `$6000` is RAM; (b) the
display-list executor's `STA $2007` at runtime `$C0A6` (`tools/emu/ppuw/
ppu-writes.csv`, 19 919 writes) is bank 14 file offset `$00A6`
(`python3 disasm/gen/read_range.py 14 0xA6`), not bank 13; (c) the audio loader
dereferences `$A000,x` after command `$0A` (`docs/RE-NOTES.md`).

`$6000` stays WRAM for the whole gameplay capture (command `$8` is `$C0` in 670
of 673 writes). `$8000` and `$A000` are **dynamic**: `cmd $9` takes values
`$02/$0A/$12/$18/$1C/$14/$15/$10` and `cmd $A` takes `$1D/$16/$00/$01/$11/$13`
(`tools/emu/ppuw/mapper-writes.csv`), i.e. banks 0/2/4/5/8/10/12 and
0/1/3/5/6/13 respectively. So a `JSR` to a fixed address reaches different
routines at different moments; the *addresses below are runtime addresses for
the captured moment*, and the bank label is secondary.

**Consequence.** `disasm/MAP.md` §1's command→window table and
`docs/LEVELDATA.md` §6 are shifted by one bank. The per-bank *naming* windows
(`disasm/gen/generate.py` `REAL_WINDOWS`: bank 8 → `$8000`, 13 → `$A000`,
14 → `$C000`) happen to be correct, which is why `bank08.s`/`bank0D.s`/
`bank0E.s` listings read fine. But the addresses in the task lead are therefore:

| lead | actually |
| --- | --- |
| "bank 13 offset `$0A29`, runtime `$CA29`" | bank 13 offset `$0A29`, runtime **`$AA29`** (`$CA29` is a different routine in bank 14) |
| "bank 8 offset `$056E` position integrator" | bank 8 offset `$056E` runs at **`$856E`**; the far call `JSR a:$A56E` reaches **bank 13** offset `$056E` |

## 1. Slots

There are **16 actor slots**, index `x = 0..15`.

- Scan loop: bank 13 `$AA57` (`LDX #$0F` … `LDA $7BD0,x; BNE … DEX; BPL`),
  entered from bank 13 `$AA29`. Process a slot when `$7BD0,x != 0`.
- `$7BD0,x` is the slot **type**; the initialiser writes `#$06` into it
  (bank 14 `$D680`), so `0` = free and `6` = a live actor type **(inferred:
  the value is a type id, not the byte's only use)**.

## 2. Per-slot arrays (all page `$7B00-$7FFF`, index `x` = slot)

Read out of the collision scan (bank 13 `$AA57-$AB5A`) and the slot initialiser
(bank 14 `$D64A-$D6AE`); each row names the instruction that established it.

| address | width | meaning | evidence |
| --- | --- | --- | --- |
| `$7BD0` | 16 | slot type; 0 = free | bank13 `$AA59`, bank14 `$D680` |
| `$7B80` | 16 | hit-box x origin | bank13 `$AA6C` (`SBC $7B80,x`) |
| `$7B90` | 16 | hit-box y origin | bank13 `$AA7D` (`SBC $7B90,x`) |
| `$7C60` | 16 | position y (low) | bank14 `$D67D` (`STA $7C60,x`) |
| `$7C70` | 16 | position x (low) | bank14 `$D674` |
| `$7BE0` | 16 | velocity x (16-bit low) | bank13 `$AB02`, bank00 `$7BE0,y` |
| `$7EF0` | 16 | velocity x (high) | bank13 `$AB0E` (`SBC #$00`) |
| `$7D00`/`$7D10` | 16 | facing / step (sign) | bank13 `$AAAB-$AAD5`, bank14 `$D688` |
| `$7D50` | 16 | **state index** (dispatch) | bank13 `$AB48` (`ASL; TAY; LDA $ABA8,y`) |
| `$7D70` | 16 | death/hit flag | bank13 `$AB25`, bank00 `$7D70,y` |
| `$7D90` | 16 | **behaviour index**, into the velocity pair `$87F5/$87F7` | bank00 `$1125` (`LDY $7D90,x; LDA $87F5,y`) |
| `$7DB0` | 16 | **type index** (source for a spawned copy's `$7CC0`) | bank00 `$1087` (`LDA $7DB0,x; STA $7CC0,y`) |
| `$7E10` | 16 | state timer | bank00 `$7E10,x`; set bank13 `$AB23`? |
| `$7E90` | 16 | state timer countdown | bank00 `$1179` (`INC $7E90,x`) |
| `$7F30` | 16 | per-slot flags (bit7 = ignore) | bank13 `$AA64`, `$AAA3` |
| `$7F00` | 16 | hit/death mask (`AND #$FE`) | bank13 `$AB29` |
| `$7EE0` | 16 | hit-lock byte | bank13 `$AA93` |

**(inferred)** Rows with two addresses (`$7D00`/`$7D10`, `$7BE0`/`$7EF0`) are one
16-bit field split by the access width, not two fields.

## 3. Collision / hit boxes

Bank 13 `$AA62`-`$AA8F`. For player state `$A5`, the near-miss box is:

```
LDY $A5                       ; player state selects the box
LDA $7F30,x ; BMI -> skip slot (bit7 = no-contact)
LDA $94 ; SEC; SBC $7B80,x ; abs -> |dx|   ; $94 = player x (low)
CMP $ABD0,y   ; x half-extent   (bank13 offset $0BD0)
LDA $95 ; SEC; SBC $7B90,x ; abs -> |dy|   ; $95 = player y (low)
CMP #$05 / CMP $ABC1,y   ; y half-extent (bank13 offset $0BC1)
```

So the hit-box table is indexed by the **player's state `$A5`**, with the x
extent at `$ABD0` and y extent at `$ABC1` (bank 13). `$ABD0` begins
`09 16 10 10 10 10 10 10 10 0E …`, `$ABC1` `28 21 20 20 30 40 50 60 70 14 …`.

## 4. State dispatch

Bank 13 `$AB48`: `LDA $7D50,x; ASL; TAY; LDA $ABA8,y; STA $30; LDA $ABA9,y;
STA $31; JMP ($0030)`. `$ABA8` (bank 13 offset `$0BA8`) is a 2-byte-pointer
table; its first six entries all point at `$AB5A`.

## 5. Spawn path (level scripts, not a static ROM object list)

Objects come from **scripts embedded in the level payload**, selected by
(area, stage). Bank 14 `$D992` (runtime `$C000` window):

```
LDX $0400                  ; area index (increments 2 per screen)
LDA $DA18,x ; STA $30      ; 16-bit LE table at bank14 $DA18 ($1A18),
LDA $DA19,x ; STA $31      ;   indexed by $0400 as a byte offset
LDY $0402                  ; stage index
LDA ($30),y ; TAX          ; per-stage pointer
INY
LDA ($30),y ; STA $31 ; STX $30
LDY #$00
LDA ($30),y ; CMP #$77 ; BEQ clear     ; $77 = empty record
; else:  5 bytes -> $D0-$D4 ; 16 bytes -> $70-$7F ; 8 bytes -> $60-$67
;        then 16-bit pointer -> $32/$33 ; JMP ($0032)
```

So a spawn record is `[5-byte header][16-byte block][8-byte block][2-byte
script ptr][script]`, staged into zero page and executed. `tools/emu/ppuw/
mapper-writes.csv` frame 700 shows the script running from the `$6000` window.
The payload itself is copied into WRAM from **CHR banks `$F8-$FF`** (bank 02
`$99A9`, with a 0x1A0-byte tail from bank `$F7` offset `$260` via `$9925`) —
this is why no object data exists in PRG.

The process-local per-stage header table (bank 14 `$D64A`) initialises four
further slots from `$D6AF-$D6CF`; `$0460-$0463` are the level position added to
the per-slot offsets.

### 5a. Level paging — `$0400` steps by 2

Bank 14 runs a screen-transition dispatcher on RAM `$BA`: `$D4D8`
(`LDA $D60D,x` with `x = $BA*2`) jumps through the pointer table at `$D60D`.
The relevant handlers:

- `$D513`: `JSR $D61B`, `STA $0402 = 0`, `$0400 += 2`, `JMP $D55E` — **advance
  one screen**.
- `$D527`: `DEC $0482`; when it underflows, map bank 2 (`$E9 = #$12`) and
  `JSR $8F05` (the bank-02 area loader `$8F2B`, §1), then
  `$0400 = $D57D[$0400/2]`, `$0482 = 2`, `$0404 = 0` — **load the next area**.
- `$D55E`: map bank 2 and `JSR $92AD` (the screen loader: clears the nametable
  and runs the `$93D7` patch list), then return via `$D6E2`/`$D6EE`, `$BA = 0`.

So each screen is 16 metatile columns (`$0400 += 2`), the 15-page map at `$6000`
is one **area**, and `$D57D` (`00 02 02 06 08 0A 0C 0C 10 12 12 16 18 1A 1C 1C
…`) is the area list. Per-frame columns are *not* streamed from ROM: the
renderer (bank 2 `L8000`/`$8162`) reads the resident map. No capture exercises a
screen transition (the camera is pinned at 54 px), so this path is read but not
observed.

### 5b. Player movement (context, from `docs/LEVELDATA.md`)

Bank 14 at `$C000`: direction handlers dispatch through `$CADD`; the x-move adds
`$20` to the 16-bit accumulator `$90/$91` (fine part `$92/$93`), the y-move uses
`$22`; the collision response is `$CA1D` (restore position from `$A0/$A1` into
`$A2/$A3`, zero the velocity). The actor's level x is `$94`; the camera is
`$00AC` (`$94 - 131` settled).

## 6. Not established

- The **meaning of the 5/16/8-byte blocks** — resolved in §7.
- Which `$7BD0` type values correspond to which enemy; the **animation/OAM
  setup** (bank 13 `$0852`) and the `$8A68`/`$8A76` tables cited in
  `docs/LEVELDATA.md` §7 were not re-read this pass.
- `$ABD0`/`$ABC1` are indexed by `$A5` but the `$A5` value space (player state
  vs. "actor class") was assumed from the brief, not proven.
- The second 16-slot bank at `$7CC0` (free-scan in bank 00 `$8704`) and its
  relation to `$7BD0` is not decoded.
- What sets the transition selector `$BA` and the screen counter `$0482`, and
  whether the 15-page map is rebuilt per *area* from CHR (the loader `$8F05`
  reads a per-area definition through `$2007`; the compact map source is the
  open problem in `docs/LEVELDATA.md` §12d).

## 7. Spawn records and scripts — decoded (`rotj actors`)

Read from the ROM bytes and exposed by `rotj_core::actors`. This section
corrects §5: `$D992` is a **screen-metadata loader, not an actor spawner**, and
the `JMP ($0032)` target is **plain 6502, not a bytecode**. The actual actor
spawner is a separate, table-driven routine at `$D714`.

### 7a. The `$DA18` screen records (bank 14)

`$D992` (`LDX $0400 / LDA $DA18,X / STA $30 / LDA $DA19,X / STA $31 / LDY $0402 /
LDA ($30),Y / TAX / INY / LDA ($30),Y / STA $31 / STX $30 / LDY #0 / LDA ($30),Y /
CMP #$77 …`) builds the record address as:

```
word  = $DA18[$0400]                 ; 16-bit LE, $0400 = area byte offset
rec   = word( word + $0402 )         ; $0402 = stage byte offset
```

`$DA18` (`$DA18`..`$DA3B`) holds **18 entries** (areas 0-17), e.g. area 0 ->
`$DA3C`, area 8 -> `$DA4A`. Each per-area table is a run of 16-bit stage
pointers. A record is:

| bytes | staged to | evidence |
| --- | --- | --- |
| first byte `$77` | (empty) zero `$D0-$DC`, `$70-$7F`, `$60-$67`, `RTS` | `$D9AD` |
| 5 | `$D0-$D4`, then `$D0-$DC` zero-extended | `$D9C6`-`$D9DB` |
| 16 | `$70-$7F` | `$D9DD` |
| 8 | `$60-$67` | `$D9E9` |
| 2 (LE) | `$32/$33`, then `JMP ($0032)` | `$D9F2`-`$D9FC` |

The records live in **bank 14 PRG**, `$DA66-$DB7E` (the distinct record starts
are `$DA66`(empty), `$DA67`, `$DA86`, `$DAA5`, `$DAC4`, `$DAE3`, `$DB02`,
`$DB21`, `$DB40`, `$DB5F`, `$DB7E`). **Every** script pointer they contain is
one of three fixed `RTS`-terminated routines:

| entry | bytes | effect |
| --- | --- | --- |
| `$D9FF` | `A9 85 8D 66 04 A9 0B 8D 6E 04 A9 A0 8D 81 04 60` | `$0466=$85 $046E=$0B $0481=$A0` |
| `$DA0E` | `60` | return |
| `$DA0F` | `A9 0A 8D 6E 04 A9 E0 D0 F3` (branches to `$DA0B`, `$0481=$E0`) | `$046E=$0A $0481=$E0` |

So the record's output is the staged zero-page block; the script only sets
level-state RAM and returns. `$D0-$D4` is IRQ split state per `docs/MAP.md`
§10, so this path is screen/IRQ configuration.

### 7b. The actor spawn records (bank 3 `$A000`)

Bank 14 `$D714` maps bank 3 into `$A000` (`LDX #$0A; LDA #$13; JSR $C229`,
`$C229` = `STX $DF; STX $8000; STA $A000`; the parameter's bit 4 marks ROM, so
`$13` = bank 3) and runs:

```
$A000[$0400] -> per-area table
per_area[$0402] -> per-stage table (16-bit record pointers, id*2)
record_table[id*2] -> 12-byte record
```

The id for a slot comes from RAM `$7410`, a 256-byte block per area copied by
`$D854` from `$A2D4[$0400]` (`$D854: LDA $A2D4,x … LDA ($20),y STA $7410,y`).
A schedule byte with **bit 7 set** is pending; its **bits 0-5 are the id**, and
the routine reads `record_table[id*2]` (`$D767: ASL A; AND #$7F; TAY`). The
12 record bytes are written, in order, by `$D778`-`$D805`:

| byte | RAM | byte | RAM |
| --- | --- | --- | --- |
| 0-3 | `$7B00,$7B20,$7B40,$7B60` (+x) | 6 | `$7D50` **state** |
| 4 | `$7C00` (`&$E3`), `$7C90` (`>>2 &7`) | 7 | `$7D90` **behaviour** |
| 5 | `$7CC0` (`&$7F <<1`); bit 7 -> second bank (`X=$18`) | 8 | `$7DB0` **type** |
| 9-11 | `$7EF0`/`$7BD8`, `$7BE0`, `$7EE0`/`$7C30` | | |

For area 0 / stage 0 (`$0400=0 $0402=0`, the frame-700 state of sublevel 1-1)
the schedule (`$A2F8`) marks slots 2, 3, 14, 24, 25, 29 pending, resolving to
records `$ACA0`, `$ACE8`, `$AC94`, `$ACDC`, `$ACAC`, `$ACB8`.

### 7c. Behaviour and dispatch

- `$7D90` is **not** a handler index. Bank 0 `$1125` does `LDY $7D90,x; LDA
  $87F5,y; STA $7E70,x; LDA $87F7,y; STA $7E80,x` — two byte tables whose two
  entries give the 16-bit knock-back velocity `$FFD8`/`$0028` (+-40).
- `$7DB0` indexes a per-type position offset at bank 0 `$9932` (`$98DA: LDY
  $7DB0,x … ADC $9932,y`), and is copied to a spawned copy's `$7CC0` (bank 00
  `$1087`).
- The per-slot *handler* dispatch is `$7DD0` -> a **per-type** jump table
  (`LDA $7DD0,x; ASL; TAY; LDA <table>,y; JMP`): e.g. bank 0 `$8C74` (`$0A9B`),
  bank 8 `$8A68` (`$07A8`), bank 12 `$81C0` (`$009D`). There is one per actor
  handler; no single global table exists.
- `$7D50` -> `$ABA8` (`$AB48: LDA $7D50,x; ASL; TAY; LDA $ABA8,y; … JMP
  ($0030)`). The table has **5 entries**: states 0-3 -> `$AB5A`, state 4 ->
  `$AB69`. `$AB5A`: `BIT $0472; BMI …; TYA; LSR; ADC #$01; STA $0476` then
  clears the 16 `$7BD0` slots. `$AB69`: advances `$0477` and, when it passes
  `$0E`, arms the level transition (`$BA=$05`, `$AF=$80`).

### 7d. The slot arrays, `$7CC0` and `$7D50` — decoded (trace)

Evidence: a new capture (`tools/emu/dump.lua`, `INPUT=actorseek` plus its
`WATCH`/`EXEC` trace files) reproduces the `seek` screen and logs every write to
`$7Bxx-$7Exx` and every execution of the candidate consumers with the program
counter. This subsection corrects §2's `$7BD0` and `$7DD0` rows.

**The slot arrays hold 32 entries, in two banks.** The spawner's free-slot
search (`$D830`) walks `X=0..15` (`Y=#\$0F`) when record byte 5 has bit 7 clear,
and `X=$18..$1F` (`Y=#\$07`) when it is set, indexing `$7CC0,x` each time. So 0-15
is the first bank, 24-31 the second, and 16-23 are never scanned. The
spawn-record field `slot_bank`/`slot_type` already encode this choice.

**`$7CC0` is the "occupied" byte, not `$7BD0`.** `$D840` frees a slot with
`LDA $7CC0,x; BEQ` and the spawner writes the object's type there
(`$D7A5: AND #$7F; ASL; STA $7CC0,x`). `$7BD0` is **zero in every captured
gameplay frame** (measured over ~30 capture directories; its only writes in the
trace are the level clear at `$B5ED` and the initial `$FF` fill at `$98FD`), so
the `$7BD0`-based scan at bank 13 `$AA57` and the update loop at bank 5 `$8037`
find nothing in these scenes. `$7BD0` is a different slot class (lead below).

**`$7CE0` records which `$7410` schedule slot the object came from** (`$D820:
LDA $22; STA $7CE0,x`). At frame 700 it reads `02 03 0E` for slots 0-2.

**Frame 607 spawns three objects in one call.** The write trace (PCs are the
instruction after the store) shows, for slots 0/1/2:

| slot | `$7CC0` | `$7CE0` | `$7D50` | `$7D90` | `$7DB0` | schedule id | record |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | `$B2` | `$02` | `$81` | `$0C` | `$68` | 2 | `$ACA0` |
| 1 | `$C6` | `$03` | `$00` | `$00` | `$00` | 8 | `$ACE8` |
| 2 | `$06` | `$0E` | `$81` | `$0C` | `$68` | 1 | `$AC94` |

Each record's byte 5, 6, 7, 8 match `$7CC0`/`$7D50`/`$7D90`/`$7DB0` exactly.
By frame 700 slots 0 and 1 have been freed (`$7CC0 = 0`); only slot 2 stays
occupied, and its record data is still in the arrays. `$7DD0` at slot 1 is `1`:
a bank-0 `$8704` copy spawned there (`INC $7DD0,x`).

**`$7D50` is the per-slot state byte (record byte 6).** `$D7AE` writes it
verbatim. Consumers, all read from the ROM bytes and two with an execution
trace:

- `$AB48` (bank 13) indexes `$ABA8` with it: `LDA $7D50,x; ASL; TAY; LDA
  $ABA8,y`. `ASL` **drops bit 7**, so `$80-$84` reach the same five entries as
  `$00-$04`; bit 6 pushes the index past the table. Handler code *counts* the
  byte and wraps it back to `$80` when the next value would be `$84` (bank 0
  `$89C6`, bank 8 `$8B6A`). This path **did not execute** in any of five
  gameplay captures (the `$AB39` hit stub it is entered through never ran).
- **bit 6 (`$40`) = "finished, not a valid target".** Bank 8 `$8D33` sets it
  (`ORA #$40; STA $7D50,x`); the per-slot target finder bank 5 `$83FC` tests it
  with `LDA $7D50,y; ASL; BMI` (`$8417`/`$8430`, called from `$830E`), as does
  bank 4 `$9F2C`/`$9F54`. Both `$8410` and `$8429` ran in the trace (599 and 70
  times).
- **bit 7**: bank 13 `$A5C3` (`LDA $7D50,x; BMI` = still alive; otherwise it
  clears `$7CC0,x` and returns `$80`), bank 8 `$9342` (`LDY $7D50,x; BPL`).
- the low bit is also read as a flag by bank 11 `$89B7` (`AND #$01` -> copies
  `$7DB0`/`$7BE0` into `$040E`/`$040F`).

Because each actor type writes and counts `$7D50` in its own handler, its exact
meaning per value is type-dependent: the state space is wider than the
`$00/$01/$20/$61/$81/$84` first recorded (a full walk of every area/stage in the
bank-3 tables also yields `$03/$06/$07/$08/$09/$0C/$1E/$22/$3A/$40/$41/$4C/$4E/$58/$5E/$60/$78/$80/$82/$84/$8B/$8D/$8E/$9A/$A0/$AC/$AE/$B3/$E0/$E1`, though some of those come from stage pointers past a table's end and are unverified).

### 7e. Still unknown

- **Which record is which enemy — resolved for stage 1-1 by §7f** (two enemies
  named from a scripted route capture). The `seek`/`seek2` captures alone cannot
  answer it: at frames 700/800/900 their OAM holds only 15 on-screen sprites -
  the player's 11-sprite cluster at x≈89-114 and four HUD icons at y=16 - and
  nothing at the green plant (x≈40-70) or orange flame (x≈180-200), which are
  background nametable tiles. The spawn happens but the `$81`-state actors are
  not drawn in that hacked state (§7f found the drawn ones are `$84`/`$20`
  state).
- `$7BD0`: a second slot class. Its updates live in bank 4 and bank 5 (many
  `STA $7BD0,x`), bank 5 `$8037` iterates it as the live flag, and the initialiser
  bank 14 `$D64A` sets `$7BD0,x = #$06` for slots 0-3. No capture has it
  non-zero while the spawned objects are present, so its relation to the
  `$7CC0` bank is unresolved.
- The rest of `$7D50`'s value space (`$20`/`$40`/`$60`/`$61`/...) per actor type.
- The job of the `[5][16][8]` screen blocks (they land on `$D0-$DC`/`$70-$7F`/
  `$60-$67`; no capture exercises them).
- `$A000` is indexed by `$0400` directly; whether sublevel -> area comes from
  `$D57D` alone (docs/LEVELDATA §5a) is unproven. `$0402` reset is 1 (odd), so
  it is not a plain stage*2 in the attract state.

### 7f. The route capture: two enemies drawn, tied to their slots (trace)

Evidence: a new capture, `tools/emu/route-dense` (`dump.lua` `INPUT=route`
playing stage 1-1). This is the first capture in which OAM holds sprites that
are neither the player nor the HUD (`docs/ACTORS.md` §7e).

**The jump is variable-height, and the route needs the held jump.** A one-frame
A tap rises **22 px** (`$00B1` 192 -> 170, `tools/emu/route-jump`); holding A for
~25 frames rises **54 px** (`$00B1` 192 -> 138, `tools/emu/route-hold`). With
only tap jumps the walk right stops at the captured ledge/block (level x 160-192,
per `docs/LEVELDATA.md` §11): `route-j2` jumps against it and never advances
while airborne, even at the apex (`$00B1 = 170`), so the block's top is above
y 170. The route therefore **holds A across each step**: `ROUTE_JUMPS` = frames
735-760 then 792-817, `ROUTE_FROM` = 610 (gameplay starts ~607). The first held
jump lands him on the y 160 step (`$00B1 = 160` in `route-hold`); the second
clears the next step. Those jump frames are the geometry, not trial: he reaches
the first block face at ~frame 735 and the second step at ~792.

**Scene.** At frame 900 the camera `$00AC = 231`. The OAM holds 10 player
sprites, 4 HUD sprites (tile `$11`, attr `$03`, y 16), a **9-sprite orange
cluster** (x 145-174, y 114-142) and a **4-sprite green cluster** (x 234-242,
y 67-83). Rendered sprite-only (`ram-900.bin` through the dumped PPU
pattern/palette) they are one orange soldier/thug and one green enemy, well
separated. (`$0094` is the game's own actor coordinate, not simply the level x:
at this frame it is 166 while the camera is 231, so the level-x derivation of
§5b does not hold once the screen has scrolled far.)

**Slot -> record.** The slot-window write trace (`route-dense/ram-writes.csv`)
records which spawn wrote which slot and the program counter:

| frame | slot | writes | record |
| --- | --- | --- | --- |
| 846 | 0 | `$7CC0=$22 $7D50=$84 $7D90=$00 $7DB0=$00 $7CE0=$18` (PC `$8879`) | id 7 = `$ACDC` |
| 855 | 1 | `$7CC0=$06 $7D50=$81 $7D90=$08 $7DB0=$6A $7CE0=$19` (PC `$8879`) | id 3 = `$ACAC` |
| 894 | 3 | `$7CC0=$0A $7D50=$20 $7D90=$00 $7DB0=$05 $7CE0=$1D` (PC `$8879`) | id 4 = `$ACB8` |

(`$7CE0` is the `$7410` schedule slot; 24 -> id 7, 25 -> id 3, 29 -> id 4 as
§7b's tables give.) Slot 2 still holds the frame-607 record `$AC94`. The
**orange cluster is slot 0**: its sprites are on screen at frames 850-854, four
frames after slot 0 spawns and five frames *before* slot 1 spawns (855). The
**green cluster is slot 3**: it appears at frame 896, two frames after slot 3
spawns (894).

| enemy | slot | `$7CC0` | `$7D50` | `$7D90` | `$7DB0` | schedule | record | OAM (frame 900) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| orange soldier/thug | 0 | `$22` | `$84` | `$00` | `$00` | `$18` (24) | id 7 `$ACDC` | 9 sprites, tiles `C1 C9 CF D9 DB E9 EB F9 FB`, attr `$42`/`$43`, x 145-174, y 114-142 |
| green enemy | 3 | `$0A` | `$20` | `$00` | `$05` | `$1D` (29) | id 4 `$ACB8` | 4 sprites, tiles `AC AE BC BE`, attr `$21`, x 234-242, y 67-83 |

The records, from bank 3: `$ACDC` = `18 F8 02 00 5E 11 84 00 00 00 04 10`,
`$ACB8` = `1D FF 05 FF 01 05 20 00 05 00 FF 88`. The occupied byte is
`(byte5 & $7F) << 1` and state/behaviour/kind are bytes 6/7/8, matching the
slots exactly. (`live_slots_decode_the_route_capture` and
`route_slots_match_their_spawn_records` check this against the committed slice
`crates/rotj-core/tests/data/actors-route-900-wram.bin`.)

**The names are from the screenshot, not proven sprite tables.** "Orange
soldier/thug" and "green enemy" are what the committed
`tools/emu/route-dense/shot-900.png` shows; the tiles are the OAM bytes above.
Not established: the names' in-game identity, and the sprite-tile -> record map
beyond these two records.

**Slots 1 and 2 are not drawn** in this scene: both have `$7D50 = $81`, and no
distinct sprite cluster tracks their spawns. The drawn actors here have
`$7D50 = $84` (slot 0) and `$20` (slot 3). This is consistent with `$81` being a
dormant sub-state, but the state -> draw decision is **not** traced, so it is a
lead.

### 7g. The `route2` capture: every stage 1-1 spawn on the route (trace)

`tools/emu/route2-route` (`dump.lua` `INPUT=route2`: Right held plus a bunny-hop
that presses A while grounded and holds it through the whole airborne phase;
`WATCH` on `$7CC0-$7CDF` / `$7D50-$7D6F` / `$7DB0-$7DCF`) logs every slot-array
write from gameplay start to the frame-1150 room change. Three program counters
spawn: `$8879` writes `$7CC0,x` (occupied), `$887F` writes `$7D50,x` (state) and
`$888B` writes `$7DB0,x` (type) — the same spawner PC as §7f. `$D7AB`/`$D7B1`/
`$D7BD` are the level-start slot init and `$B5ED` is the slot clear.

| frame | slot | `$7CC0` | `$7D50` | `$7DB0` | PC | screenshot |
| --- | --- | --- | --- | --- | --- | --- |
| 14 | all | `$FF` | `$FF` | `$FF` | `$98FD` | boot fill |
| 606 | all | `$00` | `$00` | `$00` | `$B5ED` | level-init clear |
| 607 | 0 | `$B2` | `$81` | `$68` | `$D7AB` | `screen-808.png` |
| 607 | 1 | `$C6` | `$00` | `$00` | `$D7AB` | - |
| 607 | 2 | `$06` | `$81` | `$68` | `$D7AB` | - |
| 808 | 0 | `$22` | `$84` | `$00` | `$8879` | `screen-808.png` (orange soldier) |
| 816 | 1 | `$06` | `$81` | `$6A` | `$8879` | `screen-816.png` |
| 852 | 3 | `$0A` | `$20` | `$05` | `$8879` | `screen-852.png` (green) |
| 963 | 4 | `$0A` | `$20` | `$06` | `$8879` | `screen-963.png` (green) |
| 1031 | 2 | `$0A` | `$20` | `$05` | `$8879` | `screen-1031.png` (green) |
| 1075 | 5 | `$02` | `$61` | `$00` | `$8879` | `screen-1075.png` |
| 1150 | all | `$00` | `$00` | `$00` | `$B5ED` | `screen-1152.png` (room-change clear) |
| 1183 | 24 | `$04` | `$08` | `$00` | `$D7AB` | `screen-1183.png` |
| 1183 | 0 | `$06` | `$00` | `$61` | `$D7AB` | `screen-1183.png` |

- The occupied byte is `(record byte5 & $7F) << 1` and `$7D50`/`$7DB0` are
  record bytes 6/8, so slots 0 and 3 reproduce §7f's records exactly: slot 0 =
  `$22`/`$84`/`$00` = `$ACDC`, slot 3 = `$0A`/`$20`/`$05` = `$ACB8`. The same
  two enemies spawn on both routes, at the same slots and PCs.
- Slot 31 (`$7CCF`/`$7D5F`/`$7DBF`) is a live actor whose state counts down by
  `$20` each frame from frame 950 (`$7D5F`: `$F7 -> $D7 -> $B7 -> ...`, PCs
  `$8570`/`$8594`), re-seeded to `$08` at `$ACB2` (frames 950/1053/1099) and
  cleared to `$00` at `$85B5` (983/1086/1132).
- Slot frees: 641 slot 0 (`$8C56`), 686 slot 1 (`$99E8`), 1006 slot 2 (`$CD5E`).
- **Enemy names are from the screenshots** (`tools/emu/route2-route/screen-*.png`),
  not a proven sprite table: an orange soldier (slot 0) and green enemies
  (slots 2/3/4, all `$0A`/`$20`), consistent with §7f.
- The route never reaches the second room's spawns: at frame 1150 the room
  change clears all slots (PC `$B5ED`) and only the level-start init (frame 1183)
  follows. The route is stuck there (`docs/SUBLEVELS.md` §7d).

## 8. Per-level actor inventory (oracle)

Status: **the per-type dispatch is decoded; the table below is one oracle pass
and is not provably complete** (see 8d). Evidence: `tools/rotj-oracle/examples/
inventory.rs`, run with `cargo run -p rotj-oracle --release --example inventory`;
it forces `$20` at `$D3D8`, plays each level with five scripted inputs, and logs
every write to `$7CC0-$7DEF` and every instruction executed in `$8000-$9FFF`.

### 8a. The per-type handler dispatch (bank 14)

The actor update loop is `sub_CED5` at bank 14 CPU `$CED5` (bank 14 sits at
`$C000` for all of gameplay, §0):

```
$CED5  LDX #$17            ; slot cursor 23 -> 0
$CED7  STX $10
$CED9  LDY $7CC0,x
$CEDC  BNE $CEE5
$CEDE  DEC $10             ; far_CEDE: the tail every handler jumps back to
$CEE0  LDX $10
$CEE2  BPL $CED9
$CEE4  RTS
$CEE5  TYA                 ; A = the occupied byte
$CEE6  LSR                 ; type / 2
$CEE7  TAY
$CEE8  LDA $CF0D,y         ; bank table, 101 entries, $CF0D-$CF71
$CEEB  LDX #$09
$CEED  STA $E9
$CEEF  JSR $C229           ; command $09 -> map that bank at $8000
$CEF2  LDA #$1D / LDX #$0A / STA $EA / JSR $C229   ; $A000 = bank 13
$CEFB  LDX $10
$CEFD  LDY $7CC0,x
$CF00  LDA $CF72,y         ; handler low, indexed by the full occupied byte
$CF03  STA $11
$CF05  LDA $CF73,y         ; handler high
$CF08  STA $12
$CF0A  JMP ($0011)
```

So **`$7CC0` (the occupied byte) is the per-type dispatch index**: `$CF0D[type/2]`
selects the PRG bank mapped at `$8000`, and `$CF72[type]` is the 16-bit handler
entry. The occupied byte is always even (§7b), so the byte indexes a stride-2
table directly. The bank table is exactly 101 bytes (`$CF0D-$CF71` = through
type `$C8`) and `$CF72` is the stride-2 handler table for the same 101 even types
(202 bytes, `$CF72-$D03B`); its raw values are
`$00/$10/$18/$1B/$1C`, i.e. PRG banks 0/0/8/11/12 after the 128 KiB ROM wraps
them mod 16 (`tools/rotj-oracle/src/fme7.rs`; §0's observed `cmd $9` values
agree).

Some handlers then dispatch again on `$7D90`/`$7DB0` through the same idiom
(`disasm/MAP.md` §11): bank 11 `$8000` (the handler of type `$28`) indexes `$81D7`
(`$7DB0`, the jump target `$11`), `$81DF`/`$81ED` (`$7D90`, pointers `$13`/`$15`)
and `$7DF0`; bank 11 `$8A10` maps `$7D90` -> `$8A88`; bank 12 `$9249` maps
`$7DB0` -> `$9378`/`$9380`. The example prints the primary (`$7CC0`) dispatch.

### 8b. Inventory table (whole-game TAS)

Evidence: `tools/rotj-oracle/examples/inventory_tas.rs`, run with
`cargo run -p rotj-oracle --release --example inventory_tas` (13 s). It replays
the published TAS (`tools/emu/fetch-tas.sh`, `tools/emu/tas/inputs.csv`) **once**
from power-on with a write watch on the slot banks `$7CC0-$7DFF` and peeks on the
actor engine (bank 13 `$A9C4` contact entry, `$AA91` weapon-scan hit, `$A8AD`
on-screen projection, `$BE12` boss-HP setup, all read for the slot cursor `$10`).
This supersedes the five-scripted-run table an earlier pass built.

Columns: `$7CC0` type (the dispatch index of §8a); the level(s) (`$D94E[$20]`
start areas, any area folded in by `$D57D`); every `area:room` (`$0400:$0402`) it
appears alive in; the `$CF72`/`$CF0D` handler and bank; the first and last frame
alive; the slots it occupied; the PCs that wrote `$7CC0` (`$D7A8` bank 14 =
level-start records, `$8876` bank 2 = runtime spawner; `$ACAF`/`$CCC7`/… are
handler-owned children); the set of `$7D50` states and `$7DD0` phases; whether
`$A9C4` ever ran for its slot (`contact`); whether the `$AA57` scan ever found it
(`weapon`, through `$AA91`); how many frames it was projected on screen (`$A8AD`,
`$94`/`$95` < 240); and a `scene` frame — on screen, with the most distinct
`$7DD0` phases in the next 300 frames. **`B`** marks a boss candidate (§8b.1).

| type | level(s) | area:room | handler | first | last | slot(s) | spawner | `$7D50` states | `$7DD0` phases | contact | weapon | on-screen f. | scene |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `$02` | 0,2,5,7,8,9,10,12 | 00:00 00:02 06:00 06:02 0C:00 0C:02 0E:00 12:00 16:00 18:00 1A:00 1A:02 20:00 20:02 | `81D4@8` | 565 | 39149 | 0+2+3+4+24+25+28 | 8876@02 | 01+02+61+81+C1 | 00+FF | 0 | 0 | 0 | - |
| `$04` | 0,2,8,9 | 00:02 06:02 16:00 18:00 | `823C@8` | 646 | 28784 | 24+25+27 | 8876@02+D7A8@0E | 04+08 | 00 | 0 | 0 | 0 | - |
| `$06` | 0,1,2,4,5,6,7,8,10,12 | 00:00 00:02 00:04 02:00 06:00 06:02 0A:00 0C:00 0C:02 10:00 12:00 12:04 16:00 1A:00 1A:02 20:00 20:02 | `823C@8` | 317 | 38970 | 0+1+2+3+4+8+24+25+26 | 8876@02+CCC7@0E+D7A8@0E | 00+01+02+03+04+05+80+81 | 00 | 1 | 1 | 4219 | 351 |
| `$08` | 0,1,2,4,5,6,7,8,9,11,12 | 00:00 00:02 00:04 02:00 06:00 06:02 0A:00 0C:00 0C:02 10:00 12:00 16:00 18:00 1C:00 20:00 20:02 | `8506@8` | 429 | 39149 | 10+11+12+13+14+15+24+25+26+27+28 | 8876@02+ACAF@0D | 00+01+07+17+18+27+37+38+…(26) | 00+E0 | 0 | 0 | 5466 | 814 |
| `$0A` | 0,2,3,12 | 00:00 06:00 06:04 08:00 20:04 22:00 | `85FD@8` | 492 | 39520 | 0+1+2+24+25+26 | 8876@02+D7A8@0E | 03+04+05+20+80+81+82+86+…(9) | 00+01+02+03+FF | 1 | 0 | 213 | 494 |
| `$0C` | 0,7 | 00:04 12:02 12:06 14:00 | `8771@8` | 1306 | 26100 | 0+1+24 | 8876@02+D7A8@0E | 00 | 00+01 | 1 | 1 | 309 | 1323 |
| `$0E` | 0,11 | 00:04 1C:00 1E:00 | `8B44@8` | 1485 | 36659 | 2+24+25 | 8876@02+9460@00 | 00+01+02+03+04+05+60 | 00 | 0 | 0 | 0 | - |
| `$10` | 0,1,2,3,4,5,6,7,8,9,10,12 | 00:00 00:02 00:04 02:00 06:00 06:02 06:04 08:00 0A:00 0C:00 0C:02 10:00 12:00 12:02 16:00 18:00 1A:00 20:00 20:02 20:04 | `8B55@8` | 442 | 39335 | 0+1+2+3+4+8+26 | 8876@02+A5D2@0D | 80+81+82+83+84+87 | 00+01+02+03+04+05+06+07+…(241) | 1 | 1 | 3786 | 3968 |
| `$12` | 1 | 02:00 04:00 | `8C56@8` | 2116 | 6095 | 0 | D7A8@0E | 60 | 00+01+02+03+04+05+06+07+…(17) | 0 | 0 | 0 | - |
| `$14` | 1 | 02:00 04:00 | `8D70@8` | 2278 | 6095 | 19+20+21+22+23 | 8D19@08 | 40 | 00+01+02+03 | 0 | 0 | 2755 | 2279 |
| `$16` | 6,7 | 10:00 12:00 | `8F77@8` | 15677 | 20083 | 0 | D7A8@0E | 60 | 00 | 0 | 0 | 0 | - |
| `$18` | 1,5 | 02:00 0C:00 | `8FD9@8` | 2392 | 13933 | 13+15 | 8545@00+8EAB@08 | 00+02+04+06+08+0A+0C+0E+…(128) | E0+F0 | 0 | 0 | 2229 | 2392 |
| `$1A` | 5,7 | 0C:00 12:02 12:04 | `8205@8` | 13933 | 23806 | 0+1+3 | 8876@02 | 62+E0+E1 | 00 | 0 | 0 | 0 | - |
| `$1C` | 7 | 12:02 12:06 | `933A@8` | 20499 | 23856 | 0 | D7A8@0E | 20+FF | 00 | 0 | 0 | 0 | - |
| `$1E` | 0,1,2,5,7,8,10 | 00:02 02:00 06:00 0C:00 12:00 16:00 1A:00 | `9364@8` | 781 | 32565 | 0+1+2+3 | 8876@02 | 00+84 | 00+01 | 1 | 1 | 980 | 781 |
| `$20` | 1,2,5,7,8,10 | 02:00 06:00 0C:00 12:00 16:00 1A:00 | `94B2@8` | 3843 | 32571 | 22+23 | 9497@08 | 00+01 | 00+01+03 | 0 | 0 | 675 | 3844 |
| `$22` | 0,2,7,10,12 | 00:00 00:02 06:04 12:02 12:06 1A:00 20:00 | `9555@8` | 455 | 38617 | 0+1+2+3 | 8876@02+D7A8@0E | 00+84 | 00+01+02+03 | 1 | 1 | 4165 | 21540 |
| `$24` | 1,2,3,6,7,8,9,12 | 02:00 06:00 06:02 08:00 10:00 12:02 12:06 16:00 18:00 20:02 | `9823@8` | 2566 | 39125 | 16+17+18+19+20+21+22+23 | 81B6@0B+8A3A@0C+8CAE@0B+9014@00+97FF@08+9C09@08 | 00 | 00+01+02+03+08 | 0 | 0 | 9992 | 21452 |
| `$26` | 1,2 | 02:00 06:02 | `98D6@8` | 2508 | 8090 | 0+1+2 | 8876@02+D7A8@0E | 00 | 00+02+04 | 1 | 0 | 548 | 2517 |
| `$28` | 7 | 12:02 12:06 | `8000@11` | 21028 | 25331 | 0+1+2+3 | 8876@02+D7A8@0E | 20 | 00 | 1 | 1 | 2071 | 21029 |
| `$2A` | 7 | 12:02 | `8284@11` | 20964 | 21471 | 0 | 8876@02 | 00 | 00+02+04 | 1 | 1 | 442 | 21030 |
| `$2C` | 7 | 12:02 | `851A@11` | 21222 | 21293 | 12+13+14+15 | 84FF@0B | 40 | 00+80 | 0 | 0 | 184 | 21223 |
| `$2E` | 5,7 | 0C:00 12:00 | `85A7@11` | 13605 | 20283 | 1+2 | 8876@02 | 84 | 00+02+04 | 1 | 1 | 315 | 13613 |
| `$30` | 5,7 | 0C:00 12:00 | `887A@11` | 13605 | 20465 | 13+15 | 86E2@0B | 00+40 | 00+80 | 0 | 0 | 37 | 13670 |
| `$32` | 1,2,3,4,5,6,7,8,9,10,11,12 | 02:00 04:00 06:04 08:00 0A:00 0C:00 0C:02 0E:00 10:00 12:00 12:06 14:00 16:00 18:00 1A:00 1C:00 1E:00 20:04 22:00 | `8950@11` | 5810 | 39520 | 0+1+2+3+8 | 8876@02 | 40+60+61+FF | 00+01+02+03+04+05+06+07+…(194) | 0 | 0 | 0 | - |
| `$34` | 1,2,5,6,7,8,11,12 | 04:00 06:00 0E:00 10:00 14:00 16:00 1E:00 20:00 22:00 | `8A03@11` | 6261 | 40894 | 0 | D7A8@0E | 5B+60+FF | 00+17+1A+1C+1D+1F+AD+FF | 0 | 0 | 0 | - |
| `$36` | 2,3,6,9 | 06:00 08:00 10:00 18:00 | `8A92@11` | 7424 | 30975 | 0+1+2+3+4 | 8876@02 | 00+84 | 00+01 | 1 | 1 | 1126 | 7426 |
| `$38` | 1,2 | 04:00 06:00 | `8D1F@11` | 6261 | 7203 | 1 | D7A8@0E | 00+FF | 00+03+04+FF | 0 | 0 | 325 | 6282 |
| `$3E` | 6 | 10:00 | `92B9@11` | 17502 | 19408 | 1+3 | 8876@02 | 00 | 00+02+04 | 1 | 1 | 837 | 18521 |
| `$40` | 6 | 10:00 | `9410@11` | 17942 | 19099 | 21+22+23 | 93F0@0B | 00 | 01 | 0 | 0 | 504 | 17943 |
| `$42` | 7,8 | 14:00 16:00 | `947A@11` | 26260 | 27510 | 1 | D7A8@0E | 00+FF | 00+01+FF | 0 | 0 | 622 | 26808 |
| `$44` | 7 | 14:00 | `9739@11` | 26380 | 26897 | 16+17+18+19+20+21+22+23 | 96EE@0B | 00 | 00+D8+DD+E0+E2+E4+E7+E8+…(21) | 0 | 0 | 1896 | 26381 **B** |
| `$48` | 7,8 | 14:00 16:00 | `9902@11` | 26260 | 27510 | 2+3 | D7A8@0E | 60+FF | 00+01+02+03+04+05+06+07+…(256) | 0 | 0 | 1564 | 26773 |
| `$4A` | 7 | 14:00 | `9A4D@11` | 26426 | 26904 | 16+17+18+19+20+23 | 9A13@0B | 00 | 00+01 | 0 | 0 | 908 | 26426 **B** |
| `$4C` | 7,8 | 14:00 16:00 | `9ADE@11` | 26260 | 27510 | 4 | D7A8@0E | 60+FF | 00+80+FF | 0 | 0 | 0 | - |
| `$4E` | 7 | 14:00 | `9B62@11` | 26677 | 26692 | 15 | 9B4C@0B | 00 | 00 | 1 | 1 | 0 | - **B** |
| `$50` | 1,2,5,6,7,8,11,12 | 04:00 06:00 0E:00 10:00 14:00 16:00 1E:00 20:00 | `9C13@11` | 6576 | 38084 | 13+14+15 | B1E6@0D | 00+64+A1+FF | 00+5E+FF | 0 | 0 | 0 | - |
| `$52` | 1,2,5,6,7,8,11,12 | 04:00 06:00 0E:00 10:00 14:00 16:00 1E:00 20:00 22:00 | `9CFC@11` | 6580 | 40265 | 8+9+10+11+12+13+14+15 | 90BF@0C+9CBB@0B | 00+5D+A0+A1+FF | 00+01+5D+5E+FF | 0 | 0 | 2314 | 37460 |
| `$54` | 3,4,9,10 | 08:00 0A:00 18:00 1A:00 | `9D49@11` | 8600 | 32007 | 0 | D7A8@0E | 41+FF | 00+FF | 0 | 0 | 0 | - |
| `$56` | 3,9 | 08:00 18:00 | `9DF9@11` | 8600 | 28835 | 1 | D7A8@0E | 41 | 00+10+20+30+40+50+60+70+…(16) | 0 | 0 | 0 | - |
| `$58` | 5 | 0C:02 0E:00 | `9E23@11` | 13982 | 14459 | 0 | D7A8@0E | 40 | 00 | 0 | 0 | 0 | - |
| `$5A` | 5 | 0C:02 0E:00 | `9E7A@11` | 13982 | 14459 | 1 | D7A8@0E | 40 | 00+01+02+03+04+05+06+07+…(65) | 0 | 0 | 0 | - |
| `$5C` | 4,5 | 0A:00 0C:00 | `9EC2@11` | 11925 | 13169 | 0+1+2+3+4+5+6+7 | D7A8@0E | 40 | 00 | 0 | 0 | 0 | - |
| `$5E` | 5 | 0C:00 | `831D@0` | 13288 | 13896 | 0 | 8876@02 | 40 | 00+04+08+0C+10+14+18+1C+…(33) | 0 | 0 | 1077 | 13291 |
| `$60` | 5 | 0C:00 | `84F4@0` | 13366 | 13900 | 1+2+3 | 8876@02 | 40 | 00+01 | 0 | 0 | 253 | 13374 |
| `$62` | 4 | 0A:00 | `8580@0` | 12021 | 12857 | 8+9+10+11 | 8876@02 | 00 | 00 | 0 | 0 | 386 | 12022 |
| `$64` | 3 | 08:00 | `8606@0` | 9581 | 10758 | 1+2 | 8876@02 | 40 | 00+80 | 1 | 0 | 1536 | 9581 |
| `$66` | 3,9 | 08:00 18:00 | `8665@0` | 9032 | 30577 | 11+12+13+14+15 | 8762@00 | 00 | 00+80 | 1 | 1 | 876 | 9042 |
| `$68` | 3,9 | 08:00 18:00 | `86F5@0` | 9017 | 30562 | 1+2+3 | 8876@02 | 00 | 00+01+02+03+04+05 | 0 | 0 | 0 | - |
| `$6A` | 3,9 | 08:00 18:00 | `877D@0` | 9263 | 30019 | 6+7+8+9+10+11+12+13+14+15 | 8762@00 | 00 | 00+80 | 1 | 1 | 3102 | 9273 |
| `$6C` | 3,9 | 08:00 18:00 | `87F9@0` | 9734 | 30347 | 1+2+3+4 | 8876@02 | 00 | 00+80 | 1 | 1 | 613 | 9735 |
| `$6E` | 3,9 | 08:00 18:00 | `891D@0` | 8864 | 30165 | 1+2 | 8876@02 | 81 | 00+80 | 1 | 1 | 251 | 8864 |
| `$70` | 3,9 | 08:00 18:00 | `89B1@0` | 8884 | 29060 | 1 | 8953@00 | 81+82 | 00+01+02+03+04+05+06+07+…(84) | 1 | 1 | 87 | 28978 |
| `$72` | 3,9 | 08:00 18:00 | `8A7D@0` | 10246 | 29502 | 1+2 | 8876@02 | 00 | 00+01 | 1 | 1 | 259 | 10246 |
| `$74` | 3 | 08:00 | `8C9C@0` | 10319 | 10379 | 20+21+22+23 | 8C31@00 | 00 | 00+80 | 0 | 0 | 213 | 10320 |
| `$76` | 2 | 06:04 | `8D3F@0` | 8091 | 8139 | 0 | D7A8@0E | 84 | 00 | 0 | 0 | 0 | - |
| `$78` | 8,9 | 16:00 18:00 | `8D4F@0` | 27520 | 28784 | 0 | D7A8@0E | 40 | 00 | 0 | 0 | 0 | - |
| `$7A` | 8 | 16:00 | `8DB5@0` | 27584 | 28634 | 9+13+14+15 | 8DAC@00 | 00+40 | 00+01+02+03+04+05+06+07+…(17) | 0 | 0 | 181 | 27585 |
| `$7C` | 8 | 16:00 | `8DFB@0` | 27520 | 28652 | 1+2+3+4 | 8876@02+D7A8@0E | 00+40 | 00+01+02+03 | 1 | 1 | 383 | 28037 |
| `$7E` | 8 | 16:00 | `9032@0` | 27599 | 28456 | 12+13+14+15 | 92CC@00 | 40 | 00+01+02+03+04+06 | 0 | 0 | 806 | 27931 |
| `$80` | 8 | 16:00 | `9297@0` | 27599 | 28488 | 1+2+3 | 8876@02 | 40 | 00 | 0 | 0 | 645 | 27600 |
| `$82` | 10,11 | 1A:04 1C:00 | `931F@0` | 33305 | 33596 | 0 | D7A8@0E | 80 | 00 | 0 | 0 | 0 | - |
| `$84` | 11 | 1C:00 | `9361@0` | 33598 | 36586 | 0 | D7A8@0E | 61 | 00+01+02+FE | 0 | 0 | 0 | - |
| `$86` | 11 | 1C:00 1E:00 | `9516@0` | 33639 | 36659 | 11+12+13+14+15 | 93D1@00 | 08+18+28+38+48+58+68+78+…(16) | 10 | 0 | 0 | 11782 | 33640 |
| `$88` | 5 | 0C:02 | `95F6@0` | 14141 | 14283 | 2+3+4 | 8876@02 | 00 | 00 | 1 | 1 | 182 | 14141 |
| `$8A` | 4 | 0A:00 | `9762@0` | 12097 | 13064 | 8+9 | 8876@02 | 00 | 00+01 | 1 | 1 | 341 | 12099 |
| `$8C` | 4,5 | 0A:00 0C:00 | `98A3@0` | 12098 | 13169 | 23 | 985D@00 | 00 | 00+08+10+18+20+28+30+38+…(30) | 0 | 0 | 302 | 12783 |
| `$8E` | 5,6 | 0E:00 10:00 | `993A@0` | 14628 | 15675 | 1 | D7A8@0E | 00+FF | 00+01+02+03+04+05+FF | 0 | 0 | 433 | 14672 |
| `$94` | 11 | 1C:00 | `8000@12` | 33598 | 36586 | 1 | D7A8@0E | 40 | 00+80 | 0 | 0 | 0 | - |
| `$96` | 11 | 1C:00 | `8065@12` | 33632 | 36483 | 15 | 8043@0C | 00 | 00+01+02 | 1 | 1 | 30 | 33652 |
| `$98` | 11 | 1C:00 1E:00 | `81DB@12` | 33598 | 36659 | 2 | D7A8@0E | 61 | 00 | 0 | 0 | 0 | - |
| `$9A` | 11 | 1C:00 | `8208@12` | 33956 | 36528 | 22+23 | 8180@0C | 00 | 00+03+06+09+0C+0F+12+15+…(26) | 0 | 0 | 4221 | 33957 |
| `$9C` | 11 | 1C:00 | `82F7@12` | 33598 | 33631 | 3 | D7A8@0E | 61 | 00 | 0 | 0 | 0 | - |
| `$9E` | 11 | 1C:00 | `8309@12` | 33794 | 36367 | 3+4 | 8876@02 | 20 | 00+01 | 1 | 0 | 2755 | 33794 |
| `$A0` | 10 | 1A:02 | `83CD@12` | 32657 | 32886 | 0+1 | D7A8@0E | 61 | 00 | 0 | 0 | 0 | - |
| `$A2` | 10 | 1A:02 | `8412@12` | 32674 | 32945 | 21+22+23 | 84E2@0C | 00 | 00 | 0 | 0 | 223 | 32675 |
| `$A4` | 10 | 1A:02 | `846A@12` | 32745 | 32945 | 2 | 8876@02 | 61 | 00 | 0 | 0 | 0 | - |
| `$A6` | 11,12 | 1E:00 20:00 | `84EB@12` | 36825 | 38084 | 1 | D7A8@0E | 00+5C+FF | 00+01+02+AE+FF | 0 | 0 | 642 | 36863 |
| `$A8` | 11 | 1E:00 | `87EC@12` | 36967 | 37509 | 20+21+22+23 | 877B@0C | 00 | 10+F0 | 0 | 0 | 1071 | 36968 **B** |
| `$AA` | 11 | 1E:00 | `8870@12` | 37456 | 37553 | 12+14 | 87BD@0C | 00 | 00+01 | 0 | 0 | 192 | 37457 **B** |
| `$AC` | 12 | 20:02 | `89A1@12` | 38884 | 39149 | 1+3 | 8876@02 | 84 | 00 | 1 | 1 | 203 | 38885 |
| `$AE` | 12 | 20:02 20:04 22:00 | `8A52@12` | 38786 | 39520 | 7+8+9+10+11+12+13+14+15 | 8C20@0C | 00 | 00+02+04 | 1 | 1 | 1514 | 38793 |
| `$B0` | 12 | 20:02 20:04 22:00 | `8B92@12` | 38784 | 39520 | 0+2 | 8876@02 | 61 | 00+01 | 0 | 0 | 0 | - |
| `$B2` | 0 | 00:00 | `8C4E@12` | 317 | 350 | 0 | D7A8@0E | 81 | 00 | 0 | 0 | 0 | - |
| `$B4` | 12 | 22:00 | `8C59@12` | 39680 | 40894 | 1 | D7A8@0E | 00 | 00+01+02+03+04+05+06 | 0 | 0 | 0 | - **B** |
| `$B6` | 12 | 22:00 | `9153@12` | 39680 | 40084 | 2+3+4+5 | D7A8@0E | 00 | 00+01+02 | 1 | 1 | 12 | 39913 **B** |
| `$BA` | 12 | 22:00 | `94BC@12` | 40012 | 40084 | 21+22+23 | 92B5@0C | 00 | 00 | 0 | 0 | 212 | 40013 **B** |
| `$BC` | 12 | 22:00 | `952E@12` | 39833 | 40084 | 18+19+20+21+22+23 | 8F5F@0C | 00 | 00+FF | 0 | 0 | 344 | 39834 **B** |
| `$BE` | 12 | 22:00 | `9603@12` | 40532 | 40638 | 20+21+22+23 | 8FA0@0C | 00 | 00 | 0 | 0 | 286 | 40533 **B** |
| `$C0` | 12 | 22:00 | `96B1@12` | 40787 | 40894 | 22+23 | 9016@0C | 00 | 00+FF | 0 | 0 | 214 | 40788 **B** |
| `$C2` | 12 | 20:00 | `97B6@12` | 38180 | 38617 | 0+1+4 | 8876@02 | 40 | 00+02 | 0 | 0 | 0 | - |
| `$C4` | 12 | 20:00 | `9897@12` | 38249 | 38617 | 9+10+11+12+13+14+15 | 97F0@0C | 40 | 00 | 1 | 1 | 873 | 38250 |
| `$C6` | 0 | 00:00 | `9953@12` | 317 | 395 | 1 | D7A8@0E | 00 | 00+01 | 0 | 0 | 45 | 351 |
| `$C8` | 12 | 20:00 | `9A26@12` | 38086 | 38119 | 0 | D7A8@0E | 61 | 00 | 0 | 0 | 0 | - |
Notes:
- Attribution is by `$7CC0`; the `$7D50`/`$7DD0` columns union the writes to
  those bytes with the value sampled every alive frame, so a type that spawns in
  several states shows all of them. Types whose `$7D50`/`$7DD0` is a counter
  (`$18`, `$32`, `$48`, `$52`, `$56`, `$A6`, `$B4`, `$86`, `$8C`, `$9A`, `$5A`,
  `$6A`, `$70`, `$10`, …) show a long run; `…(n)` gives the count.
- Non-spawn `$7CC0` writers are filtered: the bank-2 level/screen loader
  (`$8A9F`/`$8AED`) and the boot `$FF` fill (`$98FB`). The old columns `kind`,
  `behaviour` and the first-record `state` are gone because they only described
  the first spawn; the full `$7D50` set is given instead.
- §8c (shared types) and §8d (handler sizes) below are from the earlier
  scripted-run pass and were not regenerated here.

### 8b.1 Boss candidates

The published TAS never arms the `$BA = 5` / `$D58F` level-end path, so the
task's primary rule finds no boss room. Evidence: `$BA` is written only with
`$00`, `$01`, `$02`, `$03` (PCs `$81F3`, `$822A`, `$9537`, `$89D1`, `$9C62`,
`$935C`, `$8B48`, `$D4F7`, `$D57A`); no instruction in `$D58F-$D5B5` executes;
and the bank-13 state-4 branch `$AB69` runs 3 times but `$0477` never reaches
`$0E` (`$BE4E`/`$AB69`, bank 13).

The game's own boss signal is the HP setup at bank 13 `$BE12`:
`LDA $040F; ASL; TAY; LDA $BE4E,y; STA $AA; LDA $BE4F,y; STA $A9` — HP
`0FA0 2710 1F40 2EE0 61A8` (4000/10000/8000/12000/25000). It fires exactly five
times, at frames 6281, 14648, 26280, 36845 and 39700, once per stage-end area
`$0400` = `$04`, `$0E`, `$14`, `$1E`, `$22` — the ends of stages 1-2, 3-2, 4-2,
6-2 and 7-1, which matches the manual ("bosses at the ends of stages 1, 3, 4, 6,
7"). A type is marked **`B`** when *every* room it is seen in is one of those
five areas, i.e. it is the boss or a part confined to the boss fight, not a
shared regular enemy. That yields `$44`/`$4A`/`$4E` (stage 4, area `$14`),
`$A8`/`$AA` (stage 6, area `$1E`) and `$B4`/`$B6`/`$BA`/`$BC`/`$BE`/`$C0`
(stage 7, area `$22`). Stages 1 and 3 have **no** type exclusive to their boss
area, so their bosses are not separable by this criterion: the level-start
spawns there are `$38`+`$34` (stage 1) and `$8E`+`$34`+`$58`/`$5A` (stage 3),
and `$34` is a shared, never-drawn actor (0 on-screen frames, nine areas) that
is alive when `$BE12` fires in all five boss areas.

### 8c. Types seen in more than one level

These types are shared between levels, so porting one handler covers all of them:

```
02 in levels [0, 2, 5, 7, 10, 12]
04 in levels [0, 2]
06 in levels [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12]
08 in levels [0, 1, 2, 5, 8, 11]
0A in levels [0, 2]
18 in levels [1, 5]
1A in levels [5, 7]
1E in levels [2, 5, 7, 8, 10]
20 in levels [2, 5, 8, 10]
22 in levels [0, 7, 10, 12]
24 in levels [0, 1, 3, 6, 8, 9, 10, 12]
2E in levels [5, 7]
30 in levels [5, 7]
36 in levels [2, 3, 6, 9]
54 in levels [3, 9]
56 in levels [3, 9]
5C in levels [3, 4]
62 in levels [3, 4]
66 in levels [3, 9]
68 in levels [3, 9]
6A in levels [3, 9]
6C in levels [3, 9]
6E in levels [3, 9]
```

### 8d. Rough handler size

Distinct instruction PCs executed in `[entry, next entry)` of the handler's own
bank, over every level's run: an upper bound (a shared subroutine laid out between
two entries counts for the earlier one), enough to size a port task.

| handler | bank | instructions reached |
| --- | --- | --- |
| `$8000` | 11 | 18 |
| `$8000` | 12 | 36 |
| `$8065` | 12 | 148 |
| `$81D4` | 8 | 19 |
| `$81DB` | 12 | 19 |
| `$8205` | 8 | 18 |
| `$8208` | 12 | 97 |
| `$823C` | 8 | 39 |
| `$82F7` | 12 | 8 |
| `$8309` | 12 | 41 |
| `$831D` | 0 | 93 |
| `$83CD` | 12 | 24 |
| `$8412` | 12 | 38 |
| `$846A` | 12 | 45 |
| `$84F4` | 0 | 54 |
| `$8506` | 8 | 86 |
| `$8580` | 0 | 52 |
| `$85A7` | 11 | 262 |
| `$85FD` | 8 | 158 |
| `$8606` | 0 | 38 |
| `$8665` | 0 | 58 |
| `$86F5` | 0 | 51 |
| `$877D` | 0 | 46 |
| `$87F9` | 0 | 118 |
| `$887A` | 11 | 81 |
| `$891D` | 0 | 38 |
| `$8950` | 11 | 44 |
| `$89A1` | 12 | 60 |
| `$8A52` | 12 | 119 |
| `$8A7D` | 0 | 206 |
| `$8A92` | 11 | 235 |
| `$8B92` | 12 | 73 |
| `$8C4E` | 12 | 4 |
| `$8C56` | 8 | 92 |
| `$8C9C` | 0 | 70 |
| `$8D4F` | 0 | 39 |
| `$8D70` | 8 | 171 |
| `$8DB5` | 0 | 26 |
| `$8DFB` | 0 | 221 |
| `$8F77` | 8 | 42 |
| `$8FD9` | 8 | 229 |
| `$9032` | 0 | 220 |
| `$9297` | 0 | 54 |
| `$9361` | 0 | 182 |
| `$9364` | 8 | 136 |
| `$94B2` | 8 | 69 |
| `$9516` | 0 | 79 |
| `$9555` | 8 | 292 |
| `$9762` | 0 | 116 |
| `$97B6` | 12 | 75 |
| `$9823` | 8 | 49 |
| `$9897` | 12 | 49 |
| `$98A3` | 0 | 62 |
| `$98D6` | 8 | 342 |
| `$9953` | 12 | 81 |
| `$9A26` | 12 | 7 |
| `$9D49` | 11 | 44 |
| `$9DF9` | 11 | 20 |
| `$9EC2` | 11 | 56 |

**Completeness.** These runs did **not** advance the level screen index `$0400`
past the level's start area, so only the first area's spawns (plus whatever the
five routes triggered) are covered. The game's own schedule (`$7410` loaded from
bank 3 `$A2D4[$0400]`, §7b) holds more types per level (e.g. 16 for level 0), and
the runtime spawner (`$8876`) is gated by the level's scroll cursor `$CC`
(bank 2 `$86xx`), which these routes only partly advanced. A later pass should
drive `$CC` (or a full route) to close the gap; the dispatch in 8a is global and
does not change.

## 9. The port's actor core (`rotj_core::actor`)

Design: `docs/specs/2026-10-14-actors-design.md`; plan:
`docs/plans/2026-10-14-actors.md`. Every claim below is pinned by an oracle
test in `crates/rotj-core/src/actor/`.

### 9a. The frame and the shared engine

The gameplay loop is bank 14 `$D3F9`: HUD (`$DB9D`), slots 24-31 (`$CE7F`),
player movement (bank 4 `$800B`), scroll speed (`$D2F3`), camera, column
streaming and column spawns (bank 2 `$8528`), player animation and draw (bank 4
`$84AB`), slots 24-31 drawn (`$DC87`), weapons and `$7BD0` (bank 5 `$8000`), the
actor loop (`$CED5`, 8a), OAM finish (`$C145`), then the NMI wait that steps the
RNG. Gameplay code only reads `$F9/$FA`, so the RNG is constant within a frame.

Handlers are scripts over bank 13: `$A000` loads a slot into `$90-$A6`, `$A79B`
integrates velocity, `$A852` projects to the screen (on, near `$80` within
128 px, far `$C0`; rows 15-19 are bottom-anchored with `$9A` bit 6), `$A9C4`
checks contact with the player and falls into `$AA29`, the scan of the `$7BD0`
weapon slots, and `$A8E4` ends the frame (gravity +6 clamped on the low byte,
x velocity snapped to 0 on its low byte, `$7C00` bits 5-7 from `$99`). Bank 8
draws through `$8FC4` (bank 1 at `$A000`, then `$C247`). Metasprites are
`[n][tile, x, attr, y]×k[chr]`; `$C247` allocates OAM entries 49 apart from
`$FB` and `$C145` parks the rest and adds `$4C` to the start (`$0471`).

### 9b. Spawners

- Column spawn, bank 2 `$87C5`, called only when the 8 px scroll accumulator
  `$BE` wraps: scroll lock from the left-edge cell (bits 7-6 = `01`), spawn
  from the leading edge, `$0440[(CC >> 4) + right] | (CC & $0F)`.
- Room start, bank 14 `$D714`: every pending cell of a 16-cell row from
  `$0403`.
- Vertical rooms (`$DD` bit 6) use `$8779`, the row twin of `$87C5`; both read
  `$0440` only, also in rooms two nametables wide or tall (`$DD` bit 7).

### 9c. The engine: census and ports

`engine::tests::census_of_stage_1_1` replays both stage 1-1 inputs plus scenes
with the player next to each live actor and pins which routine each handler
family calls (`JSR`/`JMP` to `$A000+`, bank switches excluded). Every routine
below is ported and diffed on its own (`assert_routine`: the game's entry to its
`RTS`, every RAM/WRAM byte either side changed plus its register and `$20-$2F`
results), from route, `stage1-inputs.csv`, brawl and up-close scenes. Calls made
by the player's code are included where the player calls the same routine.

| routine | port | families (handler entry) | notes |
| --- | --- | --- | --- |
| `$A000` | `engine::load` | all but `$8506`, `$8C4E` | |
| `$A025` | `engine::player_proximity` | `$81D4` | |
| `$A0CE`-`$A569` | `engine::queue_block` | `$823C` (`$82F9`) | the nametable rows and attribute bytes of a tile block; its bottom clip (`$A148`-`$A15E`) is unreachable and dropped (it would leave `$2D` stale); another caller returns to bank 8 `$89D7`. In a room two nametables tall a block on the lower page (row bit 4) goes to `$2400` (`$A264`: `LDA #$24`, an immediate the port first read as `$24`, which put TAS 14128's blocks at `$14xx`) |
| `$A56E`/`$A601` | `engine::dying` | `$9555`, `$823C` | hit flash; dying animation; becomes item `$10` |
| `$A79B` | `engine::integrate` | `$85FD`, `$8B55`, `$9555`, `$9953` | |
| `$A7DD` | `collision::feet` | `$8B55`, `$9555`, `$9953` | |
| `$A7F0` | `collision::head` | `$9555` | |
| `$A80A`/`$A837` | `collision::ahead`, `collision::column` | `$9555` | |
| `$A852` | `engine::on_screen` | all but `$81D4`, `$8C4E` | |
| `$A8E4`/`$A934` | `engine::end_frame`, `engine::store` | `$823C`, `$85FD`, `$8B55`, `$9555`, `$9953`; `$A934` alone by `$9823` | |
| `$A95E` | `collision::corners` | `$9555` | |
| `$A9C4`-`$AB38` | `engine::contact`, `engine::weapon_scan` | `$823C`, `$85FD`, `$8B55`, `$9555` | the shield deflection (`$AA98`, contact bit 7) is unexercised |
| `$AB39` | `engine::collect` | `$8B55` | |
| `$ABDF` | `engine::player_dx` | `$85FD`, `$9555` | |
| `$AC06` | `engine::tile_bits` | `$9555`, `$9823` | |
| `$AC16` | `engine::spawn_parts` | `$85FD`, `$823C` | type `$08`, four parts |
| `$AE0A` | `engine::copy_metatiles` | `$8771` | the rows only; its attribute pass (`$AE2F`, `$29` bit 7) is not ported, `$8771` never asks for it |
| `$AEE3` | `engine::touch` | `$9823` | |
| `$AF97` | `engine::boss_flash` | `$8D1F` | |
| `$AFD6`-`$B0F8` | `engine::boss_contact` | `$8D1F` | `$B076` is its tail; the TAS boss is only hit by projectiles, so the touch paths (`$B040`, `$B05F`) are unexercised |
| `$B1C4` | `engine::boss_defeated` | `$8D1F` | spawns type `$50`; with no free slot the port spawns nothing (the game writes past the arrays) |
| `$AF38` | `engine::free_child_slot` | `$9555` | |
| `$AF6A`/`$CD5F` | `collision::ride_here`, `collision::ride` | `$8B55`, `$9555` | riding (`$CDD9`) proven on the red-brick room's platform; `$CE3E` (`$7E28,x = 2`, the second-class `$0C` of stage 4-2) probes `$CAFE` at the working position made screen-relative (`y - $CE:$CF`, `| $3000`, so pages 16-31) with `$22` left as the caller set it, then adds the camera back and steps up 1, and always answers "riding" (chain, TAS 20499-26100) |
| `$B17F` | `engine::hurt_player` | `$85FD`, `$9555`, `$9823` | |
| `$C247` | `engine::draw` (`OamBuilder::draw`) | all that draw | `$C26F` is its own bottom-anchor path |
| `$C93B` | `collision::solid` | `$9823` | |
| `$C99D` | `collision::wall` | `$9555` | class handlers `$CA4E`, `$CA66` panic (unreached); `$CA86` (class 5) is the plain push; `$CAA9` (class `$10`, spikes): unless `$B9` bit 7, damage `$10`, `$9F` bit 7 (kept in `$A8`), `$0472` bit 7 cleared, `$C2 = $40 = $FF`, `$B6 = 0`, then the push (`spikes_match_caa9`, stage 7-1's spike wall, TAS 38205 with the player placed) |
| `$CAFE` | `collision::floor` | `$8B55`, `$9555`, `$9953` | `$CC6D` (class `$0C`, stage 6-1's crumbling blocks, TAS 32911-33255): nothing from the metatile's lower half (`$22` bit 7), else the `$CB39` bounce, a second-class `$06` in the first free slot 24-31 (none with `$B9` bit 7) at (`$21 & $FE`:`$F0`, `$23`:`$F0`), and the landing; `$CD09` (class `$10`) is `$CAA9`'s spikes then `$CB39`, unexercised; `$CC1A` (class `$0A`, stage 3-1, TAS 12113-13021) lands like `$CB39` but brakes only every 32 frames (`$A6 & $1F`), by a quarter of the difference, and keeps `$A6` |
| `$C0C6` | `Vram::push` | `$A0CE`, `$D1F5` | the queue in `$0100-$0180` with its pointer `$0181` (`Effects::vram`) |
| `$CD50` | `engine::despawn` | `$823C`, `$9555` | |
| `$D1F5` (`$C199`, `$D24A`) | `engine::queue_counters` | `$8A03` | the status bar's number and the boss's hit points (`Effects::boss_hp`) as tiles |
| `$D274` | `engine::level_end` | `$8A03`; also bank 11 `$9E69` (type `$58`, unported) and the player's `$9397` | the player frozen in motion 7, `$047A`/`$0481`/`$B8`/`$0480`/`$0465` set, the `$7BD0` projectiles freed; the whole-game TAS never calls it, so `k34::tests::level_end_matches_d274` diffs it on area 4 with `$B2:$B3` poked to 0 |

`$CAFE`/`$C99D` look the metatile up as the game does: row `(y >> 8) & 15`,
column `rows[page] | ((x >> 8) & 15)`, page `(y_hi & $10) | (x_hi >> 4)`
(swapped on vertical levels), class `$7340[id]` (ids from `$D0` read the
schedule bytes that follow). This is the game's solid-tile rule; the port's
player still uses the flood-fill shortcut.

### 9d. Ported types and readiness

| kind | handler | family module | proof |
| --- | --- | --- | --- |
| `$02` | bank 8 `$81D4` | `families/k02.rs` | `matches_the_oracle` (stage1 968-1023, phase 0, on screen, never drawn); the trigger path (frame 1024, `$BA`/`$BB`) is unported — `$BB` has no `Effects` field |
| `$0A` | bank 8 `$85FD` | `families/k0a.rs` | `matches_the_oracle` (route 894-995, phase 0, on screen), `matches_the_oracle_up_close` (route 953, player beside it, 200 loops, phases 0-3, the `$08` child spawn), `off_screen_merges_the_last_drawn_attr` |
| `$0C` | bank 8 `$8771` | `families/k0c.rs` | `matches_the_oracle` (stage 1-1 room 4, TAS 1306+: phases 0-2, on screen, drawn, the player's weapon hits kept non-lethal by a health poke), `matches_the_oracle_at_step_five` (the `$893C` `$7DF0==5` branch), `matches_the_oracle_before_the_death` (the same scene, no poke: phases 0-1 and the `$7D70` bit-6 flash draw), `hurts_the_player_up_close` (the `$A9C4`/`$AEE3` contact); the `$89A6` dying path reads RAM `$0181` and jumps into the level-edit chain `$AE0A`/`$A0CE` (both not in 9c), so the port panics there |
| `$0E` | bank 8 `$8B44` | `families/k0e.rs` | `matches_the_oracle` (TAS 1485, its one occupied loop: arms `$BA`, frees its own slot, never drawn, phase 0, no engine/collision call), `every_hinted_state_is_ignored` (forces each `$7D50` state `00`-`05`/`60`). It also zeroes `$0408` (`Effects::r_408`) |
| `$0E` | bank 8 `$8B44` | `families/k0e.rs` | `matches_the_oracle` (TAS 1485, its one occupied loop: arms `$BA`, frees its own slot, never drawn, phase 0, no engine/collision call), `every_hinted_state_is_ignored` (forces each `$7D50` state `00`-`05`/`60`). The handler also zeroes `$0408`, absent from `Effects`; it is 0 in the observed loop, so the diff cannot see the omitted write (a `shortcut:`) |
| `$56` | bank 11 `$9DF9` | `families/k56.rs` | `matches_the_oracle` (TAS 8600 area 08:00 and 28835 area 18:00: all 16 calls, `$7DD0` `00`-`F0`, 15 erase map columns `$10`-`$FF` in 16 rows and the 16th frees the slot; never drawn, no contact, no engine/collision call). The erase loop runs through an NMI, so `assert_actor_phases`/`assert_routine` reject every bracket but the last; the test diffs each `$9DF9`-to-exit call on the level map `$6000-$6FFF`, `$7CC0` and `$7DD0` instead (no `Coverage`) |
| `$04`, `$06` | bank 8 `$823C` | `families/k06.rs` | `matches_the_oracle` (route 701-800, phase 0, on screen, no draw), `matches_the_oracle_when_hit` (the `$7D70` bit-6 flash draw), `matches_the_oracle_off_screen` (far slot freed, cell re-armed `$80`) |
| `$B2` | bank 12 `$8C4E` | `families/kb2.rs` | `matches_the_oracle` (route frame 641, its one occupied frame: phase 0, not drawn, the `$0443` clear and its own slot free, both pinned non-vacuous), `the_occupied_frame_starts_with_a_set_row_base` |
| `$B6` | bank 12 `$9153` | `families/kb6.rs` | `matches_the_oracle` (`Scene::tas(39680)`, area 22:00, slots 2-5: their whole lives, 400 loops, `$7DD0` phases 0/1/2 (400/941/84), drawn every loop, then the player's weapon kills them at 39913 and the dying `$9249` nametable queue runs; the unported `$BA` children are freed each loop), `the_phase_two_spawn_drops_a_ba_part` (the `$92AB` spawn lands a `$BA` at a running `$B6`'s position with its `$7D50`/`$7E90` cleared). Draws fixed-offset bank-9 metasprites through `$81C6`; never integrates, projects or stores (no `$A852`/`$A8E4`), calls `$A9C4` contact and no `$A79B` |
| `$B0` | bank 12 `$8B92` | `families/kb0.rs` | `matches_the_oracle` (TAS 38784-39072, area 20:02 slot 0: its whole isolated life, 288 loops, phases 0/1, never drawn, no contact; each phase-1 `$AE` child spawn to slot 15 is diffed byte for byte), `the_first_child_carries_the_handlers_bytes` (the `$AE` child's `vy`/`vx_drift` from `$F9`/`$FA`). The handler reads only its own `$7DD0`/`$7E90`/`$7E10` and calls no §9c routine; §8b's second slot (2) is unreached because isolating the type frees the `$AE` children it drops and the room ends before that spawn |
| `$C2` | bank 12 `$97B6` | `families/kc2.rs` | `matches_the_oracle` (`Scene::tas(38180)`, area 20:00: 440 loops, occupied every frame, phases 0 (405 slot-frames waiting for the player's x page `$9883[0]`) and 2 (136, spawning a `$C4` every 9 frames), never drawn — the `$C4` child is the drawn actor and is unported, so a `$D450` poke frees slots 13-15 each loop. Calls no §9c routine) |
| `$B4` | bank 12 `$8C59` | `families/kb4.rs` | `matches_the_oracle` (`Scene::tas(39680)`: frames 39680-40087, 387 loops all occupied, 384 drawn, phases 0-4, byte for byte) and `matches_the_oracle_through_every_phase` (the whole life to frame 40894: 1194 loops all occupied, 1089 drawn, all seven phases 0-6). The stage 7 boss controller: never projected, integrated or drawn by `$A852`/`$A79B` — it draws fixed-screen metasprites through `$81C6` (bank 9) and hands the visible parts to free slots (`$52` into 0-15, `$B8`/`$BC`/`$BE`/`$C0` into 16-23 and `$50` through `$B1C4`), and runs the `$AFD6` boss contact with mode `$C0` (weapons only, so the TAS's `contact`/`weapon` columns stay 0) every frame; `$8DB8` leaves phases 2/3 for phase 4 once the boss's hit points reach `$4650`. `$C34B` (the palette queue) runs 25 times inside the compared window, which proves its `Vram::push(0x3F00, 0, palette)` modelling. The second test compares the boss's real `$FD` writes: the count-up's `$8EF1` write goes to the camera scroll through `Cx` (`scroll::Scroll::scroll_x`), so neither `$F1` nor `$FD` is held any more. The death path (`$AF97` bit 7 → `$8F37` + `$B1C4`, a `$B8` and a type `$50`) runs at 40921, after the TAS's last input (40896): its last shots finish the boss, and the chain (9k) compares it. Its parts are unported, so both tests free every slot but the boss's each loop at `$D450` |
| `$04` (slots 24-31) | bank 8 `$807C` | `families/s04.rs` | `matches_the_oracle_in_the_red_brick_room` (`stage1-inputs.csv` from 1060: 286 update loops and the draws, the turn-round at 1192) |
| `$02` (slots 24-31) | bank 8 `$8000` | `families/s02.rs` | `matches_the_oracle_on_the_tas` (TAS 960-1240: the sideways platform, 1 px a frame, turning at `$7BD8,x`) |
| `$0A` (slots 24-31) | bank 8 `$90C7` | `families/s0a.rs` | `matches_the_oracle_in_stage_2_1` (TAS 7250-7570: still until the player first stands on it, `$7F10,x` bit 7 latched in `dying_timer`, then 2 px a frame along the path `$9287[state]` of corners and `$9297[state]` of turns: a direction, `$FF` to free it, or a jump to a fixed position; the carry it gives the player is `subtype:phase` across and `hit:behaviour` down, `$92AF`). The chain runs it in the port through stage 2-1 |
| `$06` (slots 24-31) | bank 8 `$80F5` | `families/s06.rs` | `matches_the_oracle_in_stage_6_1` (TAS 32900-33260: the crumbling block `$CC6D` spawns; its first frame waits until the queue pointer `$0181` is at least `$60`, queues four blank tiles across two rows (`$A0CE`, no attributes) and zeroes its two map cells, then copies `$7D70` (0) into its other bytes and sets `$7C30 = 5`; five frames on it falls, `$7D70 += 3` up to `$80` as 1/16 px a frame plus `$7D90`, until the `$C99D` probe (flags `$80`) answers clear and it frees itself) |
| `$0C` (slots 24-31) | bank 8 `$9319` | `families/s0c.rs` | `matches_the_oracle_in_stage_4_2` (TAS 20500-20800): hidden (`$7E90 = $80`), `$7D30 = 1` (120 px wide through `$CE7E`), `$7E28 = 2` (the `$CE3E` ride), at camera y + `$C01` every frame: an unseen floor under the screen whose standing test is the level map read screen-relative |
| `$0E` (slots 24-31) | bank 0 `$94C3` | `families/s0e.rs` | `matches_the_oracle_in_stage_6_2` (TAS 33640-34240, the pieces type `$84` drops): the falling half of `$06`, also freed once its y passes row 16 |
| `$10` (slots 24-31) | bank 12 `$8952` | `families/s10.rs` | `matches_the_oracle_in_stage_7_1` (TAS 39270-39350): still with its speed bytes cleared until `$7F10,x` bit 7 (stood on), then falls like `$06` and is freed past row 16 |
| `$B8` | bank 12 `$93FC` | `families/kb8.rs` | the chain (9k, TAS 40921-41177; no isolated test, as it only lives in the last boss's death). The last boss's fall, a fixed-screen metasprite at (`$7D90`, `$7DB0`) from `$94B0[$7C30]` with `$E7 = $94B8[$7C30]`, drawn through `$81C6`: phase 0 holds frame 0 at (`$80`, `$58`) for `$80` frames, phase 1 flickers frames 1/2 for 16, then sinks frame 3 a pixel every four frames and from `$80` frames on sets `$BA = 6`, the ending |
| `$08` (slots 24-31) | bank 8 `$8ED5` | `families/s08.rs` | `matches_the_oracle_on_the_tas` (TAS 1060-1240: sinks 1 px a frame while `$7EF8,x` bit 7 says the player stands on it, unless `$C93B` finds the level solid, else rises to its home row). Writes `$90-$93` and `$A5`, so the second-class dispatch now takes `Work` |
| `$0E` | bank 8 `$8B44` | `families/k0e.rs` | `matches_the_oracle` (TAS 1480-1489: the stage-end trigger sets `$BA = 2`, `$0408 = 0` and frees its slot at 1485) |
| `$24` | bank 8 `$9823` | `families/k24.rs` | `matches_the_oracle` (route 997-1056: 10 loops alive in slot 23, 9 drawn, then it leaves the screen), `matches_the_oracle_when_it_hits` (player on it: `$AEE3` hit frees it), `matches_the_oracle_with_the_ac_children_alive` (`Scene::tas(38884)`, area 20:02, the `$AC` kept alive so it keeps dropping children: 300 loops, every one occupied and drawn, phases 0 (571 `$AC` slot-frames) and 8 (512 `$24`), the `$7C30` = 6 children drawn byte for byte). The `$98BC` pointer table is in bank 8; `$C247` reads the metasprite through `$96/$97`, so the `$A000` entries (`$B384`/`$B256`/`$B7A2`/`$BE88`/`$BE8E`) draw from the `$8FC4` wrapper's bank 1 and the `$8000` entries (`$98CA` for `$7C30` 1, `$98D0` for 6) from the handler's own bank 8 (oracle: `$E9`=8, `$EA`=1 at the call) |
| `$22` | bank 8 `$9555` | `families/k22.rs` | `matches_the_oracle` (route 845-1145, phases 0-3, on screen, the `$24` child spawn and its muzzle-flash second draw), `hurts_the_player_up_close` (route 900, player beside it, 120 loops, contact damage), `off_screen_despawns`, `near_off_screen_is_stored` (route 900 pushed off screen, 30 loops) |
| `$C6` | bank 12 `$9953` | `families/kc6.rs` | `matches_the_oracle` (route 641-685, slot 1, phases 0-1, on screen), `matches_the_oracle_through_the_warp` (route 641-686, the frame-686 player warp and slot free; the warp's `$046A`/`$0481` writes are pinned because the harness's `Effects`/`Player` do not carry them) |
| `$C4` | bank 12 `$9897` | `families/kc4.rs` | `matches_the_oracle` (`Scene::tas(38250)`, area 20:00: the `$C2` controller's projectile, slots 9-15; its whole isolated life, 36 loops, phase 0, on screen and drawn all but the freeing loop), `every_subtype_drift_matches_the_oracle` (all four `$990B` drift entries forced through `$7DB0`), `hurts_the_player_up_close` (the `$A9C4` contact costs 2 energy). Calls `$A000`, `$A79B`, `$A852`, `$C247` (bank 12: the `$991B` pointer is a `$8000`-window address, so the `$81C6` wrapper's bank-9 mapping at `$A000` is unused), `$A9C4`, `$A934`, all in §9c; the `$C2` spawner is unported, so `isolate` leaves the actors alive at the lift |
| `$08` | bank 8 `$8506` | `families/k08.rs` | `matches_the_oracle_next_to_its_parent` (route 953, player beside it, 200 loops; the `$0A` parent's four-part burst is diffed byte for byte). No `$7DD0` phase dispatch: the byte is part 0's x velocity, so `Coverage` reports it as phase `$E0` |
| `$14` | bank 8 `$8D70` | `families/k14.rs` | `matches_the_oracle` (TAS 2279+, the `$14` moved from slots 19-23 to a low slot so the loop steps its `$18` child (slot 15) before it and the `$D450` poke frees it: phases 0-3, on screen, drawn), `hurts_the_player_when_it_lands` (the `$AEE3` touch costs the player energy) |
| `$12` | bank 8 `$8C56` | `families/k12.rs` | `matches_the_oracle` (TAS 3000-4700: every `$7DD0` phase 0-`$0F`, 1574 loops, the `$14` seeded once per phase). A stage controller: never drawn (`drawn` 0) and never contacts. The column spawner keeps arming cells after `isolate`, so every slot but slot 0 is freed at `$D450` each frame and the diff is the slot-0 `$12` alone (its `$14` children reach k14 phase 3, whose `$18` spawn leaves the brain unset) |
| `$16` | bank 8 `$8F77` | `families/k16.rs` | `matches_the_oracle` (`Scene::tas(15677)`, area 10:00: 27 loops, drawn every loop, no `$7DD0` phase dispatch so phase 0). Draws the bank-1 metasprite `$BCC0` twice at screen row `$EF`, x from `$7D90` then x XOR `$80`, then `$7D90` -= `$0C`; no `$A000` load, no contact, never frees itself |
| `$18` | bank 8 `$8FD9` | `families/k18.rs` | `matches_the_oracle` (stage 1-2 TAS 2392-2691, phase `$F0`, on screen, drawn), `matches_the_oracle_in_level_5` (TAS 13419, phase `$E0`), `hurts_the_player_up_close` (the `$AEE3` touch); the area-`$0A` metasprite (level 4, unreached) is a `shortcut:` |
| `$1A` | bank 8 `$8205` | `families/k1a.rs` | `matches_the_oracle` (TAS 13933-13980, area 0C:00 slot 1, `$7D50` `$E1`: 31 loops, phase 0, never drawn; the room change at ~13981 ends it, and later loops hold other unported kinds), `arms_the_transition_when_the_player_reaches_it` (`$BA` = 1, `$BB` = `$E1`, stays alive), `frees_itself_when_the_player_is_far` (`$C0` re-arm, the `$A025` far result that `$81D4` ignores), `a_scrolled_camera_or_a_hurt_player_skips_it` (`$CE`/`$CF`, `$0478`/`$047A` bit 7). Like `$02` plus the `$CE|$CF` gate and the far despawn. No `$7DD0` dispatch; `$7D50` is only the `$BB` parameter. Areas 12:02/12:04 not run |
| `$10` | bank 8 `$8B55` | `families/k10.rs` | `matches_the_oracle` (`Scene::tas(3968)`: the item's whole 240-frame life, `$7DD0` 0-`$EF`, 123 of 240 loops compared — area 2's FME-7 IRQ drops the rest), `every_phase_of_the_counter_runs` (three lifetimes, `tas(3968)`+`tas(5254)`+`tas(1044)`, whose non-IRQ brackets union to every `$7DD0` value), `the_dying_state_wraps_from_83_to_80` (the `$84`->`$80` wrap), `the_player_collects_the_item` (`$7EE0` `$49`: `$A9C4` collects it on contact) |
| `$20` | bank 8 `$94B2` | `families/k20.rs` | `matches_the_oracle` (stage 1-2 3844-3963, `$7D50` 0, steps 2 px/frame, on screen, drawn, then off-screen free), `matches_the_oracle_when_it_hits_a_wall` (`$94F3`: a solid step sets `$7D50` 1, seven frames of the `$B2D0` flash, then despawn), `hurts_the_player_up_close` (`$AEE3` contact frees it and damages the player). No `$7DD0` phase dispatch: the byte is residue |
| `$1E` | bank 8 `$9364` | `families/k1e.rs` | `matches_the_oracle` (TAS 3831-4208: phases 0-1, on screen, the `$20` child rear, the death and the `$10` item it becomes), `matches_the_oracle_from_spawn` (TAS 782-801, phase 0, on screen) |
| `$36` | bank 11 `$8A92` | `families/k36.rs` | `matches_the_oracle` (level 2 TAS 7426-7826, phases 0-1, on screen, drawn), `hurts_the_player_up_close` (player on it, 40 loops, contact costs 2 energy), `drops_its_bomb` (the `$24` bomb lands in slots 16-23) |
| `$26` | bank 8 `$98D6` | `families/k26.rs` | `matches_the_oracle` (TAS 2507-2706, phases 0/2/4, on screen and drawn, the `$24` child spawn and the muzzle flash draw), `hops_with_no_ground_below` (the `$998C` hop, the `$99EC` return path and the far despawn), `hurts_the_player_up_close` (contact damage). The `$9BB0` child spawn writes slots 16-23 directly, like `$22`'s |
| `$34` | bank 11 `$8A03` | `families/k34.rs` | `matches_the_oracle` (TAS 6261-6782, the whole area-4 boss room: 407 loops, `$7DD0` phase 0 then `$1F`, on screen, drawn every frame, the `$D1F5` counter queue on the first loop); `a_finished_display_value_ends_the_level` (`$B2:$B3` poked to 0: the `$8A3B` arm stops the music and runs `$D274`, `engine::level_end`; the later loops take the `$046E`=`07` arm). The TAS never reaches that arm |
| `$28` | bank 11 `$8000` | `families/k28.rs` | `matches_the_oracle` (`Scene::tas(21029)`, area 12:02: 209 occupied loops, `$7DD0` 0, on screen, drawn every loop), `every_direction_matches_the_oracle` (the TAS only runs `$7DB0` 1; 0/2/3 are forced by a `$7DB0` poke, every actor loop diffed byte for byte — this reaches the `$8134` advance and snapped-waypoint path the natural run never does), `spawns_the_24_pair` (the `$815B` `$24` pair, `x -/+ $80`, phases 3/2, the parent's y and CHR shadow), `hurts_the_player_up_close` (the `$A9C4` contact). No `$7DD0` dispatch: the byte stays 0, and `$7D50` is the spawn record's `$20`. The `$A9C4` call also runs the `$AA57` weapon scan (inventory `weapon` 1); a scan hit is not separately forced. The `$24` children are freed every loop by a `$D450` poke, as `k0a` does: a `$24` with `$7C30` 1 fetches its `$98CA` metasprite from the `$8000` window (bank 8), which `k24` now draws (see its row) |
| `$34` | bank 11 `$8A03` | `families/k34.rs` | `matches_the_oracle` (TAS 6261-6782, the whole area-4 boss room: 407 loops, `$7DD0` phase 0 then `$1F`, on screen, drawn every frame, the `$D1F5` counter queue on the first loop); the level-clear arm (`$D274`, reached when `$046E`=`07` or `$B2:$B3` finishes) never runs in area 4 and is a `shortcut:` |
| `$6E` | bank 0 `$891D` | `families/k6e.rs` | `matches_the_oracle` (TAS 8864-, phase `$80`, on screen, drawn via the bank-9 metasprite `$84CA`, then freed off it), `off_screen_keeps_the_slot_undrawn` (phase 0: never on screen, it stores each frame), `the_death_hands_the_slot_to_type_70` (the `$AB13` kill and the `$8951` tail repurposing the slot as `$70`, four hit points, 76 px lower). Its `$7EE0` `$40` gates `$A9C4`'s player check off, so it is only a weapon target; the `$70` continuation is `families/k70.rs` |
| `$66` | bank 0 `$8665` | `families/k66.rs` | `matches_the_oracle` (level 3 TAS 9032-9186, phases 0/`$80`, on screen and drawn, then the off-screen free; the `$7DD0` phase is 0 only on its spawn frame), `off_screen_with_a_clear_phase_is_stored` (`$8908`'s `bpl`), `hurts_the_player_up_close` (the `$A9C4` contact). It bobs `vy` between `-$20` and `+$20` on `$7D90` bit 7 and ends a frame with `$A934` alone; the draw's `$84CA` maps bank 9 at `$A000`, so the port passes bank 9 to `engine::draw` |
| `$68` | bank 0 `$86F5` | `families/k68.rs` | `matches_the_oracle` (level 3 TAS 9017+, the counter's phases 0-1: the area-8 FME-7 IRQ splits the loop from frame 9046), `every_phase_of_the_counter_runs` (the `$B288` record placed in stage 1-1: the whole 80-frame cycle, phases 0-4 and the free, the `$66` children drawn), `the_fifth_phase_frees_the_slot` (phase 5 held before the free). It never draws or contacts and calls no §9c routine; its child is the parent's `$7DB0` type, and the other area-8 `$68` records set `$7DB0` `$6A`, whose handler (bank 0 `$877D`) is unported, so a long area-8 window diverges in the `$6A` child |
| `$38` | bank 11 `$8D1F` | `families/k38.rs` | `matches_the_oracle` (TAS 6282-6771, the stage 1 boss fight: 285 loops, phases 0/3/4, drawn, the death and its `$B1C4` part), `every_phase_matches_the_oracle` (phases 1/2 pinned by `$7DD0`, the TAS dies at frame ~6574 before reaching them), `the_boss_hurts_the_player_up_close` (`$AFD6` contact sets `$0479`) |
| `$54` | bank 11 `$9D49` | `families/k54.rs` | `matches_the_oracle` (TAS 8600-8660, area 08:00: 27 loops from frame 8634, phase 0, never drawn). A player script: unless `$046E` = 7 (then it clears the 16 `$7BD0` weapon slots) it sets `$046E` = `$0B`, `$0481` = `$A0`, `$0406` = `$A0`, adds `$0A` to the player's x, steps x/y by `$20` from the pad when `$C2` != `$FF` (y clamped to `$0400`-`$0C00`) and picks `$0466` from `$9DF5[($F1 & 6) >> 1]`. Unexercised: the weapon-clear arm, the y clamps, areas 0A/18/1A |
| `$70` | bank 0 `$89B1` | `families/k70.rs` | `matches_the_oracle` (TAS 8884, the stage 3 slot the `$6E` hands over on death: `$7DD0` 0-`$AF`, on screen, drawn both every frame and on the counter's odd frames, the `$89B4` hit reaction stepping `$7D50` 81 -> 82), `matches_the_oracle_in_level_9` (TAS 28978), `every_phase_of_the_counter_runs` (forces each of the counter's 176 values one frame at a time; bracket drops leave 34 unseen in the scenes), `the_hit_reaction_wraps_the_state_from_83_to_80` (the `$84` -> `$80` wrap), `the_player_collects_it_on_contact` (`$7EE0` `$49`: `$A9C4` collects it, the player is not hurt) |
| `$76` | bank 0 `$8D3F` | `families/k76.rs` | `matches_the_oracle` (TAS 8091-8150, area 06:04: its one loop at frame 8140 stores `$70` in `$0451`/`$0452` and frees the slot; no draw, no contact, no engine call) |
| `$78` | bank 0 `$8D4F` | `families/k78.rs` | `matches_the_oracle` (`Scene::tas(27520)`, area 16:00, the level-start record in slot 0: 167 clean loops, occupied every one, phase 0 throughout, never drawn — the player never reaches row `$0C`, so it never spawns), `spawns_the_marker_at_row_c` (forcing `$0462` = `$0C` at `$D450`: the first loop latches phase `$80` and the `$8D84` helper drops one `$7A` at the player's x and row `$0C`, 15 drawn, then idles at `$80`), `frees_its_slot_when_the_level_ends` (also `$046E` = 7 on the spawn frame: the marker drops and the handler frees its own slot). A stage controller that calls no §9c routine and reads only the player's x, row (`$0462`) and motion (`$046E`) |
| `$7A` | bank 0 `$8DB5` | `families/k7a.rs` | `matches_the_oracle` (`Scene::tas(27584)`, area 16:00, spawned by `$7C` (11 of its phase-0 `$8E46` spawns plus one from phase 3 at `$8ED8`) and by `$7E` phase 6 (`$91EF`), never by `$78`: an oracle write watch on the `$8DAC` `STA $7CC0,y` names the writing slot and type for all 16 TAS spawns (the first, TAS 27584, is `$7C` in slot 1; `$7E` first appears only at 27599): 16 loops, every `$7DD0` phase `$00`-`$0F` exactly once, on screen, drawn 15 of them — the phase-`$10` step frees the slot before any draw and is never a loop top). A stationary 15-frame flash: `load`, `on_screen` (free off screen), `phase += 1` (free at `$10`), the bank-0 `$8DF3` metasprite pointer (`$A506` phases `$00`-`$0B`, `$A518` `$0C`-`$0F`) through `tile_bits` and the bank-9 draw, then `store`. No `$A79B` integrate, no `$A9C4` contact, so no player test |
| `$7C` | bank 0 `$8DFB` | `families/k7c.rs` | `matches_the_oracle` (area 16:00, TAS 28140+: all four `$7DD0` phases 0-3, on screen and drawn 70 of 92 occupied loops, the `$8D84` `$7A` marker and the `$8FC0` `$24` blast spawns). `hurts_the_player_up_close` (pinned onto the parent with `$047A` bit 6 cleared at `$D450`, the `$A9C4` contact costs energy). Wakes within 80 px, faces the player, drops up out of view (the record's `$7DB0` `$FF`/`$7D90` `$A0` give a negative vy) or, when clear of the shots, dwells then blasts `$24` aside and `$7A` below, freeing below row `$0F`. The `$7A` marker (bank 0 `$8DB5`) is unported, so the tests free every main slot but the parent's each loop |
| `$74` | bank 0 `$8C9C` | `families/k74.rs` | `matches_the_oracle` (`Scene::tas(10320)`, area 08:00, slots 20-23: phases 0/`$80`, on screen, drawn, then the off-screen free; 211 frames), `hurts_the_player_up_close` (the `$AEE3` contact costs energy). Slides `x` left 40 and `y` by a 16-bit offset (`$7E10:$7DF0`, +/-3 a frame by `$7DD0` bit 7) until the offset crosses zero, then holds (`$7E90` = `$80`) |
| `$7E` | bank 0 `$9032` | `families/k7e.rs` | `matches_the_oracle` (TAS 27931+80, area 16:00: the one live `$7E` moved to slot 0, phases 1-3, on screen, drawn 51 loops), `matches_the_oracle_from_the_first_spawn` (TAS 27599+140: phase 0 wakes to 1/2, then phase 6 spawns the child and frees the slot), `falls_off_the_ledge_and_reaches_the_head_row` (a phase-3 `$7E` put over a ledge: 9 frames held by `$AF7D` with the `+$10` drift, then the `$912F` arm drops it through phase 4 (8) and 5 (47) to 6), `near_off_screen_is_stored` (the `$9236` Near arm stores without drawing), `hurts_the_player_up_close` (the `$AEE3` touch costs energy); the three scenes' phases cover 0-6. The phase-6 `$8D84` helper is ported inline: it puts a type `$7A` (bank 0 `$8DB5`, unported) in the highest free slot at the parent's x, `$0C00` y, CHR `$7`, and sets the parent's `$7D50` `$40`; the tests keep that slot free |
| `$64` | bank 0 `$8606` | `families/k64.rs` | `matches_the_oracle` (level 3 TAS 9581-, two actors, 113 frames, phase `$80`, on screen and drawn, the timer wrapping, then the off-screen free), `off_screen_with_a_clear_phase_is_stored` (`$8908`'s `bpl`, the phase-0 path), `hurts_the_player_up_close` (`$A9C4` contact; the player sits 64 px below because the draw adds the `$83CC` bob, 48-96 px, before the contact). The draw's `$84CA` maps bank 9, as `$66` |
| `$3A`, `$3C` | bank 11 `$9144`, `$91DC` | `families/k3a.rs` | one module for both, as `k06` covers `$04`/`$06`. `matches_the_oracle` (`$3A`): the boss's phase-1 spawn (`$8FB9`) into slots 16-23, then each of the four `$7DD0` drift offsets held by a poke (the boss's own facing only picks 2/3), on screen, drawn. `hurts_the_player_up_close` (`$3A`): pinned onto the player, `$AFD6` sets `$0479` and frees the part. `matches_the_oracle_shell` (`$3C`): the phase-2 spawn (`$909B`) into slots 0-15, one shell kept (with all three alive the area-4 FME-7 IRQ lands inside every actor loop and the harness diffs none), drawn, `$7DD0` 0 throughout (the handler never reads it). `the_shell_detonates_on_the_player`: `$A9C4` on an idle player drains `$0258` from `$B2:$B3` at `$927B`, then the `$9288` detonation. `a_dead_boss_frees_the_shell`: `$A9:$AA == 0` frees the slot on the first loop. `the_boss_runs_with_its_parts`: the boss and the `$3A` it keeps spawning run in one loop (the aliasing and both draws), which `k38`'s own tests cannot reach — they clear every part slot each loop |
| `$60` | bank 0 `$84F4` | `families/k60.rs` | `matches_the_oracle` (`Scene::tas(13374)`, area 0C:00, 300 frames: phases 0 (122 loops) and 1 (26), on screen and drawn, a burst of the `$18` parts seen), `falling_out_of_the_level_frees_the_slot` (`$8520`: phase 1 below row `$0F`). `the_burst_reuses_the_bombs_own_slot` (the bomb moved to slot 15: `$8533` frees it through `engine::free_own_slot` and `$AC16` puts the burst in the same slot) |
| `$6A` | bank 0 `$877D` | `families/k6a.rs` | `matches_the_oracle` (TAS 9263-9383: the stage 3 copy the `$68` parent's `$8762` routine makes, phase 0 then `$80`, on screen and drawn for 54 loops, then the `$8908` free; the same loops carry its one `$A9C4` player hit and one `$AA91` weapon hit), `off_screen_before_the_first_draw_is_stored` (`$8908`'s `bpl`), `hurts_the_player_up_close` (contact damage), `knock_back_fires_when_the_player_is_close` (the `$8799` arm: `$7DF0` `$80`, `$7E50:$7E60` `+$48`, `$7E70:$7E80` from `$87F5`/`$87F7`; the draw's `$84CA` maps bank 9 at `$A000`) |
| `$5E` | bank 0 `$831D` | `families/k5e.rs` | `matches_the_oracle` (`Scene::tas(13291)`, area 0C:00: 972 frames, 413 occupied, 360 drawn, `$7DD0` 0-`$7C` all 32 countdown values, on screen and drawn), `hurts_the_player_up_close` (the `$AEE3` contact costs energy). Three pieces at `$83CA`/`$844A` offsets, no `$A000` load, no `$7DD0` dispatch; the piece loop frees the slot (rearm `$80`) on a far piece |
| `$1C` | bank 8 `$933A` | `families/k1c.rs` | `matches_the_oracle` (`Scene::tas(20499)`, area 12:02: its one loop, phase 0, never drawn; stores `$37` in `$43`, paints three map cells `$60` and frees its slot), `both_branches_match_the_oracle` (forces `$7D50` `$20` and `$FF`: the `$6740/$6840/$6940` and `$67AF/$68AF/$69AF` cells). No engine or collision call |
| `$2E` | bank 11 `$85A7` | `families/k2e.rs` | `matches_the_oracle` (TAS 13613-14012, area 0C:00: 399 loops, phases 0/2/4, on screen, drawn, the `$30` child spawned, lowered and thrown), `matches_the_oracle_when_the_weapon_kills_it` (TAS 20000-20282, area 12:00: the player's weapon hits, the `$A56E` death), `hurts_the_player_up_close` (the `$A9C4` contact), `dying_while_the_child_is_out`, `dying_in_phase_one_frees_the_child_slot` (the `$85B1` branch, forced), `far_away_frees_the_slot_and_the_child`, `far_away_in_phase_zero_frees_only_the_slot`, `near_the_edge_nothing_runs` (`$86B7`: a near parent neither stores nor ends the frame). The child's `$8774` draw leaves the child's position in the working set when it is off screen, so the parent then draws and stores there (matches the oracle). The child (`$30`, handler `$887A`) is not ported: the tests free its slot each loop. The throw writes the parent's slot (`Work::slot`) into the child's `$7DB0` |
| `$30` | bank 11 `$887A` | `families/k30.rs` | `matches_the_oracle` (`Scene::tas(13670)`, area 0C:00, the `$2E`'s thrown child: 37 frames, phase `$80`, the descent, then the arrival (`$88AD`, sound `$22`, `$7C00 &= $03`, `vx` cleared, `y += $100`) and the eight-frame landing sweep (`$7E90` `$80`-`$87` through the `$893E` pointers) over the child's last frames, on screen and drawn 36 of 37, then the free at `$7E90` `$88`); `the_child_clears_its_live_parent_phase` (the `$2E` kept alive: its phase 4 -> 0 at the free is the child's `$7DD0,y` clear, its phase-4 timer 27 then, and the union coverage 0/4/`$80`), `a_clear_phase_bit_is_dormant` (`$7DD0` bit 7 is the handler's only dispatch; it is unreached naturally because the spawner takes the highest free slot, above its parent, so the child never runs before the throw sets `$80`; forced clear, the slot is untouched) |
| `$6C` | bank 0 `$87F9` | `families/k6c.rs` | `matches_the_oracle` (level 3 TAS 9735, four slots idle on screen, phases 0/`$80`, drawn), `the_spell_throws_four_shots_then_rests` (spell forced through `$7E10`/`$7E90`: sprite cycle `$7C30` 0-6, four `$24` shots by `$AF38`, the `$7DF0` rest hold), `matches_the_oracle_in_level_7` (area `$18`, TAS 29178), `off_screen_with_a_clear_phase_is_stored`, `off_screen_after_a_draw_frees_the_slot`, `hurts_the_player_up_close` (the `$A9C4` contact). `$7DD0` is only an on-screen flag; the real phases are `$7E10`/`$7E90`/`$7DF0`. The rest-to-spell arming (`$886F`, `screen_x < $A0`) was only seen by a RAM probe at TAS 29307 (area `$18`), not confirmed inside a compared frame |
| `$40` | bank 11 `$9410` | `families/k40.rs` | `matches_the_oracle` (`Scene::tas(17943)`, area 10:00, slots 21-23: 23 loops, side 1, on screen, drawn every loop, then the timer frees it at `$18`), `the_other_side_matches_the_oracle` (forces `$7DD0` 0: 23 loops, 22 drawn), `hurts_the_player_up_close` (the `$AEE3` contact costs energy). `$7DD0` picks the x offset (+/-`$40`); the metasprite cycles with `$7DF0 & $18` (bank 9 `$A460/$A46A/$A474`); no `$A9C4`, no physics |
| `$48` | bank 11 `$9902` | `families/k48.rs` | `matches_the_oracle` (`Scene::tas(26773)`, area 14:00, slots 2-3: 1564 loops, on screen and drawn every loop, both path halves of `$7DB0` and every `$7DD0` counter value 0-`$FF`, so the throw starts at `$00`/`$50`/`$80`/`$B0` and the `$7E10` volley all run). The `$4A` children (`$99D9`) go into slots 16-23, which the test keeps free with a poke because that family is unported. No `$A9C4` contact, no physics, no self-despawn |
| `$4E` | bank 11 `$9B62` | `families/k4e.rs` | `matches_the_oracle` (`Scene::tas(26677)`, area 14:00, slot 15: its whole 16-frame life, on screen and drawn every frame; the handler dispatches on no `$7DD0`/`$7D50`, so phase 0), `touches_the_player` (player on it, `$047A` cleared: the `$A9C4` probe hurts him, drains `$B2:$B3` by `$0158` at `$9BC1` and runs the `$9BCB` kill), `the_death_clears_the_parents_timer` (forced `$7D70` bit 7: the `$9B78` arm clears the parent slot's `$7DD0`/`$7E10`), `frees_itself_when_the_boss_is_dead` (`$A9:$AA` = 0 frees the slot at `$9B68`), `the_counter_wrap_kills_it` (`$7E10` forced to `$FF`). Draws the bank-9 metasprites `$B0A8`/`$B0B2`/`$B0BC`/`$B0CC` (`$9C0B` indexed by `$F1 & 6`) |
| `$32` | bank 11 `$8950` | `families/k32.rs` | `matches_the_oracle` (`Scene::tas(5810)`, area 2: idle, then the `$C0` countdown 1..`$C0` with four IRQ-dropped loops, then the frame-6028 transition writing `$040E`=`06`, `$040F`=`00`, `$0404`=`80`; never drawn), `matches_the_oracle_from_state_40` (`tas(11014)`, `$7D50` bit 0 clear: `$0404`=0 and no `$040E`), `every_countdown_phase_runs` (the two lifetimes union to every `$7DD0` value 0-`$C0`), `matches_the_oracle_when_the_player_is_in_reach`/`despawns_when_the_player_is_far`/`idles_while_the_player_is_clear` (the `$7D90`=0 `$A025` arm). A screen-transition controller: with `$7D90` != 0 it waits for the camera page to reach `$7EF0`, then counts `$7DD0` down and arms `$BA`=3; never draws or contacts. The `$0472` bit-7 guard (`$8955` reads `$7BD0,x` sixteen times with x the running slot, `Work::slot`, and returns on a negative byte) never runs in the whole-game TAS: `a_negative_weapon_byte_holds_it` and `a_clear_weapon_byte_lets_it_count_down` poke it |
| `$2A` | bank 11 `$8284` | `families/k2a.rs` | `matches_the_oracle` (`Scene::tas(21030)`, 200 loops: phase 2 then 4, on screen, drawn; the player's weapons hit it, a health poke keeps it alive and a `$D450` poke frees its `$2C` bomb each loop), `spawns_into_the_dive_off_screen` (TAS 20960: the phase-0 frame, never drawn), `drops_a_bomb_hovers_and_rises` (TAS 21215: the `$84BD` bomb spawn into slots 0-15, the blast flash draw, the rise back to phase 2), `steers_the_lift` (TAS 21400: the `$F9`-keyed `$F7`/`$FC` lift), `turns_at_both_edges` (pokes `$7B00` to force the `$0C` and `$08` column branches), `hurts_the_player_up_close` (the `$A9C4` contact). Its `$2C` child is `families/k2c`-to-be (bank 11 `$851A`, unported) |
| `$62` | bank 0 `$8580` | `families/k62.rs` | `matches_the_oracle` (`Scene::tas(12022)`, area 0A:00: 100 occupied loops of 400 frames, 78 drawn, no `$7DD0` dispatch so phase 0; a probe pins that a piece lands (`$7D70` `$80`, the `$A56E` dying animation) and one frees), `below_the_map_it_is_freed` (y high byte `$0F`, the `$85A7` free), `hurts_the_player_up_close` (the `$AEE3` contact; the piece is stepped 30 frames first so it is on screen). Drifts left by `$7E50:$7E60` = `-$20`, `$9D:$9E` -= 3 before `$A8E4`; draws the bank-9 metasprite at `$8604` |
| `$72` | bank 0 `$8A7D` | `families/k72.rs` | `matches_the_oracle` (TAS 10246, area 08:00, slot 2: phases 0-1, on screen, drawn, 374 loops), `matches_the_oracle_in_level_9` (TAS 29408, area 18:00, slot 1: phases 0-1, on screen, drawn, 37 of 100 loops — the area's FME-7 IRQ drops the rest), `sheds_its_parts` (the four `$74` parts at timer `$40`, pinned in the oracle), `dives_toward_the_player_above_and_below` (phase 2, both `$AF43` signs), `a_dive_ends_in_another_wait_or_the_leave` (2 to 1, 2 to 3), `the_leave_slows_its_drift` (phase 3), `hurts_the_player_up_close`, `flashes_and_dies_when_hit`. Phases 2/3 and the part spawn never occur in the TAS (the sprite is gone by timer `$52`), so those tests poke `$7DD0`/`$7DF0`/`$7E10`/`$7DB0`; the spawn with no free slot (`$AF38` returning Y=`$FF`, a wild write) panics and is unexercised |
| `$3E` | bank 11 `$92B9` | `families/k3e.rs` | `matches_the_oracle` (`Scene::tas(18521)`, area 10:00, slots 1/3: 691 loops, phases 0/2/4, on screen, drawn, the `$40` projectile spawn into slots 16-23, freed each loop by a `$D450` poke), `throws_right_when_facing_right` (both actors' `$7C00` forced to `$02` at `$D450`, so the `$93A8` right arm and the child's `$7DE0` state 0 run; the scene's own flags are `$62`, which takes the left arm), `hurts_the_player_up_close` (the `$A9C4` contact costs energy; `$047A` is poked clear at `$D450` since the TAS player has bit 6 set there). Never integrates (`$A79B` is not called); a far actor despawns with `$CD50` and re-arm `$80` |
| `$86` | bank 0 `$9516` | `families/k86.rs` | `matches_the_oracle` (`Scene::tas(33640)`: 256 loops, 31 occupied and drawn, `$7DD0` `$10`), `matches_the_oracle_with_three_alive` (`Scene::tas(33671)`: 100 loops, 33 occupied and drawn, 51 slot-frames of `$10`). Four parts: each keeps a 16-bit position and a signed velocity in the slot arrays (`$AC16` variant 6, so the `$7DD0` "phase" is part 0's x velocity) and falls 12 a frame; `$F1`'s low bit draws 2 of the 4 a frame; `$7ED0` counts the slot down. No `$A000` load, no physics, no contact. Its spawner `$84` (bank 0 `$9361`) is unported, so an isolated scene covers only the children already alive (~31 frames each) |
| `$88` | bank 0 `$95F6` | `families/k88.rs` | `matches_the_oracle` (`Scene::tas(14141)`, area 0C:02: 299 loops, phase 0, on screen and drawn 142 loops), `turning_round_matches_the_oracle` (the `$7DD0`/`$7E90` phase-2 turn forced: the 16-frame round flips `$99` bit 6 and returns to phase 0), `hurts_the_player_up_close` (the `$A9C4` contact sets `$0479`). Walks ±16 (`$975E`/`$9760`), turns at a wall or the `$C93B` ledge probe (`$84`), draws the bank-9 metasprites `$A83C`/`$A84E`/`$A860`/`$A872` through `$84CA`. The `$975A` table holds only phases 0 (`$9667`) and 2 (`$96E4`); any other value jumps past it and the port panics |
| `$42` | bank 11 `$947A` | `families/k42.rs` | `matches_the_oracle` (TAS 26808, area 14:00, slot 1: 300 loops, phases 0/1, on screen, drawn every frame; the player's fire drives the `$9616` knock-back into phase 1), `the_charged_phase_matches_the_oracle` (forces `$7DD0` 2, which the hit-drained `$7E10` never reaches), `the_flash_matches_the_oracle` (forces `$7D70` bit 7: the first-frame `$B1C4` `$50` spawn and the `$9482` bar-only / full-tail alternation), `hurts_the_player_up_close` (`$AFD6` drain of `$B2:$B3` sets `$0479`). It spawns `$44` shots and a `$46` child, both freed each loop by the test; reads no RAM byte `Effects` lacks |
| `$44` | bank 11 `$9739` | `families/k44.rs` | `matches_the_oracle` (`Scene::tas(26381)`, area 14:00, slots 16-23: 120 loops with the `$42` kept alive so it keeps firing shots; on screen and drawn every loop, the `$7DD0` "phase" is the y velocity and takes 20 distinct values), `matches_the_oracle_alone` (only the `$44` alive: its seven remaining frames, drawn each), `every_shot_table_runs` (the spawn counter `$7E10` pinned to 0-7, so all eight acceleration tables `$97DF`/`$97E7`/`$97EF` run). The handler integrates its own offset into `$7B00/$7B20/$7B40/$7B60`, adds the slot in `$7C30` (the `$42` that fired it) for the projection, draws `$C247` (bank 9 via `$826F`) and frees itself at `$7E90 = 8`. No `$A9C4` contact, no physics routine; it reads only `Body` and Work, so no `Effects` byte is needed |
| `$4C` | bank 11 `$9ADE` | `families/k4c.rs` | `matches_the_oracle` (`Scene::tas(26260)`, area 14:00: 880 compared loops, `$7DD0` phase 0 then `$80`, the one `$4E` release; never drawn, never on screen, no contact). The `$4E` child is unported, so its slot is freed each loop with a `$D450` poke (§9e); the spawn then fires at ~26646, earlier than the bare TAS's 26677, because the real run's slot 15 is briefly busy. `every_slot_busy_resets_the_counter` forces all 16 slots to `$4C` to run the `$9AFE` no-free-slot reset. Calls no §9c routine; the §8b `$7DD0` `FF` is bank 2 `$99DB` (the area-transition payload) overwriting slot 4's arrays at frame 27107, not a handler phase |
| `$4A` | bank 11 `$9A4D` | `families/k4a.rs` | `matches_the_oracle` (`Scene::tas(26500)`, area 14:00, slots 16-23: 73 occupied loops, `$7DD0` phases 0 (138 slot-frames) and 1 (96), the `$A852` projection left unchecked, the bank-9 draw every frame, then the y-`$0B00` clamp and the free), `hurts_the_player_up_close` (`$AFD6` with mode `$80`: standing in a part drains the player's energy). A `$44` boss part (`$9A13` sets its phase from the parent's attr bit 6); `$7E10` only indexes a death metasprite the `$7DF0 >= $0A` test skips, so that arm is unreachable |
| `$32` | bank 11 `$8950` | `families/k32.rs` | `matches_the_oracle` (`Scene::tas(5810)`, area 2: idle, then the `$C0` countdown 1..`$C0` with four IRQ-dropped loops, then the frame-6028 transition writing `$040E`=`06`, `$040F`=`00`, `$0404`=`80`; never drawn), `matches_the_oracle_from_state_40` (`tas(11014)`, `$7D50` bit 0 clear: `$0404`=0 and no `$040E`), `every_countdown_phase_runs` (the two lifetimes union to every `$7DD0` value 0-`$C0`), `matches_the_oracle_when_the_player_is_in_reach`/`despawns_when_the_player_is_far`/`idles_while_the_player_is_clear` (the `$7D90`=0 `$A025` arm). A screen-transition controller: with `$7D90` != 0 it waits for the camera page to reach `$7EF0`, then counts `$7DD0` down and arms `$BA`=3; never draws or contacts. The `$0472` bit-7 guard (`$8955` reads `$7BD0,x`, the slot index `Cx` lacks) is a pass-through; it never runs in the whole-game TAS |
| `$5C` | bank 11 `$9EC2` | `families/k5c.rs` | `matches_the_oracle` (`Scene::tas(11925)`, area 0A:00, slots 0-7: 27 loops, phase 0, every slot draws), `the_edges_wrap_like_the_oracle` (pokes both position pairs into a borrow and a carry: 60 loops, the `$FF` high and 0 low clamps). `$F7` bit 7 picks the piece — even frames move the x pair (`$7B00` high -2, `$7B20` low +3), odd frames the y pair (`$7B40` -5, `$7B60` +6) — then the `$826F` wrapper (`$C247`) draws bank 9's `$B0D6`/`$B0DC`; the census sees only `$C247`. No `$A000` load, no `$A852` projection, no contact, and it never frees its slot |
| `$32` | bank 11 `$8950` | `families/k32.rs` | `matches_the_oracle` (`Scene::tas(5810)`, area 2: idle, then the `$C0` countdown 1..`$C0` with four IRQ-dropped loops, then the frame-6028 transition writing `$040E`=`06`, `$040F`=`00`, `$0404`=`80`; never drawn), `matches_the_oracle_from_state_40` (`tas(11014)`, `$7D50` bit 0 clear: `$0404`=0 and no `$040E`), `every_countdown_phase_runs` (the two lifetimes union to every `$7DD0` value 0-`$C0`), `matches_the_oracle_when_the_player_is_in_reach`/`despawns_when_the_player_is_far`/`idles_while_the_player_is_clear` (the `$7D90`=0 `$A025` arm). A screen-transition controller: with `$7D90` != 0 it waits for the camera page to reach `$7EF0`, then counts `$7DD0` down and arms `$BA`=3; never draws or contacts. The `$0472` bit-7 guard (`$8955` reads `$7BD0,x`, the slot index `Cx` lacks) is a pass-through; it never runs in the whole-game TAS |
| `$2A` | bank 11 `$8284` | `families/k2a.rs` | `matches_the_oracle` (`Scene::tas(21030)`, 200 loops: phase 2 then 4, on screen, drawn; the player's weapons hit it, a health poke keeps it alive and a `$D450` poke frees its `$2C` bomb each loop), `spawns_into_the_dive_off_screen` (TAS 20960: the phase-0 frame, never drawn), `drops_a_bomb_hovers_and_rises` (TAS 21215: the `$84BD` bomb spawn into slots 0-15, the blast flash draw, the rise back to phase 2), `steers_the_lift` (TAS 21400: the `$F9`-keyed `$F7`/`$FC` lift), `turns_at_both_edges` (pokes `$7B00` to force the `$0C` and `$08` column branches), `hurts_the_player_up_close` (the `$A9C4` contact). Its `$2C` child is `families/k2c` (bank 11 `$851A`) |
| `$2C` | bank 11 `$851A` | `families/k2c.rs` | `matches_the_oracle` (TAS 21223, area 12:02: the falling phase 0, 54 loops on screen and drawn, the `ride_here` miss, the off-screen free), `lands_and_blasts_on_solid_ground` (a `$2C` placed over stage 1-1's floor: the `$8529`/`$852C` `below` landing and the six-step `$8570` blast, phase `$80`), `hurts_the_player_up_close` (the `$AEE3` touch costs 4 energy). The TAS bomb's own landing is a *ride* on a `$0C` second-class platform whose `$CE3E` handler is unported (§9f), so it is not compared |
| `$8A` | bank 0 `$9762` | `families/k8a.rs` | `matches_the_oracle` (`Scene::tas(12099)`, area 0A:00 slots 8-9: 360 loops, 77 occupied and drawn, both `$7DD0` phases 0 and 1, the `$A9C4` contact the player's attack kills the actor through, and its `$AA57` weapon-scan tail), `spits_a_plain_child_without_a_behaviour` and `spits_a_child_with_a_behaviour` (the `$9822` child spawn's two arms: with `$7D90` set, the `$8C` child takes its `$F9` subtype and its `$989F` phase), `the_hit_flash_and_the_death_match_the_oracle` (forced `$7D70` `$80`, the `$A56E` dying animation and free), `off_screen_frees_the_slot` (`$A852`'s `bmi`), `hurts_the_player_up_close` (`$0473` cleared so `$A9C4` costs the player energy). The `$8C` child (bank 0 `$98A3`) is not ported: the tests free its slot each loop with a `$D450` poke. The `$8A` never integrates |
| `$52` | bank 11 `$9CFC` | `families/k52.rs` | `matches_the_oracle` (TAS 6580 area 04:00, the stage 1 boss parts' 8-frame effect: 7 loops, `$7DD0` 0, on screen and drawn every loop), `matches_the_oracle_in_the_last_room` (TAS 37460 area 1E:00), `every_hinted_phase_is_ignored` and `every_hinted_state_is_ignored` (forcing the reported `$7DD0` `00`/`01`/`5D`/`5E`/`FF` and `$7D50` `00`/`5D`/`A0`/`A1`/`FF` still matches and still draws), `off_screen_skips_the_draw` (0 drawn past the right margin). `load`, `on_screen`, `tile_bits`, `draw` (bank 1) and `store`; never `integrate`, never contacts, so its `$7DD0`/`$7D50` columns are residue |
| `$50` | bank 11 `$9C13` | `families/k50.rs` | `matches_the_oracle` (`Scene::tas(6576)`, area 04:00, the `$B1E6` boss-beaten script in slot 15: 30 loops, phase 0, never drawn; each frame it forces the player's motion `$0A`, `$047A` `$40`, `$0481` `$80`, `$B8` bit 7 and clears the 16 `$7BD0` weapons, and every fourth `$7E90` frame spawns a `$52` at a `$7E10` offset around the `$7DF0` slot via `$9C67`), `rewrites_the_player_and_clears_the_weapons` (those bytes forced distinctive first, so the diff is not vacuous), `arms_the_level_end_at_the_countdown` (`$7E90` `$BF` -> `$C0`: energy `$0E`, `$0406` `$0E`, `$0404` 0, `$BA` 3), `frees_itself_in_the_last_area` (the same arm at area `$22` frees the slot instead), `every_hinted_state_is_ignored` (`$7D50` `00`/`64`/`A1`/`FF` x `$7DD0` `00`/`5E`/`FF` are slot residue the handler never reads). It calls no §9c routine and never draws or contacts. The `$52` child (bank 11 `$9CFC`) is unported, so the tests move the `$50` to slot 0 and free every other slot at `$D450` each loop (the child lands in slot 15, already run, and is freed next frame). `$0475` is written 0 and nothing else writes it, so the omitted write is a `shortcut:` |
| `$58`, `$5A` | bank 11 `$9E23`, `$9E7A` | `families/k58.rs` | one module for both, as `k06` covers `$04`/`$06`. `matches_the_oracle` (`$58`): `Scene::tas(13982)` (area 0C:02), 192 loop frames, phase 0, never drawn; it probes the metatile at the player's x and a player-relative y with `$C99D` and runs the level end `$D274` on a damage response. `every_phase_of_the_flash_runs` (`$5A`): 397 loops, `$7DD0` 0-64 (each count once, then held at 64 once the `$7DF0` retry latches), the two 4-byte palettes `$9EBA` alternating into `$04E4-$04E7` and `$B5`, never drawn. The `$58` diff stops before frame 14239, where its probe reaches the class-`$10` spikes handler `$CAA9` (ported since, `collision::spikes`), so its level-end branch is a lead |
| `$8C` | bank 0 `$98A3` | `families/k8c.rs` | `matches_the_oracle` (`Scene::tas(12099)`, area 0A:00 with its `$8A` parent kept so children keep spitting: 360 loops, 101 occupied and 100 drawn, a straight child (behaviour bit 7 clear, phase 0) and a swinging one whose `$7DD0` counts `$60` -> `$00` -> `$F8` -> its `$C0` floor), `matches_the_oracle_while_it_swings` (`Scene::tas(13011)`, the lone subtype-3 child: the whole `$A0` -> `$00` -> `$F8` countdown, held at the `$C0` floor for 55 frames, then the off-screen free), `every_x_offset_matches_the_oracle` (all four `$9932` entries, the subtype forced at `$D450`), `hurts_the_player_up_close` (the `$AEE3` contact costs the player energy; the whole-game TAS never takes it, §8b's contact 0). It calls `$A000`/`$A852`/`$AC06`/`$84CA`->`$C247`(bank 9)/`$AEE3`/`$A934`, all in 9c, and never `$A79B`: the `$7DF0:$7DD0` pair is its own y velocity, stepped `-$08` a frame while `$7D90` bit 7 is set and floored at `-$40` |
| `$80` | bank 0 `$9297` | `families/k80.rs` | `matches_the_oracle` (`Scene::tas(27600)`, area 16:00: 400 loops, phase 0, on screen, never drawn), `far_off_screen_despawns` (the `$CD50` free with re-arm `$80`). A hidden spawner: every 64 frames (`$7E90 & $3F`) it drops a `$7E` (unported) into the highest free main slot at y `$0140`, copying its own x and `$7D90` (recomputed from `$ABDF` when the parent's bit 7 is set), then increments its `$7E90` counter; off screen but near it only counts, far it despawns. No `$7DD0` dispatch, no draw, no contact, so the tests free the `$7E` slots (4-15) each loop |
| `$84` | bank 0 `$9361` | `families/k84.rs` | `matches_the_oracle` (`Scene::tas(33598)`, area 1C:00: 264 non-IRQ brackets, phases 0-2, occupied every loop, never drawn, no contact). A stage controller: it ages its `$7D90`/`$7DD0` and, through the `$93E1` helper, queues two blank nametable runs (`$C0C6`) and a second-class `$0E` slot, and every 16th frame drops a `$86` via `$AC16`. Phases 0-2 are the frame-start set; §8b's `$FE` is the helper's `$7DD0 -= 2` write, overwritten with `$02` in the same run. When the camera reaches page 0 (`$CC = 0`) `$936A` clears `$0401` (the CHR animation switch, now `Effects::animated` so handlers reach it) and frees the slot: TAS 36587, in the chain |
| `$8E` | bank 0 `$993A` | `families/k8e.rs` | `matches_the_oracle` (`Scene::tas(14672)`, area `0E:00`/`10:00`: phases 0-5 over 570 of 1000 loops — the area's FME-7 IRQ drops the rest — every loop holds the `$8E`, drawn; §8b's 433 on-screen frames), `the_burst_spawns_shots` (the `$9D13` five-`$90` burst, which the natural run only reaches on IRQ-dropped loops, so `$7DD0`/`$7DF0`/`$7E90` are held; the `$90` shots are freed each loop, as `k90` tests them from the unpoked burst), `the_boss_hurts_the_player_up_close` (the `$AFD6` tail contact, both actors pinned: `$0479` is set), `the_death_matches_the_oracle` (the `$AF97`/`$B1C4` pair, `$7D70` held: the `$50` spawn and the every-other-frame flash). The tail runs the `$A852` on-screen check, the three bank-9 draws with `$E6`/`$E7` from `$9DA1`, and the `$AFD6` amount `$00E0` every on-screen loop |
| `$94` | bank 12 `$8000` | `families/k94.rs` | `matches_the_oracle` (`Scene::tas(33631)`, area 1C:00 slot 1: 120 loops, phase 0 then `$80`, never drawn, no contact, no `$9c` routine). A dormant controller: if the camera page `$CC` is 0 it zeroes its own slot (`$7CC0,x`); otherwise, on every other frame (`$F7` bit 7), it decrements `$7E90` and, when that underflows, hands a `$96` child (`$7CC0,y=$96`, `$7E10,y=parent` slot, `$7EE0=$50`, `$7BE0=$08`, `$7C00=1`, `$7C90=6`, the rest cleared) to the highest free main slot and sets its own `$7DD0`/`$7E90` to `$80`. The `$96` child is unported and its death clears the parent's `$7DD0`, so the test frees slot 15 with a `$D450` poke each loop (as `k84` does), which also holds the parent at phase `$80` after its one spawn |
| `$96` | bank 12 `$8065` | `families/k96.rs` | `matches_the_oracle` (`Scene::tas(33632)`, area 1C:00, weapon and `$9A` child slots cleared each loop: the whole 70-frame phase cycle, phases 0 (16), 1 (30) and 2 (24), drawn 69 of 70, the `$8162` `$9A` child dropped when phase 2 reaches `$10`; disabling the spawn makes it fail), `matches_the_oracle_through_its_death` (`Scene::tas(33652)`: the player's weapon kills it after 15 loops — `$A56E` then `$806F` clears the `$94` spawner's `$7DD0`, visible because `isolate` left it at `$80`). The spawner's slot is the `$96`'s own `$7E10` (`Body::counter`); the port read it through `Cx`, which holds a blank copy of the running slot, so it cleared slot 0's phase instead (the chain, TAS 33652, where the `$94` is in slot 1). The `$94` spawner writes its contact byte `$50`, so `$A9C4` skips the player check and the `$96` never hurts the player; only the `$AA57` weapon scan acts. The `$9A` child (bank 12 `$8208`) is unported |
| `$98` | bank 12 `$81DB` | `families/k98.rs` | `matches_the_oracle` (`Scene::tas(33598)`, area 1C:00 slot 2, `$7D50` `$61`: 267 loops, phase 0, drawn 264 — the same two pieces every loop, so `drawn` counts only loops whose OAM page changed). No `$A000` load, no projection, no physics, no `$7DD0` dispatch and no contact: it sets `$94`-`$99` itself, then `$AC06` (`tile_bits`) and the bank-9 draw `$C247` through `$81C6` (which maps bank 9 at `$A000` and restores bank 13). `the_chr_shadow_branch_matches_the_oracle` pokes `$E2` to `$00` then `$0F` (the sprite drops one pixel for `$0F`); both match, so the unread `$90-$93`/`$9A`/`$9B+` are the carried working set |
| `$9C` | bank 12 `$82F7` | `families/k9c.rs` | `matches_the_oracle` (`Scene::tas(33598)`, area 1C:00 slot 3, `$7D50` `61`: its one occupied loop, phase 0, never drawn). It calls no `$A000` load, no projection, no physics and no contact: it stamps `$60` over the second half of the room's page -> row table (`$0450-$045F`, `Schedule::rows[16..32]`) and frees its own slot. `the_occupied_frame_rewrites_the_row_table` (the first gameplay loop frame, 33632) pins the write non-vacuous |
| `$9A` | bank 12 `$8208` | `families/k9a.rs` | `matches_the_oracle` (`Scene::tas(33956)`, area 1C:00 slot 23: the whole life of the `$96` parent's child — `$7D90` bit 7 clear moves x by `$7E90:$7E10` and y by a `$7DF0:$7DD0` offset stepping 3 a frame from `$E8` to the `$30` clamp, draws through the bank-12 `$82E5` table with the bank-9 metasprite, then at y row `$0C` the bit-7 dying tail parks it at `$0C20` and the twelve-frame `$82F1` animation frees the slot: all 25 reachable phases, 65 occupied, 63 drawn; `$FE`/`$FF` never occur, the offset stepping by 3 from `$FD`), `hurts_the_player_up_close` (player on it: the `$AEE3` touch costs 2 energy and takes the same dying tail). The `$96` spawner (`$8162`) is unported, so both tests isolate the already-spawned actor |
| `$82` | bank 0 `$931F` | `families/k82.rs` | `matches_the_oracle` (`Scene::tas(33305)`, area 1A:04: the level-start record in slot 0 runs its whole `$C0`-frame count — 192 loops, phase 0, never drawn, no contact — through the first-frame music stop (`$40` = `$FF`), the song request at `$7E90` = `$40` (`$41` = `$44`), the CHR bank 2 selection (`$E2` = `$0E`/`$0F` by `$7E90` bit 2) and the `$BA` = 3 next-area arm). It calls no §9c routine; it never reads `$7DD0`, so its only phase is 0 |
| `$A2` | bank 12 `$8412` | `families/ka2.rs` | `matches_the_oracle` (`Scene::tas(32675)`, area 1A:02: the `$A0` controller's two `$A2` children in slots 16-23, 28 occupied and drawn loops before they fall off the bottom, phase 0 throughout, the `$BE98` bank-1 metasprite, the `$AEE3` touch called each loop), `off_screen_frees_its_slot` (x pushed past the right edge: the `$A852` off-screen branch frees both slots undrawn), `hurts_the_player_up_close` (the `$AEE3` contact costs 2 energy). No `$7DD0`/`$7D50` dispatch: the handler reads neither, and it calls `$A934` (`store`) instead of `$A8E4` (`end_frame`). The spawner `$84C2` is inside the unported `$A0` handler (bank 12 `$83CD`), so the tests isolate the already-spawned children |
| `$A0` | bank 12 `$83CD` | `families/ka0.rs` | `matches_the_oracle` (`Scene::tas(32657)`, area 1A:02, slots 0-1: 284 compared loops of 300 — the area's FME-7 IRQ drops the rest — 214 occupied, 397 slot-frames all `$7DD0` phase 0, never drawn and no contact, matching §8b's 0 on-screen frames). The `$A2` children (`$84E2`, slots 16-23) are unported, so a `$D450` poke frees them at the loop head each frame; the diff covers the spawn. All three branches run: the `$21` >= `$04` reset (`$7E10` = `$7E90` = `$28`), the closer `$21` < `$04` path (`$7E10` -= 4 when >= `$09`, then `$7E90` = `$7E10`), and the `$21` >= `$18` far despawn that ends both slots. The handler dispatches on no `$7DD0`/`$7D50` |
| `$AA` | bank 12 `$8870` | `families/kaa.rs` | `matches_the_oracle` (`Scene::tas(37457)`, area 1E:00: the `$A8` parent's two parts in slots 12/14, 110 loops, 97 occupied, both `$7DD0` phases — phase 0 (79 slot-frames, the `$28`-frame countdown and its `$893F` sprite selector) and phase 1 (114, the `$8944`/`$894E` bank-9 metasprite pair and the `$7E70:$7E80` velocity decrement with its `$C0` low clamp), on screen and drawn 81 loops, then the off-screen free at `$8931`). The handler calls no `integrate` or contact routine, so there is no player touch |
| `$A4` | bank 12 `$846A` | `families/ka4.rs` | `matches_the_oracle` (`Scene::tas(32745)`, area 1A:02 slot 2: the whole 200-frame life, phase 0, never drawn — the 79 drawn loops are its `$A2` children), `drops_a_child` (the `$84C2` drop on the 25th frame, a live `$A2` in slots 16-23), `frees_itself_when_the_player_is_far` (`$8472`/`$CD50`). A dormant patrol: it reloads (`$A000`), tests the player distance (`$ABDF`), counts `$7E90` down and every 24 frames drops a `$A2` from three x positions (`$84BF`, ping-ponging `$7E10` via `$7DF0` bit 7), never reads `$7DD0`/`$7D50` so its phase is the spawn value. `$84C2` (shared with `$A0`) is ported inline |
| `$A8` | bank 12 `$87EC` | `families/ka8.rs` | `matches_the_oracle` (`Scene::tas(36968)`, area 1E:00: the `$A6` boss's `$A8` parts in slots 16-23, both `$7DD0` x-velocity phases `$10`/`$F0` run, on screen and drawn 59 of their 60 occupied loops before the y-`$0C` free). The intended 1071 on-screen frames need the `$A6` parent, which is unported, so `isolate` frees it and no more parts spawn; `hurts_the_player_up_close` pins a part onto the player and the `$AFD6` mode-`$80` contact drains him. Calls `$A000`, `$A852`, `$C247` (bank 9 through the `$81C6` wrapper), `$AFD6` and `$A934`, all in §9c |
| `$A6` | bank 12 `$84EB` | `families/ka6.rs` | `matches_the_oracle` (`Scene::tas(36863)`, area 1E:00: the stage 6 boss, 800 loops all occupied, 553 drawn, phases 0/1/2 (82/550/168); the phase-1 `$8737` `$A8` part pair into slots 16-23 and the death's `$B1C4` `$50` and `$8785` `$AA` spawns, each freed next loop so the unported parts never run). The `$8737` spawn loop leaves X = `$1F` (0 after a full pair), so that frame's steering (`$8582`-`$8630`) runs on slot 0, not the actor's slot — ported as-is. `hurts_the_player_up_close` (pinned onto the player with `$0473` cleared, the `$AFD6` touch sets `$0479`). `$7DD0` only reaches 0-2: §8b's `AE`/`FF` are the area-transition payload (`$99DB`, frame 37672) and the bank-2 loader (`$8AED`, 38075) overwriting slot 1 after the boss's last frame, not handler phases (the `$87C5` table has three entries; `$AE` would `jmp $0069`). Calls `$A000`, `$AF97`, `$B1C4`, `$AF38`, `$A852`, `$C247` (bank 9), `$AFD6` and `$A934` — all in 9c — and no `$A79B` |
| `$9E` | bank 12 `$8309` | `families/k9e.rs` | `matches_the_oracle` (`Scene::tas(33794)`, area 1C:00, slots 3-4: the `$9E` hovers at a fixed x, so with `isolate` the player is placed beside it (as `k0a` does) to enter its `$300` wake range — phases 0 (85 slot-frames) and 1 (199), on screen and drawn all 200 loops), `hurts_the_player_up_close` (the `$A9C4` contact costs 2 energy). Calls only §9c routines (`load`, `integrate`, `player_dx`, `collision::feet`, `spawn_parts`, `on_screen`, `draw` bank 9 through `$81C6`, `contact`, `end_frame`); reads no RAM byte `Effects` lacks. The phase-1 `spawn_parts` burst branch is not exercised (a lead: the fall is too slow to reach ground before the room change) |
| `$AE` | bank 12 `$8A52` | `families/kae.rs` | `matches_the_oracle` (`Scene::tas(38793)`, area 20:02: the `$B0`'s `$8C20` spawns fill slots 7-15, 279 compared loops of 405, 68 occupied, all `$7DD0` phases 0/2/4 (105/79/96 slot-frames), drawn 34), `hurts_the_player_up_close` (the `$A9C4` contact costs the player energy and enters phase 4). Phase 0 falls (`vy -= 5`) and, once it underflows, takes its float velocity from `$F9`; phase 2 is every other frame — the subtype countdown (from the spawner's `0`) or the `$C93B` solid probe 128 px above it enters the death phase; phase 4 steps `$7C30` 2-4 through the bank-9 metasprites (its `$B178`/`$B182`) and frees the slot at `$7E90` `$0C`. The window stops before an unported `$C8` spawn at frame 39199 |
| `$C8` | bank 12 `$9A26` | `families/kc8.rs` | `matches_the_oracle` (`Scene::tas(38119)`, area 20:00 slot 0, `$7D50` `61`: its one occupied loop at 38120, phase 0, never drawn). It calls no `$A000` load, no projection, no physics and no contact: it clears the second half of the room's page -> row table (`$0450-$045F`, `Schedule::rows[16..32]`) and frees its own slot. `the_occupied_frame_clears_the_row_table` pins the fill non-vacuous (the loop frame starts with `$40 $00 $E0 …`) |
| `$BA` | bank 12 `$94BC` | `families/kba.rs` | `matches_the_oracle` (`Scene::tas(40013)`, area 22:00: the `$B6` parent's `$92B5` spawns in slots 21-23; with `isolate` only the already-spawned parts remain, 120 loops, 97 occupied, drawn 96). A boss shot: it adds the 16-bit `$7DD0:$7DB0` to x and `$7E10:$7DF0` to y, frees itself below row `$0B` (`$92` >= `$0B` with `$93` bit 7 set), draws the four `$9526` bank-9 metasprites cycling with `($7E90 >> 1) & $06`, runs the `$AFD6` boss contact (`$0258`, mode `$80`) and stores with `$A934`. `$7DD0` is the x velocity's high byte, 0 in the scene, so its only phase is 0 (244 slot-frames); it never calls `$A79B` (`integrate`); its `$AFD6` touch is forced by `hurts_the_player_up_close` (a part pinned onto the player, `$0473` cleared, the `$0479` hurt timer set), since the whole-game TAS never takes it (§8b contact 0) |
| `$BE` | bank 12 `$9603` | `families/kbe.rs` | `matches_the_oracle` (`Scene::tas(40532)`, area 22:00: the `$B4` controller's `$BE` parts, 120 loops, 107 occupied, 106 drawn, `$7DD0` 0 throughout — the whole fall, the `$0B00` bounce, the bank-9 draw every on-screen loop, the `$AFD6` brush and the off-screen free; the `$B4` parent is unported, so `isolate` frees it and no more parts spawn). It calls `$A000`, `$A852`, `$C247` (bank 9 through the `$81C6` wrapper), `$AFD6` (amount `$0258`, mode `$80`) and `$A934`, all in §9c. `$7DB0`/`$7DD0` are its x velocity's low/high bytes and `$7DF0`/`$7E10` its y velocity's, so the `$7DD0` "phase" column is that velocity high byte (0 throughout) |
| `$C0` | bank 12 `$96B1` | `families/kc0.rs` | `matches_the_oracle` (`Scene::tas(40787)`, area 22:00, the stage 7 boss's parts in slots 22-23: 110 loops, both occupied and drawn every loop, both `$7DD0` x-velocity phases `$00`/`$FF`), `hurts_the_player_up_close` (pinned onto the player, the `$AFD6` mode-`$80` touch drains him). A bouncing body: `$7DD0:$7DB0` is its x velocity and `$7E10:$7DF0` its y velocity, added to the position each frame, with bounces off the room's walls (`x` `$0200`/`$0E00`) and floor row `$0B`; it draws the six bank-9 metasprites at `$97AA` (indexed by `$7E90 << 2 | $F1 & 2`) and, when freed, clears a bit of the parent slot named in `$7C30` before clearing its own. Calls `$A000`, `$A852`, `$C247` (bank 9 through the `$81C6` wrapper), `$AFD6` and `$A934`, all in §9c |
| `$AC` | bank 12 `$89A1` | `families/kac.rs` | `matches_the_oracle` (`Scene::tas(38884)`, area 20:02, slots 1/3: 300 loops, every one occupied and drawn, phase 0 — the handler dispatches on neither `$7DD0` nor `$7D50`; the `$24` spawn is pinned by a probe that sees the child appear). `touches_the_player_up_close` (the player on the actor: the `$A9C4` contact sets his `$0479`; its damage `A` is 0, so energy does not move). Every 65 frames it drops a type `$24` into a free slot 16-23 with `$7C30` = 6; the children are left alive and drawn (their `$98D0` metasprite is bank 8, see the `$24` row). Off screen (`$A852` N set) the handler skips to `$89D9` without `LDX $10`, so its timer `DEC $7E90,x`, the reload and the child's `$7C90,x` all index with the X `$A852` left: the high byte of the failing axis's distance (`engine::off_screen_x`). In the TAS the off-screen `$AC` at x `$2B1F` counts `$7F8F` (`$7E90 + $FF`), which no array owns (`Slots::spare` holds `$7F70-$7F8F`), and spawns its `$24` from it at 38985 |
| `$BC` | bank 12 `$952E` | `families/kbc.rs` | `matches_the_oracle` (TAS 39834-39903: its one part's whole life — 30 loops of the phase-0 fall, then the row-`$0B` park and the 32-frame phase-`$FF` fall, drawn 61 of 62 — then the self-free), `matches_the_oracle_with_three_alive` (TAS 40045-40089: three parts in slots 18-20 run together, both phases, drawn every loop), `hurts_the_player_up_close` (a part pinned onto the player, `$0473` cleared: the `$AFD6` mode-`$80` touch sets the `$0479` hurt timer). It sets its own position: the x step is `$7DB0:$7DD0` and the y step `$7DF0:$7E10`, gaining 3 a frame with the low-byte `$80` sign clamp; it calls `$A000`, `$81C6` (bank 9), `$A852`, `$AFD6` (mode `$80`, no weapon scan) and `$A934`, never `$A79B`/`$A9C4`. The `$B4` spawner (bank 12 `$8C59`, `$8F55`) is unported, so the tests isolate the already-spawned parts |
| `$46` | bank 11 `$97FF` | `families/k46.rs` | `matches_the_oracle` (`Scene::tas(26808)` with the pad released from there: unhit, the `$42` charges `$7E10` to `$C0` and spawns the `$46` into slot 23 at 27208 with no poke; 120 loops with `$42`/`$44`/`$46`, all occupied and drawn, through every `$7DF0` state: the fall (0, gravity +3 into `$7D90:$7DB0` to row `$0B`, effect `$35`), the three-frame landing (`$80`, effect `$23`) and the 12-frame white flash (`$C0`, the `$04A0` palette queued through `$C33C`, no draw or store); on the last frame the oracle restores the palette (`$C34B`), sets `$0479` to `$20`, drains `$B2:$B3` by `$03E8` (`$B17F`), frees slot 23 and zeroes the parent's `$7DD0`), `spares_the_player_once_the_boss_is_beaten` (`$A9:$AA` held at 0: the `$9817` branch, no hurt or drain). The TAS never spawns it because its fire keeps draining the `$42`'s counter |
| `$90` | bank 0 `$9DAD` | `families/k90.rs` | `matches_the_oracle` (`Scene::tas(14672)` with the pad released from there: the `$8E`'s phase-3 count `$7E90` reaches `$50` and `$9D13` fires its five shots at 15083 with no poke; from there, 120 loops with `$90`/`$92`: directions 5-9 (facing left) all run, the player takes the `$AFD6` contact (`$0479` rises), and the lowest directions hit the wall, effect `$23`, and burst into a `$92`), `the_right_facing_burst_matches_the_oracle` (the `$8E`'s `$7C00` bit 6 cleared the frame before: directions 0-4, another `$92`). The step tables are `$9E59`-`$9E80`, the metasprites `$9E81` (bank 9 through `$84CA`); a wall or leaving the screen frees it. The TAS never spawns it: it kills the `$8E` at 15050 with `$7E90` at `$3B`. The `$9E3C` arm (`$AC16` variant 8) is unreachable: no branch, jump or pointer in the PRG names it (`docs/CUTCONTENT.md` §3) |
| `$92` | bank 0 `$9E8B` | `families/k92.rs` | `matches_the_oracle` (the `$90` scene run to the first `$92`, 15093, alone: its 32 remaining loops, drawn every one, then the `$7ED0` free), `the_clamped_fall_matches_the_oracle` (`$7C30` bit 7 held: the `$9F15` clamp of a negative fall speed to `$7F`; `$9E51` always stores 0, so no game path sets it), `hurts_the_player_up_close` (part 3 held on the standing player: `$AFD6` mode `$80`, amount `$0100`). Type `$08`'s parts layout (the `$9F56` records equal `$ADDA`), one metasprite `$A812` with attribute 2, two parts a frame by `$F1` |

What the rest of stage 1-1 needs from the core:

| kind | handler | needs |
| --- | --- | --- |
| `$04`, `$06` dying | `$823C` -> `$8295` | **core**: reads RAM `$0181` (not in `World`) and calls the map stampers `$A0A9`, `$AEC6` (now in 9c) and `$A0CE` (the level-edit chain, not ported); the port panics on `$7D70` bit 7 until then |
| `$04`, `$06` dying | `$823C` -> `$8295` | ported: the shatter (`$82BF`) writes `Level::map` and `Level::attributes` and queues the tiles through `engine::queue_block`; `matches_the_oracle_when_shattered` covers TAS shatters in areas 0 and 6, both vertical modes and a wait for queue room, `shatter_matches_82bf_in_area_2` the area-2 ones as calls (the IRQ splits that area's actor loops) |
| `$08` | `$8506` | nothing (its four parts live in `Body` registers written by `$AC16`) |
| `$10` | `$8B55` | nothing |

### 9d-ii. Stage 1-2 (area 1, `$0400 = 2`): census and readiness

`engine::tests::census_of_stage_1_2` surveys the TAS (`Scene::tas`, frames
2047-6044, area 2) and pins each family's routines. The ones 9c lacked are
ported with routine diffs on TAS scenes: `$AF43` (`engine::player_dy`), `$AF7D`
(`collision::below`), `$AEC6` (`engine::is_page_row`), `$A0A9`
(`engine::map_page`). Internal jumps (`$A0B8`, `$A59F`, `$AB93`, `$B108`, `$C26F`)
are parts of ported routines.

| kind | handler | needs from the core |
| --- | --- | --- |
| `$04`, `$06` | `$823C` | nothing (the shatter is ported, see 9d) |
| `$08`, `$10`, `$24` | | nothing (ported or ready from 9d) |
| `$12` | `$8C56` | nothing |
| `$14` | `$8D70` | nothing |
| `$18` | `$8FD9` | nothing |
| `$1E` | `$9364` | nothing |
| `$20` | `$94B2` | nothing (it runs in slots 16-23 like `$24`; aliasing is handled) |
| `$26` | `$98D6` | nothing |

Area 2 arms the FME-7 IRQ; brackets with an IRQ inside are dropped like those
with an NMI (`PeekEvent::irqs`). Area 4 (frames 6045-6783, before stage 2) adds
`$34` (`$8A03`: `$C0C6`, `$C199`, `$D1F5`, `$D24A`), `$38` (`$8D1F`: `$AF97`,
`$AFD6`, `$B076`, `$B1C4`) and `$52` (`$9CFC`, ready). Their core routines are
ported (`engine::queue_counters`, `boss_flash`, `boss_contact`, `boss_defeated`);
the families are not.

### 9d-iii. Core prep for a stage (checklist)

1. Find the stage's frames in the TAS: `tools/emu/tas/inputs.csv` (fetch with
   `tools/emu/fetch-tas.sh`); the area (`$0400`) and room (`$0402`) changes are
   listed by `rotj-oracle/tests/tas.rs`'s `areas_and_rooms`.
2. Survey them: `oracle::survey(&mut Scene::tas(first), frames, &mut census)`
   gives family -> routines (interrupt handlers excluded); map families to kinds
   through `$CF72` (main) and `$CEC3` (second class).
3. Diff the census against 9c. For each routine missing: read it, port it into
   `engine.rs` or `collision.rs` (additions only while a fan-out runs), and give
   it an `assert_routine` diff from a TAS scene where it runs (find the frames
   with an exec trace on its entry).
4. Pin the census in a `census_of_stage_*` test and record readiness in 9d.
5. Note any family that writes outside the slot arrays and `Effects` (map,
   VRAM queue, PPU registers): that is a core task, not a fan-out task.

### 9e. Fan-out recipe (one handler family per task)

1. Add `crates/rotj-core/src/actor/families/k<kind>.rs`: `pub struct State`
   (fields that live outside `Body`), `State::step(&mut self, kind: &mut u8, body:
   &mut Body, work: &mut Work, cx: &mut Cx<'_>)` calling the `engine` and
   `collision` functions exactly where the 6502 calls the routines in 9c (`load`
   for `$A000` included: `Work` is the game's zero page `$90-$A6` and keeps the
   previous slot's values; a routine's `$20-$2F` results are return values or a
   `collision::Probe`), the ROM addresses it reads (`engine::rom_byte`,
   `rom_word`), and `#[cfg(test)]` tests built from a `Scene` (`route`,
   `replay`, `level`, `brawl`, `place`, `camera`, `player`, `play`, `script`, RAM
   pokes) that call `assert_actor_phases(scene, frames, &[kinds])` and assert
   the returned `Coverage` (loops, occupied, drawn, frames per phase) so every
   phase of the handler is exercised. Children of other types that the scene
   spawns can be removed each loop with a poke at `$D450` (see
   `k0a::matches_the_oracle_up_close`).
2. In `actor/families.rs`: `pub mod k<kind>;`, a `Brain` variant, and one arm
   each in `Brain::new`, `step` and `lift` (`lift` defaults to `Brain::new`). Every
   family keeps its state in `Body`, so there is no projection hook; a family that owns
   other bytes adds one to `Slots::project`.
3. One row in 9d.
4. `tools/check-fanout.sh <base> crates/rotj-core/src/actor/families/k<kind>.rs`
   must exit 0 (it checks committed, uncommitted and untracked changes, rejects
   deletions, renames, other families' files and `impl` blocks on core types;
   `tools/check-fanout.test.sh` tests it). A task that needs `Body`, `Work`,
   `World`, `Cx`, `engine.rs`, `collision.rs`, the spawners, `OamBuilder` or the
   harness to change, or reaches a panicking path, stops and reports instead.

### 9f. Core limits a task must not patch around

- Slots 16-23 alias the 16-wide arrays: `$7BA0,x` is `$7BB0` (weapons),
  `$7EE0,x` is health high of slots 0-7, `$7E30-$7E80,x` the motion of slots
  0-7. The actor loop loads a slot 16-23's 16-wide fields from those owners
  before its handler and stores them back after (`slots::load_aliases`,
  `store_aliases`), and `slots::read`/`write` (`Cx::byte`/`set_byte`) resolve
  any slot-array address to its owner; `store_in_child_slots_writes_the_weapon_bytes_they_alias`
  proves it. Inside a handler, a write to another slot's field that a 16-23
  slot aliases is seen by that slot when it runs next, as in the game, because
  the slots run 23 down to 0.
- Unported kinds in play run `engine::unported`: load, `$A852`, and on far off
  screen `$CD50` (free and re-arm with `$80`), which is what `$823C` does; they
  never draw or collide.
- Second-class actors (slots 24-31) run in `step_second` (`$CE7F`, dispatch by
  the raw kind through `$CEB1`/`$CEC3`, `Brain::new_second`) and draw in
  `draw_second` (`$DC87`), over the same 32-wide arrays at slots 24-31
  (`Second::body`); their records write `$7BD8,x` and `$7EF8,x`, which are main
  slot 16+i's health low and dying timer (the platform's travel limit). A
  second-class family is `step(&mut self, index, body, cx)` in
  `families::step_second`; `assert_second_phases` diffs both brackets.
- Unreached collision class handlers (wall `$CA4E`/`$CA66`) and the `$89AF` spawn path panic.
- A handler reads its own slot's fields from its `Body`: `Cx` holds a blank copy of the
  running slot (only its kind). For an `array,x` access whose X may be another slot's
  index, `slots::own_field` says whether it lands on the running slot (`kac::Indexed`).
- Map pages 16-31 (`$0450`, the second nametable of a `$DD` bit 7 room) are run by
  the whole-loop chain (9k); the shield deflection is ported from the
  code but no test reaches it.
- `Cx::slots` holds only the running actor's `kind` in its own slot while it runs.
- The running slot's index is `Work::slot` (`$10`): the actor loop sets it
  before each handler and the harness lifts it from `$10`, so `$B1C4`'s X
  (`k38`), the child's parent link (`k2e`) and `$7BD0,x` (`k32`) read it.
  The second-class loop does not set it; its families take `index`.
- A handler that frees its own slot and then spawns calls
  `engine::free_own_slot` instead of `*kind = 0`: it writes the freed slot into
  `Cx::slots`, so `$AC16` (searching 15 down to 0) can reuse it, and the actor
  loop keeps the slot's contents over the handler's copy whenever the handler
  freed itself and the slot's kind no longer matches. A spawn of the running
  kind into its own slot would be lost (no handler does it).
- The palette shadow `$04E0-$04FF` and `$B5` are `Effects::palette` and
  `Effects::r_b5`, so handlers (type `$5A`) reach them through `Cx::effects`;
  `frame::Display` keeps only the CHR animation (`$0401`, `$0410-$043F`).

### 9g. Weapons (bank 5 `$8000`, task `weapons`, 2026-10-14)

The gameplay loop calls bank 5 `$8000` at `$D444` (`LDA #$15; STA $E9; JSR
$8000`) and returns at `$D447`. The routine maps bank 1 into `$A000` and
dispatches on `$0476` through the 8-entry table at `$801B`. The captured
handlers are ported in `rotj_core::actor::weapons`; `step` reuses `engine::draw`
(`$C247`) and `collision::solid` (`$C93B`).

| routine | port | notes |
| --- | --- | --- |
| `$9E13` | `integrate` | adds `$7D00:$7D10` to `$7BB0:$7BC0` (level x) and `$7D20:$7EB0` to `$7C60:$7C70` (level y), into `$90-$93` |
| `$9E46` | `project` | the `A = 0` call only: dx/dy minus the camera; off-screen frees the slot, on-screen writes `$94/$95` and `$7B80/$7B90` |
| `$8029` | `bolt` (was `zero`) | `$0476` 0: the fired projectile, including its spent sprite (`$8080`) |
| `$8170` | `homing` (was `one`) | `$0476` 1: handler 0's flight and bounce, steered by `$8288` (below); types `$80`-`$82` are its charged attack `$847D` |
| `$8891` | `splitter` (was `two`) | `$0476` 2: the splitter (not a boomerang: it never turns back); wall bounce `$8993`, the 4-part split `$89AC`/`$89D7`, the `$7CB0`/`$7C20` velocity trim, two sprite pointers |
| `$8D39` | `wave` (was `three`) | `$0476` 3: the wave, two weaving parts (not a homing missile); wall bounce `$8E65`, the weave `$8DD8`/`$8DF3`/`$8E42` (once read as a screen-edge bounce: it reads only `$7F40` and the velocity), three sprite pointers |

**Arrays.** The bracket touches `$7B80`, `$7B90`, `$7BB0`, `$7BC0`, `$7BD0`,
`$7C20`, `$7C60`, `$7C70`, `$7C80`, `$7CB0`, `$7D00`, `$7D10`, `$7D20`,
`$7EB0`, `$7F20`, `$7F30`, `$7F40`. `Weapon` gained `r7bc0`/`$7BC0` (level-x
low), `r7c60`/`$7C60` and `r7c70`/`$7C70` (level-y high/low); the rest were
already there. Metasprite pointers `$814A`/`$814C`/`$814E` and `$8A2F-$8A4C`
live in bank 5; `$8EE7`/`$8EEB`/`$8EEF` are `$A000`-window pointers (bank 1 at
the call), so `engine::draw` picks the bank from the pointer's window. `$9E46`'s
`$C5F2`/`$C6F2` tables are `dx >> 4` and are computed.

**Evidence.** `weapons::tests`: `assert_routine` over the bracket `$D444`-`$D447`
compares 1520/1520 calls of `Scene::tas(351)` over 1650 frames (490 with a live
`$7BD0` projectile) plus 160 and 200 calls of two `Scene::route` scenes, matching
the oracle byte for byte (RAM, WRAM and the OAM page). The TAS's `$0476` at the
call is `0` for 97 frames, `2` for 812 and `3` for 611, so handlers
`$8029`/`$8891`/`$8D39` run and `$8170`/`$91B2`/`$9647`/`$9D2E` do not. Handler 0's wall bounce `$80C5` (the
hit marks `$7CB0` bit 0 on the first frame in a wall; the next frame `$80DA`
sends the shot back, or a falling one to the side, with `$F9` bit 0 as the
random sign) is ported like handler 3's `$8E65` (which takes `$F9` bit 7);
`a_shot_bounces_off_a_wall` places a shot in the ground under the player at TAS
370 and diffs 40 calls through both frames. The ports of 2's and 3's first wall
frame had set `$7CB0` to `timer << 1 | 1`; the game's `LSR; SEC; ROL` is
`timer | 1`.

**The wave's weave** (once called a screen-edge bounce: it reads no position). `$8DD8` (x) and `$8E0E` (y) step the
velocity's low byte by 4 (down while `$7F40` bit 7 is clear, else up) and keep it
in `$7F40` when it lands in `$40-$BF`; each `BMI`/`BEQ` after `STA $7F40,X` tests the `CMP #$C0`/`CMP #$40`
before it, not the stored byte. So a step down always ends the bounce, and a
step up on x goes on to y unless it landed on `$40`. The port had followed the
stored byte: a deflected wave part falling into the window (`$7F40` bit 7 clear,
low byte `$40-$7F`) got a step down and a step up, 4 more than the game's
(walking right and firing from TAS 470 with the old bounce:
`$7EB6` port 4C, oracle 48, at frame 603). `firing_scenes_match_the_oracle`
keeps those scenes (six pad patterns: walking either way, jumping, crouched,
aiming up), and `weapon_{0..5}_fires_in_every_pose` play the same patterns from
TAS 600 with `$0476` poked to each weapon, 140+ loop frames each.

**Weapon 4's bubble** (`$930B`) is dropped before its frame count steps:
`LDA $7C80,X; BMI $9356` comes before `INC $7F60,X`. The port counted first,
one more than the game on the frame a hit set `$7C80` bit 7 (seeded play from TAS
2150: `$7F6D` port 1E, oracle 1D at frame 2679; from 6300: `$7F6C` at 7887). The
TAS's own weapon 4 stretch in stage 4 hits it too (`$7F6A` port 15, oracle 14 at
17353); `weapon_4_bubbles_match_the_tas_in_stage_4` chains 99 loop frames over it.

**Weapon 0's charge can freeze the game.** Handler 0 flies its charge (type
`$80`, from `$9A81`) as a plain shot, and `$9A81` leaves `$7C80`/`$7CB0` as the
slot's last shot left them. A shot that ended spent leaves `$80`/`$FF`, so the
charge takes the spent path and draws `$814E[$FE]` = `$D020`, a pointer into
bank 14's code (the `$C000` window during the loop). `$C247` walks a metasprite's
pieces from offset `n - 1` down by 4 until the offset is 0, and `$D020`'s length
byte `$52` is not `4k + 1`, so the walk never gets there: the call returns only
when the page fills on a frame with `$F7` bit 7 clear, and otherwise loops for
ever. Seeded play from TAS 1000 with weapon 0 (pad seed 48514) charges into slot
15 at frame 1676 on a drawn frame: the oracle's `$C247` never returns and no
`$D45C` or `$C232` follows in the next 600 frames. All five fuzz seeds that played
weapon 0 from TAS 1000 froze this way. The port decodes the walk the same way
(`Metasprite::endless`, one lap of 64 pieces), draws a pointer in the `$C000` or
`$E000` window from banks 14/15, and sets `OamBuilder::hung` when a lap places
nothing or the frame would draw past a full page; `Game::frame` then does nothing.
`oam::tests::forced_cases_match_the_oracle` points `$C247` at `$D020` on even
frames and matches the oracle's page through the second lap and the full-page
exit; `weapon_0s_charge_in_a_spent_slot_freezes_the_game` chains the loop to the
freeze (674 loop frames), where `assert_game` checks that the oracle never waits
again (`GameRun::hung`). A stale timer from another weapon's shot could point the
draw below `$8000` (68 of the 128 words at `$814E-$824C` do); the port panics there.

**The charged attack.** Holding B for `$30` frames after a shot (`$B8` bit 6,
`$B7` counting) runs bank 4 `$9A56`, which puts a type `$80` part in the first
free slot through `$9A75[$0476]`: `$9A81` (weapons 0 and 1: a part handler 0 flies as a plain
shot and handler 1 gathers as its charge), `$9AA0` (weapon 2, four parts numbered in `$7F50`) or `$9AE3`
(weapons 3-5). `Weapon` gained `r7f50`/`$7F50` and `r7f60`/`$7F60` (the part's
index and frame counter). Handler 2's type `$80` is `$8A4F`: each part circles
the player (velocity steps from `$8C71`-`$8C89`, the turn flipping every 64
frames in `$7F40`) and, when B is let go, flies off as a type `$81` boomerang
from the `$8C61`/`$8C67` tables. Handler 3's is `$8EF5`: type `$80` gathers in
front of the player for eight frames, `$81` fires a type `$82` shot every
fourth frame for 64 frames (angle `$9192`/`$91A2`, stepped by `$7F50`) and then
clears every slot and returns from the whole routine, and `$82` flies 24
frames. Both draw through `$9E46` with `A = $80` (an off-screen part keeps its
slot; a row just under the screen sets `$9A`). `weapons::tests::weapon_*`
stand at TAS 470 with the actors cleared, poke `$0476` to 0, 2 or 3, hold B
120 frames and let go, and chain the whole loop (`assert_game`) through it: 190
loop frames each equal the oracle's. `weapon_{1,2,3}_charges_aiming_up` press B, then hold Up+B, so the
charge runs in motion 6 (`$9EA7`'s spot, handler 2's `$8AFF`/`$8C18`, handler
3's `$8F16`/`$9023`) and match the same way; `every_weapon_fires_up` fires
`$9EB5[$0476]` for weapons 0-5 (`$9BCE`, `$9BF5`, `$9C06`, `$9C2E`, `$9C7E`,
`$9CD9`) standing with Up held, 110+ loop frames each. With no free slot `$8FE8` increments `$7F60,X` at X = `$FF`, which writes the
FME-7 command latch `$805F` (bank 5's byte, then it plus one). Nothing can follow
from it: the latch only selects the register the next `$A000` write sets, and
every `$A000` write in the game comes right after its own `$8000` write (`$C229`,
the NMI's `$E1C0`-`$E226`), so the port leaves it out.
`weapon_3_spread_with_no_free_slot` fills slots 0-13 with spread shots once the
charge is out and chains 125+ loop frames through the full-slot spawns and on.

**Handler 4** (`$91B2`, the three-way shot `$9C7E`/`$9E09` fires) is handler 0's
flight with its own sprites (`$92BA` by `$F1` bit 2, the spent `$92BE`), `$99`
held at 0, and a wall bounce whose falling case takes its new velocity from
`$9ED6` (`$9EEF`-`$9F01`, picked by `$F9` bit 1). `weapon_4_fires_three_ways`
pokes `$0476` to 4 at TAS 370 and chains 200 frames of walking and firing;
`a_weapon_4_shot_bounces_off_a_wall` places a level and a falling shot in the
ground. Its charged attack (`$92C4`, dispatch `$95EF`): type `$80` (`$92D3`)
blows a type `$81` bubble every eighth frame while B is held (effect `$3F`;
`$9489`: in front of the player by the `$9F07`-`$9F43` offsets or `$9EA7`, rising
at `$FF:($F9|$E0)` with a random drift from `$F9 >> 4` and `$FA` bit 7, palette
`$9637` by `$F1` bit 3); a bubble (`$930B`) lives 48 frames, slowing 1 a frame,
drawn with its `$7C20` (which the shared `$99` keeps for the shots drawn after it).
Let go, the charge becomes the type `$82` tether (`$9521`, effect `$2A`): its
position is an offset from the player, its velocity (`$9ED6[3/4/5]`) pulled 4 a
frame back toward him (held to `$40`/`$C0` across, `$50`/`$B0` aiming up, where
`$7F60` bit 7 marks it vertical), drawn `$180` up, blowing a bubble every eighth
frame (`$959F`, with its x velocity) for `$C0` frames.
`weapon_4_charges_and_blows_bubbles` and `weapon_4_charges_aiming_up` chain 190+
loop frames each. Handlers 5 (`$9647`, the power-up's weapon: handler 0's flight
and bounce with the sprites `$9771`/`$9779`/`$977B`) and 6 (`$9D2E`, the power-up's
pieces) are ported with the power-up (9m).

**Handler 5's super attack** (`$9781`, dispatch `$9D24`). The type `$80` charge
(`$9790`) tests `LDA $B8; BVC`, where `LDA` leaves the overflow flag as the
code before it left it. The port had taken it as `$B8` bit 6, which held in 315
of 346 `$9790` runs of seeded play (six TAS starts, 12 seeds each, weapon 5); the
31 others were all V clear with `$B8` bit 6 set, so the game unleashed while B
was still held and the port waited. An oracle trace of every instruction
(interrupts skipped, since `RTI` restores P) named the last V-setting
instruction before each `$9790`: motion code `BIT`s (`$85A4` motion 0's `BIT $F5`
in 12, `$857F`, `$84D1`, `$88FC`, `$892E`), the draw `$C247`'s `ADC $94`
(`$C288`, 8), the second-class draw's `SBC $31` (`$DCDF`, 5) and the solid
probe's `BIT $DD` (`$C93D`, 2).

The port now carries the flag: `Work::cpu_overflow` holds what the last
V-setting instruction left, and `unleash` branches on it. Which instructions can
be last was found statically, by a reaching-definitions pass over the
disassembly from each entry to its `RTS` (calls summarised, the jump tables
`$96C3`, `$9A75`, `$9EB5`, `$9EC1`, `$9D24` and `$D16C` expanded): 78 in `$84AB`,
the draw's and the solid probe's among them (it always sets V: the projection
`$93A0` starts with `BIT $0481`), `$DCA2`/`$DCB5`/`$DCDF` and the draw in
`$DC87`, none in `$C229` or bank 5's `$8000` dispatch, and in handler 5's slot
loop above the charge `$9676` (gravity, on a shot it then frees), `$9E46`'s
`SBC $CE` or `BIT $30` and the draw for a shot, `$9AAF` `BIT $F7` for an
orbiter, nothing for types `$82`-`$84`. Each is set where the port runs it: a
`BIT` takes bit 6 of its operand, an `ADC`/`SBC` the signed overflow of the
add (for a 16-bit add, the signed 16-bit overflow), the solid probe `$DD` bit 6
(the vertical room), `$C247` per piece (`BIT $BC`, `BIT $9A` or the anchored
`SBC #$0F` for y, `ADC $94` for x, `BIT $F7` when the page fills;
`OamBuilder::draw` returns it). `BIT $F7` always clears V: after the reset's
`$E330` clear only the NMI's `EOR #$80` (`$E243`) writes `$F7` (a write watch over
TAS frames 1-41000 saw those two PCs and no value with bit 6 set). Motion 9's tally frame ends in `$C34B`, whose
`$C0C6` adds 0 to the return address's high byte `$C3`, so V is clear there too.

**Evidence.** The lockstep peeks `$D42F` (after `$84AB`), `$D43B` (after `$DC87`)
and bank 5's `$9790`, and compares the oracle's V there with the flag the port
held at the same points, every loop frame. The whole TAS chain (35 299 loop
frames, its tape unchanged) and all 72
weapon 5 runs of the sweep above (2000 frames each) match.
`weapon_5s_charge_waits_on_the_overflow_flag` pins four seeded runs whose
`$9790` V differs from `$B8` bit 6 (left by `$C288`, `$DCDF`, `$85A4` and
`$C93D`); with the old `$B8` bit 6 test the first diverges in RAM.

Let go with the meter `$0477` below 2, the charge is dropped, `$B8` keeps bit 7 and the handler returns (`$979E`). Otherwise
(`$97AC`): `$AF = $80`, effect `$47`, `$0481` bit 7 (the pads cleared, 9m), the
auto-scroll stopped (`$B6`, `$C4-$C7`), `$C2` kept in the charge's `$7CB0` and set
to `$FF`, every slot below `$80` (free ones too) claimed as type `$83`, and the
charge, now type `$82`, placed in front of the player. Every 17th frame `$9B4A`
turns two claimed slots into a type `$81` orbiter and its type `$84` centre (at the
charge's position; the first pair's orbiter has `$7F50` bit 7), five pairs in all.
An orbiter (`$9843`, the meter held at 2 or more) strikes every hittable actor
in slots 0-15 on the screen for `$C0` hit points (`$9F4F`: one it kills drops
`$4C` px and starts dying, `$7D70 = $80`), moves as an offset from its centre that
springs (`$3C` a frame, the x velocity turning and `$30` more when it crosses,
kept in the centre's `$7EB0`), drawn turned 45 degrees (`(x+y)/4`, `(y-x)/4`) and
mirrored on odd frames from `$9C19`/`$9C99`, effect `$48` every 32 frames; the
first pair's orbiter flashes the sprite palette (`$9D19`/`$9D1D`) and every
palette's first colour (`$30` each eighth frame, else `$0F`). After `$E0` frames
`$9AF7` ends it: `$0472` bit 7 and `$0481` cleared, the meter `$FE`, the stored
weapon `$0483` back (`$B8 = $80` unless it is 0), the sprite palette from `$9D21`,
`$C2` restored, every palette's first colour `$0F`, `$AF` and every slot cleared,
`$B6 = $FF`, and the handler returns. `weapons::step` takes the `Scroll` for
`$B6`/`$C4-$C7`. `weapon_5_unleashes_its_super_attack` (meter `$0E`, 420+ loop
frames through the end), `weapon_5_unleashes_aiming_up` and
`weapon_5_charge_needs_the_meter` chain the whole loop through it. The whole-game TAS never selects 1, 5 or 6 (`$0476` only takes
0, 2, 3 and 4, §10a).

**Handler 1** (`$8170`, the homing shot). Its flight and wall bounce are handler
0's (spent sprites `$8477`); in flight `$8288` picks one of 20 sprites and its
`$7C20` from the velocity's signs and `$7EB0` (`$8437`, `$844B`), then turns the
shot 2 a frame on each axis toward its target slot `$7F40` (set by the shot spawn
from `$9F05`), holding the low byte to `$20`/`$E0`; a gone or dying target is
replaced by `$83FC` (the first hittable actor slot, from 0 up on even weapon
slots and 15 down on odd ones, or `$FF`: no turn). Going left and up, the game's
`$8379` branches back into the subtraction, so `$7EB0` steps down until it drops
below `$E0` (`$FFE0`). The target's bytes are read through `slots::read`, so a
stale `$7F40` reads what the game would. The charged attack (`$847D`, dispatch
`$876E`; effect `$2E` every 32 frames per charged slot): type `$80` (`$8496`)
stands in front of the player (`$86A5`: `$8726`-`$8762` by motion, `$9EA7`
aiming up), its sprite `$8774[$7D00]` drawn `$380` lower, `$7D00` stepping on even
frames; at 7 it sets `$7F40 = $80` and `$85CF` puts three type `$82` parts in
free slots (then over plain shots; their slots packed into `$7F50`/`$7F60`,
angles `$887C`), which `$8525` draws circling it at `$87FC[$7F40]` (32 angles,
one a frame, `$7F30` `$60` drawn or `$E0` off screen). B let go while gathering
clears every charged slot (`$8712`); once circling, letting go or pressing A or a
direction throws it as type `$81` (`$864A`: velocity `$8881`-`$888D` by the pad,
none meaning down when aiming up), which flies with its parts until off the
screen. If `$85CF` finds too few slots it frees all 16 and returns with X = `$FF`,
so the caller's `STA $7D00,X` writes slot 15's `$7DF0`; the port does the same
write. `weapon_1_shots_home_in` (TAS 370, walking and firing, 190+ loop frames),
`weapon_1_charges_and_throws_a_circle`, `weapon_1_charges_aiming_up` (thrown up)
and `weapon_1_charge_let_go_early_is_cleared` chain the whole loop through it.
### 9h. The player (`actor/player.rs`, tasks `pmove` and `pdraw`)

`step_move` is bank 4 `$800B`, the gameplay loop's `$D408` call (the `$D3F9`
frame in 9a), ported whole: the walk intent and its `$047F`/`$047A` gates, the
±2 walking ramp, the wall probes at three heights (`$C99D`), the floor and
ceiling probes (`$CAFE`), the ride check (`$CD5F`), the motion-4 landing probe
(`$C93B`), gravity (`$8311`) and the `$046E` state machine (`$83BE`, `$8451`,
`$8268`). It reads `$F6` as a parameter, touches the working set `$90-$A6` and
writes the player's `$0460-$0463`, `$0466-$046F`, `$0473`, `$047C-$047F`, `$A8`
and `$B9` — all of them fields of `world::Player` (the probe bytes `$20-$28` stay
scratch).

`player::tests::matches_the_oracle_on_stage_1_1` diffs every `$800B` bracket of
the TAS (frames 351-2030) and the route (640-999) against the oracle: 1907
brackets, all motion states the routine dispatches on (`$046E` 0-5; the scenes
also run 6 and 10), and all its internal paths except two — the `$8112` ceiling
snap (needs y above `$2B0`) and the `$8000` script exit's `$F5/$F6` clear (taken
only when `$0481` bit 7 is set and bit 5 clear; every bracket here sets both, so
it is not modelled). The `$82B9` damage block is **not** dead: the class
handlers `$CA4E`/`$CA66` (wall) and `$CBF0`/`$CC08` (floor) set `$9F` bit 7 and
`$A7`, gated by `$0472` bit 7, on a damage-class tile. No stage 1-1 route or TAS
bracket holds one, so `player::tests::a_damage_tile_costs_energy` pokes a route
frame's metatile classes to the damage class and diffs the block: 4 brackets,
energy `$0E` -> `$0C`.

`step_draw` is bank 4 `$84AB`, the loop's `$D42C` call: the projection `$93A0`
(the screen x/y tables `$C5F2`/`$C6F2` of bank 14, the left/right clamps with
their crush probes and level-end check), the draw `$94A6` (the blink, the
`$989C` reach, the `$9924`/`$9A24` frame pointers drawn from bank 1, the `$9705`
overlay, the `$9535` hurt drain — meter, weapon, palette, `$B5` — and the
`$95E4` muzzle flash), the `$046E` state machine (`$84D1` 0, `$8795` 1, `$88FC`
2, `$8B78` 3, `$8CC6` 4, `$8EB1` 5, `$8F01` 6, `$908D` 7 (9m), `$91B5` 8, `$925C` 10,
`$91DB` 11; 9 is not ported) with the shared routines `$925D` hurt, `$92B4`,
`$92D1`, `$92E8`, `$9338`, `$9359` and the shot spawn `$9AF5` (`$9B08` slot
starts, the motion-6/else geometry and every per-weapon handler, `$9EB5[$0476]`
aiming up and `$9EC1[$0476]` otherwise, `$0476` 0-5: `$9BCE`/`$9D2D`,
`$9BF5`/`$9D4A`, `$9C06`/`$9D6F`, `$9C2E`/`$9DA0` (a second part, type 1, in the
next free slot), `$9C7E`/`$9E09` (three parts, type `$0A`) and `$9CD9`/`$9E72`
(the spread `$9D0D` indexed by `$F1 >> 2`); weapon 1 aims at `$9F05`'s random
target, the first live, hittable actor slot below 16 from `$FA & $0F`, stepping
by the sign of `$F9`).
`step_draw` masks `$F5`/`$F6` in place (through `$92E8`), so the caller gets
them back; `draw_player` is `$94A6` alone, the routine the room change calls
through `$D6FA` without the rest of `$84AB`.

`player::tests::draw_matches_the_oracle_over_the_tas` diffs every `$D42C`
bracket of the route (640-999) and the TAS (351-2030): 1907 brackets over
motions 0-6 and 10, with the shot handlers `$9C2E`, `$9D2D`, `$9D6F` and `$9DA0`
running (the TAS's weapons 0, 2 and 3; porting them found two muzzle-flash bugs,
`$047F` bit 7 and the carry sense of the `$966B`/`$9677` add, and the idle
pose's `$047E` index); and
`draw_matches_the_oracle_in_the_room_changes` diffs the `$D6FA` -> `$D704`
bracket of the two room changes (26 calls, motions 0 and 2). The draw was
exercised standing, walking, turning, jumping, falling at the level end and
firing, with the muzzle flash drawn. The death (`$9397` -> `$D274`, motion 7)
is in 9m; motion 9 is unported. The charged-attack scenes (9g) found two more draw bugs: the
hurt blink skipped the draw on the even frames instead of the odd, and the
standing attack pose (`$8710`) clamped `$046B` and kept pose `$25` even when not
charging, where the game ends the pose through `$24`.
Seeded play found one more: motion 2's walk tail (`$8AC8`) loads X with `$047E`
for `$96FD,X`, and when the charge goes off there (`JSR $9A56` at `$8B04`) the
pose is read with the X `$9A56` leaves: the charge's slot, weapon 2's last
part's slot, or `$FF` with no free slot (`$0466` port 0E, oracle 00 at frame
1327, TAS 1000 with weapon 1). The other five callers of `$9A56` do not read X
after it.

### 9j. The room change (`$D4D3`, bank 13 `$B7EC`, `$B40A`)

`$D4D3` runs at the end of every gameplay frame and dispatches on `$BA` through
`$D60D`: 1 `$D4E7` the room change (bank 13 `$B7EC` with `X = $BB`), 2 `$D4FE`
bank 2 `$940C` every frame, 3 `$D513` the next area (`$0400 += 2`, `$0402 = 0`,
bank 2 `$92AD`, bank 13 `$B200`; both in 9l), 4 `$D527` a life lost (9m), 5 `$D58F` the full meter's power-up, 6 `$D967` the ending (9r).
The TAS's stage 1-1 uses 1 (frames 613-662 and 1243-1322), 2 (1486-2029) and 3
(2030-2149).

`$B7EC` is a coroutine with a `$C232` NMI wait per frame: align the camera to a
256 px page (`$C4-$C7` toward it, `$8528`, the player sprite `$D6FA`, `$C145`),
on vertical rooms draw the current page into the other nametable (`$B351`,
16 frames: 15 metatile rows then the attributes, each with `$D2B0`), then
`$B927`: `$0402 += 2`, the loader `$B40A`, the arrangement `$EC` for the slide
and the palette (`$C34B`); draw the new room's first page (`$B351` from
`$0403`), slide 16 px a frame until the scroll wraps, spawn the room's first
row (`$D714`), draw the camera's page(s), restore `$EC`, place the player and
read the room's parameters (`$D992`).

`$B40A` (`room::load`) clears `$7B00-$7FFF`, builds the palette (`$C367`),
CHR banks, orientation `$DD`, the CHR animations, the page order `$0440`
(`rows_from_rom`), the start record (`$B653`: player, camera, `$C8`, `$C2`,
`$C3`, `$EC`, `$0403`, facing), zeroes the player's motion bytes and copies the
area's schedule (`$D84D`, which also zeroes `$0467-$046B`).
`room::tests`: `PageDraw::step` matches 60 `$B351` frames (TAS 614, 646, 1256,
1274) and `room::enter` the `$B927` frame at 1273. `room::RoomChange` runs the
whole coroutine a frame per call; the first room change is 50 frames (no align,
horizontal), the second 80 (12 align frames, vertical, so the turn page draw).

**Rooms two nametables across (`$DD` bit 7).** The new room's first row spawns
from `$0403`, except when `$DD & $C0 = $80` (two nametables tall), where `$D73C`
reads `$0440[($CC & $0F) << 4]` (an index the game builds with `$C6F2`, not
`$C5F2`; past 16 it would read `$0460+`, which the port refuses). Then `$BA06-$BA51`
draws the camera's page into `$2000` from row `$60` and, with bit 7, the page beside
it into `$2400` (`$28` under `$EC & 3`) from row `$60` (wide) or `$61` (tall); a tall
room takes its top page from `$0450` and its bottom from `$0440` (`room::page_draws`,
`RoomChange::second_page`). For a tall room the loader also runs `$B586`
(`room::raise_attributes`, its count the room record's byte 6): each listed page's
64 attribute bytes become `(byte >> 4) | (byte 8 on << 4)`, in place and in order,
a metatile row up, because the lower page is drawn from row 1. The scroll for these
rooms is 9q.

**The fade path, `$BB` bit 7 (`$BA8B`).** No align and no slide: `$04F1-$04F3` kept
in `$1A-$1C`, a fade out with the scroll stream (`$C30A` at caps `$20/$10/$00`, four
frames each, `$8528` between the bank switches, then `$C35A` with its own wait, 13
frames), then `$0402 += 2`, `$B40A`, `$1F = $EC`, `$D714`, the kept colours back,
the page draws above, `$FF &= $FC`, `$EC = $1F`, `$D992`, `$C145`, and the fade in
`$C2B8` (no scroll, 16 frames, the first in the same frame as `$D992`); `$D4F5` then
clears `$BA/$BB` with one more wait. The player keeps the loader's start record:
none of the slide's placing (`$BA54-$BA88`) runs. The TAS takes it twice: stage 2-1
room 2 -> 4 (`$BB = $81`, TAS 8074-8139) and stage 3-2 room 0 -> 2 (`$BB = $E1`, TAS
13964-14046, into a tall room, `$B586` at 13981); both are in the chain (9k).

**The stage start.** `room::start(prg, chr, level)` builds the game at the
level's first loop frame: the new-game state (`$D48D`: meter `$FE`, energy
`$0E`), `Level::from_rom`, the room loader, the bank-13 `$B200` room start (the
sprite palette `$0F/$11/$2C`, the player's screen position `$BF28`, the room
spawn `$D714`, `$E4 = $A0`, `$FF = $A8`) and `$D992`, whose last two bytes are a
per-room handler (`$DA0F`: motion `$0A` and `$0481 = $E0`, the drop-in;
`$D9FF`; `$DA0E` an `RTS`). `room::first_page` is `$B26D`'s direct nametable
draw. `the_stage_start_matches_the_first_loop_frame` compares it field by field
with the TAS at frame 351, the whole `Level` and the `$7570` shadow included (9l);
left out, as leftovers of the banner and title screens: the OAM builder's pointers
and the page, and the working registers. Batman's sprite
banks `$E5-$E7` are set to stage 1-1's (`shortcut:`, the player draw sets them).
`room::start` ends at the first loop frame, after the fade-in (`$B237`) whose
NMIs took the song request `$D973` made, so it leaves `$41` clear; the next
area's load (9l) runs both.

### 9k. The whole loop (`Game::frame`, `oracle::assert_game`)

`Game::frame(prg, chr, outside)` runs one NMI-to-NMI frame: from `$C23F` (`$FF &=
$FB`) through `$D45F-$D46C` (the speeds cleared, the `$BA` dispatch, the pause
check) and the loop body to the `$D45C` wait, or one frame of a `Script`: the
room change (`$BA = 1`), the next area's load (`$BA = 3`, 9l), or the rest of the
frame after the stage end's own NMI wait (`$BA = 2`'s last frame, `$9539`). A
script's last frame ends at its own wait, so the next call resumes the loop at
the pause check, as the game does. The stage-end script itself runs inside the
dispatch every frame (9l). The player move is `player::step_move`, with
`Game::probe` carrying the probe bytes `$20-$28` it reads back between frames.
The only routine it hands to `outside` is the second-class update `$CE7F`
(`Outside::SecondClass`) when a live second-class type is unported, after the
ported ones ran; the pause is a script (9p). `$BA = 4` is the next area's script
restarting the area (9m), 5 the power-up (9l), 6 the ending (`$D967`, 9r). The player draw `$84AB` runs in the port every loop frame,
and the room change draws
Batman with `player::draw_player` (`$94A6`). `Game::nmi(pad)` is the NMI wait
and the NMI's state change (`RNG_SPINS` = 415 steps of `$C21C`, the median the
TAS's loop frames spin, 44-493; `$F1`, `$F7`, the pads as `$C1DB` debounces
them) and returns a `Vblank`: the frame's direct PPU writes (`Vram::direct`, made
with rendering off: the banner, an area's first page), then the drained queue,
and the sound requests `$40-$43`, which the driver takes and clears.

`assert_game(scene, frames)` lifts once at a `$D45C` peek and then only chains:
before each call it injects the bytes the oracle's NMI changed (`$C232` ->
`$C23F`: frame counters, pads, RNG spins, the queue drain, the sound driver),
each `Outside` call injects its bracket's changed bytes, and after every loop
frame the port's whole memory (its state projected over the bytes it does not
model, which only injections change) must equal the oracle's at `$D45C`,
scratch aside. A script is compared once, at the next loop frame. Area 2 runs
the FME-7 raster IRQ (armed by the NMI from the room byte `$D0` bit 7; the
handler `$E2A9` dispatches on `$D3` through `$E2CA` and leaves by the shared
`RTI` at `$E2A8`, three a frame, mostly inside the NMI wait): the lockstep takes
every IRQ's changed bytes (`$08/$09`, `$D4-$D6`, `$F2-$F4`, the stack bytes it
pushed) and writes them over the port at the next injection or compare; a
bracket the IRQ fell inside already carries them, and stack bytes under the VRAM
queue's live entries are skipped (the queue pushed over them since). An NMI
(`$E1D0`) outside the wait is a lag frame: the loop ran past vblank, the NMI took
the not-ready path (`$F0` bit 7 clear: no OAM, no `INC $F1`, no pads) and ran the
sound driver, which took and cleared the loop's request in `$42`. The port runs
one loop per NMI, so the lockstep keeps that NMI's changed bytes like an IRQ's
(seeded play in the boss fight, pad seed 180518: seven of them, the first at
frame 6953 with two weapon 4 tethers blowing bubbles; `$0042` port 3F, oracle 00
without it). The lockstep starts inside the wait, and a relift drops the
interrupt bytes kept before it.

**Seeded random play.** `random_play_in_stage_1_1_matches_the_oracle`, `..._in_stage_1_2_...` and
`..._in_the_boss_fight_...` start from TAS 350/1000, 2150/3000/4500 and 6300 with
`$0476` poked, play 2000 frames of xorshift pads (each held 2-21 frames, mostly
right, B held three times in four, A one in four, a rare Select, a rare Start
paused for three holds) and chain the whole loop: 13 runs, 24 659 loop frames,
deaths and area restarts included. Most seeds are ones a sweep (six starts, 32
seeds each, weapon `seed % 6`, 192 runs) found diverging, which found the `$9A56`
X (9h), the bubble and the frozen charge (9g) and the lag-frame NMI above. The
sweep's only other divergence was weapon 5's overflow flag, now carried (9g).

`actor::tests::the_loop_matches_the_tas_from_stage_1_to_the_ending`: TAS 351-41177,
35299 loop frames equal, the whole game: stages 1-3 as below, then stage 4 (4-1 from
15711, 4-2's three room changes, the stage 4 boss), stage 5 (5-1, the one-screen 5-2),
stage 6 (6-1's two room changes, 6-2, the stage 6 boss), stage 7 (7-1's two room changes,
the last boss), and past the TAS's last input (40896) with no input: the boss falls to
its last shots at 40921 and type `$B8` sets `$BA = 6` at 41177; the ending (`$D967`,
the WRAM payload's `$6135` entry, §9r) then runs to TAS 48026, past its last screen.
12 room changes, 17 next-area loads, 1 ending (`GameRun::endings`), 8901 script NMI
waits compared (2060 cutscene and pause frames, 6841 ending frames), nothing relifted; injected: the raster IRQ (70438 runs) and 21 lag-frame NMIs. Stages 4-7 needed
(each found by the chain, root cause in the disassembly): the ride handler `$CE3E` and the
second-class `$0C` it serves (stage 4-2); the crumbling floor `$CC6D` and the second-class
`$06` it spawns (6-1); the second-class `$0E` (6-2) and `$10` (7-1); type `$96` clearing
slot 0's phase instead of its spawner's (9d); type `$84`'s `$0401` write (9d); type `$AC`'s
off-screen timer at `$7E90 + X` with `$A852`'s X (9d); area 17's class table (bank 0
`$0319`, missing from the class pointer table (bank 14 `$C000`), whose blob runs into code past its 102 ids:
the loader takes `A9 02 85 11 A5 F1` as classes, so `Level::loaded` shifts them with
wrap); and type `$B8`. Up to stage 4-1 (TAS 351-15720),
12753 loop frames equal: stage 1-1 with both room changes, the stage
end (544 frames, 9l), the next-area load (2030-2149), all of stage 1-2 (2150-
6027, 3878 loop frames), the boss room's load (6028-6281, `$BB3D`, 9n), Batman's
entry (motion 9, 6282-6381), the whole stage 1 boss fight, the next-area load at
6767 with the stage 2 cutscene (9o), all of stage 2 (2-1 from 7239: the slide into
the two-wide vertical room at 7571, the fade into room 4 at 8074; 2-2 from 8634),
the stage 2 end and the stage 3 cutscene (11488-11958), stage 3-1 (11959) and 3-2
(13215, with the fade into its tall room at 13964), the stage 3 boss (14409-15241)
and the stage 4 cutscene into stage 4-1's first loop frames (15711). 8 next-area
loads, 5 room changes, 1030 cutscene frames compared at their waits. Injected: only
the IRQ (27041 runs); the area loads' leftovers are the port's own (9l). The player
draw runs in the port on every frame. Stage 2-1 needed one more family, the
second-class path platform `$0A` (found with `Outside::SecondClass` injecting it
first). Stages 2 and 3 found five core bugs, each fixed at its root: the player
draw read its metasprite pointer at `$9924/$9A24 + anim * 2` where the game indexes
with `ASL` (`(anim << 1) & $FF`, so poses `$80+` drew from WRAM and panicked; first
at the stage 2-1 end), the floor class handler `$CC1A`, the tall-room block address
`$A264` (9c), the tall-room first row `$D73C` (9j) and the y auto-scroll's backward
speed (`$D34C`: `$FC & $B6`, not `$F5`, unexercised). Running it through stage 1-2 found two more draw bugs:
the edge push `$9425` zeroes the x velocity, and motion 4's wall probe `$8DFE`
looks 24 px ahead (`x ± $180`) and sets pose 4. `the_port_starts_stage_1_1_and_plays_it_like_the_tas` runs
the same chain from `room::start` instead of a lift (the banner leftovers above
overlaid) to frame 2150: 1550 loop frames, the first included; the attribute
pages and shadow it once overlaid are now the port's own (9l). Dropping `$D6D3`
from the loop fails it at the first frame. The loop needed three more types the
per-family scenes never isolated: second-class `$02` and `$08` (the `$CE7F`
dispatch silently skips an unported second-class type) and the trigger `$0E`.

**State tapes** (`actor/tape.rs`, spec `docs/specs/2026-10-15-state-tape-design.md`).
`run_game` is the loop's only copy; it asks an `Environment` for every injection
and compare. `Lockstep` is the oracle above, and `assert_game_recording` also
records each step into a `Tape`: the start and relift snapshots (RAM then WRAM),
every IRQ injection (lag-frame NMIs included, empty ones too, before the `$0181`
filter), each NMI's and `Outside` bracket's changed bytes, an FNV-1a 64 hash of
every compare (loop frames: RAM and WRAM, scratch zeroed; script frames: RAM,
scratch and `$FE` zeroed), and how the run ended (`End`, `Reset`, `Hung`), each
with its TAS frame. `TapePlayer` feeds those back through the same `Memory`
injection code and checks the hashes, so `replay` runs the whole chain from
`target/tapes/the_loop_matches_the_tas_from_stage_1_to_the_ending.tape`
(TAS 351-48026, the whole game and the ending; gzip 2.7 MB) in a plain debug `cargo test`. Since 1.0.1
the tape is recorded locally and never committed: its snapshots hold RAM and WRAM, and
WRAM holds code the game copied from CHR, so it is ROM data; the port needs the ROM to run
anyway. A changed
byte fails as `tape <name>: loop frame N (TAS frame F): state hash differs`, with
the lockstep command that names the byte. A tape that no longer matches the port's
injection points (a routine ported, an injection dropped) fails as `stale` at the
first event the replay did not ask for, or did ask for and the tape lacked. The
release lockstep chain writes the tape on every run (`ROTJ_REQUIRE_ROM=1 cargo test --release
-p rotj-core the_loop_matches_the_tas -- --ignored`, which `gates.sh` runs before the debug
tests); without it the replay tests skip, or fail under `ROTJ_REQUIRE_ROM` (the lockstep chain is `#[ignore]`: 8-10 min
in a debug build, 19 s in release). The replay's `injected` lists only `Outside` routines,
since the tape does not count interrupts.

### 9l. The stage end and the next area (`actor/stage.rs`)

**The stage end, `$BA = 2`** (bank 2 `$940C`, run by `$D4FE` every frame before
the loop body), dispatches on `$0408` through `$95AD` (`stage::StageEnd`,
`$0408-$040C`; type `$0E` clears the phase):

| phase | routine | what it does |
| --- | --- | --- |
| 0 | `$941E` | waits for Batman at x `$1F60` facing right (or past `$2000`); then `$40 = $FC`, every projectile freed, `$0472 = 0`, `$B8 &= $80` |
| 1 | `$9450` | walks him right (`$F5/$F6 = 1`) to x `$2740`; then sprite palette 2's colours 1-3 black, queued (`$C34B`), the song `$0E` (`$41`), the departing sprite at (`$FF`, `$70`) |
| 2 | `$9492` | `$80` frames: pads cleared, the sprite (bank 1 `$BBC6`, `$C247`) moving left every 4th frame and up every 16th |
| 3 | `$94AA` | `$60` frames: the same move, pads Up; palette 2 brightens from `$95B9` at timer `$10/$20/$30` |
| 4 | `$94FE` | the move every 8th/32nd frame; from timer `$90` (`$40 = $FC`) the palette fades (`$C30A` with the cap `$040C`: `$40`, less `$10` at once and every 16 frames); when the cap goes negative, `$0404 = 0`, `$BA = 3` and `$C35A` (the dark palette, queued) with its own NMI wait |

`$C30A` (`X = $FF`, both callers) caps every colour but each palette's first at
the cap's brightness (`(c & $0F) | cap` when `c >= cap`) and queues the result
from `$04A0`; `$C35A` is `$C367` with background and sprite set 0 (the dark
palette), queued, then the wait. The TAS's stage end is 1486-2029.

**The next area, `$BA = 3`** (`$D513`, `stage::NextArea`, a frame per call, each
ending at the game's own `$C232`): `$C145`, the fade out `$D61B` (`$C30A` at caps
`$20/$10/$00`, four frames each, with the scroll `$D29B`, then `$C35A`: 16 frames);
`$0402 = 0`, `$0400 += 2`; the banner `$92AD` (`$EC = 2`, `$FF &= $7C`, `$40 = $FF`,
`$C35A`; then rendering off, the first nametable cleared, the "STAGE" text from
`$93D7`, the digits `$93F0[$0400/2]` -> glyphs `$9402`, font banks `$CC-$CF`, scroll
x 4; the palette `$C367` with background `$1B`, the fade in `$C2B8`, a `$40`-frame
hold that Start, A or B cuts short, the fade out `$C2E1`); then the area start
bank 13 `$B200` with `$0404` bit 7 clear (`$BB3D` otherwise, unported): `$BD4D`
(`$40 = $FF`, every sprite parked `$C16F`, `$D0 = 0`, a wait), the level
(`$89FE`, `$8B99`, `$89C1`), the room loader, the sprite palette `$B26A`, the
first page `$B26D`, `$BF28`, `$D714`, the kept weapon, `$E4 = $A0`; `$BE6E` (scroll
0, `$FF = ($FF & $FC) | $80`, two waits); the song `$D973` (`$41 = $D980[$0400/2]`,
unless bit 7), `$D992`, the fade in `$B237` with the scroll each frame, `$AF = 0`,
and `$BA = 0`. `room::enter_area` is the part shared with `room::start`. The
`$93C5[$0400/2]` cutscene `$98BD` (when `$0406` is set) runs first (9o).

**What the loader leaves.** `$89FE` decompresses over `$7000-$7FFF` as it finds
them and transposes the whole 4 KiB into the map, so everything past the stream's
reach (`levelmap::map_reach`: `$A00`-`$1000` for most areas, `$100` for the one-screen
areas 4/10/12/17) is what WRAM held: after the boot or a cutscene the payload
(`levelmap::payload_image`), otherwise the last area's tile tables, classes,
schedule, shadows, attributes and slot arrays. The auto-tiler then writes the tile
tables for the area's ids only (entries past them keep the image's bytes), the
classes `$7B00` four ids a byte up to the id count rounded up to 4, and all 16
attribute pages (`$8E57`, `$12 = $0F`), each byte `c(TL) | c(TR) << 2 |
(c(BL) & $0F) << 4 | (c(BR) & 3) << 6` with `c(id) = $7B00[id]`, unmasked: an id past
the packed classes reads whatever `$7B00` holds. `$7510-$752F`, `$75B0-$75FF` and
`$7A00-$7AFF` keep the image's bytes until the next load (`Level::unread`).
`room::load_level` builds that image (`wram_7000`: the port's state there, the slot
area's unowned bytes zero as every room load's `$B5E2` leaves them) or the payload's,
and `Level::loaded` runs the loader over it; the chain injects none of it any more.
The one-screen area 8 (stage 2-2) is where it shows: its stream reaches `$7100`, so
its tile tables past id 57 are area 6's (TAS 8634, `$7109`).

### 9m. Death and a life lost (`player.rs` motion 7, `stage::NextArea::life_lost`)

What earlier notes called "the level end" (`$D274`, `$9397`) is the player's
death. `$9397` stops the music (`$40 = $FF`) and sets `$C2 = $FF`, then `$D274`
puts him in motion 7 (`$0481 = $80`, `$047A = $C0`, `$046F |= 3`, `$B8`,
`$0480`, `$0465` and every projectile cleared). Four places reach it: the
screen-edge crush probes of the projection `$93A0` (after which the pose is `$40`
and the frame's draw goes on), falling past the screen's bottom (`$9495`: also
`$0481 = $C0`, `$0465 = $80`, no draw), and the hurt and stun timers running out
with the energy `$0478` negative (`$9285`, `$9321`). With `$0481` bit 7 set and
bit 5 clear the move `$800B` also clears the pads `$F5/$F6` (`$8000`), so the
draw sees none.

Motion 7 (`$908D`) sets `$AF = $80` and counts `$0480`: up to `$2F` the flash
over the pose (`$9A40` frames by `$0480 & 6`, the effect `$40` every 16 frames,
the sprite palette's second set from `$918D` at 4, the facing flipped every 8
and at 4), at `$2F` the effect `$33`, `$0481 = $C0` and `$0465 |= 1`, from `$30`
to `$47` the burst (`$9191`/`$91A9`, bank 1); both draws are skipped when `$0465`
bit 7 is set. At `$C0` it sets `$BA = 4` and the energy to `$0E`, and clears
`$B8`, `$0472` and the weapon (with `$0404` bit 7 the weapon is kept, `$0483`
replacing weapon 5).

`$BA = 4` (`$D527`) is the next-area load's path with a life taken: the same
fade `$D61B`, then `$0482 -= 1` in place of `$0400 += 2`, `$0402 = 0` and the
banner and area start (9l), so the area restarts from its first room with the
state the loader rebuilds. Lives start at 2 (`$D48D`; `Effects::lives`).
`actor::tests::standing_in_stage_1_2_dies_in_the_pit_and_restarts_the_area`
stands still from TAS 2150: the auto-scroll pushes Batman into the first pit
twice (deaths at 2501 and 3226), and 1135 loop frames chained across both
restarts equal the oracle's. `running_out_of_energy_loses_a_life` pokes the
energy to `$FE` mid-hurt at TAS 470 (the scene's pokes apply once, at the lift)
and chains 263 loop frames through the restart. In the app,
`play::tests::standing_in_stage_1_2_loses_a_life_and_restarts`.

**The continue screen and GAME OVER** (`stage::Continue`). When `$0482` goes
negative, `$D52F` runs bank 2 `$8F05` before the banner: the screen cleared and
the "CONTINUE" text from `$91B7` written with rendering off, the ten digit glyphs
of `$9204` (8 bits a row, tile `$11` per set bit) expanded into `$6000-$627F`,
the font banks `$CC-$CF`, the palette from background `$1B`, then a countdown
from 9 (`$10` phase, `$11` digit, `$12` hold, `$13-$16` speed and offset):
each digit holds `$30` frames, splits away (`$8FF9`: speed +`$20` a frame up to
`$600`, then `$400`; effect `$41`, `$46` for the last) and the next closes in 4
rows a frame (`$9048`); every digit frame `$9135` builds the split digit in
`$7000-$703F` and queues it as eight 8-byte rows at `$218C`. Start held
(`$F6 & $10`, checked after each frame's wait) blacks out, clears `$B8`, the weapon
and the unread `$0485/$0486`, and returns to `$D53B`: the area becomes its stage's
first (`$D57D[$0400 / 2]`; stage 1-2 restarts at area 2), lives 2, `$0404 = 0`,
then the banner and area start as before. When the last digit has gone: a
blackout, 49 frames, then `$9081`: GAME OVER from `$925A`, the song `$08`, the
text scrolled up (`$FC` from `$B0` to `$F0`), held `$C0` even frames or until
A, B, Select or Start is pressed, then `JMP ($FFFC)`, a reset (`Game::reset`;
the app returns to the title). `start_on_the_continue_screen_restarts_the_stage`
stands from TAS 2150 through three deaths (the third at 3951 leaves `$0482 =
$FF`), presses Start at 4401 and chains 1909 loop frames through the restart;
`the_countdown_running_out_shows_game_over_and_resets` lets the countdown run
out and requires the oracle's reset (`$E2E6`) at the port's (1627 loop frames
before it). `play::tests::the_last_life_ends_in_game_over_and_a_reset` runs it
in the app.

**The power-up, `$BA = 5`** (`$D58F`, `stage::PowerUp`), armed when a meter
capsule (`$7D50` state 4, `$AB69`) fills `$0477` to `$0E` (the weapon kept in
`$0483`, `$AF = $80`): the effect `$31`, every projectile freed, `$C2` kept in
`$0409` and set to `$FF`, then 38 frames (`$0408` counting) of each palette's
first colour `$30` every fourth frame and `$0F` otherwise, with the scroll and the
palette upload. At the next frame's start (`PowerUp::finish`, before that
frame's loop body): `$0481 = $80`, `$BA = 0`, the motion kept in `$040F` and set
to 8, `$F7 = $80`, and `$D64A` places four type-6 pieces at Batman's corners
(`$D6AF-$D6D2`) with weapon 6 selected. Handler 6 moves them on even frames,
stepping their velocity toward him (`$9E00-$9E0F`), and after 8 steps frees them
and powers him up: `$0472 = $80`, `$B8 = $80`, weapon 5, `$0481`, `$047A`,
`$0464` and `$AF` cleared, his motion and `$C2` restored, and the sprite
palette's second set from `$9E10`. Motion 8 (`$91B5`) hides him on odd frames
(`$0481 = $C0`; the port had the parity reversed, never exercised before).
`actor::tests::a_full_meter_powers_batman_up` pokes `$BA = 5` at TAS 470 and
chains 361 loop frames through the power-up and weapon-5 shots (B every 16
frames from 120 on).

### 9n. The boss room's start (`$BB3D`, `stage::BossIntro`) and motion 9

`$0404` bit 7 (set with `$BA = 3` by bank 11 `$89CC`, in the last room of stage 1-2)
makes the area start bank 13 `$BB3D` instead of `$B200`. Frame by frame: the
music stopped, every slot's type cleared, `$EC = 0` and `$BD4D` (the sprites
parked, `$D0` and `$FF` bit 7 cleared, a wait); the level (`room::load_level`),
both nametables cleared and the boss's name card drawn with rendering off
(`$BEAE`: five rows of five tiles from `$BEDD` at `$2160-$21E0`), scroll 0; a
wait; then the song `$0A`, the palette from background `$0F` and sprite set
`$040E`, the sprite banks `$BCB0` in `$E0-$E2` and `$E4-$E6`. The card then
slides in from the right (`$BB92`: the speed `$10:$11` +`$20` a frame up to
`$C00`, then `$600`; the offset `$14:$15` to `$5000`, the scroll x its rounded
negation, `$FF` bit 0 set), fading in over its first ten frames (`$C30A` at
caps `$00-$40`); at the end two more tiles (`$BCA0` at `$21A5`/`$21C5`), a
`$60`-frame hold (`$BC07`) and a fade to white (`$BC15`: `$BEF6` raises every
colour below the cap to it, five times, then all `$30`). Every card frame draws
the two metasprites `$BCB3` and `$BCA6[$040F]` (bank 13) at (scroll x - `$14`,
`$87`). Four frames after the white, `$BD6C` loads the room (`$B40A`, `$BF28`,
`$D714`) and draws its page (`$B351`, 16 frames), then three 64-byte rows from
`$D87E` at `$2300-$23BF` and the attributes `$D93E` at `$23F0`, a frame each;
the last frame restores `$EC`, runs `$D992`, parks the sprites, blacks each
palette's first colour, sets the sprite set `$BE4B`, the boss's hit points
`$A9:$AA` from `$BE4E[$040F]` (stage 1: `$0FA0`), `$B2:$B3 = $1F40`, clears
`$0488-$048B` and `$048F`, keeps `$E4` in `$0487`, and puts Batman in motion 9
with `$0481 = $C0`; the area's song, `$BA = 0`.

Motion 9 (bank 14 `$D03C`, through `$91D8`; `Player::entry_*`, `tally`) sets
`$AF = $80` and steps `$0488`: 0, four frames of the entry pose (`$D17A`: bank 1
`$D1A4[n]` at (`$3E`, `$B0`)) and his line (`$D1B0`: `$D1D8[n]` at (`$3E`,
`$78`)), then the effect `$3E`; 1, the tally `$048A:$048B` counted up by 100 a
frame to `$B2:$B3` with its last two digits cycling (`$048F`), queued with the
boss's hit points as five digits and a 0 (`$C199`, `$D24A`, tiles `$D26A`) at
`$2348`/`$2368` and `$2352`/`$2372`, the effect `$3E` every 16 frames and the
palette's first colours flashing `$30` every 8; 2, the final tally once, four
frames and the effect `$23`; 3, twelve frames of poses from `$D174`/`$D177`, then
the song `$03`, `$E4 = $A2`, motion 0, `$0481` and `$AF` cleared. The chain test
(9k) runs both on the TAS. The name card and the room page are written straight
to the PPU or queued; the oracle diff compares the queue's bytes in RAM, not the
nametables.

### 9o. The banner's cutscene (`$98BD`, `actor/cutscene.rs`)

When the next area's banner (`$92AD`) finds a picture for the area
(`$93C5[$0400/2]` bit 7 clear) and `$0406` set (by the boss-beaten script `$50`
with `$BA = 3`),
it clears `$0406` and runs `$98BD` first. Bank 2 `$99A9` sets the CHR shadows
`$E0-$E7` to `$F8-$FF`, waits a frame with rendering off (`$9985`) and reads the
pattern tables through `$2007` into WRAM: `$6000-$7E5F` from banks `$F8-$FF`, then,
with the mapper's banks set to `$F0-$F7` directly (`$9925`), `$7E60-$7FFF` from bank
`$F7` at `$260` (the copy runs about seven frames with NMIs off); one frame with
NMIs on (`$996D`), `$C35A`'s dark palette, then
`JMP $6000`. The whole WRAM (map, tables, slot arrays) is overwritten; the area
load after the banner rebuilds what the loop reads. The payload is listed in
`disasm/cutscene.s` (chr.s includes it; `assemble.sh` stays byte-identical).

The `$6000` entry, one frame per `Cutscene::step` (`$C232` waits):

| frames | code | what it does |
| --- | --- | --- |
| 36 | `$7EC9` (`$2F = 0`) | `$D0 = 0`; then `$EC`, the scroll and `$FE` cleared, `$FF = $A8`, the sprites parked (`$C16F`), a black palette queued from the buffer `$7FC0`; then both nametables zeroed 64 bytes a frame (`$2000-$27FF`, 32 frames) and their attributes (`$23C0`, `$27C0`). With `$2F` set, Start, A or B (`$7E60`) would cut it short; neither call sets it |
| 1 | `$600B` | the song `$0F`; the picture `$76D1[$2B]`; its CHR banks `$76D7[8n..]` in `$E0-$E7` (and the mapper); `$D5 = $D6 = 0`, `$DA = 3`; the palette `$76EF` queued |
| 9 | `$7E69` | the picture's nametable runs, one block a frame from the list at `$71DD` (`$775B[2n]` the start, `$775C[2n]` the count): per entry `count` runs of `len` bytes from `source` to the PPU address, the next a row down (flags 0) or a byte across (`INC $27`, no carry) |
| 65 | `$607B` | the raster split: `$D3 = $0A`, `$D1:$D2 = $7755[2n]` (the first IRQ's cycle count), `$D0 = $80`, rendering on; waits until `$D5`, `$D6` and `$DA` are all 0 |
| 32 + 1 + 32 | `$60A4` | `$D0 = 0`, a 32-frame hold, every colour `$10`, another 32 |
| 1 + 128 | `$60C4` | the picture's metasprite (`$7653[4n]`: pointer, x, y; `$C247`, `$C145`) and its palette (`$770F` and 16 bytes from `$771F[2n]`'s low byte with `$7720`'s high byte, a game bug harmless for picture 0), held `$80` frames |
| 36 | `$611D` | the music stopped (`$40 = $FF`) and `$7EC9` again; `RTS` to `$92C6`, the banner in the same frame |

The split is bank 2 `$9731` (`$E2CA[$0A]`), three IRQs a frame armed by the NMI
from `$D1:$D2` and then `$1000` cycles apart (`$98AD`): the first sets the x scroll
to `$D5` (nametable `$DA` bit 0), the second to `$D6` (`$DA` bit 1), the third to
0 and steps the slide (`$977D`): `$D5` down 8 a frame from `$F8` (its first step
forced by `$DA` bit 0), then `$D6` up 8 a frame until it wraps (forced by bit 1),
so the picture's top band slides in, then the bottom. The lockstep injects the
IRQ's bytes as elsewhere (9k); for the app, `raster::run_irqs` runs `$977D`'s
step once per frame the NMI armed, and `raster::lines` places the three bands at the
cycles measured from the oracle (since 1.0.2; before, a fixed 60-cycle latency).

`the_loop_matches_the_tas_from_stage_1_to_the_ending` (9k) runs this cutscene
(TAS 6784-7136) and those of stages 3-7, and compares RAM at every one of their
NMI waits as well (`GameRun::script_frames`; WRAM is the payload's and the PPU mask
shadow `$FE`, which the port does not model, is taken from the oracle), then each
banner and area load, which leave memory equal to the oracle's at the area's first
loop frame (stage 2-1's is 7239), the loader running over the payload's bytes (9l). The `$6135` entry is
the ending (9r). The copy, the screen clear `$7EC9`, the block draw `$7E69` and the
payload's sprite draw are shared with it (`cutscene::copy_frame`, `ScreenClear`,
`draw_block`, `draw_sprite`).

### 9p. The pause (bank 8 `$9C37`, `actor/pause.rs`)

Start newly pressed at the loop's pause check (`$D46C`, after the `$BA`
dispatch) runs `$9C37` unless `$AF` bit 7 is set (scripted motion). Its first
frame: the music paused (`$40 = $FE`) and the pause effect `$3A` (`$41`), the
auto-scroll gate `$B6` and the speeds cleared, the sprites parked, Batman's
sprite banks `$E4-$E7` kept (`$1C-$1F`) and the font's `$CC-$CF` set, the colour
`$1B = $27`, the dimmed palette (`$9CB3`: the background one step darker, the
sprite colours `$9DC2` with `$1B` at `$04B6`, each set's first black, queued from
`$04A0`) and `$9CAD` (the scroll stream `$D29B` at zero speed, then the wait).
Every paused frame then draws the pause screen (`$9CE5`: five metasprites from
bank 1, the area's name built as a metasprite in WRAM `$7FD0-$7FE1` from
`$9DF6[$0400]`'s letters, the lives left (at most 9) and the stage's two digits
`$9DD2`/`$9DE4`), steps `$1B` through `$20-$2C`, queues the dimmed palette,
parks the rest and waits; from the second, Start pressed instead restores the
sprite banks, queues the real palette (`$C34B`), parks the sprites, resumes the
music (`$40 = $FD`, `$41 = $3A`) and waits. The next frame sets `$B6 = $FF` and
runs the loop body (`JMP $D3F9`), past the speeds, the dispatch and the pause
check. `Script::Pause` runs it in the port (`Effects::pause_name` keeps the
WRAM name, which the room loader clears with the slot arrays).
`start_pauses_the_game_and_start_again_resumes_it` walks right from TAS 470 and
presses Start at 491, 621 and 771: 168 loop frames equal, and the RAM at every one
of the two pauses' 229 NMI waits as well (`GameRun::script_frames`, as for the
cutscene); `play::tests::start_pauses_and_resumes` runs it in the app.

### 9q. Scrolling rooms two nametables across (bank 2 `$8528`, `actor/scroll.rs`)

`$8528` moves the camera along the room (`$852F` vertical, `$864C` horizontal) as in
9a; with `$DD` bit 7 it then also moves it across the second nametable:

- `$85F7` (vertical rooms, `$DD = $C0`, `scroll::across_x`): the x step clamped to
  camera x 0..`$1000` (left: stop at 0, else `$FF` bit 0 cleared; right: at `$1000`
  set `$FF` bit 0), then `$FD` from the camera as `$8920` does, setting `$FF` bit 0 when
  the rounding wraps. At 0 or `$1000` going outward it clears `$C4:$C5` and skips `$8920`.
- `$86D9` (horizontal rooms, `$DD = $80`, `scroll::across_y`): the ring row `$C8:$C9`
  moved by `$C6:$C7`, skipping row `$0F` both ways and clamped to rows 0..`$10` (the
  camera to `$0100`/`$1000`, `$FC` and the speed cleared); `$FF` bit 1 is set while the
  ring is in its first 16 rows; the camera moves; `$FC` is rebuilt from the ring.

The streams: rows of a `$C0` room (`$83C4`) go to both nametables, `$0440`'s page at
`$CA:$CB` and `$0450`'s at `$CA | 4`, then each table's eight attribute bytes from its
own page merged into its own shadow (`$7530`, `$7570`); columns of a `$80` room
(`$824F`) are rows 0-14 of the `$0450` page into `$2000` and rows 1-15 of the `$0440`
page into `$2800`, then four attribute pairs each into `$23C0`/`$2BC0`, with neither
the fixed rows `$70` nor the fixed quads `$60` of the single-table `$8000`. Stage 2-1's
room 2 (`$C0`, TAS 7637-8073; its rows at 7743-7948) and stage 3-2's room 2 (`$80`,
TAS 14046-14391) are in the chain (9k).

### 9r. The ending (`$BA = 6`, `$D967`, `actor/ending.rs`)

`$BA = 6` (set by the stage 7 boss's last figure `$B8`, bank 12 `$9457`, at TAS 41177)
dispatches `$D967`: `$999C` stops the music, fades out (`$C2E1`), copies the payload
(`$99A9`, as in 9o) and jumps to `$6135`, which never returns (`docs/SUBLEVELS.md` §7e
has the scenes). `Script::Ending` runs it one frame per `Game::frame`; its `done` is
never true. `Ending` steps through `SCRIPT`, the payload's code as a list of actions
in order (each one or more frames ending at an NMI wait, or none and the next runs in
the same frame): the fades `$C2B8`/`$C2E1` (`stage::Fade`), the clear `$7EC9` with its
fill tile, CHR banks and palettes from payload tables, the buffer `$7FC0-$7FFF` the
payload builds sprites and palettes in (`$7FA6`/`$7FB1`/`$7F89`), the draw lists
(`$7E69`), the raster splits (`$D3 = $0C`, `$0D`), and the scenes' own loops: the
falling and rising figures (`$61D0`, `$6254`), the bursts (`$665C`), the skyline loop
(`$6361`), the walker (`$6526`) and the credits (`$64D5`). `Ending::over` is true once
the last screen holds (`$6656`). The song `$0D` and the effects `$22`/`$23` go through
`$41`/`$42` like any request.

The whole-game chain (§9k) runs on from TAS 41177's loop wait (`$BA = 6` just set)
and compares RAM at every one of the next 6841 NMI waits (to TAS 48026, past the last
screen's first frame at 47938; first checked by a separate lift at 41176, since folded
into the chain so its state tape covers the ending in CI), injecting only the raster IRQ
(21196 runs: both splits change `$D4`, and `$0D` steps `$D5-$DA` and `$FD` every
frame). A credit line held `$9F` frames instead of `$A0` fails it at script frame 1557
(TAS 42742, `$015C`: the next line queued a frame early). The type `$B8` is ported on
the stage 4-7 branch; an independent port of it here chained 296 loop frames from TAS
40880 through the boss's fall into the same ending, all equal, before it was dropped
for that one. For the app, `raster::run_irqs` steps the `$0D` split's scrolls
each frame and `raster::lines` draws both splits' bands (the `$0C` split hides the
sprites below its second IRQ). `play::tests::beating_the_final_boss_plays_the_ending`
sets `$BA = 6` in stage 7-1 and plays 7000 frames: the ending reaches its last screen,
requests `$0D`, `$22` and `$23`, and of 70 sampled pictures at least 60 must be drawn
and 45 distinct (67 and 58 measured; with no draw-list blocks it fails at 58 drawn).

### 9s. Power-on, the intro and the title (`actor/boot.rs`, `attract.rs`, `sound_test.rs`)

`Game::power_on` is the game at reset with `Script::Boot` running; the script is done
at the new game's first loop frame (`$D3F9`, where the area start `$D6E2` returns to,
not the end of a loop frame, so that frame skips the pause check). One `Boot::step` per NMI wait, as every script:

| step | code | what it does |
| --- | --- | --- |
| reset | `$E2E6-$D3B8` | zero page and `$0200-$07FF` cleared (`$E330`, `$C117` A=0 X=2 Y=7), the PRG windows `$E8-$EB = $1B-$1E`, `$0402 = $FF` (X after `$C117`, `DEX`), `$0401 = 2`, `$F9:$FA = $49:$2F`, `$0180:$0181 = $FF:$7F`, the sound driver's init (bank 10 `$8000`, the app's), `$FF = $A8`; the first wait (fceux frame 4) |
| 1 | `$D3BB` | each set's first colour `$0F` (`$D481`), the dark palette (`$C35A`) |
| 3 | `$98C3` | the music stopped, `$E0-$E7 = $F0-$F7`, `$9985` (NMIs off), the copy of CHR `$F0-$F7` to WRAM `$6000-$7FFF` (frames 7-14 have no NMI), `$996D`, `$C35A`: `cutscene::copy_frame` with the first bank `$F0` |
| 36 | `$6000`, `$7EC9` | the screen cleared, the palette shadow from `$691C` |
| 2 | `$62F7` | CHR `$C8-$CB` in `$E0-$E3`, 31 attribute bytes `$68F5` to `$23D9` |
| 1 | `$6040` | rendering on (`$FE = $1E`), the palette queued |
| 6 x | `$606A-$60CE` | a block of the draw list `$6601`, then the raster split (`$D3 = 9`, first IRQ `$2680` cycles in, `$FC = $C0`) armed every other frame until `$DA & $1F == $1F`, then two more blocks |
| 1 + 1 + 16 + 2 | `$60D2-$610C` | split off, a block, a 16-frame hold, two blocks |
| 79 | `$610F-$617E` | the flicker: `$FC = $2D` with one-screen table A (`$EC = 2`), then `$FC = 0` with table B (`$EC = 3`), alternate frames; `$2D` from `$C0` up to `$F0`; a 16-byte sprite palette from `$693C` every eight of the first 32 frames |
| 5 | `$6180-$61AA` | the title's last five blocks |
| | `$61AC-$6232` | the title: song `$01`; left/right held or Select pressed move the cursor (`$2F = $38` NEW GAME, `$98` PASSWORD, metasprite `$6988` at y `$94`); A, B or Start choose; `$0700` frames left alone runs the attract mode |
| 1 + 120 | `$6233-$626F` | the choice: the music stopped, effect `$3B`, the line flashed ten times (list `$66F3` from offset 8 or `$10`, then from 0, six frames each), the fade out `$C2E1`, rendering off |
| 36 | `$6284`, `$7EC9` | NEW GAME: the screen cleared, `$20 = 0`, back to `$991A`/`$D3C6` |
| | `$D3C6-$D3F6` | `$F6 == $90` (A and Start held): the sound test; else `$0400 = $D94E[$20]`, `$C35A`, `$FE = $18`, `$D48D` (the new game: room 0, no weapon, meter `$FE`, energy `$0E`, two lives, `$E8 = $C0`, `$E4 = $9D`), the banner `$92AD` and the area `$B200`: `stage::NextArea::new_game` (the banner without `$0400 += 2`, the load over `Leftover::Boot`) |

A, B or Start newly pressed after any of the intro's waits but its first, the clear's
first, `$62F7`'s second and `$6040`'s (`$7E60` with `$2F = $FF`) skips to `$62A3`: the
music stopped, the clear, `$62F7`, the palette from `$691C`, the list's twelve blocks
from the offsets at `$65F5`, rendering on, the title.

**The split.** `$D3 = 9` is bank 2 `$95C9`: ten IRQs a frame, `$D4` choosing the handler
from `$95DA`. The first writes `$2006 = $2240` (row 18, fine y 2) and x `$D5`, which
also clears the table bit its `$2000` write set; the next eight set the table from
`$DA` bits 1-4, 3-1, 0 and x from `$D6`-`$D9`, `$D8`-`$D6`, `$D5` (`$0480`, then `$0300`
cycles apart, the ninth `$0600`); the tenth (`$96A1`) shows table B from x 0 and steps
each band at `$96AF-$972C`: `$D5`-`$D9` up by `$20`, `$28`, `$40`, `$50`, `$58` until
it wraps, which sets its `$DA` bit and parks it at 0. The lockstep injects the IRQ's
bytes as elsewhere; for the app `raster::run_irqs` runs the step once per frame
the NMI armed, and `the_intro_runs_its_own_raster_split` runs the port alone (its own
IRQ model, no oracle bytes) from power-on and holds the oracle's `$D0-$DC` at every
NMI wait to the title. `raster::lines` draws the ten bands at the oracle's measured
cycles (since 1.0.2, the 12 areas' splits too: `actor/raster.rs`).

**The password** (`$6330-$65F4`, PASSWORD chosen): the clear, CHR from `$6E0A`, eleven
blocks of `$6992`, the entry's metasprite in WRAM `$7FC0` (from `$6E12`), the palette
`$6DEA`, the fade in. Up and down step the changed letter (`$20-$23`, A-Z) and left and
right the column (`$24`), each sounding `$39`; a held direction repeats after `$1F`
frames, then 4 frames sooner each time down to `$0F` (`$27`, `$28`, `$29`). A, B or Start
submit: the letters are checked against the fourteen passwords at `$6E6C` from the
last; a match is its place (the fourteenth starts the thirteenth level too), sounds
`$3B`, blinks 128 frames, fades and starts that level through `$D3C6`; a miss sounds
`$45`, blinks the message `$6E38` for 128 frames and returns to the entry. Select fades
out, clears and resets the game (`JMP ($FFFC)`). Read from the ROM, the passwords are:

| level | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| stage | 1-1 | 1-2 | 2-1 | 2-2 | 3-1 | 3-2 | 4-1 | 4-2 | 5-1 | 5-2 | 6-1 | 6-2 | 7-1 |
| password | LPRZ | MDRR | NMLL | NWKL | LGZQ | GPTW | GNXF | KHCN | QGVN | WBZT | FFHG | CKQG | GPZT, NXRD |

**The attract mode** (bank 2 `$99F7`, `attract.rs`), when the title times out: three
screens drawn from CHR copied to WRAM with NMIs off (`$9B51`: banks `$EC-$EE` to
`$7000`, `$F4-$F5` to `$6000`; `$9BA8` writes `$7400-$7BFF` straight to both nametables).
The first (song `$02`) scrolls half a pixel a frame into the second nametable, steps a
colour every eight frames (`$9C70`), then lights three colours in turn (`$9C76`) while
the next page is drawn over the first, 64 bytes a frame (`$9BDA`), 256 frames; the
second draws its page into the second nametable (`$9D3F`) and scrolls a quarter pixel a
frame to `$58`; the third draws its page into the first (`$9E29`), the figure `$9E80` and
sixteen stars rising (`$9EDF`, places from `$9EBF` in WRAM `$6F00`), `$B0` x 4 frames.
A, B or Start held in any screen leaves it with `$20 = $80`; then (`$9F69`, `$9B43`)
the boot copies its payload again (`$98CB`) and enters `$6000`, which goes straight to
`$62A3` when `$20` is set and runs the intro again when not.

**The sound test** (bank 12 `$9A36`, `sound_test.rs`), A and Start held as the title
hands over (`$D3C6`, after the choice's flash, fade and clear): the screen written with
rendering off (`$9B3B`, the text list `$9BEB`), CHR from `$9CAC`; up and down choose
the row, left and right the song (13 from `$9C85`, the music stopped) or the effect
(26 from `$9C92`), A plays it (a song after a frame with the music stopped), B stops
the music, A on the last row waits a frame and resets the game (`JMP $E2E6`).

**Lockstep.** `oracle::assert_boot` starts the port's `Game::power_on` at the oracle's
power-on and compares RAM at every NMI wait of the boot, the first right after the
reset, and the VRAM and palette at every NMI, then the loop as `assert_game`. The bytes
the port does not model are the oracle's at its first NMI wait (the stack page, the
sound driver's init RAM), and the PPU mask `$FE` follows the oracle's. `boot::tests`: Start on the title
(612 boot frames, then 60 loop frames); `run1`'s input over all 3000 frames (the intro
skipped at 100, the title's Start at 160, twelve pauses); Start at fifteen points of
the intro, each skip path; three passwords into their stages (the oracle's `$0400`
checked: `$00`, `$0C`, `$20`) and a wrong one with a held direction's repeat, then
Select's reset; the attract mode's three screens left alone (4500 frames, into the
second intro) and left with Start in each; the sound test's every row, both directions
and its reset. `actor::tests::a_recording_replays` and the app's recordings replay from
power-on (`oracle::assert_playtest`: one pad per NMI wait, the RNG pinned, `ROTJ_LEVEL`
as the `$D3D8` poke).

**The app** (`play.rs`) powers the game on at start and steps it on the game clock; the
state is `Title` while the boot runs and `Playing` from its first loop frame, back to
`Title` when GAME OVER, the password's Select or the sound test resets the game.
`ROTJ_LEVEL=n` forces `$20` at `$D3D8`, as the oracle's poke does, so the title and
the stage start still play. Every song, the title's `$01` included, comes from the
game's requests. `play::tests::the_title_draws_the_emulators_frame`: the title the port
boots into is the emulator's frame 300, 0 of 57,344 pixels different.

### 9t. Off-TAS fixes from random play (task `triage`, 2026-10-15)

The random-play sweep (`actor::tests::random_play_sweep`, spec
`docs/specs/2026-10-15-off-tas-verification-design.md` §1) re-run on `587b94c`, after
`$46`/`$90`/`$92` landed: 480 runs (weapons 0-4, seeds 1-3, one death run a start),
402 pass, 5 reach GAME OVER's reset, 3 hang as the game does, 70 diverge (45 in the
port, 25 in the oracle). The `$0200`-`$0208` boss clusters of the first sweep were
gone. Each cluster below was traced in the oracle (`trace.watch` writes with their PC,
peeks with the stack's return addresses) and is pinned in the per-stage
`random_play_in_stage_N_matches_the_oracle` lists, which fail on `587b94c`.

- **`$8BEA` with no free slot (type `$72`, 15 runs from 28821).** `$AF38` scans
  `$7CD0` 7..0 with `LDA; BEQ; DEY; BPL`, so a full scan returns from the `DEY` with Y =
  `$FF` and N set; `$8BF1`'s `BPL $8BF9` falls through to the `RTS`: nothing is written
  through Y, the remaining parts are not spawned and A holds slot 16's type for the
  caller's `CMP #$80`. The port asserted the stale-Y write would happen when slot 16's
  type had bit 7 clear; the assert is gone.
- **`$9D50`'s weapon clear (type `$54`, 15 runs from 8634, and 28821's past `$8BEA`).**
  Grabbed (motion `$0B`) at zero energy, `$91E0`'s `$92E8` dies (`JMP $9397` -> `$D274`,
  which clears the shots) and `$923E` still fires (`$9AF5`, `$9B18` writes `$7BD0`);
  the weapon loop projects the shot (`$9EA1` writes `$7B80`), then `$9D49` with motion 7
  clears `$7BD0,x` for x = 15..0 and nothing else. The port reset every weapon field.
- **The spikes' `$A8` (15 runs from 39200).** `$CAA9`, reached from the wall probe at
  `$81B7`, sets `$9F` bit 7 and writes it to `$A8` (and clears `$0472` bit 7). The
  port's movement works on a copy of the player and stored it back at the end, over
  what the probe wrote through `cx`. The player's wall and floor probes now run on
  the working copy (`player::wall`, `player::floor`).
- **Type `$1E`'s turn (4 runs from 7239).** Phase 0 turns to the player only from `$200`
  away (`$9393 JSR $ABDF; LDA $21; CMP #$02; BCC`); phase 1's first frame (`$93D5`)
  turns at any distance. The port gated both.
- **A lag NMI's `$42` (1 run, 38120 seed 2 weapon 4).** The loop frame spans two NMIs:
  `$89AA` requests `$24`, the lag NMI's driver clears `$42` (`$8239`), then the weapon
  scan requests `$38` (`$AAF8`). The harness injected the lag NMI's bytes after the
  port's whole frame, over the port's own `$38`. `Lockstep::inject_irqs_at` now leaves
  out an interrupt's byte when the oracle's loop changed it again by the injection
  point (each address judged by the last interrupt value written to it; the stack page
  stays whole, as the port has no stack). The chain's tape is unchanged.
- **Blocked jumps (forced scene, `player::tests::a_blocked_jump_matches_the_oracle`).**
  `$8508`, `$89A8` and `$901C` probe x, x + `$A0` and x + `$A0` - `$140` (`SBC #$40`,
  then `SBC #$01` on the high byte): the third probe is `$A0` behind, not `$60` ahead.
  And motion 2's blocked side-jump (`$8A07`) is `JMP $895F`, which takes `$8A32` when a
  direction is held, not always `$8968`. The scene puts a solid metatile 56 px up at
  TAS 2425: at the player's column with Right held, and `$A0` behind him; each fails
  without its fix (`$0466` port `09` oracle `02`; `$0469` port `FF` oracle `00`).
- **The death runs' read gap (25 runs).** The HUD gauge (`$DB9D`) reads its metasprite
  pointer at `$DC0F + energy`, a word table for even energies. The death runs poked
  `$0478 = 1`, which points it at `$27DC`, the PPU registers. The game's damage is even
  everywhere except contact with a type `$04`/`$06` block (damage 1), and of those only
  records whose `$7EE0` has bit 6 clear check the player (the stage 1-1 block at route
  frame 700 has `$48`; whether any such record is live is a lead). So the death poke is now `2`.
  The oracle now answers like fceux 2.6.5's old core (`A200x`, `A2004`: `PPUGenLatch`)
  for `$2000/$2001/$2003-$2006` and their mirrors, checked against an fceux capture of
  the energy-1 run: the gauge's count byte (`$2004`) is `$A8` in both. A `$2007` read
  during rendering is still refused, by name: fceux answers it from its renderer's VRAM
  address (there it returned `$08`, the model `$03`), which the oracle does not track
  and the port, with no PPU timing, cannot. An odd energy in the port draws the gauge
  from bank 14 bytes instead (gap).

After the fixes the sweep (weapons 0-5, seeds 1-3, a death and a GAME OVER run per
start, `$0482 = 0`) runs 600: 577 pass, 20 reach the reset (14 of the 30 GAME OVER runs,
and 33632 seed 3 with every weapon), 3 hang as the game does (weapon 0's charge, §9g),
none diverge.

## 10. Weapons and pickups (task `bossroom`, 2026-10-14)

Status: **partial**; the call sites are located, the item path is not.

- **The "weapons call" is bank 5 `$8000`.** The gameplay loop (bank 14 `$D3F9`)
  does `LDA #$15; STA $E9; JSR a:$8000` (`$14 = 20 -> bank 4`, `$15 = 21 -> bank
  5` after the 128 KiB ROM wraps mod 16). Bank 5 `$8000` dispatches on `$0476`
  through the 8-entry pointer table at bank 5 `$801B`
  (`$8029`/`$8170`/`$8191`/`$8839`/`$8D39`/`$91B2`/`$9647`/`$9D2E`), whose
  `$0476 = 0` handler scans the `$7BD0` slots.
- **`$0476` is set by the actor hit response, not by a weapon menu.** Bank 13
  `$AB5A` (the `$7D50` state 0-3 handler) does `TYA; LSR; ADC #$01; STA $0476`
  with `Y = $7D50*2`, i.e. `$0476 = state + 1`. So on a hit the actor's state
  selects the bank-5 handler, and a hit on a state-4 actor (`$AB69`) instead
  adds 2 to `$0477` and arms the level end.
- **`$7DD0`/`$7DD1`/`$7DD2` are per-slot actor bytes, not the player's item
  inventory.** Watching them on the committed stage-1-1 route shows only actor
  handlers writing them (bank 8 PC `$8BBD` walks slot 0's `$7DD0` up from `$04`,
  once per frame; `$863E`/`$8688`/`$86B7` write slots 1/2/3), and the room change
  clears all four (`$B5EB`, bank 13). They are zero at level start only because
  the slot array is cleared then; the STATUS "B does nothing" evidence stands but
  its "item bytes" label is wrong (`docs/SUBLEVELS.md` §7d-iii).
- **The player's attack gate is `$0473` bit 7.** Bank 13 `$A9C4` (the
  actor-vs-player contact, entered from the type `$04`/`$06` handler bank 8
  `$823C` at `$828A`) only runs the distance compare when `$0473` bit 7 is set
  (`$A9D7 BIT $0473; BMI $A9E6`); everything else falls through to the `$7BD0`
  scan at `$AA57`. Which code sets `$0473` bit 7 (i.e. how a press of B arms a
  hit) is **not located**.
- **No pickup on the stage 1-1 route.** No watched spawn writes an item and the
  route's `$7DD0-$7DD2` writes are the actor counters above; the route reaches
  the red-brick room with no item change.

### 10a. What the whole-game TAS adds (2026-10-14, task `inventory2`)

Evidence: `tools/rotj-oracle/examples/inventory_tas.rs` (§8b) over the whole TAS.

- **`$0476` is `0` for every shot in the game.** Watching `$00BA`-style writes
  to `$0476` yields exactly 22 in 40 895 frames: 20 from bank 13 `$AB63` (the
  `$7D50` state 0-3 hit path, `$0476 = state + 1`; values `$02` ×13, `$03` ×6,
  `$04` ×1, at frames 448/1109/17139) plus `$C124` (frame 4) and `$D492` (frame
  248, the level-start init) writing `0`. So the bank-5 handler `$801B[$0476]`
  that runs for Batman's own shots is always the `$0476 = 0` one, `$8029` — the
  §10 read of `$0476` as an actor-hit selector, not a weapon inventory, holds for
  the whole run.
- **The fire path writes the projectile type from `$0476`.** Bank 4 (window
  `$8000`) `$9AF5` selects the starting slot `$9B08[$0476 & $0F]`, scans down for
  a free `$7BD0` slot and stores `$7BD0,x = ($0476 + 1) << 1` at `$9B0F`. With
  `$0476 = 0` Batman's projectile slot value is `$02`; the actor-hit spawns
  write `$9AB8`/`$9AC0`'s `$80`. So `$7BD0`'s occupied byte *is* the bank-5
  handler index, `(value >> 1) - 1`.
- **`$7BD0` is not dead in gameplay.** It is written from frame 14 to the last
  frame; the writers are the bank-4/5 weapon code above plus `$89FB`, `$9B18`,
  `$9DBD`, `$9C4B`, `$99DB`, … and the bank-2 map loader `$8A9F`/`$8AED` at each
  level load (frames 11915, 20075, 27510, 31999, 38074). The §7d measurement
  ("`$7BD0` is zero in every captured gameplay frame") is a short-capture
  artifact.
- **The contact path runs, the damage path almost never.** `$A9C4` executes for
  29 of the 94 types the TAS sees and the `$AA57` weapon scan finds
  (`$AA91`) 25 of them; only type `$C4` ever reaches the `$AA4B` energy-loss
  branch. So "touches the player" and "is hit by a weapon" are separately
  observable per type (§8b columns), but the `$0473` bit-7 setter is still
  unlocated.
- **Which weapon each stage uses: not determined.** Every stage's shots take the
  `$0476 = 0` path; no per-level weapon selector was isolated (a level-varying
  byte in `$0460-$04FF` was searched for and not found cleanly). Left as a lead;
  the manual's Cross Bow / Batarang / Sonic Neutralizer / Shield Star list was
  not matched to a RAM byte.

