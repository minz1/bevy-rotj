# The sound, music and song ids named

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: the survey loop's #7 split
in two, these ids first and the spawned kinds after them, `docs/specs/2026-10-18-kind-ids-design.md`;
`$45` added; the guessed names replaced by names from the call sites and ACTORS.md; the songs
`$03`, `$0A`, `$3A`, `$44` named from their sites; `$FC` is the driver's fade-out; the module says
the four slots share one id space; the per-family `*_SOUND` consts folded in; the counts; the
off-lockstep guard). No behaviour change: constant substitution, proven by the lockstep.

## What is there

`Effects.music` (`$40`), `song` (`$41`) and `sound` (`$42`, `$43`) are the driver's four request
slots; the driver treats them alike (`tick` -> `handle_request` -> `load_song`), so the ids are one
space and the slot belongs to the call site. They are written raw: 46 `sound[n] = 0x..` sites, four
literals in `player.rs`'s shot match and two in `stage.rs`, `Row::Sound(0x23)` in `ending.rs`, and
16 sites through per-family `*_SOUND` consts (aliases of 7 ids); 24 music commands (19 `$FF`, 3
`$FC`, one each `$FE`/`$FD`) and 3 through two local consts; 10 song ids (`attract.rs` writes `2`)
and `next_area_cue`'s `SONG`. The stage's own song comes from the stage descriptor, and the sound
test reads its table: neither is a literal.

## The change

`pub mod sound` in `actor/world.rs` beside `Effects` (the shape of `hurt` and `pose`), its doc
saying the four slots share one id space; effects flat in it, `sound::music` and `sound::song`
inside:

| id | name | id | name | id | name |
| --- | --- | --- | --- | --- | --- |
| 20 | BLOCK_BREAKS | 2D | SHOT | 3B | CONFIRM |
| 21 | ACTOR_DIES | 2E | CHARGE | 3C | SHOT_DEFLECTED |
| 22 | IMPACT | 2F | SPLITTER_SHOT | 3D | DROP |
| 23 | EXPLOSION | 30 | SPLITTER_ORBIT | 3E | TALLY_TICK |
| 24 | JUMP | 31 | POWER_UP | 3F | THREE_WAY_BUBBLE |
| 25 | PLAYER_HURT | 33 | PLAYER_DIES | 40 | DEATH_TICK |
| 26 | PICKUP | 34 | DIVER_LANDS | 41 | COUNTDOWN_TICK |
| 28 | ENEMY_FIRES | 35 | STOMP | 43 | WAVE_SHOT |
| 29 | THREE_WAY_SHOT | 37 | LEVEL_START | 45 | PASSWORD_REJECTED |
| 2A | THREE_WAY_TETHER | 38 | ACTOR_HIT | 46 | COUNTDOWN_END |
| 2B | SPAWN | 39 | PASSWORD_LETTER | 47 | SUPER_UNLEASH |
| 2C | WAVE_SPREAD | | | 48 | SUPER_ORBITER |

Doc lines name each id's sites where the name doesn't (`JUMP` also the slide's launch, `STOMP`
also `volley_thrower`'s volley, `CHARGE` also `hop_thrower`'s throw, `LEVEL_START` ACTORS' `$1C`).

- `sound::music`: `$FF` `STOP`, `$FC` `FADE_OUT`, `$FE` `PAUSE`, `$FD` `RESUME`.
- `sound::song` (requested through `$41`): `$01` `TITLE`, `$02` `ATTRACT`, `$03` `BOSS_FIGHT`, `$08`
  `GAME_OVER`, `$0A` `BOSS_CARD`, `$0D` `ENDING`, `$0E` `STAGE_CLEAR`, `$0F` `BANNER`, `$3A`
  `PAUSE` (both pause and resume), `$44` `NEXT_AREA`.
- Every site above uses the names; the 16 `*_SOUND` consts and the two local music consts go,
  a ROM address their doc held moving to the function's doc where it isn't there yet.
  `rotj_music::JUMP_SFX` stays (the core takes no dependency on rotj-music). `Effects.music`'s doc
  points at `sound::music`.
- Docs fixed on the way: `docs/MUSIC.md`'s `$FC` ("restart with tempo scaling") is the fade-out
  (`start_fade`, `$81A2`); `next_area_cue.rs`'s header puts song `$44` at `$41`, not `$40`.

## Proof

The lockstep compares the port with the oracle at `$D45C`, after each loop frame and before the
oracle's NMI clears the requests, so a wrong id on any path it runs shows at `$40-$43`. Off the
lockstep's paths: the diff only swaps literals for consts of the same value, and afterwards nothing
matches `sound\[[01]\] = 0x|music = 0x|song = 0x` except the table reads (`stage.rs`, the sound
test). The Classic suites, a sweep and a 300-branch exploration unchanged; tapes byte-identical.
