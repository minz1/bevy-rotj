# Levels as assets, slice 5: the surfaces (1.1.x step B1e)

Status: approved with changes by the Opus stand-in, 2026-10-17 (applied: the probes' real
users; `from_stored` on the stored byte; `solid`'s ROM fallback kept; plain enum, no
outcome types; `SolidToBodies`; `solids.rs`, the dead constants and the wrong §11 table
go; the table above checked against the ROM, 0 mismatches). Fifth slice of step B1 (`docs/specs/2026-10-17-levels-as-assets-design.md`,
"Next slices" 3: "the class handlers as per-class properties instead of 6502 handler
addresses"), from a read-only survey of `collision.rs`, `actor/collision.rs` and
`terrain.rs`.

## What the survey found

- A metatile's collision class comes from the area's class table (`Terrain::classes`, raw
  0-16; `Level::classes` and WRAM `$7340` hold it `<< 1`). The only run-time reader is
  `actor::collision::class_at`; ids from `$D0` read `schedule.cells[id - $D0]` instead
  (any byte).
- Three probes index global bank-14 tables with the stored byte and the port dispatches on
  the **handler address** it reads from the ROM at run time, panicking "not ported" on any
  other: `wall` (`$C99D`, body handlers at `$CADC`; used by `head`, `column`, `ahead`,
  `corners` rising), `floor` (`$CAFE`, floor handlers at `$CD2E`; `feet`, `corners`
  falling, `stand_on_screen`) and `solid` (`$C93B`, a sign/result pair at `$C97D`;
  `below`, the player's point checks `solid_at`/`solid_x`, `weapons.rs`, `enemy_shot`,
  `hover_shot`, `stage3_boss_shot`, `falling_enemy`, `ledge_walker`; `wall` and `floor`
  also through `player.rs`'s wrappers and `falling_floor_piece`). Every caller routes
  through `actor/collision.rs`, the one dispatch point. Nothing else branches on a class.
- The 17 classes by their handlers (checked against the ROM by the stand-in, 0 mismatches):

  | class | wall | floor | solid | what it is |
  | --- | --- | --- | --- | --- |
  | 0 | `$C9DA` push | `$CB39` land | `$80` | solid |
  | 1 | `$CA2C` push | `$CB80` land, landing `+$10` | `$80` | solid, belt to the right |
  | 2 | `$CA3D` push | `$CBB8` land, landing `-$10` | `$80` | solid, belt to the left |
  | 3 | `$CA4E` damage 2, behind | `$CBF0` the same | 0 | hazard you pass behind |
  | 4 | `$CA66` damage 2, push | `$CC08` damage 2, land | `$80` | solid hazard |
  | 5 | `$CA86` push | `$CC1A` friction on `ax`, settle | `$80` | solid, slippery |
  | 6 | `$CA97` behind | `$CC61` behind | 0 | open, drawn behind the scenery |
  | 7-10 | `$CAA0` clear | `$CC6A` clear | 0 | open |
  | 11, 13 | `$C9DA` push | `$CB39` land | 0 | solid to the body and feet, clear to `solid` |
  | 12 | `$CAA3` clear | `$CC6D` crumble | flags bit 2 | crumbling |
  | 14 | `$CAA6` clear | `$CCFC` one-way | flags bit 2 | one-way platform |
  | 15 | `$CA97` behind | `$CCFC` one-way | flags bit 2 | one-way platform, drawn behind |
  | 16 | `$CAA9` spikes, push | `$CD09` spikes, land | flags bit 2 | spikes |

  "Behind" is `attr |= $20`, the OAM priority bit: the actor draws behind the background.
  The ROM's 18 areas use 12 of them (0-6, 10, 12, 14-16); 7-9, 11 and 13 never occur.
- `collision::{Collision, Kind, collision_of, collision_at}` is an older summary with no
  callers outside its tests.

## Design

- `rotj_core::collision::Surface`, a plain enum with one variant per distinct handler
  triple, which is also one per distinct behaviour: `Solid` (0), `BeltRight` (1),
  `BeltLeft` (2), `HazardBehind` (3), `SolidHazard` (4), `Slippery` (5: the floor arm zeroes
  `ax` except every 32nd frame of `age`), `Behind` (6), `Open` (7-10), `SolidToBodies`
  (11, 13: blocks the wall and floor probes, clear to `solid`'s point tests, so shots pass),
  `Crumbling` (12), `OneWay` (14), `OneWayBehind` (15), `Spikes` (16).
  `Surface::from_stored(stored: u8) -> Option<Surface>` takes the byte as stored (`2 *
  class`): `None` when it is odd or above 32. The `$D0+` fallback reads schedule cells,
  which hold any byte; today every odd or out-of-table byte reaches an unknown handler
  word and `wall`/`floor` panic "not ported", and they still do on `None`. `solid` has no
  panic today (it reads the `$C97D` pair for any byte): on `None` it keeps that ROM read,
  byte-exact for bytes that index past the table.
- The three probes match on the variant instead of the handler address and call the
  existing helpers (`push`, `damage`, `spikes`, `land`, `settle`, `crumble`); each arm's
  body moves verbatim from today's address arm. No outcome types: the helpers need `&mut
  Cx`, several floor arms leave `probe.landing` untouched, and the consumers that need a
  typed value (an editor, kaizo tooling) need the variant.
- The asset keeps raw classes: `Terrain::classes` stays `[u8; 208]` (the aliases 7-10 and
  11/13 differ at `$7340`, and area 17's tail past its 102 ids is code bytes), and
  `Level::classes` stays `<< 1`. The enum is the truth: the handler tables are fixed
  bank-14 code, not level data, so an authored level chooses a class, and the mapping test
  pins the enum to the tables.
- Deleted: `Collision`, `Kind`, `collision_of`, `collision_at`, `IN_ORDER`,
  `BODY_HANDLER_ADDR`, `HEAD_TABLE_ADDR`, `CLASS_MAX`, `tools/rotj-oracle/tests/solids.rs`
  (its checks are covered by `wall_matches_c99d`, the mapping test and
  `area_zero_table_matches_the_capture`), the handler-table and "Is this the rule?"
  paragraphs of `collision.rs`'s module doc. `docs/LEVELDATA.md` §11's class table (wrong
  today: 16 as "blocks", 15's wall handler, `$CAA0`-`$CAA6` off by one) becomes the table
  above, its `collision_at` and `solids.rs` lines go.

## Tests

- `surfaces_match_the_handler_tables`: a test-only table of variant to (wall address,
  floor address, `solid` kind), asserted for `from_stored(2 * class)`, classes 0-16,
  against the ROM's `$CADC`, `$CD2E` and `$C97D`; odd bytes and bytes above 32 give `None`.
- The probes' oracle tests (`wall_matches_c99d`, `floor_matches_cafe`, `solid_matches_c93b`,
  `spikes_match_caa9`, the others) unchanged; the Classic suites, a 1-seed sweep and a
  300-branch exploration; the tapes unchanged.

## Plan

1. Tests first: `surfaces_match_the_handler_tables` (fails to compile).
2. `Surface`, `from_stored`; the three probes dispatch on it, arms moved verbatim.
3. Delete the old summary types, `solids.rs`, the dead constants; fix `LEVELDATA.md` §11.
4. Checks, the `reviewer` agent, commit `Levels as assets: the surfaces`.

## Not here

The second-class actors' ride tables (`$CE79`/`$CE7D`, not class-driven), the landing
velocities' consumers, an authored terrain format.
