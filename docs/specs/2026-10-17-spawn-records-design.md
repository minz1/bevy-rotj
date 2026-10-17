# Levels as assets, slice 2: the spawn records (1.1.x step B1b)

Status: approved with changes by the Opus stand-in, 2026-10-17 (applied: both slot paths'
destinations; the duplicate `$7C30` write, `Placed`, `SpawnTable::schedule`,
`PendingSpawn`/`pending_spawns` deleted; the cell bits defined once in `actors.rs` with a
`CELL_` prefix and every site listed; "kind" made to mean one byte; complementary split
tests). Second slice of step B1 (`docs/specs/2026-10-17-levels-as-assets-design.md`, "Next
slices" 1), from a survey of `actor/spawn.rs`, `actors.rs`, `slots.rs` and their callers.

## Goal

A room's spawn records are decoded from the ROM (`SpawnTable::from_rom`) on every room
start and every scroll crossing that finds a pending cell. Each record is `ActorSpawn {
addr, raw: [u8; 12] }`, and the code that places an actor reads it by byte index (`r[4] &
0xE3`, `u16::from_be_bytes([r[9], r[10]])`) in two places (`Body::spawn` for slots 0-15,
`spawn::place` for slots 24-31); the schedule cell bits are bare masks (`& 0x80`, `& 0xC0 ==
0x40`, `& 0x3F`, re-arm values `0x80`/`0xC0`). This slice types the record and names the
cell bits, so a spawn is something an editor can author: where the actor starts, its slot
class and type, its state, behaviour, subtype, health and contact damage. Behaviour, RAM,
WRAM and the tapes unchanged.

## Design

- `actors.rs`: `ActorSpawn` becomes `SpawnRecord`, with named fields and no ROM address,
  `#[derive(Default)]` for fixtures:

  | field | type | record bytes | slots 0-15 (`Body::spawn`) | slot 24+i (`spawn::place`) |
  | --- | --- | --- | --- | --- |
  | `x` | `u16` | 0-1, big-endian | `$7B00`/`$7B20` | the same, at 24+i |
  | `y` | `u16` | 2-3, big-endian | `$7B40`/`$7B60` | the same |
  | `flags` | `u8` | 4 `& $E3` | `$7C00` | the same |
  | `chr_slot` | `u8` | 4 bits 2-4 | `$7C90` | the same |
  | `second_class` | `bool` | 5 bit 7 | picks slots 24-31 | |
  | `kind` | `u8` | 5 `& $7F`, shifted left | `$7CC0` | the same |
  | `state` | `u8` | 6 | `$7D50` | the same |
  | `behaviour` | `u8` | 7 | `$7D90` | the same |
  | `subtype` | `u8` | 8 | `$7DB0` | the same |
  | `health` | `u16` | 9-10, big-endian | `$7EF0`/`$7BE0` | byte 9 at `$7BD8+24+i` (slot 16+i's `$7BE0`), byte 10 at its own `$7BE0` |
  | `contact` | `u8` | 11 | `$7EE0` | `$7C30` (its `Body::sprite`) |

  `health` and `contact` hold their meaning for slots 0-15 only; the field docs say so, and
  `spawn::place` keeps writing the second-class path's addresses as today. Byte 4 splits
  losslessly (`$E3` and bits 2-4 partition it), and so does byte 5 (the shift keeps all
  seven bits in a `u8`). `impl From<[u8; SPAWN_RECORD_BYTES]> for SpawnRecord` is the one
  decoder; `actor_spawn(prg, addr)` becomes `spawn_record(prg, addr)`, beside
  `spawn_record_table` and `spawn_record_pointer`. The accessors (`state()`,
  `behaviour()`, `kind()`, `slot_bank()`, `slot_type()`) and `SPAWN_BEHAVIOUR_INDEX`/
  `SPAWN_TYPE_INDEX` go.
- "Kind" means `$7CC0` everywhere in `actors.rs`: `LiveSlot.kind` and `SLOT_KIND` (which
  mean `$7DB0`) become `subtype` and `SLOT_SUBTYPE`. The intra-doc links to `ActorSpawn`,
  `ActorSpawn::slot_type` and `ActorSpawn::state`, and `docs/ACTORS.md`'s mention of
  `slot_bank`/`slot_type`, are corrected.
- `Body::spawn` and `spawn::place` read the fields by name. `place`'s first `(0x7C30, 0)`
  write goes: `set_byte` is a plain store, so the second write alone leaves the same state.
- Deleted, as nothing reads them: `Placed` and `Second::placed` (written once, lifted as
  `None`; `Spawner` stays, it gates the `$7F00` write); `SpawnTable::schedule` (the level
  load reads `spawn_schedule` itself; the schedule belongs to the area, the records to the
  room, so it has no place in a per-room asset); `PendingSpawn` and `pending_spawns`, which
  read schedule cells 0-15 and 24-31 as if they were slots, though cells are `rows[page] |
  column` across all 256. The tool (`rotj-tools` `actors`) walks all 256 cells of
  `spawn_schedule`, printing each pending cell's id, `spawn_record_pointer`'s address and
  the record's fields by name.
- `SpawnTable` becomes the room's records only, and `SpawnTable::from_rom` becomes
  `SpawnTable::import`, as `Terrain::import`; the callers (`scroll::edge_spawn`,
  `room::spawn_first_row`) keep importing on use.
- The schedule cell bits, defined once in `actors.rs` (which does not depend on `actor`),
  replacing `SPAWN_PENDING`: `CELL_PENDING = $80`, `CELL_LOCK = $40` under `CELL_FLAGS =
  $C0` (`value & CELL_FLAGS == CELL_LOCK`), `CELL_RECORD = $3F` (the record id; `&=
  CELL_RECORD` clears both flags). `spawn.rs`'s `IDS` becomes `CELL_RECORD + 1`. Used at:
  `spawn.rs` (`lock_at`, `spawn_at`, `spawn_row`, `SpawnTable::record`), `scroll::
  edge_spawn`, `Scene::isolate`, `engine.rs`'s assertion near line 1699, and the
  `engine::despawn` re-arm values (12 literal `0x80` call sites and the four local `REARM`
  consts: `0x80` in `hover_shooter` and `hazard_dropper`, `0xC0` in `next_area_gate` and
  `top_screen_room_exit`), which become `CELL_PENDING` and `CELL_PENDING | CELL_LOCK`, the
  four consts deleted. The schedule stays bytes: the runtime one is WRAM the game rewrites;
  an authored schedule is typed with the authored format.

## Tests

- `spawn_record_fields`: `SpawnRecord::from` of the synthetic record (the rewritten
  `actor_spawn_exposes_state_behaviour_and_type`) gives each field; byte 4 = `$E3` gives
  flags `$E3`, chr 0, and `$1C` flags 0, chr 7; byte 5 = `$80` gives second-class, kind 0,
  and `$7F` first-class, kind `$FE`.
- Rewritten for the new names: `route_slots_match_their_spawn_records` (`slot.subtype` vs
  `record.subtype`, `slot.occupied` vs `record.kind`), `occupied_byte_matches_the_record_
  and_schedule`, `scenes_force_a_level_and_edit_ram`, the families' fixtures
  (`bobber_spawner::place_record` from its bytes, `bomber_bomb::bomb` as a struct literal
  with `..Default::default()`); `pending_spawns_resolve_through_the_id_table` keeps its
  pointer assertions on `spawn_record_pointer`. `room_spawn_matches_the_oracle` checks
  `spawn_schedule(..)` against WRAM `$7410-$750F`.
- The Classic suites, a 1-seed sweep and a 300-branch exploration; the tapes unchanged.

## Plan

1. Tests first: `spawn_record_fields` against `SpawnRecord::from` (fails to compile).
2. Add `SpawnRecord`, convert `spawn_record`, `Body::spawn`, `spawn::place`, the oracle
   helpers and the fixtures; the `subtype` renames; delete the accessors, the index
   consts, `Placed`, `SpawnTable::schedule`, `PendingSpawn`/`pending_spawns`; the tool.
3. The cell constants at every listed site; `from_rom` → `import`.
4. `cargo test -p rotj-core`, the release `--ignored` suites, sweep, explore, clippy; the
   `reviewer` agent; commit `Levels as assets: the spawn records` (the owner's standing
   go-ahead for this run covers commits once every check passes).

## Not here

The room record, start record, raster parameters and page order (slice 3); collision
properties; where the assets live; typing the schedule as cells.
