# Physics and runtime facts (from emulator captures)

Everything here was read out of live RAM/PPU while the game ran, then
cross-checked against `disasm/MAP.md`. Method: `tools/emu/dump.lua` (fceux Lua)
runs a scripted input sequence and dumps, per frame, CPU RAM (`$0000-$07FF`),
cart WRAM (`$6000-$7FFF`), PPU space (`$0000-$3FFF`), plus screenshots, APU
register writes and savestates. `tools/emu/compare.py` then diffs the port's
rendering against the emulator's own screen, index-for-index.

Reproduce a capture:

    DUMP_DIR="$PWD/tools/emu/run" INPUT=physics DUMPS="700,850" \
    SHOTS="600,1000,20" RAMSEQ="600,1000" UPTIME="1001" \
    timeout -k 5 600 fceux --sound 0 --loadlua tools/emu/dump.lua \
      "raw/Batman - Return of the Joker (USA).nes"

(Paths must be absolute: fceux's working directory is not the repo. The process
does not always exit cleanly; every artifact is written before that matters.)

## Renderer verification — PASSED

`rotj-verify screen` renders a captured frame through the port's renderer;
`compare.py` diffs NES colour indices against the emulator's screenshot
(RGB is not comparable: the emulator and the port use different RGB tables for
the same palette).

| frame | scene | colour-index mismatch |
| --- | --- | --- |
| 300 | attract/title | **0.00%** (10,191 / 10,191 non-black pixels identical) |
| 200 | attract transition | 9.84% |
| 1200+ | gameplay | ~38% |

The 0.00% result proves tile selection, attribute palettes, scrolling, sprite
composition and palette RAM extraction are correct. The gameplay mismatch is
**not** a renderer bug: those screens are composed mid-frame — the game changes
scroll and 1 KiB CHR banks at scanline splits via its FME-7 IRQ counter
(`disasm/MAP.md` §3) — so no single-configuration render can match them. Band
rendering (the renderer already draws per scanline band) needs the split
schedule captured next.

Also established: fceux writes its screenshot as a palette PNG whose entry
`i + 128` is NES colour `i` (verified against `$00`, `$0F`, `$11`).

## Game flow (NTSC, scripted input)

| frame | what happens |
| --- | --- |
| 57 | rendering starts |
| 300–306 | pressing Start here leaves the title |
| 414–476 | `$0401/$0402` = stage 0, sublevel 2 |
| 574–607 | stage 0, sublevel 8 = **stage 1-1 gameplay** |

- Pressing Start *again* during gameplay **pauses** it: the game's own frame
  counter (`$00A5` high / `$00A6` low, big-endian) freezes. That cost a whole
  capture — the counter is the cheap "is it running" signal.
- Input via fceux Lua `joypad.set(1, {...})` does reach the game (A/B test:
  with input off the object slots stay `$FF`; with input on there is a live
  object, `type $7DB0 = 104`, `state $7D90 = 12`).

## Batman physics — read from the game's own integrator

Vertical velocity lives at **`$009E`** (mirrored at `$046A`) and the vertical
position at **`$00B0`/`$00B1`** (high byte = pixels). Both are signed, in the
game's fixed point: **1/16 pixel**, with the pixel coordinate being the value
*truncated toward zero* (`trunc(units / 16)`).

One jump from standstill, per frame — velocity, then the position it produces:

| frame | `$009E` | position (px) | rise |
| --- | --- | --- | --- |
| 802 | 0 | 192 | — |
| 803 | **-44** | 190 | 2 |
| 804 | **-60** | 186 | 6 |
| 805 | -54 | 183 | 9 |
| 806 | -48 | 180 | 12 |
| 807 | -42 | 177 | 15 |
| 808 | -36 | 175 | 17 |
| 809 | -30 | 173 | 19 |
| 810 | -24 | 171 | 21 |
| 811 | -18 | 170 | 22 |
| 812 | -12 | 169 | **23 (apex)** |
| 813 | -6 | 169 | 23 |
| 814 | 0 | 169 | 23 |
| 815 | +6 | 169 | 23 |
| 816 | +12 | 170 | 22 |
| … | +6/frame | … | … |
| 825 | +6 (landed) | 192 | 0 (landed) |

| constant | value | evidence |
| --- | --- | --- |
| jump launch phase | **-44, then -60** | `$009E` on the takeoff frame and the next |
| gravity | **+6 units/frame²** | every subsequent step |
| vertical clamp | **-128 .. +127** | `$8329`/`$8337` on the 1/16 px byte; `+60` was a landing velocity, not a cap |
| jump rise | **23 px** | `$00B1` 192 → 169; the launch phase plus gravity sums to 374 units |
| airtime | **~23 frames** | 803 → 825 |
| walk top speed | **32 units/frame (2 px/frame)** | `$00AC` steps 2 px/frame through the walk burst |
| walk acceleration | **16 units/frame²** | the first walking frame moves 1 px, then 2 px/frame |

The "second launch frame" `-60` is really `-44` + gravity `+6` + the first held-A
thrust `-22`; see **Variable jump height** below for the decoded rule. The
velocity is a signed byte in 1/16 px, clamped to `[-128, 127]`.

The launch phase is why the rise is 23 px rather than the 20.6 px a single -60
impulse would give, and why the pixel position *holds* for three frames near the
apex: those frames move less than a whole pixel (1/16 px units accumulate and
the displayed pixel only changes when they cross one).

Other variables:

- Jump state **`$047D`**: 0 grounded, 1/2 airborne phases; also the held-A
  thrust counter (see **Variable jump height**).
- Walk animation state **`$0466`** (12–19 while running).
- `$00A5`/`$00A6` = frame counter (big-endian); `$00A6` alone counts frames.
- `$00AC` = the **camera's horizontal scroll** in level pixels, and `$00AD` its
  vertical counterpart. Verified without the port: between frames 600 and 700 of
  a scripted walk the emulator's own screen scrolled +52 px while `$00AC` went
  0 → 50, and it then parked at 54 exactly as the screen stopped moving. The
  camera follows the actor, keeping him near screen x 120.
- The camera/scroll working set is `$043D-$047F`. `$AD` (the "scroll X" cheat
  lead) is not the live camera — it stayed 0 through a full walk burst.
- **The actor's y is the ground *line* he stands on** (192 at rest): he rests on
  the solid metatile row just *above* that line, and his sprite's last pixel is
  one above it. That is what the game's collision actually checks.

### Still to pin down

1. **The parallax band was a measurement artifact (retracted).** I reported a
   standing 32 px offset in the y 32-96 band. That band is *sky*: 124 non-black
   pixels in 14,336, so its "best shift" fit was fitting noise — with the
   correct pairing the same band differs by only 0.2%. There is **no** band
   offset, and no parallax split to port: on frames 800-802 the game writes
   *nothing* at all to `$2000`/`$2005`/`$2006` (the trace records zero writes),
   so no scroll difference can exist during those frames. What the game does
   write is discussed below, and `disasm/MAP.md` §3's IRQ raster story stays
   unverified (its probe addressed never executed).

1b. **There is no mid-frame scroll split to port.** `$98A7`/`$98AC` write
   `$2005` twice (x and y) on the frames they run, which looked like a split;
   tracing them shows the values equal the `$00AC`/`$00AD` shadows the port
   already renders from, and the frames where they run (the intro's early
   section) still reproduce **exactly** (0.00%). Combined with this: the frames
   where they do *not* run write only the NMI's `0,0` pair. So the game's
   multi-layer look comes from nametable streaming plus a single scroll, not
   from a raster program — the port needs no raster engine, which removes that
   item from the release ladder (0.2 is streaming only).

2. **The pairing offset differs per component — it is not "everything +1".**
   Measured on frame 800 against its own screen, all with nametables and palette
   from the frame itself:

   | component pairing | mismatch |
   | --- | --- |
   | pattern @ frame+1, OAM @ frame | **0.67%** |
   | pattern @ frame+1, OAM @ frame+1 | 0.76% |
   | pattern @ frame, OAM @ frame | 6.81% |

   So the CHR banks a dump shows are the ones the *next* frame displays (the NMI
   rewrites them during vblank), while OAM describes its own frame. Getting this
   backwards is what produced the 28% "mismatch" I chased for a while.

3. **The remaining 0.67% is one streamed column, not a renderer difference.**
   All 385 differing pixels sit in the leftmost 16 px of the screen (the rows
   they fall on are scattered). That is the nametable column the game streams
   *during* the visible frame as the camera scrolls: a dump taken at the end of
   the frame has the new column, the screen has the old one. The port's own
   picture is therefore faithful; this is a harness limitation, and it is the
   one thing `tools/emu/check-frame.py` cannot pair away.

4. **The intro cinematic's 9.4% is the same phenomenon, larger.** Frames 218-224
   of the attract intro differ from the emulator in a clean 80 px band
   (y 64-143; zero everywhere else), and the difference is *invariant to every
   component source* — swapping pattern, palette, nametables or OAM from any of
   frames 218-224 leaves it at exactly 5,381 px. The trace explains it: at frame
   220 the display-list executor runs **twice** (12 `$2006` writes instead of 6),
   rewriting both nametables. The screen therefore shows a *mixture* of the old
   and new nametables for part of the frame, which no single dump can represent.
   No sprites are involved (OAM is empty), no scroll split (`$2005` writes that
   frame are only the NMI's `0,0` pair).

   **Rule for verification:** a frame is reproducible from a dump only when the
   game did not write the nametables during it. Sampling frames where the
   trace shows no `$2006`/`$2007` writes (or only the vblank set) gives exact
   comparisons; transition frames are unrepresentable by construction.

   **The HUD needs no work.** The status bar (score, lives) is *nametable*
   content, not sprites — the only top-bar sprites in the capture are four
   identical tiles ($11, palette 3) at y=16, x=24..48, a bar. The port already
   draws the HUD correctly through the captured nametables; only that bar (game
   state the port does not model yet) is missing.

   **Verification lesson (cost me two false conclusions):** for everything
   written during vblank — the pattern banks and OAM — a dump describes the
   *next* frame. Pairing dump N with screen N shows ~28% mismatch that is pure
   harness artefact; pairing dump N's nametables/palette with dump N+1's pattern
   and OAM drops scrolled frames to 0.76%, and per-band fitting explains the
   rest. `rotj-verify` has `--pattern-from`, `--ram-from`, `--palette-from` and
   `--no-sprites` for exactly this.




2. Per-stage tables: which PRG/CHR banks, palettes and level lists a stage
   selects (`$0401`/`$0402`/`$0400` are sublevel/stage/area).
3. There is **no air control**: in the captures the actor's horizontal position
   does not change while airborne (`$00AC` frozen across a running jump).

## Player-movement shortcuts closed (2026-10-14)

Three behaviours in `crates/app/src/player.rs` were ported from fitted captures
behind `shortcut:` notes; each is now tied to the game's own code and a capture.

### The walk ramp's six-frame timer is `$047C`

The step on `$9B/$9C` (computed from `$046C/$046D + $0467/$0468`) is applied by
the routine at bank 14 offset `$0B39` (runtime `$CB39`). It reads the walk timer
`$A6` (set from `$047C` at `$801A`), and only when `$A6 >= 6` does it compute
`step = -((velocity - target) >> 1)` (`$CB4A`-`$CB66`) and reset the timer with
`STY $A6`. The target is `0x2200` (= 34, 1/16 px) from the table at `$84A0`,
indexed by `($F6 & $0470 & 3) >> 1`; the step is therefore
`velocity += (target - velocity + 1) >> 1` **in whole 1/16 px units**, so the
halving rounds on 1/16 px, not on this port's finer 1/4096 px.

Evidence: `tools/emu/player-walktrace2` (`WATCH` on `$A3` and `$047C`).
`ram-writes.csv` shows the `$A3` step write at `$CB61` on frames 694, 700, 706,
712, 718, 724 — the only frames it is non-zero — the `$047C` writes at
`$839C`/`$83BD` cycling 1..6 with the step on the sixth, and `$A0/$A1` read as
`0x2200`. So the first step lands six moving frames after the walk starts, not
on the first. `player::tests::walk_ramp_steps_on_the_six_frame_timer` pins the
captured `0,0,0,0,0,0,17,26,30` series.

### The airborne y truncates the displacement from the ground line

The capture's y (`$00B1`) is the ground line plus the *displacement* truncated
toward zero, and the displacement is the y velocity masked to a multiple of 256
(its 1/16 px unit): `$0462/$0463` steps by `-11264 = -11009 & $FF00` on the
takeoff frame, not by the full `-11009`. For the standstill jump in
`reference/physics-700-830.csv` that is
`y = 192 + trunc(Σ($9D/$9E & $FF00) / 4096)`, which reproduces all 22 captured
airborne rows exactly (`190, 186, 183, ..., 171, 170, 169, ..., 190`).
Truncating the absolute position instead reads one pixel lower from frame 810 on
(172 where the capture says 171). Evidence: the fixture's `$9D/$9E` and `$00B1`
columns, and `tools/emu/phys/ramseq-600.bin`, where `$0462/$0463` steps by
`-11264 = -11009 & $FF00` on the takeoff frame. (The position update disassembled
at `$80B1` adds the whole `$9D/$9E`; the captured step is masked, so the port
reproduces the captured step rather than trusting that listing.) The port now
integrates the masked velocity, keeps `ground_y`, and draws
`ground + trunc(displacement/4096)`;
`replaying_the_capture_matches_frame_for_frame` asserts the whole arc (velocity
and pixel y) exactly, with no allowance.

The same capture exposed a companion bug: the port landed the jump five frames
early. The floor rule then used the old derived tile set; since 2026-10-14 the
floor is the game's own metatile class answer (`rotj_core::collision`): the
ground body (metatile row 12, y 192-207) is class 14, which blocks the
head/ground probe (`$C93B`) but is not a wall, so the line is the top of that
metatile (192) and the jump lasts the capture's 22 airborne frames. The
ceiling/wall probes and the drawn position use the position accumulator, not the
drawn pixel, which is what the game tests.

### Variable jump height: the held-A thrust

The extra lift is the held-A thrust at bank 4 `$8B99` (`LDA $046A; SEC; SBC #$16;
STA $046A`). The whole gate is now decoded and the port implements it, so the
jump replays frame for frame for every hold length.

**Where it lives.** The handler is bank 4 in the `$8000` window, offsets
`$0B78-$0BB2` (runtime `$8B78-$8BB2`). The player's state byte `$046E` selects it
from the indirect table at bank 4 `$96C3` (`$046E = 3` -> `$8B78`). The 9-byte
thrust sequence and the 8-byte gate prefix (`EE 7D 04 AD 7D 04 C9 04`) occur
nowhere else in the 128 KiB PRG, and `tools/emu/player-slide-trace`'s RAM-write
trace records the PCs `$8B92`, `$8BA2` and `$8BAD` executing — so bank 4 is the
bank mapped at `$8000` when the handler runs.

**The gate** (bank 4 `$8B78`, disassembled with `disasm/gen/read_range.py 4 0xB78`):

```
$8B80 LDA $047F ; AND #$20 ; BNE $8BBC   ; skip once the window has closed
$8B87 BIT $F7   ; BPL $8BBC              ; act only when $F7 bit 7 is set
$8B8B BIT $F6   ; BPL $8BAA             ; A not held -> the $8BAA arm
$8B8F INC $047D ; LDA $047D ; CMP #$04 ; BCS $8BAA
$8B99 LDA $046A ; SEC ; SBC #$16 ; STA $046A ; BCS $8BBC ; DEC $0469
$8BAA INC $047D ; LDA $047D ; CMP #$02 ; BCC $8B99
$8BB4 LDA $047F ; ORA #$20 ; STA $047F   ; close the window
```

- `$046A` (high byte) with `$0469` (low) is the 16-bit y velocity the port keeps
  as `vel.y`; the thrust subtracts `$16` from the high byte, i.e. `22` in the
  game's 1/16 px unit = `5632` in the raw 1/4096 counts the port stores.
- `$047D` is a counter: this handler increments it on each eligible frame, and
  the airborne animation arm (`$8A86`) also increments it. It resets to 0 on
  takeoff (bank 4 `$8A0A`) and on landing (`$83FD`).
- **A held** thrusts while `$047D < 4`, so at most **three** thrusts.
- **A released** does not end the thrust immediately: the `$8BAA` arm still
  thrusts while `$047D < 2`, so the first eligible frame always thrusts (that is
  a one-frame tap's `-15105`), and the window closes on the next one (`$047F`
  bit 5).
- `$F7` bit 7 flips every vblank (bank 15 `$E243`: `LDA $F7; EOR #$80; STA $F7`),
  so the handler only acts every other frame — that is why the thrusts in
  `player-slide-trace` land on 704, 706 and 708.

**Captured effects** (`INPUT=holdA`, `HOLD_FROM=700`, `HOLD_LEN` = A held that
many frames; new captures `tools/emu/jump-hold*`; rise is `$00B1` 192 minus its
minimum):

| hold | thrusts | rise |
| --- | --- | --- |
| 1, 2, 4 | 1 | 23 px |
| 5, 6 | 2 | 37 px |
| 7, 8, 16, 30 | 3 | 54 px |

(The count is set by whether A is still held on the 2nd and 3rd eligible frames;
the first always thrusts. Parity matters: pressing A one frame earlier shifts the
eligible frames, giving a 22 px rather than 23 px tap — `tools/emu/jump-phase*` —
so `$F7` must be seeded in a replay.)

The port now models the gate (`player::Player::thrust_state`, `thrust_over`,
`thrust_parity`) and replays the captures exactly: `player.rs`
`held_a_jump_reproduces_each_capture` checks holds 1, 5 and 8 through the whole
arc — takeoff, the thrust frames and the landing frame — frame for frame (raw
velocity `$0469:$046A` and pixel y `$00B1`, no tolerance) against
`assets/levels/stage-1-1/reference/jump-hold-lengths.csv`; the existing
standstill-jump replay still passes.

One port fix came with it: the ceiling probe was fitted at 48 px (the sprite
height), and the port's *derived* collision set contained a full-width backdrop
band at y 72-94, so the probe stopped the 54 px jump at ~49 px. That workaround
is gone: the game's own rule (`rotj_core::collision`) marks the backdrop
(metatile class 10) clear, so the ceiling probe is back to the sprite height
(`HEADROOM = 48`). The horizontal probe is 10 px (bank 4 adds `$A0` to the
probed x, `$8153`).

**The apex low byte was the sign-extension.** The gravity routine also settles
the old open question: `$8311` adds 6 to `$046A`, carries into `$0469`, then
forces `$0469` to `$00`/`$FF` (its sign byte). At the apex crossing (`$046A`
`$FA` -> `$00`) that turns the 16-bit value from `-1281` to `0`, not to the
`255` a plain `+1536` add gives. The port now runs `gravity_step`, which adds 6
to the 1/16 px byte, clamps it to `[-128, 127]` (`$8329`/`$8337`) and rewrites
its sign byte, so the raw velocity matches the capture exactly — including the
crossing and the landing frame. The old `FALL_CAP = 15360` (`60 * 256`) was a
standstill jump's landing velocity mistaken for a terminal cap; it is gone, and
the port now descends freely to the same landing frame as every held jump.

### Down+A on flat ground is just the jump

The manual calls Down+A a slide, but on flat ground it is the same held jump as
A. New captures `tools/emu/jump-downA-1|4|8|30` (A+Down held 1, 4, 8 and 30
frames from frame 700) are byte-for-byte identical to the A-only captures in
`$046A`/`$0469` and `$00B1` over the whole arc, and the x velocity stays zero. A
alone already triggers the jump, so the port needs no extra Down binding; the
ground crouch pose is simply overridden by the airborne pose on the press frame.




