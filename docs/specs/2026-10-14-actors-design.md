# The actor system — design

Status: reviewed 2026-10-14 (APPROVE_WITH_CHANGES, by a stand-in for the owner).
The changes are folded in below; the plan is `docs/plans/2026-10-14-actors.md`.
Owner decisions already made are not reopened here: oracle-proven actors, RNG
injected in diff tests, the spawn window and record source as plain inputs,
idiomatic safe Rust, match-based dispatch unless justified, and a fan-out of one
enemy type per task.

## What the game does (read from the ROM, confirmed with the oracle)

Traced with `rotj-oracle` on the route replay (`tests/data/route-inputs.csv`,
frames 641-999, no lag frame in that span; frames 601-640 are the level load)
using the write watch and exec trace, then read in `disasm/` or with
`read_range.py`. Corrections from review were re-checked in the bytes.

**The frame.** Gameplay is one loop in bank 14 at `$D3F9`; each pass is one
logic frame:

| # | call (PC of the `JSR`) | what it does |
| --- | --- | --- |
| 1 | `$DB9D` (`$D3F9`) | HUD, first into OAM |
| 2 | `$CE7F` (`$D3FC`) | update slots 24-31 |
| 3 | bank 4 `$800B` (`$D408`) | player movement (`$0460-$0463` level position, 1/16 px) |
| 4 | `$D2F3` (`$D414`) | scroll speed `$C4-$C7` |
| 5 | bank 2 `$8528` (`$D420`) | camera (`$CC:$CD` x, `$CE:$CF` y, 1/16 px), column streaming, scroll locks, column spawns |
| 6 | bank 4 `$84AB` (`$D42C`) | player state and animation, player drawn |
| 7 | `$DC87` (`$D438`) | draw slots 24-31 |
| 8 | bank 5 `$8000` (`$D444`) | player weapons (`$0476`), the `$7BD0` projectile slots |
| 9 | `$CED5` (`$D450`) | **actors**: slots 23 down to 0, update and draw |
| 10 | `$C145` (`$D453`) | park unused OAM entries, rotate the OAM start |
| 11 | `$D2B0`, `$D6D3` | `$043E/$043F/$E3`, `$B5` (not actors) |
| 12 | `$C232` (`$D45C`) | wait for the NMI, stepping the RNG |
| 13 | `$D4D3` | screen transitions (`$BA`); Start pauses |

**Dispatch** (`$CED5`, slots 0-23): `X = $10 = slot`, `$7CC0,x` (type × 2)
selects a bank from `$CF0D` (types 1-19 bank 8, 20-46 bank 11, 47-73 bank 0,
74-100 bank 12) and a handler from `$CF72`; `$A000` is always bank 13. Stage
1-1: `$04`/`$06` -> `$823C`, `$0A` -> `$85FD`, `$22` -> `$9555`, `$24` ->
`$9823`, `$C6` -> bank 12 `$9953`. Slots 24-31 are a separate class: `$CE7F`
indexes `$CEB1`/`$CEC3` with the raw `$7CD8` byte (there `$04` -> bank 8
`$807C`), `$DC87` draws them, and their extra arrays sit at other offsets. So
`$04 -> $823C` holds only for slots 0-23.

**Handlers are scripts over a shared engine in bank 13.** `$0A` (`$85FD`):
`$A000` loads the slot into the zero-page working set `$90-$A6`; `$A79B` adds
velocity to position; a phase dispatch on `$7DD0` (table `$875B`); then `$8709`:
`$A852` projects level -> screen (`cmp #$10`: the 256 px screen; rows 15-19
set `$9A` bit 6, the bottom-anchor case) and skips draw and contact when off
screen; `$8FC4` maps bank 1 at `$A000` and draws through `$C247` (pointer table
`$8763`, index `$7E90 & $0F` capped at 6, one further in phase 3); `$A9C4`
checks contact with the player (`$94 - $B0` against `$ABB2[$A5]`, gated by
`$0472/$0473/$047A`) and falls into `$AA29`, the scan of the `$7BD0`
projectiles; `$A8E4`; `JMP $CEDE`. `$A8E4` is not a plain store: it adds
gravity (+6 to the y velocity `$9D:$9E`, clamped to -128..127, into
`$7E70/$7E80`), adds `$A2:$A3` into the x velocity `$7E30/$7E40` and snaps it
to 0 when the low byte is `$00-$06` (positive) or `$FA-$FF` (negative) whatever
the high byte, stores position and age (`$7BA0`), and sets `$7C00` to
`($7C00 & $03) | ($99 & $E0)`. Bank-8 helpers are shared between types too
(`$24` runs `$85FD-$8758`).

**Spawning has three entries**, all writing the same 12-byte record layout
(`docs/ACTORS.md` §7b; slots 0-15 also zero the motion arrays and set
`$7E80 = 6`, slots 16-31 write bytes 9-11 to `$7BD8/$7BE0/$7C30`):

- *Column spawn* (bank 2 `$87C5`). Not per frame: the scroll code adds the
  speed `$C5` into the 7-bit accumulator `$BE`, and only when it wraps (every
  8 px) and `$CD` bit 7 is in phase with the direction does it call `$87C5`
  (from `$8665` scrolling left, `$86A8` scrolling right). The cell is
  `$0440[(CC >> 4) + right] | (CC & $0F)`, the 16 px column at the leading
  edge. A cell whose bits 7-6 are `01` ORs into the scroll locks `$C2` (masked
  by `$20`); bit 7 set spawns; `AND #$3F` then clears bits 7 and 6. Cells
  `>= $80` go through `$89AF`: with no free slot it calls a RAM stub at `$0000`
  with `X = 0` (an open path).
- *Row spawn on vertical levels* (`$8779`, on `$CE`), entered from `$852F` when
  `$DD` bit 6 is set; same shape.
- *Room start* (bank 14 `$D714`, called from bank 13 `$B217/$BA06/$BAD8/$BD76`,
  single exit at `$D75E`): spawns every pending cell of one 16-cell row
  (`$0403`, or the camera's row when `$DD & $C0 = $80`). Seen at frame 607
  (slots 0-2) and after route2's frame-1150 room change (slots 24 and 0,
  `docs/ACTORS.md` §7g).

Records come from `$A000[area][room]` (`$0400`, `$0402`) in bank 3; the
schedule `$7410` and the row table `$0440` are live RAM. Freed slots keep their
bytes and a spawn rewrites only some arrays, so the next occupant can read
residue.

**Metasprites** (`$C247`): `($96)` points at `[n][tile, x, attr, y]×k[chr]`;
byte `n` is a CHR bank written to `$E0 + $98` when non-zero; pieces are drawn
last to first at `$94/$95` with `attr | $99` and `tile | $A4`; attr bit 6 flips
x (`x = -x - 8`). Normally a piece whose y wraps past the top is skipped; with
`$9A` bit 6 the y is reduced by 16 and a piece whose sum passes 255 is skipped.
A piece whose x leaves the screen is skipped after its y, attr and x bytes were
written, which leaves residue in that OAM entry. Entries come from `$FB`, next =
`$C7F2[x]` (+49 entries mod 64), count `$BD`; when the count runs out, even
frames (`$F7` bit 7 clear) set the overflow flag `$BC` and later calls draw
nothing, odd frames keep writing. `$C145` parks the rest (`y = $F4`) and adds
`$4C` to the start (`$0471`).

**RNG.** Gameplay code only reads `$F9/$FA`; the single writer in every traced
frame is the wait loop (`$C222/$C224`). Within one logic frame the RNG is two
constant bytes.

## 1. Where state lives

Actor logic is pure and goes in `rotj-core` (`crates/rotj-core/src/actor/`), next
to the decoders in `actors.rs`; the app only wires it into Bevy.

```rust
pub struct Slots { pub main: [Slot; 24], pub second: [Option<Placed>; 8] }
pub struct Slot { pub kind: u8 /* $7CC0, 0 = free */, pub body: Body, pub brain: Brain }
pub enum Brain { Unported, K0a(k0a::State) /* one variant per handler family */ }
```

`Body` holds every per-slot array the spawners or the bank-13 engine read or
write, one field each, named by meaning where it is known and by address where
it is not (`r7c30`); positions stay 16-bit 1/16 px like the game. Motion fields
exist for all 24 slots but are projected only for 0-15 (their arrays are 16
wide); no `Option`. A body survives a free (only `kind` goes to 0), so residue
behaves as in the game. `Brain` holds a family's fields that live in no `Body`
array, and the family's typed views over `Body` registers (`phase`, `timer`,
`anim`). Slots 24-31 hold only what the spawners write (`Placed { cell,
record }`); their class is designed when its first type is ported.

**Mapping to RAM is test-only.** `Slots::project` and `Slots::lift` (shared part
for `kind`/`Body`/`Placed`, one arm per family for `Brain`) sit under
`#[cfg(test)]` with the harness, so the shipped code carries no 6502 layout.

## 2. Update order

The app (a later plan) runs one `FixedUpdate` chain in the game's order: HUD
draw, slots 24-31 update, player move, camera + spawns, player animate + draw,
slots 24-31 draw, weapons, actor loop, OAM finish. Inside the actor loop:
slots 23 down to 0, update and draw interleaved per slot. The loop takes each
slot out (`mem::take`) while its handler runs, so the handler can spawn into or
read other slots without `RefCell`.

## 3. Spawning

```rust
pub struct SpawnTable { pub schedule: [u8; 256], pub records: Vec<Option<ActorSpawn>> } // initial data
pub struct Schedule { pub cells: [u8; 256], pub rows: [u8; 16] }                       // live ($7410, $0440)
pub struct View { pub left: i32, pub top: i32, pub width: i32, pub height: i32 }       // level, 1/16 px
```

`SpawnTable::from_rom(prg, area, room)` decodes through `actors.rs`; a Tiled
importer later builds the same struct, so the record source is plain data.
`spawn_column` (`$87C5`) and `spawn_row` (`$D714`) take the live `Schedule`,
the table and a cell or row; `spawn_column` returns the scroll-lock bits for
`$C2`. The camera step (a later port of `$8528`) emits "column crossed"
events; `View::column_cell(&Schedule, scrolling_right)` picks the cell from
`left + width` or `left`, so a wider view moves the spawn edge with it.

## 4. Handlers

```rust
impl k0a::State {
    pub fn step(&mut self, slot: usize, body: &mut Body, cx: &mut Cx<'_>);
}
pub struct Cx<'a> {
    pub prg: &'a [u8],                 // PRG image, or the shipped slice; data decodes from it
    pub world: &'a World,              // player, camera, view, rng [u8; 2], frame_odd
    pub slots: &'a mut Slots,          // children and targeting
    pub schedule: &'a mut Schedule,    // re-arm on despawn
    pub effects: &'a mut Effects,      // player hits, sound requests ($42)
    pub oam: &'a mut OamBuilder,
}
```

A handler touches its own slot and writes elsewhere only through `Cx`. Bank-13
routines are functions in `actor/engine.rs`, called exactly where the 6502
calls them (`integrate` = `$A79B`, `on_screen` = `$A852`, `contact` =
`$A9C4`/`$AA29`, `player_dx` = `$ABDF`, `end_frame` = `$A8E4`); the working-set
load `$A000` disappears because `Body` is mutated in place. Dispatch is one
`match` on `Brain` (plus the arms in `spawn`, `lift`, `project`): the set of
families is closed and the matches are exhaustive, so a trait object adds
nothing. A module ports a handler *family*: kinds whose handlers share code live
together (`$0A`/`$24`, `$04`/`$06`). Per-family data (metasprite and
pointer-table addresses) stays in the family module and decodes from `cx.prg`
when used.

Randomness: handlers read `world.rng`; the owner's policy plugs into one
function, `Rng::next_frame(&mut self)`, called once per frame by the app.

## 5. Oracle diff harness

**Oracle additions** (the only ones): peeks, the read twin of pokes
(`Trace::peeks: Vec<u16>`; reaching one appends `PeekEvent { frame, pc, bank,
nmis, ram, wram }`, where `nmis` counts NMIs since power-on), and the test
helpers `power_on()` (with the `ROTJ_REQUIRE_ROM` gate) and `read_inputs(path)`
moved from `tests/common` into the lib.

**Phase diff.** The harness (`actor/oracle.rs`, `#[cfg(test)]`) brackets a
routine with two peeks: the actor loop `$D450`/`$D453`; the column spawn
`$87C5` and its return addresses `$8668`/`$86AB` (bank 2); the room spawn
`$D714`/`$D75E`; each `$C247` call and the next peek. For each pair:

1. Reject the pair if `nmis` differs (an NMI ran inside the routine).
2. Lift `Slots`, `World` and the OAM state from the begin peek; assert
   `project(lift(begin)) == begin`. There is no carry between frames: every
   frame starts from the oracle's state.
3. Run the port routine; project onto a copy of the begin RAM/WRAM.
4. Compare every RAM and WRAM byte that either side changed, except the named
   scratch list `$10-$1A`, `$20-$3F`, `$90-$A6`, `$DF`, `$E9/$EA`; report the
   frame and the first differing address. OAM, `$E0-$E7`, `$FB/$BC/$BD`, sound
   requests and stage-map writes are included by construction.
5. Assert the lifted brain of every slot of the type under test is not
   `Unported`, and that `$7BD0-$7BDF` stays zero (weapons are not ported).

**Isolation.** At the start frame the harness edits the oracle's RAM: it zeroes
`$7CC0` of every slot not of the type under test and clears bit 7 of schedule
cells whose record is another type, so only owned actors run on both sides.
Interactions between types are left to one unedited stage replay per stage
once its types are all ported.

**Scenes.** So a fan-out task never has to script a route, `Scene` starts from
the route at a frame or from a forced level (the `$20` poke at `$D3D8`, as
`tools/rotj-oracle/tests/levels.rs` does) and offers `place(slot, record)`
(the spawn bytes, written through the port's own spawn and projection),
`camera(x)` and `player(x, y)`, all RAM edits between frames, the same
mechanism isolation uses.

## 6. Drawing

`Metasprite::from_rom(prg, bank, addr)` decodes the format above; `OamBuilder`
ports `$C247`/`$C145` (allocation order, count, overflow, residue, the `$0471`
rotation, the `$E0-$E7` CHR shadows). Screen positions are `i16` and clipping is
against `View`, byte-exact at width 256. The builder is proven by bracketing
every `$C247` call of route frames, plus pokes at `$C247` that force the
bottom-anchor, x-clip and overflow cases. The HUD and player move onto the
builder in the app-wiring plan.

## 7. First proof: `$0A` (the green enemy)

Its own code is the smallest of the drawn stage 1-1 types (82 distinct bank-8
instructions executed on the route, against 217 for `$22`) and covers motion, a
phase dispatch, the player's position, a 4-sprite metasprite, contact and the
spawn record path (`$ACB8`). `$06` is smaller but dormant and never drawn.
`k0a::tests::matches_the_oracle` uses the route from frame 894 (slot 3 spawns
there), isolates `$0A`, and diffs every actor phase through frame 995. At 996 it
spawns its first child, `$24`, whose handler shares `$0A`'s code: porting `$24`
into the same module is the first fan-out task.

## 8. Fan-out recipe (one handler family per task)

Touch only `crates/rotj-core/src/actor/k<id>.rs` (new: `State`, `spawn`,
`step`, addresses, and under `#[cfg(test)]` `lift`, `project` and
`matches_the_oracle` built from a `Scene`, covering spawn to despawn or 300
frames), `actor/mod.rs` (one `mod` line, one `Brain` variant, one arm per
match), and `docs/ACTORS.md` (one row). `tools/check-fanout.sh <base>` fails on
any other path. `engine.rs`, `Body`, `World`, `Cx`, `Slots`, the spawners,
`OamBuilder`, the harness and the oracle are core-owned: before each stage's
fan-out the orchestrator ports the bank-13 routines that stage's types call. A
task that needs a core change stops and reports.

## Open questions (with the default I would take)

1. **Names.** By kind byte (`k0a.rs`, `Brain::K0a`) until in-game identities
   are proven.
2. **Shipped RNG policy** (pending in STATUS). The game's LFSR (`$C21C`) stepped
   a fixed count per frame, the median measured on the route.
3. **The game's camera.** Port `$8528`'s scroll part next; it produces the
   column events. Actor tests take the camera from the oracle meanwhile.
4. **Unported kinds in play.** Spawned as inert slots (occupancy right, never
   drawn), not skipped.
5. **`$0440`'s source.** Set at level load and changed at runtime (index 3:
   `$60` -> `$00` between frames 607 and 700). Lifted from RAM in tests; its
   writer is found before the app wiring.
6. **The `$89AF` path** (cells `>= $80`, no free slot, RAM stub at `$0000`). Not
   seen on the route; the port panics there until a capture shows it.
7. **Vertical levels** (`$8779`). Ported once a level with `$DD` bit 6 set is
   found to test it in.
8. **HUD and player OAM parity.** Batman's animation table was decoded from
   captured OAM, not bank 4's metasprites; whole-page parity may need him drawn
   from the ROM. Actors are proven inside their phase meanwhile.
9. **Other RNG writers.** Byte patterns in banks 5, 6 and 10 may be data;
   checked with a write watch when those stages are ported.

## Implementation notes (deviations found while building the core)

- **Scratch list.** The stack page `$0100-$01FF` is scratch too: `JSR` return
  addresses land there inside every bracketed routine.
- **Slots 24-31.** `Slots::second` is `[Second; 8]`, `Second { kind, placed:
  Option<Placed> }`: lifted occupancy plus the port's own spawn, so a live
  second-class actor's bytes are never rewritten from a stale record.
  `Placed::by` records the spawner, because the column spawn also clears
  `$7F00,x` and the room spawn does not.
- **Column spawn API.** `$87C5` reads two cells: the scroll lock from the
  left-edge column and the spawn from the leading edge. It is ported as
  `scroll_lock(&Schedule, &View, mask, locks) -> u8` and `spawn_column(slots,
  schedule, table, view, scrolling_right)`; `View::column_cell` returns `None`
  past the 16-entry row table (the game would read `$0450+`).
- **Room spawn overflow.** With no free slot `$D714` writes slot 16 (main) or
  past the arrays (second); the port panics there, as it does on `$89AF`.
- **Vertical levels.** Forcing each of the 13 levels at `$D3D8` leaves `$DD = 0`
  at frame 700, so no level *start* exercises `$8779`; rooms reached in play
  (the stage 1-1 shaft after route2's frame 1150) are the place to look.
- **Handler signature.** `State::step(&mut self, body, cx)` and `engine::contact(body,
  at, hitbox, cx)` need no slot index: `$10` only addresses the slot's own arrays.
- **`$0A` proof scope.** On the route, frames 894-995 run only phase 0
  (`$8615`: hover at `vy = -6`, count the timer down, wake when the player is
  within 48 px), the draw, and a contact check that never reaches the player;
  that is what `k0a::tests::matches_the_oracle` proves byte for byte (mutations of
  the hover, attribute, age, frame and screen y all fail it). Teleporting the
  player next to it (`Scene::player`) shows the rest: phases 1-3 (`$864B`,
  `$86A8`, `$86C3`), sound `$34`, contact damage through `$B17F` (sound `$25`),
  and a child of type `$08` spawned through `$AC16`. Those paths panic in the
  port until ported. When off screen the game merges a stale `$99` into
  `$7C00` bits 5-7; the port keeps the slot's own bits.
- **`$24` is not `$0A`'s child.** `docs/ACTORS.md` §8b shows `$24` spawned by
  `$97FF` in bank 8, inside the `$22` handler; it shares `$85FD-$8758` with
  `$0A`, so it still belongs in `k0a.rs`, but its proof scene needs `$22` (or a
  `Scene::place` of `$24` itself).

### Part 2 (stage 1-1 engine, plan Tasks 7-12)

- **`Work`.** The zero page `$90-$A6` is `engine::Work`, carried through the actor
  loop like the game's zero page; handlers read the previous slot's values
  where the game does (the off-screen `$99` merge included). Phase diffs compare
  `$90-$A6`; the scratch list is now `$10-$1F`, `$20-$3F`, `$DF`, `$E9/$EA` and
  the stack.
- **Routine diffs.** `assert_routine` brackets one routine from its entry to its
  `RTS` exits (or its callers' return addresses), compares every changed byte,
  the routine's claimed `$20-$2F` results and its `A`/`Y` results
  (`PeekEvent::cpu`).
- **The player is live state.** `World::player` moved to `Effects::player`:
  routines read the player as earlier handlers this frame left him.
- **Collision is the game's.** `collision.rs` ports `$CAFE`/`$C99D`/`$C93B` with
  the `$7340` class table and the `$0440-$045F` page table (`Schedule::rows` is
  32 bytes); the routine diffs include the player's own calls.
- **Weapons.** Contact needed the projectile scan (`$AA57`, `$B108`): the
  player's projectiles are `Effects::weapons`.
- **Families take `kind`.** `State::step(&mut self, kind, body, work, cx)`, so a
  handler can free or collect its own slot.
- **Slots 16-23 aliasing** blocks type `$24` (`docs/ACTORS.md` §9f).
