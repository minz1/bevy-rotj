# Level packs, release 3: the game reads its levels through `Levels`

Release 3 of `docs/specs/2026-10-18-level-packs-design.md`: "the game loads levels through the
`Levels` seam (the original game via the pack path); tapes unchanged". Release 1 built the
importer and exporter, and release 2 the stage knobs (1.1.43). The game still reads the ROM.
After this release every level read at runtime goes through one `Levels` value. By default that
value is the pack plus a ROM overlay of the 225 cells and one room mirroring the format drops, so release 6's
`--pack <dir>` only has to swap it. Nothing the game does changes, and the tapes and the lockstep
stay as they are.

## The seam

`Prg` gains `pub levels: Levels`, and that field replaces `Prg::stages`. Every runtime reader
already holds `&Prg` (`Cx::prg`, `Game::frame(prg, chr)`), so `Game::frame` and `Cx` don't
change. A separate `&Levels` parameter would touch the same functions plus every caller of
`frame`. A `stages` copy kept beside `levels` would be a second source of truth once release 6
assigns a pack.

- `Prg::new(bytes: Vec<u8>, levels: Levels) -> Prg`. It doesn't import anything, so callers that
  already hold a `Levels` don't pay for it twice.
- `$0400 >> 1` indexes `levels.stages` and `$0402 >> 1` indexes `stage.rooms`: both ids are
  doubled, `listed_rooms` steps by 2, and `Room::import` takes the doubled id. The ROM tables
  that stay keyed by area (below) keep using `$0400`. In the original pack
  `stage.area == $0400 >> 1`, so this matches today. Release 6 decides which of those tables
  should follow `stage.area` instead.

### The read sites

| site | today | through `Levels` |
|---|---|---|
| `actor/room.rs` `load` (`$B40A` room load) | `Room::import(prg, area, room)`; `spawn_schedule(prg, area)` into `schedule.cells` | `stage.rooms[$0402 >> 1].room`; `stage.schedule` |
| `actor/room.rs` `parameters` (`$D992`) | `Room::import` | `stage.rooms[$0402 >> 1].room` |
| `actor/room.rs` `spawn_first_row` (`$D714`) | `SpawnTable::import` | `stage.rooms[$0402 >> 1].spawns` |
| `actor/scroll.rs` `edge_spawn` | `SpawnTable::import` | `stage.rooms[$0402 >> 1].spawns` |
| `actor/room.rs` `load_level` | `collision::Level::loaded(prg, chr, area, image)`, which calls `Terrain::import` | `Level::from_terrain(&stage.terrain, image)` |
| `actor/room.rs` `start`; `actor/boot.rs` `StageStart` | `stage::level_area(&prg.stages, level)` | `stage::level_area(&prg.levels, level)` |
| `actor/stage.rs` (picture, Continue, music, banner); `actor/pause.rs` (password, banner); `families/hurtful_burst.rs`; `families/boss_defeated_script.rs` | `stage::at` / `stage::checkpoint` on `&prg.stages` | the same helpers on `&prg.levels` |
| `actor/boot.rs` `check_password` (`$6550`) | the 14 places at payload `$6E6C` (CHR), `place.min(12)` | the checkpoints' passwords, then `extra_passwords` |

`stage::at`, `checkpoint` and `level_area` take `&Levels` instead of `&[Stage]` and read
`stages[i].stage.descriptor`. Callers that hold only bytes are all tests, and they switch to
`prg::for_test()`.

**Passwords.** `check_password` compares letter indexes 0-25 (`self.entry.letters`). The
descriptor's `password` and `ExtraPassword::letters` hold ASCII. The lookup converts the pack
side: `letter - b'A'`, as the boot test already does. A checkpoint's password gives that
checkpoint's rank (level). An extra password gives the rank of its stage among the checkpoints,
which is what `min(12)` meant: the extra password starts the 13th checkpoint's stage. The search
order doesn't matter, because the importer refuses duplicate passwords, so `(0..14).rev()` isn't
kept. For the original pack every entry gives the level it gives today: places 0-12 are the 13
checkpoints' passwords (`the_password_screen_holds_the_checkpoints_passwords_and_one_more`), and
place 13 is the extra password.

### What stays a ROM read

These are behaviour, art and boot data, not level content:

- The behaviour tables `Prg` already imports: hit boxes, families, player, weapons and digit
  tiles.
- Palette presets, which a room names by index (`build_palette`).
- `enter_area`'s sprite palette entries, `rom_byte(prg, 13, 0xB26A + i)`, read through
  `&prg[..]`.
- `animate_banks`, metasprites, CHR art, music and the sound driver, and the cutscene and
  stage-end scripts.
- The boot and cutscene payloads that seed WRAM (`levelmap::boot_image`, `payload_image`),
  including the payload's copy of the passwords at `$6E6C`, which is WRAM the lockstep compares.
- Area-keyed tables and constants: `hurtful_burst`'s `area == 0x0A`, `FINAL_AREA` (`0x22`) in
  `actor/stage.rs` and `actor/modern.rs`, and the stage-end script's boss tables read through
  `stage_args` (`$040E-$040F`).
- Test-only reads: the tests in `oracle.rs`, `spawn.rs`, `drift_shot.rs`, `final_boss_part.rs`
  and `drift_wave_spawner.rs`, and `rotj-tools`.

`tools/rotj-oracle` needs no change. Its `inventory_tas` example reads raw PRG bytes and never
builds a `Prg`.

## The default: `Levels::original`

`Levels::original(prg: &[u8], chr: &[u8]) -> Option<Levels>` builds `Levels::from_rom` once and
uses it for both `Catalog::from_levels` and `write`, which is what `export` does internally.
`export` plus `Catalog::from_rom` would build it twice. It then runs
`import(.., Source::Original)` and overlays the ROM schedule:
`schedule[c] |= rom.stages[i].schedule[c]` for every cell. `import` and `Source` don't change,
and `import` still has no ROM access. It returns `None` where the ROM's levels don't import, as
`from_rom` does.

The original game's schedule is therefore the ROM's. The pack's own schedule is still checked,
through release 1's equivalence and the `RESTORED_CELLS` pin below.

**Where it's built.**

- The app builds it once, at `Startup`: `play.rs` `power_on` calls `Play::power_on`, which calls
  `Play::running`. `Play::running` takes a built `Prg`, and `Play::power_on` builds it as
  `Prg::new(prg, Levels::original(&prg, &chr).expect(..))` before handing it on. A reset after
  GAME OVER keeps the same `Prg`.
- `Play::start` (cfg(test)) builds today's `Prg` twice, once for `room::start` and once in
  `running`. It now builds one and passes it to both.
- The ~50 app tests that go through `Play::start`/`Play::power_on` take their `Prg` from one
  cached copy in the app's tests, a `OnceLock` like `prg::for_test()`.
- The build costs about 155 ms in a debug build, measured once (`from_rom` alone takes 2 ms).
- In rotj-core, `prg::for_test()` already caches its `Prg`. `Scene::prg()` switches from
  `Prg::new(cart.prg.clone())` (about 22 builds) to the cached `prg::for_test()`, since the
  oracle's cart is the same test ROM.
- `modern.rs:608` in the app needs only `families::Tables::import(&rom.prg)`, which removes the
  app's second `Prg`.

## The schedule residue and bit 6

The overlay changes 225 cells across all 18 areas:

- 201 residue cells, where the ROM is nonzero and the import gives 0. By area: 1-1 5, 1-2 9,
  1-3 6, 2-1 8, 2-2 6, 3-1 0, 3-2 0, 3-3 8, 4-1 11, 4-2 10, 4-3 10, 5-1 3, 5-2 2, 6-1 10,
  6-2 10, 6-3 2, 7-1 0, 7-2 101.
  - Outside 7-2 they all sit on pages no room shows.
  - 7-2's 101 split into 71 on shown pages and 30 off them. None of the 71 is pending- or
    lock-shaped, which the release-1 equivalence implies. 12 of 7-2's 101 lie in `$00-$2F`.
- 24 bit-6 cells (`$C0 | id` imported as `$80 | id`), all in 7-2 (release 2).

The pack never sets a bit the ROM lacks, so the overlay gives the ROM's 256 cells exactly.

**What the game does with them.**

1. The game's `$B40A` (`room::load`) writes all 256 cells to WRAM `$7410-$750F`. The tapes hash
   that WRAM (`loop_hash`) and the lockstep diffs it. (`Schedule::project` and `lift` are the
   lockstep's projection of the port's state, not game code.)
2. The `LastArea` leftover (`wram_7000`) carries the live cells into map pages 4-5 of the four
   areas whose stream is `$100` bytes long: 2-2, 4-3, 5-2 and 7-2.
3. `collision::class_at` reads cells `$00-$2F` as the class of a metatile id `$D0` or above that
   the level has no class for. No stream holds such an id, so it could read them only through
   those leftover pages.

No evidence shows the residue reaching the picture, collision or a spawn in the original game.
It matters because it sits in WRAM that the tapes hash and the lockstep compares. Without the
overlay, the hash and the diff would differ in the 15 areas that have residue: every area except
3-1, 3-2 and 7-1.

The code that writes cells after the load is `spawn_at` and `spawn_row` (a pending cell cleared
to its id: carried cells and the 24 bit-6 cells) and `despawn` (re-arms a cell a spawn set
through `Body::cell`). None of them writes a residue cell.

**Options weighed.**

- **Mask `$7410-$750F` in the diff and the hash.** That changes every tape's bytes, so it is a
  proof change, and it would also hide real schedule bugs. Rejected.
- **Rebuild the residue on the port's side in the harness.** The bit-6 cells change at runtime
  (a spawn clears them to the id and a despawn re-arms them), and the leftover carries live cells
  into the next map, so the harness would have to track the game's own state. Rejected.
- **Adopted: `Levels::original` overlays the ROM schedule.** The game is the NES byte for byte,
  the lockstep and the tapes keep their exact rule, and no proof changes. The overlay supplies
  only what the format deliberately drops, and only for the original game. A user pack runs
  without residue (release 6). Its `$7410` and leftover pages are that pack's own content, with
  no parity to keep.

**Correcting the record.** The release-1 spec and the design spec describe the dropped cells as
"residue the game never reads". That becomes "residue no game decision is known to read; the game
copies it into WRAM, which the tapes hash" (commit 5).

## `bevy_asset_loader` (MODERNIZE #14): deferred to release 6

This is a deviation from the owner's 2026-10-23 clearance ("release 3 with
`bevy_asset_loader`"), for the owner to review. Reasons:

- Since 1.0.3 the ROM is embedded in the exe (`rom_or_exit`), so there is nothing to load yet.
  The original game is built in memory.
- `read_asset`, `prg()` and `load_driver`, the targets #14 named, are gone.
- `GameState`'s `Title`/`Playing` follow the game's own boot, not loading.
- A `Loading` state around an empty collection could shift the boot's frame timing.

Its real job comes in release 6: `--pack <dir>` loads `pack.json`, the `.tmj` rooms and
`custom.tsj` as JSON assets (`bevy_common_assets` 0.18.0) in a `LoadingState` that ends in
`import(.., Source::User)` and a `Prg` with that pack's levels. rotj-core stays Bevy-free.
Headless paths (oracle, tests, `rotj-tools`) call `Levels::original` directly, and the app's
`Startup` does too until release 6. MODERNIZE #14's row moves to "with release 6".

## New public names

- `Levels::original(prg: &[u8], chr: &[u8]) -> Option<Levels>`.
- `Prg::levels: Levels`, replacing `Prg::stages`. `Prg::new` gains the `levels` parameter.

Changed signatures, with no new names:

- `stage::at`, `checkpoint` and `level_area` take `&Levels`.
- `room::load`, `parameters`, `spawn_first_row`, `load_level`, `enter_area` and `enter` take
  `&Prg` instead of `&[u8]`.
- `scroll::step`, `horizontal`, `vertical` and `edge_spawn` take `&Prg` instead of `&[u8]`.
- The app's `Play::running` takes a `Prg`.

The password lookup is a private function in `boot.rs`. The pack format is unchanged.

## Proof

- **The game's levels**, a new test in `levels/tests.rs`:
  - `Levels::original` equals `import(export(rom))` in everything except the schedules and
    1-1-2's start mirroring (`RESTORED_MIRRORING`, a vertical room in `SingleLow`, which the
    derived rule makes `SideBySide`).
  - Every schedule equals `spawn_schedule(prg, 2 * area)`, and `stages[i].stage.area == i`.
  - `stage.rooms[i].room` is `listed_rooms`'s `(2 * area, 2 * i)`.
  - `RESTORED_CELLS = 225` pins the number of cells whose value the overlay changed, counted in
    the test. A format change that carries more cells shows up here.
  - The exceptions against `from_rom` (`raised_page_count` outside the two two-page rooms, and
    unreferenced spawn records) are inherited from release 1's equivalence.
- **The password lookup**: on the original pack every place's letters give the level
  `check_password` gives today. A hand-made `Levels` whose extra password names the second
  checkpoint gives level 1.
- **No behaviour change**:
  - the Classic suites (`cargo test -p rotj-core`, the app crate);
  - the chain tape replays with `CHAIN_COUNTS` unchanged (`the_tape_replays_the_chain`);
  - the chain lockstep (`the_loop_matches_the_tas_from_stage_1_to_the_ending`, ignored, run once
    at release) passes and re-records the tape byte for byte;
  - the sweep and the exploration are unchanged;
  - `the_app_draws_like_the_oracle_from_power_on` passes.
- Release 1's and 2's proofs (equivalence, round trip, `BIT_6_CELLS`) are untouched.

## Commits

Each commit is green on the Classic suites. Commits 2-4 also replay the chain tape.

1. `Levels::original` and its test (`levels/export.rs`, `levels/tests.rs`). The game doesn't use
   it yet.
2. `Prg::levels` replaces `Prg::stages`:
   - rotj-core: `prg.rs` (`new`, `for_test`); `actor/stage.rs` (helpers, readers, tests);
     `actor/pause.rs`; `actor/room.rs` `start`; `actor/boot.rs` `StageStart` and its test;
     `families/hurtful_burst.rs`; `families/boss_defeated_script.rs`; `actor/oracle.rs`
     `Scene::prg`, which switches from `Prg::new(cart.prg.clone())` to `prg::for_test()`.
   - app: `play.rs` (`Play::running` takes a `Prg`, `Play::start` builds one, app tests share one
     cached copy); `modern.rs` uses `families::Tables::import(&rom.prg)`.
3. Rooms, spawns, terrain and schedule through `Levels`:
   - `actor/room.rs`: `load`, `parameters`, `spawn_first_row`, `load_level`, `enter_area`,
     `enter`.
   - `actor/scroll.rs`: `step`, `horizontal`, `vertical`, `edge_spawn`.
   - Their callers.
4. Passwords through `Levels`: `actor/boot.rs` `check_password`, plus the lookup test.
5. Docs:
   - `levels.rs`'s module comment (the game now reads packs).
   - The design spec's release 3, "Implemented in 1.1.N", and its residue wording corrected.
   - The release-1 and release-2 notes on the residue answered and corrected.
   - STATUS: the row, Next, and the residue decision moved to "made".
   - MODERNIZE #14 ("with release 6").
6. Cleanup, after the release is confirmed: `collision::Level::loaded` and
   `Stage::all_from_rom`, keeping either one if tests use it.

## Not here

- Playing a user pack: `--pack <dir>`, `bevy_asset_loader` and `bevy_common_assets` (release 6).
- The aliased-byte write-back that MODERNIZE ties to level packs. Only new content can reach it,
  so it goes with user-pack play.
- A user pack's `$7410` without residue, and its leftover map pages on `$100`-byte streams.
- Which area-keyed ROM tables should follow `stage.area` (release 6).

## Open questions

1. Short streams in user packs (release 5): a pack whose stream is shorter than the pages its
   rooms show inherits the NES leftover-map behaviour, where the previous area's `$7000` image
   shows through. Should `classic` validation refuse such streams, or should the format define
   that leftover?
2. The `bevy_asset_loader` deferral above, for the owner's review.
3. 1-1-2's start mirroring (release 6): an optional room `mirroring` property would let the pack
   round-trip it without the ROM overlay; the owner decides.
4. A user pack with fewer stages or rooms than the game enters (release 6): `room_assets` and
   `stage::at` index `Levels` and panic, so `--pack` must refuse such a pack at load, by name. A
   1.1.21-1.1.43 export, which lacks 1-1's rooms 2 and 4, is one.

Status: approved by the Opus stand-in (2026-10-24) with ten changes applied (room indexing,
signatures, residue wording, overlay wording, app-test Prg reuse, one from_rom, shorter
equivalence test, area-keyed tables, password letters); bevy_asset_loader deferred to release 6
(deviation, owner to review); plus an exporter fix (stand-in, 2026-10-24): `listed_rooms` assumed
the per-area room tables do not overlap, but area 0's runs into the next, so 1.1.43's pack lacked
1-1's rooms 2 and 4. The pack now has 38 rooms (40 files), and the schema is unchanged. 1-1-2's
start mirroring (`SingleLow`, a vertical room) does not fit the derived rule, so
`Levels::original` overlays it from the ROM like the schedule residue (owner to review).
