# Level packs, release 2: the stage descriptor (engine side)

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: an engine-side struct,
not release 1's `levels::Stage`; the pause letters are the checkpoint's password; the password
screen's own copy checked; the signature changes listed; `forced` bounded; `LEVEL_AREA_TABLE`'s
fate; `hurtful_burst`'s reason). Release 2 of `docs/specs/2026-10-18-level-packs-design.md`, split: this
is its engine side, which needs no new public name; the `pack.json` knobs wait for the owner's
answers (STATUS "Owner decisions pending"). No behaviour change: the lockstep proves it, tapes
unchanged.

## What the survey found

Everything the engine decides by area or level number beyond release 1's assets (terrain, rooms,
spawns, which already carry palettes, CHR, raster splits and the boss intro's gate records):

| Read | Decides | ROM |
| --- | --- | --- |
| `stage::request_song` (stage.rs) | the area's song; bit 7 = no request (the 5 boss areas keep the intro's song) | bank 14 `$D980[area/2]` |
| `stage::banner` | the banner's digits | bank 2 `$93F0[area/2]` |
| `pause.rs` | the pause screen's digits (equal to the banner's in all 18 areas) and its four letters, the checkpoint's password (shared along each continue group) | bank 8 `$9DD2`/`$9DE4`, `$9DF6` |
| `boot.rs` `check_password` | the password screen's 14 places (`place.min(12)`), its own copy of the passwords | the CHR payload's `$6E6C` |
| `stage.rs` (the picture before the banner) | the picture shown when `$0406` is set; pictured areas 3, 5, 8, 11, 13, 16 | bank 2 `$93C5[area/2]` |
| `stage.rs` (continue) | where Continue restarts | bank 14 `$D57D[area/2]` |
| `boot.rs`, `room.rs`, `levelmap.rs` (level -> area) | the title's and passwords' level places | bank 14 `$D94E` |
| `boss_defeated_script` | `FINAL_AREA` arms the ending; any other boss leads on | hard-coded |
| `stage.rs` (next area = area + 2) | the order | none |
| `hurtful_burst` (area `$0A`) | metasprite entry 2 there | hard-coded |

The continue table and the level table follow one rule over all 18 areas: the 13 level-table areas
continue to themselves (checkpoints); every other area continues to the latest checkpoint before
it; level n is the n-th checkpoint.

## The design

- An engine-side `stage::Stage` (in `actor/stage.rs`; release 1's `levels::Stage` is untouched, so
  the importer invents nothing): `area` (the CHR and screen-record source), `banner: [u8; 2]` (the
  digits), `music: Music` (`Song(u8)` or `Keep`), `picture: Option<u8>`,
  `password: Option<[u8; 4]>` (ASCII letters, indexing `$9E47` at `letter - $41`; `Some` exactly on
  the checkpoints, which also makes them the continue and level places), `on_clear: OnClear`
  (`NextStage` or `Ending`). `Stage::from_rom(prg, area)` reads the bytes the call sites read today.
- The engine reads it from `Prg::stages: Vec<Stage>`, built in `Prg::new` like the families'
  tables. `banner`, `request_song`, `continue_screen` and `room::start` take `&Prg` instead of
  `&[u8]` (the app's `Play` already holds a `Prg`); every read above becomes
  `prg.stages[area >> 1]`'s field: the pause shows the latest checkpoint's password at or before
  the stage, Continue restarts there, level n is the n-th checkpoint, `boss_defeated_script` asks
  `on_clear`, `hurtful_burst` keys on the stage's `area` (its other metasprite follows the loaded
  CHR). `world.area` (`$0400`) stays as it is, the stage index times 2. Release 3 moves `stages`
  into `Levels`.
- `Boot::forced` (the `$D3D8` level poke) is bounded to the levels there are.
- `LEVEL_AREA_TABLE` (a hand copy of `$D94E`, used by `room::start`, the tools and an oracle test)
  stays for the tools for now, with a test that it equals the checkpoint rule; the cleanup release
  deletes it.
- The exporter writes nothing new: the knobs go into `pack.json` once the owner names them.

## Proof

The Classic suites, a sweep and a 300-branch exploration unchanged; tapes byte-identical. Unit
tests: each descriptor field equals the ROM read it replaces for all 18 areas; the continue and
level rules derived from the checkpoints equal `$D57D` and `$D94E`; the password screen's 13 places
equal the checkpoints' passwords in order, and its 14th is the last checkpoint's.

## Cleanup after (its own release)

`rotj-music`'s `AREA_SONGS`, `song_for_area` and `STAGE_1_1_SONG` copy the ROM's `$D980` table and
have no users outside their own test: delete them; and `LEVEL_AREA_TABLE` once the tools read the
descriptors.
