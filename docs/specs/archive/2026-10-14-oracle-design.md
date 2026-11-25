# The 6502 oracle — design

Status: approved for implementation under the owner's delegation (2026-10-14,
"use your skills autonomously") after a reviewer-agent pass whose findings are
folded in below ("Timing is the hard part"); the owner reviews it on return and
can veto.

## Why

1.0 needs every enemy, boss and weapon ported and proven. Emulator captures only
prove what a scripted fceux run happened to show, and each new question costs a
Lua script, a capture and a byte comparison. The oracle runs the game's own code
inside the test suite, so the port can be diffed against the original from any
state, every frame, in milliseconds.

Owner decisions (2026-10-14):

- Actors are proven with a 6502 oracle, not captures alone.
- The oracle runs **whole frames**, not single routines.
- It becomes the project's main reference, not only for actors. fceux stays for
  pixels and for spot checks of the oracle itself.
- Post-1.0 goals are seams, not features: the actor port keeps the spawn-window
  width and the spawn-record source as plain inputs, and no post-1.0 code lands
  before 1.1.

Success: from power-on, the oracle reproduces fceux's CPU RAM for every frame of
`tools/emu/run1` (3000 frames of its recorded `INPUT=plan` input), and a port subsystem can be diffed
against it per frame in a normal `cargo test`.

## Timing is the hard part

The game steps its random number generator (`$F9/$FA`, `JSR $C21C`) in a
41-cycle frame-wait loop (bank 14 `$C232-$C23D`) until the NMI arrives, so the
RNG, and every actor that reads it (e.g. bank 5 `$80E9`), depends on exactly how
many CPU cycles each frame leaves over. In `run1` the RNG advances 440-680 steps
per frame. The oracle therefore matches **fceux 2.6.5 with the old PPU core**
(`SDL.NewPPU = 0`, `SDL.RamInitMethod = 0`) cycle for cycle: frame length, NMI
delay, per-instruction interrupt polling, OAM DMA cost, DMC DMA stalls, and the
FME-7 IRQ counter as fceux implements it. Where fceux and real hardware differ,
fceux decides (it is what the captures came from); fceux's source is read as a
reference and reimplemented, never copied.

Things the game provably does not use are left out: sprite-0 hit (`$2002` bit 6
is never read), `$4015`/`$4017` reads, indexed or read-modify-write access to
PPU/IO registers. An unexpected I/O read panics with its PC.

Live features the oracle must have: the FME-7 IRQ (armed in the intro, frames
60-95; handler `$E2A9`, chain at bank 2 `$95C9`), DMC sample fetches (run1 frame
801 plays a 1009-byte sample at `$E500`), the NMI-enable edge (`$2000` bit 7
set while vblank is set raises an NMI; the game does it at bank 14 `$D3B3` and at
the end of the level loader), `$4016` read twice per poll and compared.

**Port RNG.** The port cannot reproduce a cycle-driven RNG. Diff tests inject
the oracle's `$F9/$FA` into the port each frame; what the shipped port does for
randomness is an owner decision (`docs/STATUS.md`).

## Non-goals

Rendering pixels (the port's renderer already matches fceux), audio synthesis,
mappers other than FME-7, unofficial opcodes (none executed in 7500 traced frames
including stage-1 play).

## Architecture

One dev-only crate, `tools/rotj-oracle` (`publish = false`, like
`rotj-verify`). It has no workspace dependencies, so any crate can
dev-depend on it without a cycle; its tests dev-depend on `rotj-core`. The
modules are private, and the crate root re-exports the public API.

| unit | job |
| --- | --- |
| `cpu` | 2A03 CPU: official opcodes, no decimal mode, cycle counts with page-cross and branch penalties, NMI/IRQ/reset entries. Generic over a crate-private `Bus` trait; the public `Cpu` is just its registers and cycle count. |
| `bus` | `NesBus`, the CPU address space: 2 KiB RAM (mirrored), PPU registers, controller 1, APU writes (logged), the DMC's fetch timing (`Dmc`, no synthesis), FME-7, WRAM, PRG windows, and the `Trace` debug hooks. A read the game is not known to make panics with its PC. |
| `fme7` | command/parameter registers, the `$6000` PRG/WRAM select, PRG windows, mirroring, and fceux's IRQ counter (`Fme7::default()` starts it at `$FFFF`). |
| `ppu` | register-level PPU without rendering: `$2000-$2007` semantics, nametable RAM with FME-7 mirroring, palette RAM, CHR reads through the CHR banks, OAM and `$4014` DMA, vblank flag and the delayed NMI on a `$2000` bit-7 rise. |
| `cart` | the iNES image (`Cart`, shared between snapshots through an `Arc`), `load_rom()`, `rom_path()` |
| `machine` | `Machine { cpu, bus, frame }`: `power_on(cart)`, `run_frame(pad) -> FrameLog`. Snapshots are `clone()`, compared with `==`. |

`FrameLog` holds that frame's APU writes, each with the PC of the *writing*
instruction (unlike fceux, which logs the next one), the frame's cycle count,
and how many NMIs it took.

The ROM is read from `raw/` (git-ignored). Tests that need it print a skip notice
and pass when it is absent, unless `ROTJ_REQUIRE_ROM` is set; then they fail. CI could build the ROM from the committed `disasm/`
(byte-identical with ca65/ld65; the CHR is already committed as
`pattern-space.bin`); that is an owner decision.

Power-on RAM: fceux's `RamInitMethod = 0` pattern, `00 00 00 00 FF FF FF FF`
repeating (`run1` frame 0). WRAM's power-on content is not captured.

## Validation of the oracle itself

1. **Boot to attract** (run1 turned out to use `INPUT=plan`, see below): RAM after each frame equals
   `tools/emu/run1/ramseq-0.bin` for frames 0..3000. Report the first divergent
   frame and byte while it is not yet exact. Frame alignment (fceux's frame
   counter vs NMI count) is measured, not assumed.
2. **With input:** `dump.lua` gains per-frame input logging (`inputs.csv`) and
   records `INPUT`/`ROUTE_*` in `config.txt`. A gameplay capture (`INPUT=route`,
   without `ROUTE_INVIN` or any RAM poke) is replayed from power-on; its RAM and
   OAM equal the dumps, and so do its APU writes. WRAM dumps are reads through
   the CPU window, so they compare only while command 8 maps WRAM (`$C0`).
3. **IRQ and DMC:** `run1` frames 60-95 (`$08/$09 = $96A1` from frame 61) and
   801-840 (the DMC sample) are where timing goes wrong first if those models
   are off; tests pin them.

The old reference `assets/audio/reference/apu-writes-600-1200.csv` came from a
capture whose input was not recorded, so it is not replayed.

These become committed tests over small slices (the 2 KiB-per-frame sequences
are committed already for `run1`).

## Validation results (run1)

All 3000 frames of `run1` match byte for byte
(`tools/rotj-oracle/tests/run1.rs`). The capture's APU writes match too: all
14 786 writes in frames 1-3000, excluding `$4014`/`$4016`, which `FrameLog::apu`
leaves out (`apu_writes_match_the_capture`).

**run1 had input.** It was recorded with `dump.lua`'s `INPUT=plan`, not with no
input: Start is held for 3 frames out of every 20 from frame 100 to 900, then
Right is held with A on `frame % 60 < 3` and B on `frame % 137 < 3`. The
evidence is that a fresh no-input fceux run and the oracle agree with each other
on every byte through frame 110, while both leave `run1` at frame 101. That is
where `$F5/$F6` (the game's new/held pad bytes) show Start (`$10`) in `run1`.

**Frame alignment.** fceux's first `emu.frameadvance()` after loading runs no
frame, so `ramseq-0.bin` entry N is the RAM after N - 1 frames. Entries 0 and 1
are both power-on RAM. The frame numbers in `dump.lua` logs (`emu.framecount()`)
equal the oracle's 1-based `run_frame` count. The pad for oracle frame N is
`input_for(N)`.

**Gameplay with recorded input.** `tools/emu/oracle-route` is the route-dense
capture re-run without `ROUTE_INVIN`, so nothing pokes RAM. `dump.lua` now writes
`inputs.csv` and the full mode settings to `config.txt`. The command was
`INPUT=route ROUTE_JUMPS=<735..760,792..817 listed frame by frame> DUMPS=600,900
RAMSEQ=600,1000 UPTIME=1001`, with fceux run as in `dump.lua`'s header.
Replaying its inputs from power-on matches the frame-900 RAM and OAM (OAM taken
as the page the last `$4014` copied) and all 3443 APU writes in frames 600-1000
(`tests/route.rs`, slices in `tests/data/`).

**Power-on.** RAM is `00 00 00 00 FF FF FF FF` repeated (`RamInitMethod = 0`).
WRAM, nametables, palette and OAM start at zero. A=X=Y=0, S=`$FD`, P has only I
set. Reset costs no cycles, and the FME-7 IRQ counter starts at `$FFFF`,
disarmed.

**Timing, each matching fceux 2.6.5's old PPU (`ppu.cpp` `FCEUPPU_Loop`,
`x6502.cpp` `X6502_Run`):**

| what | oracle (`machine.rs` unless named) | fceux |
| --- | --- | --- |
| CPU budget | `count` in 1/16 dots; an instruction starts while it is positive and its overshoot carries into the next budget | `_count`: `X6502_Run(dots)` adds `dots*16`, each cycle subtracts 48 |
| dead frames | `DEAD_FRAMES = 2` frames of 262 lines with no vblank after power-on | `ppudead = 2` |
| frame layout | line 240 (341 dots), vblank flag and OAM address reset, 12 dots (`NMI_DELAY_DOTS`), NMI if `$2000` bit 7, `20*341-12` dots, flags cleared, `341 - short_frame` dots, 240 lines | same sequence; fceux splits some of these budgets further (`256+69`/`16-kook`, and each line into 256/6/63/16), and running a split as one budget is equivalent unless two interrupts coincide at fceux's split points |
| frame length | 89 342 / 89 341 dots alternating (`short_frame`), whether or not rendering is on | `kook` |
| frame end | end of line 239 | `FCEUPPU_Loop` returns there |
| NMI on `$2000` bit 7 rising during vblank | taken after one more instruction (`Ppu::nmi_pending` then `nmi_delayed`) | `TriggerNMI2` |
| NMI never cancelled | clearing `$2000` bit 7 does not drop a pending NMI | `_IRQlow` keeps `FCEU_IQNMI` |
| interrupt poll | at instruction start; IRQ uses P from before the previous instruction (the CLI/SEI/PLP delay); RTI updates it at once; BRK sets its I | `_PI` |
| interrupt cost | 7 cycles, counted into the next hook | `ADDCYC(7)` |
| mapper and DMC hooks | before each instruction executes, with its base cycles plus everything not yet hooked (previous page-cross or branch cycles, interrupts, DMA, DMC stalls) | `MapIRQHook(temp)`, `FCEU_SoundCPUHook(temp)` |
| FME-7 IRQ | while command D is nonzero, the counter drops by the hooked cycles; at <= 0 it raises the IRQ, disarms and reloads `$FFFF` (`fme7.rs`) | `M69IRQHook` |
| OAM DMA | 512 cycles, no odd-cycle alignment, bytes through `$2004` (`bus.rs` `OAM_DMA_CYCLES`) | `B4014`: 256 `X6502_DMR` + 256 `X6502_DMW` |
| DMC fetch | at a hook, when bytes remain and the buffer is empty: 4 cycles (`DMC_FETCH_CYCLES`); the buffer empties when the bit counter wraps; timer starts at 1, period from the NTSC table | `DMCDMA`, `DMCacc`, `tester` |
| `$2002` | status bits 5-7, low bits from the PPU bus latch; the read clears vblank and the toggle | `A2002`, `PPUGenLatch` |
| `$6000` | `$C0` WRAM, otherwise ROM bank; `$40` (open bus in fceux) panics like any unmapped read | `M69WRAMRead`, `Sync` |

These cases are left out because fceux behaves differently from hardware here
and the game never relies on them:

- odd-frame dot skip tied to rendering
- OAM DMA alignment
- sprite-0 hit
- the APU frame IRQ: the game writes `$4017 = $C0` before its first `CLI`

**Fixes found by validation.** Before the fceux scheduling, the oracle used
idealised hardware timing: vblank-to-vblank frames, 513/514-cycle OAM DMA, the
FME-7 counting only with bit 7 and firing on underflow, and the DMC on hardware
timing. It drifted from frame 2 onwards. With the scheduling above the only
divergence was frame 101, which turned out to be run1's input. The CPU core
needed no change.

**Cycle counts.** The oracle's `cpu.cycles` runs about 3 cycles behind
`debugger.getcyclescount()` (NMI at 2 948 399 against 2 948 402 at frame 100).
fceux's Lua hook reads the counter at a different point; RAM is unaffected.

## Debug API (reverse engineering without fceux)

`NesBus::trace` (`bus::Trace`) is empty and costs nothing by default. It has
these fields:

- **`watch`**: CPU address ranges. Each write to one of them is logged in
  `writes` as a `WriteEvent`: the frame, the address and value, the PC of the
  writing instruction, and the PRG bank mapped at that PC (`None` when the code
  runs from RAM or WRAM).
- **`exec`**: PC ranges. Each instruction run there is logged in `execs` as an
  `ExecEvent` (frame, PC, bank).
- **`pokes`**: `(pc, addr, value)` entries. The machine writes `value` to
  `addr` each time execution reaches `pc`, before that instruction runs. This is
  fceux's `registerexec` plus `writebyte`.

Frames are numbered from 1, like `emu.framecount()` in a capture. RAM and WRAM
can also be poked between frames through the public fields `NesBus::ram` and
`NesBus::wram`. `tests/levels.rs` is a worked example: it forces the level
index and finds which actor rewrites the map.

## How the port uses it

A test builds the oracle state (power-on plus an input sequence, or a snapshot
taken from it), sets the port subsystem up from the same RAM, steps both, and
compares a projection of the port's state onto the game's RAM layout (for actors,
the slot arrays `$7B00-$7FFF`; for the player, `$0460-$047F` and `$94-$B1`). The
projection lives in test code next to the subsystem. The port keeps idiomatic
types; nothing in the game crate depends on the oracle.

First consumers, in order:

1. `rotj-music`: run the whole capture from power-on so the driver's pre-capture
   RAM (the three frame-802 period bytes, the frame-905 reload) is known. Target
   3468/3468.
2. The actor-vs-player collision lead (bank 13 `$0A29`), as the actor proof.
3. The actor fan-out: one enemy per agent task, each with an oracle diff test.

## Agents and work order

The oracle crate is built first (Claude agents; it is foundation code). Once
validation (1) passes, DeepSeek agents own whole enemies end to end against the
oracle, and Claude reviews. Releases stay stage by stage (every tag playable);
the actor handlers are ported system-wide, then stages are assembled from data.

## Risks

- **Cycle exactness may take long.** If matching fceux to the cycle stalls,
  the fallback is an RNG-synced mode: at each NMI the oracle's `$F9/$FA` are
  set from a reference, and validation compares everything else. That keeps the
  oracle useful for port diffs (which inject the RNG anyway) while exactness is
  finished.
- **Scope creep into an emulator.** No rendering, no audio, one mapper. If a
  check needs pixels, it uses fceux.

## Open (owner)

- Commit a PRG image (or build one from `disasm/` in CI) so oracle tests run in
  CI. The repo already carries the full CHR and the PRG as `disasm/*.s`; the
  `.gitignore` states ROM data is not redistributable.
