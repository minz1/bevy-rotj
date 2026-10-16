# Capturing every stage and sublevel

How to drive `tools/emu/dump.lua` to a chosen stage/sublevel so `rotj sublevel`
can extract it. Every claim is tagged **verified** (read from the running game or
the ROM) or **inferred** (a reading of the disassembly that has not been
confirmed against a capture). All 13 levels (STAGE 1-1 through 7-1) have now
been loaded and captured by forcing the level index `$20` at `$D3D8`; see §7.

## 0. The dump format (`tools/emu/dump.lua`)

| file | contents |
| --- | --- |
| `ppu-<f>.bin` | PPU `$0000-$3FFF` (CHR windows, nametables, palette RAM) |
| `ram-<f>.bin` | CPU `$0000-$07FF` |
| `wram-<f>.bin` | cart `$6000-$7FFF` (the metatile map at `$6000-$6EFF`) |
| `ramseq-<start>.bin` | full 2 KiB RAM per frame, frame N at `(N-start)*$800` |

`rotj sublevel` needs the first three for the same frame:

    cargo run -p rotj-tools -- sublevel --dump <dir> --frame <f> --out assets/levels

It writes `<out>/stage-<stage>-<sublevel>/` named after the RAM values below:
`nametables.bin` (2048), `attributes.bin` (64), `palette.bin` (32),
`collision.bin` (8192, WRAM) and `meta.txt`.

## 1. The position address table (verified)

| RAM | meaning | frame 700 of `assets/levels/stage-1-1/` |
| --- | --- | --- |
| `$0401` | **not a sublevel index** — 4-bit field built from the CHR-bank shadow (see §7a) | `8` |
| `$0402` | stage index, 0-based | `0` |
| `$0400` | area index, **the CHR-set / level selector** (`$0400/2`); +2 per screen | `0` |
| `$00AC` | horizontal scroll in pixels the nametable is keyed to | `0` |

Evidence:

- **verified** frame-700 capture (`tools/emu/stage/ram-700.bin`): the four bytes
  read `08 00 00 00`; `docs/LEVELDATA.md` §0/§12b and `docs/STATUS.md`.
- **verified** the level loader is entered as `JSR a:$8F05` (bank 02, the
  sublevel start) and `JSR a:$92AD` (bank 02, the screen load) from bank 0E
  `$C55E`; `docs/LEVELDATA.md` §1.
- **verified** the transition dispatcher `bank0E:sub_D4D3` ($C4D3): `LDA $BA;
  BNE +; RTS; +: ASL; TAX; LDA $D60D,x; STA $10; LDA $D60E,x; STA $11;
  JMP ($0010)`. `$BA` is a jump-table index; the table at bank 0E `$D60D` has 7
  entries ($D4D7, $D4E7, $D4FE, $D513, $D527, $D58F, $D967).
- **verified** reset hardcodes `$0402 = $01`, `$0401 = $02` (`docs/LEVELDATA.md`
  §0, bank 15 `$E35C`), and `bank0E:sub_D48D` (`$C48D`, the stage init/restart)
  sets `$0402 = $00`, `$0482 = 2` lives. So stage 1-1's `$0402 = 0` comes from
  here, not from reset.
- **verified** `$0402` is the stage index: `bank0E:far_D992` ($D992) does
  `LDX $0400; LDA $DA18,x; STA $30; LDA $DA19,x; STA $31; LDY $0402;
  LDA ($30),y ...`, i.e. `$0400` selects a per-area pointer that `$0402` indexes
  by stage. Bank 02 `$8Axx` uses the same pair.
- **inferred** `$0400` steps by 2 per screen (recorded in `docs/LEVELDATA.md` §0
  from `bank0E.s`; the current regenerated listing decodes that block as data).

## 2. The 7 stages: what to write (partly inferred)

To select doc stage **S**, write `$0402 = S-1`. The sublevel byte `$0401` is the
stage-local sublevel index; only stage 1's is measured.

| doc stage | `$0402` | `$0401` | status |
| --- | --- | --- | --- |
| 1 | `00` | `08` | **verified** (frame 700) |
| 2 | `01` | unknown | `$0402` inferred from the index use |
| 3 | `02` | unknown | as above |
| 4 | `03` | unknown | as above |
| 5 | `04` | unknown | as above |
| 6 | `05` | unknown | as above |
| 7 | `06` | unknown | as above |

`rotj_core::sublevels` holds the four addresses and reads them; the name
`assets/levels/stage-1-1/` is a human label for stage 0 / sublevel 8, which is
why the tool names its own directories with the raw bytes (`stage-0-8/`).

**The sublevel values for stages 2-7 are not decoded.** The only decoded stores
to `$0401` are reset's `$02` and one object-script store (`bank00`), and
`bank0E:LD2B0` reads `$0401` as a 4-bit field, so its exact meaning (per-stage
sublevel vs progress flags) is itself **unverified**. Fill the column by probe
(§4) rather than by guessing.

## 3. Triggering the load

- **verified** the sublevel-start path calls bank 02 `$8F05` and the screen loader
  `$92AD` (§1). The game reaches them at gameplay and on restart/area change.
- **inferred** a reload can be requested by writing `$BA` (the dispatcher index)
  while the game is in gameplay, so the next `sub_D4D3` runs the chosen handler.
  `$BA = 0` runs `$D4D7` — the branch the ROM itself uses when it wants a
  transition (`bank00 $9352` writes `$BA = $03`).
- **inferred** whichever handler runs, it must re-read `$0401`/`$0402`; if it
  does not, it reloads the *current* sublevel and the write is ignored. This is
  the main reason to confirm the load (§4) instead of assuming it.

## 4. Procedure with `dump.lua`

`dump.lua` has no hook for `$0401`/`$0402`. Add one to `input_for`, next to the
existing `sweep` mode (which already writes `$0094` and `$0400`):

```lua
-- INPUT=stage: select a stage/sublevel once the level is running, then request
-- a transition and let the loader rebuild the nametable.
if mode == "stage" then
    local keys = {}
    if frame >= 300 and frame <= 306 then keys.start = true end
    if frame >= 560 then
        memory.writebyte(0x0402, tonumber(os.getenv("TARGET_STAGE") or "0"))
        memory.writebyte(0x0401, tonumber(os.getenv("TARGET_SUBLEVEL") or "8"))
        memory.writebyte(0x0400, tonumber(os.getenv("TARGET_AREA") or "0"))
        if frame == 570 then memory.writebyte(0x00BA, 3) end -- transition request
    end
    return keys
end
```

Run it (paths absolute; `DUMPS` must include the frames you want):

    mkdir -p tools/emu/stage2
    DUMP_DIR="$PWD/tools/emu/stage2" INPUT=stage TARGET_STAGE=1 TARGET_SUBLEVEL=0 \
      DUMPS="600,700" RAMSEQ=off UPTIME=1001 \
      timeout -k 5 500 fceux --sound 0 --loadlua "$PWD/tools/emu/dump.lua" \
        "$PWD/raw/Batman - Return of the Joker (USA).nes"
    cargo run -p rotj-tools -- sublevel --dump tools/emu/stage2 --frame 700 --out assets/levels

To **discover** a stage's sublevel values, drop the `TARGET_SUBLEVEL` write,
sweep it (`0..20`), and read back the value the game settles on:

    cargo run -p rotj-tools -- sublevel --dump tools/emu/stage2 --frame 700 --out /tmp/level-probe
    cat /tmp/level-probe/stage-<S>-<L>/meta.txt   # stage/sublevel as the game set them

## 5. Confirming the right level loaded

- `meta.txt` (and the directory name) report the RAM values that were live at the
  dump frame; they must equal the target.
- `$0400` should have been rewritten by the loader (it steps by 2 per screen);
  a value that never changes suggests the transition did not run.
- `$00AC` (scroll) plus the rendered nametable is the real check: extract with
  `rotj sublevel` and diff `nametables.bin` against the captured nametable. If
  the map-derived render matches the capture byte-for-byte (as it does for
  stage 1-1 at scroll 0/50), the right sublevel is loaded; a mismatched render
  means the map in WRAM is not the sublevel you asked for.
- Cross-check the pattern view: `rotj stage`'s `windows.txt` lists the CHR banks
  in use, which differ per area.

## 6. Risks

- **Writes are overwritten.** `sub_D48D` (`$C48D`) writes `$0402 = 0`, and the
  loader rewrites `$0400`; a write before the load, or while a transition is in
  flight, is discarded or leaves the two bytes inconsistent with the scripts
  already loaded. Write only while gameplay is stable (`$0401`/`$0402` steady).
- **A wrong `$BA`/dispatcher index runs an unrelated transition** (the table has
  7 entries and `$BA` is also set by object scripts). A wrong value can lock the
  game in the transition handler.
- **Bank switching.** The handlers reprogram `$E0-$EC` (mapper shadow) mid-path;
  injecting during that window leaves the bank map inconsistent for a frame.
- **Savestate first.** Keep `state-<f>.fcs` (dump.lua writes it) and re-enter it
  instead of replaying input, so a bad write is cheap to undo.
- Writing RAM does not change the ROM, so nothing is permanent; the risk is a
  corrupted running session, not the asset files (which are written by the tool).

## 7. The real selector, and what the warp captures did (measured)

**The level's CHR set is chosen by `$0400/2`** through the single bank-2 table
pair `$8B63`/`$8B75` (§13e of `LEVELDATA.md`). `$0402` (stage) does **not**
enter the tile selection; it indexes script tables (actor spawns etc.).

- The level loader runs at CPU `$89FE` (bank 2 in `$8000`); hooking it during a
  load shows `$89FE` execute and the CHR loop write `$D8..$DF` for `$0400 = 0`.
- Forcing `$0400 = 2*area` at `$89FE` and capturing WRAM reproduces
  `map_from_rom(chr, area)` **0 / 4096** for **14 of the 18 areas** (all but
  4, 10, 12, 17. Those four have one-screen streams; for 4 and 12 an actor of
  the stage clears the remaining columns, see `docs/LEVELDATA.md` §13e)
  (`tools/emu/sublevels-force/`, `crates/rotj-core/tests/data/area{5,16}-map.bin`,
  `docs/LEVELDATA.md` §13c).
- `$0400` is set at level start by `sub_D3D8` (`LDX $20; LDA $D94E,x; STA
  $0400`, bank 14 `$D94E`); stage 1-1 has `$20 = 0` -> `$0400 = 0`.
- **verified (2026-10-14)**: forcing `$20 = n` at `$D3D8` itself, for every
  `n` in 0..12, makes the game write `$0400 = $D94E[n]` (traced at PC `$D3E0`)
  and load `area_for_level(n)`'s map. The transition screen prints the game's
  own `STAGE x-y` banner with `x = n/2 + 1`, `y = n%2 + 1`, so the level index
  `$20` is the game's stage-sublevel counter:

  | `$20` | banner | `$0400` | area |
  | --- | --- | --- | --- |
  | 0 | 1-1 | `00` | 0 |
  | 1 | 1-2 | `02` | 1 |
  | 2 | 2-1 | `06` | 3 |
  | 3 | 2-2 | `08` | 4 |
  | 4 | 3-1 | `0A` | 5 |
  | 5 | 3-2 | `0C` | 6 |
  | 6 | 4-1 | `10` | 8 |
  | 7 | 4-2 | `12` | 9 |
  | 8 | 5-1 | `16` | 11 |
  | 9 | 5-2 | `18` | 12 |
  | 10 | 6-1 | `1A` | 13 |
  | 11 | 6-2 | `1C` | 14 |
  | 12 | 7-1 | `20` | 16 |

  The map matches `map_from_rom(area)` byte-for-byte for 11 of the 13 levels;
  levels 3 and 9 (areas 4, 12) load only partially (see `LEVELDATA.md` §13e).
  The banner is captured in `tools/emu/stagearea-probe/banners-514.png`.

**7a. `$0401` is not a sublevel.** `sub_B40A` (bank 13, `$B47F`) builds it from
the CHR-bank shadow (`LDA #$00; STA $0401; LDX #$03; loop { LDA $E0,x; ASL; ROL
$0401; … }`), and `sub_D2B0` reads it as a 4-bit field over the 16 actor slots.
Stage 1-1's `$0401 = 8` is a consequence of the CHR banks, not a level number.

**7b. The warp captures did not load a different sublevel.** `tools/emu/warp`
and `warp2` captured stage 0 / sublevel 2 / area 0, and their `$6000-$6FFF`
differs from area 0's map by only 197 bytes, all in metatile rows 14-15
(columns 50-255) — the tail the loader's transpose writes last, i.e. a partial
load, not another level. They also only dumped frame 600. So they are **not**
usable as a second-sublevel capture; the forced-`$0400` captures above are.

## 7c. Unknowns

- What advances `$20` during normal play: the table above is entered once, on
  the power-on/reset path (bank 15 clears RAM `$00-$FF`, so `$20 = 0`), and no
  capture yet leaves level 0 on its own. The `route2` playthrough (§7d) reaches
  the game's own mid-level transition and shows the play-time advance is `$0402`
  (a room change), **not** `$20`; `far_D394`/`sub_D3D8` never re-run.
- Whether level 7-2 exists: `$D94E` has 13 entries, covering STAGE 1-1..7-1.
- Whether a stage can be reached at all without the game's normal progression
  (stage select/password); if not, captures must be driven by play. Forcing
  `$20` at `$D3D8` is the reliable way to reach any of the 13 levels for
  capture. **Answered (2026-10-15):** the title's PASSWORD line opens a password
  screen (boot payload `$6330`) whose fourteen passwords (`$6E6C`) set `$20` to
  each of the 13 levels before `$D3D8` reads it (`docs/ACTORS.md` §9s, with the
  list).

## 7d. What advances the level in real play (the `route2` capture, measured)

A scripted playthrough of stage 1-1 (`tools/emu/route2-route`, `dump.lua`
`INPUT=route2`: Right held, a bunny-hop that holds A through each jump and
re-jumps the frame after landing, then a periodic Left back-off once `$0402`
reaches 2) reaches the game's own mid-level transition and shows what it
changes:

- **`$20` never advances during play.** Watching RAM `$20` for 5000 frames
  (`tools/emu/route2-recon/ram-writes.csv`) shows it written only by zero-page
  pointer scratch (PCs `$80D1`, `$815A`, `$81F8`, `$820A`, `$9838`, `$ABE7`,
  `$ABFD`, `$A570`, `$A7E1`, `$A7F8`); no write carries a level index.
- **`far_D394` is boot-only.** The pointer `94 D3` occurs exactly once in the
  393232-byte ROM (file offset `$1e385`, the bank 15 boot `JMP a:$D394`), so no
  PRG path re-enters the level start. `sub_D3D8` (the only reader of `$D94E`)
  runs once, at frame 474 (`ram-writes.csv`: `0400 <- $D94E[0]` at PC `$D3E0`).
- **The play-time advance is `$0402`.** At frame 1150 the capture writes `$0402`
  `00 -> 01 -> 02` at bank 13 PC `$B92A`/`$B92D` (the `INC a:$0402` pair), and
  `$B92D` is immediately followed by `JSR a:$B40A` (`sub_B40A`, the
  palette/CHR loader of `LEVELDATA.md` §14, which also rebuilds `$0401`).
  `$0400` is **not** written, and the resident map `$6000-$6FFF` is
  byte-identical before and after the change (`tools/emu/route2-route/wram-1148.bin`
  vs `wram-1152.bin`, **0 / 4096**), so this room change is a re-skin
  (palette/CHR), not an area reload. It also clears the actor slots (`$7CC0-$7CDF`
  <- `$00`, PC `$B5ED`) and re-runs the level-start slot init (PC `$D7AB`).

  The bank 13 routine that does it (`$B8DC`): `BIT $DD; BVC $B927; ...; $B927
  INC a:$0402; $B92A INC a:$0402; $B92D JSR a:$B40A`.

- **The level end is the `$BA=5` handler.** The state-4 actor handler bank 13
  `$AB69` (dispatched through the `$7D50` state table `$ABA8`) increments `$0477`
  by 2 and, when the result reaches `$0E`, sets `$BA=5`. `sub_D4D3` then
  dispatches handler-table `$D60D[5] = $D58F`, which fades, clears `$7BD0`, sets
  `$0481=$80`, `$BA=0`, `$046E=8`, `$F7=$80` and calls `$D64A`.
  **Corrected (2026-10-15):** that is the full meter's power-up (`docs/ACTORS.md`
  §9l), not a level end, and it does not continue into `$D60D[6] = $D967`. `$D967`
  is its own transition, `$BA = 6`: the game's ending (§7e).

Test: `crates/rotj-core/src/levelmap.rs`
`room_change_advances_0402_without_reselecting_the_area` pins the `$0402`
advance and the unchanged `$0400`/map against the committed slices
`tests/data/route2-ram-{1148,1152}.bin` and `route2-map-{1148,1152}.bin`.

**Not reached (honest limit).** The route stalls in the second room (a red-brick
platform shaft) at level x ~700: `$0402` stays 2, `$00BA` is never written, and
the door/boss the room needs is not reached. Holding Up or B there does not
clear it. So this capture reaches the game's own mid-level transition but
**not** the end of 1-1 or the load of 1-2: no `STAGE 1-2` banner, no level-1
palette (`palette_for_level(1)`), and no `map_from_rom(area 1)` were observed.
The full 1-1 -> 1-2 path remains unproven.

**7d-i. What advances `$20` (found 2026-10-14 in the ROM).** The level-end payload
is not PRG: it is 8 KiB copied from **CHR 1 KiB banks `$F8-$FF`** (offset
`$3E000..$3FFFF` of the 256 KiB CHR) into WRAM `$6000-$7E5F` by bank 02
`$99A9`, and entered at `$6135` (`$999C`: `JSR $99A9; JMP a:$6135`). The bytes in
WRAM are therefore the ROM's CHR at file offset
`0x20010 + 0x3E000 + (addr - 0x6000)` (`raw/...nes`, PRG 128 KiB + CHR 256 KiB):

- WRAM `$6135` (offset `$135`) = `A9 00 85 2F / A9 67 85 2E / 20 C9 7E /
  A9 0D 85 41 ...` — sets the `$2F:$2E` pointer and requests `$0D` via `$0041`.
- WRAM **`$63D0`** (payload offset `$3D0`, ROM file offset **`0x5E3E0`**) =
  **`E6 20` (`INC $20`)**, `F0 03` (`BEQ $63D7`), `4C 61 63` (`JMP $6361`);
  when the increment wraps `$20` to `$00` the payload restarts with `$20 = $60`
  and decrements. `STA $20` and `LDA $20` occur throughout the payload (it is
  also the loop counter).

The payload is listed in `disasm/cutscene.s`; its `$6000` entry is the stage
banner's cutscene, ported (`docs/ACTORS.md` §9o).

**Corrected (2026-10-15): `INC $20` at `$63D0` is not a stage advance.** It is the
ending's 256-frame skyline loop counter (`$6361-$63D4`): the oracle runs `$6361` 256
times, TAS frames 41914-42169, then `$63D7` at 42170 (§7e). The `$6135` entry is the
ending, reached only by `$BA = 6`; the stage advance is the next area's `$0400 += 2`
(`$D513`, `docs/ACTORS.md` §9l). `$20` is zero-page scratch throughout, which is why
a per-frame watch shows only scratch writes (§7c).

## 7e. The ending, `$BA = 6` (oracle, measured)

The TAS's input ends at frame 40894; the oracle plays on with no input. The stage 7
boss falls, and its last figure (type `$B8`, bank 12 `$93FC`: hold `$80` frames,
flicker 16, sink `$80`) sets `$BA = 6` at bank 12 `$9457` in TAS frame 41177. The
next frame's dispatch runs `$D60D[6] = $D967` (`$E9 = $12`, `JMP $999C`, bank 2):
the music stopped (`$40 = $FF`), the fade out `$C2E1`, the copy `$99A9` (41194) and
`JMP $6135` (41204). The payload (`disasm/cutscene.s`) never returns:

| TAS frames | payload | what it shows |
| --- | --- | --- |
| 41204- | `$6135` | the screen cleared to tile `$67`, the song `$0D` (41240), the second nametable patterned from `$66E0` (`$66A9`), the title blocks of the list `$6720`, the raster split `$D3 = $0C` (bank 2 `$97DD`: the second nametable from `$1E00` cycles in, then the first with sprites off from `$5E00`), the fade in, `$90` frames |
| 41436-41552 | `$61D0` | a figure falling down-left from (`$68`, `$60`) beside a burst (`$665C`, its four shapes from `$686E`, the effect `$22` with the first) |
| 41649-41722 | `$6254` | a figure rising up-right from (`$20`, `$FF`), 74 frames, then the fade out |
| 41835- | `$62DE` | the skyline: tile `$67`, the lists `$6720` and `$68A5`, the watcher `$6AD4` at (`$78`, `$A8`), three bursts over the city from frames 0, `$38` and `$58` of a 256-frame loop (41914-42169), the watcher 95 more, three colours lit, the effect `$23` (42266), white, the fade out |
| 42349- | `$6444` | the credits: one-screen mirroring (`$EC = 2`), the list `$6B02`, the parallax split `$D3 = $0D` (bank 2 `$9820`: four bands scrolling at 1/2, 1, 1/4 and 1 px a frame, `$FD` following the first), a walker (`$6526`) in to x `$80`; 28 lines from the list `$6D2E` (offsets `$6D12`) each held `$A0` frames (42582-46929), the walker off (47090-47284), the fade out |
| 47397- | `$65BB` | the list `$70C3`, 384 frames (`$0300` counted down by 2) |
| 47938- | `$6656` | the last screen (the list `$7147`), the NMI wait forever |

The port runs it (`actor/ending.rs`, `docs/ACTORS.md` §9r) and matches every frame.

**7d-ii. The route finder reaches the same stall (2026-10-14).**
`tools/rotj-oracle/examples/routefind.rs` (a beam search over controller
macro-actions; its `--help` documents the knobs) reproduces the scripted
capture's progress: from the title it reaches the room change `$0402` **`0 -> 2`
at frame 1025** (`tools/rotj-oracle/tests/data/stage1-inputs.csv`, 1027 frames)
and cannot leave the red-brick room. Verified in fceux with the new
`INPUT=replay REPLAY=<csv>` mode of `dump.lua` (input only, no RAM pokes):
`$0402` is `0` at frame 1020 and `2` at 1040, and `$0094`/`$00B1`/`$046E`/
`$00BA` match the oracle to the byte at the documented one-frame dump offset
(fceux `ram-N` is the oracle's frame `N-1`). Inside room 2 the search never
finds a state higher than the room-2 entry (the beam's `$00B1` never drops
below the landing line; the airborne `$046E = 3` states are all descending), so
the upper platforms (and the crate/door in the room's render) are out of reach
and the room stalls exactly as §7d describes — the level-end `$BA = 5` is armed
by the `$7D50` state-4 actor, which is not reached.

**7d-iii. Inside the red-brick room (task `bossroom`, 2026-10-14).** Measured
with the oracle from `tests/data/stage1-inputs.csv` plus idle input; the fceux
replay of the same input (`tools/emu/bossroom-recon/`, `INPUT=replay`) matches
the oracle frame for frame.

*Player motion addresses.* The player's x is **`$00B0`** and y **`$00B1`** (both
pixels). Holding Right walks him from x 20 (left wall) to x 214 (right wall);
holding A jumps (`$046E` reaches 3) so y goes 176 → 122. So §7d-ii's "the
airborne states are all descending" is the room's geometry, not a frozen
player: he walks and jumps normally; the exit that the render shows up-left
(a crate and a dark doorway) sits above the ~54 px a held jump reaches. (The
earlier lead that "the player seems unable to jump" is wrong.)

*The room's two actors.* At frame 1058 the level-start init (bank 14
`$D7A8`-`$D821`) spawns exactly two objects:

| slot | type `$7CC0` | state `$7D50` | kind `$7DB0` | schedule `$7CE0` | record (bank 3) |
| --- | --- | --- | --- | --- | --- |
| 0 | `$06` | `$00` | `$61` | `$31` (49) | id 6: `03 ff 34 ff 00 03 00 33 61 00 05 48` |
| 24 | `$04` | `$08` | `$00` | `$30` (48) | id 1: `0b 00 35 00 01 82 08 00 00 0b 00 00` |

The records come from bank 3's per-area table: `$A000[$0400] = $A024`, then the
per-stage pointer at **byte offset `$0402`** (`$D728` `LDY $0402; LDA ($24),y`),
so room `$0402 = 2` reads the *second* entry `$A072` (not a `$0402*2` index —
this is why a `stage*2` decode of the table misses). Slot 24's state is not a
hit counter: bank 8 `$8000`-`$80F2` (PC `$80E3` toggles it 08↔04 with
`EOR #$0C`) oscillates its position fields by ±16/24 px — it is the room's
moving platform (inferred from the code).

*The level-end chain is not reached in this room.* The actor-vs-player contact
routine bank 13 `$A9C4` is called from the type `$04`/`$06` handler (bank 8
`$823C` at `$8286`-`$828A`) with `$A5 = 1`; when `$7EE0,x` bit 7 is set it
resolves through `$AB39` to the `$7D50` dispatch, where state 4 (`$AB69`) adds 2
to `$0477` and arms `$BA = 5`. In the red-brick room neither actor reaches it:
slot 0's hit lock `$7EE0 = $48` (record byte 11) takes the `$A9D0` skip, and the
update loop `$CED5` scans slots **23..0** only, so slot 24 is driven by the
bank-8 platform code, never by the contact handler. The oracle executes neither
`$A9E6` nor `$AB39` in the room.

*`$7DD0-$7DD2` are not the player's item bytes.* They are per-slot actor bytes:
watching them on the committed route shows only actor-handler writers (bank 8
`$8BBD` walks slot 0's up from `$04`; `$863E`/`$8688`/`$86B7` write slots 1/2/3),
and the room change clears them (PC `$B5EB`). The STATUS "B does nothing at the
level start" evidence (they are zero there) holds; the *name* "item bytes" does
not.

*Open (honest limit).* The room's exit is unfound. With `$00B0`/`$00B1` as the
progress and height-first scoring, `routefind` reaches only x 214 / y 122 in the
room, never `$BA = 5`, and no further actor spawns (no state-4 actor ever
appears). Whether the exit needs a weapon to break the crate, a hidden door, or
something else is unverified.

**7d-iv. `routefind`'s room progress (corrected).** The example's `camera()`
reads `$CC/$CD`, which is *not* the room's scroll (`$00AC` is); for room ≥ 2 the
score now uses the player's x `$00B0` and y `$00B1`, and the dedup key keeps the
`$0477` counter so a state with more hits is never pruned.



## 8. Stages 2-7 on the TAS (oracle, measured)

The published TAS's area (`$0400`), room (`$0402`), orientation (`$DD`) and room
change (`$BA`/`$BB`) from frame 7000 to the ending, read from the oracle every frame. The
first loop frame of each area is where `$BA` returns to 0 after its load.

| frames | area:room | `$DD` | how it is left |
| --- | --- | --- | --- |
| 7239-7570 | 06:00 (stage 2-1) | `$00` | slide right, `$BB = $61` |
| 7637-8073 | 06:02 | `$C0`: vertical, two nametables wide | fade, `$BB = $81` |
| 8139-8519 | 06:04 | `$00` | `$BA = 3` |
| 8634-11487 | 08:00 (stage 2-2, a one-screen map) | `$00` | `$BA = 3`, the stage 3 cutscene |
| 11959-13091 | 0A:00 (stage 3-1) | `$00` | `$BA = 3` |
| 13215-13963 | 0C:00 (stage 3-2) | `$00` | fade, `$BB = $E1` |
| 14046-14391 | 0C:02 | `$80`: horizontal, two nametables tall | `$BA = 3`, the boss room |
| 14649-15241 | 0E:00 (the stage 3 boss) | `$00` | `$BA = 3`, the stage 4 cutscene |
| 15711-20014 | 10:00 (stage 4-1) | `$00` | `$BA = 3` |
| 20015-20465 | 12:00 (stage 4-2) | `$00` | slide, `$BB = $61` |
| 20466-23603 | 12:02 | `$40`: vertical | fade, `$BB = $E0` |
| 23604-23806 | 12:04 | `$00` | slide, `$BB = $62` |
| 23807-26049 | 12:06 | `$40` | `$BA = 3`, the boss room |
| 26050-27098 | 14:00 (the stage 4 boss) | `$00` | `$BA = 3`, the stage 5 cutscene |
| 27099-28723 | 16:00 (stage 5-1) | `$00` | `$BA = 3` |
| 28724-31588 | 18:00 (stage 5-2, a one-screen map) | `$00` | `$BA = 3`, the stage 6 cutscene |
| 31589-32623 | 1A:00 (stage 6-1) | `$00` | slide, `$BB = $61` |
| 32624-33271 | 1A:02 | `$00` | slide, `$BB = $61` |
| 33272-33529 | 1A:04 | `$00` | `$BA = 3` |
| 33530-36608 | 1C:00 (stage 6-2) | `$00` | `$BA = 3`, the boss room |
| 36609-37663 | 1E:00 (the stage 6 boss) | `$00` | `$BA = 3`, the stage 7 cutscene |
| 37664-38617 | 20:00 (stage 7-1) | `$00` | fade, `$BB = $C1` |
| 38618-39149 | 20:02 | `$00` | slide, `$BB = $61` |
| 39150-39469 | 20:04 | `$00` | `$BA = 3`, the boss room |
| 39470- | 22:00 (the last boss) | `$00` | the TAS's input ends at 40896; the boss falls at 40921 and `$BA = 6` (the ending, `$D967`) at 41177 |

Rows after 10:00 start where `$0400`/`$0402` change, so they include the area's load
and, for stages 5-7, its cutscene; the boss rooms are entered with `$0404` bit 7 set.

So stage 2 has no boss: its end is area 8's `$BA = 3` at 11488, which plays the
banner cutscene before stage 3-1; stage 5 has none either. The port replays all of it byte for byte
(`docs/ACTORS.md` §9k); the rooms with `$DD` bit 7 and the fade path are §9j, their
scrolling §9q.
