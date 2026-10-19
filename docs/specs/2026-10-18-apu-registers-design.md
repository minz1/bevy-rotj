# The APU registers named and the driver's voice typed

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: the survey loop's #10 in
two releases, this one and `docs/specs/2026-10-18-music-commands-design.md`; the registers and the
voice together, since the voice is how the driver picks a register; per-channel const modules, not
an enum, because the driver writes `$400D`, which the APU ignores; names after the APU's own
methods; the flags struct dropped). The owner approved modernising the driver (2026-10-18). Implemented in 1.1.32. No
behaviour change: constant substitution and a typed index, proven by the music test's per-frame
write streams.

## What is there

- `rotj-music`'s driver writes `Write { addr, value }` with raw addresses: `voice_base(voice) =
  0x4000 + voice * 4` and `base + 1/2/3` in `write_voice` (every voice but the triangle writes
  `base + 1`, so the noise voice writes `$400D`, an idle sweep the APU ignores); literals
  `0x4000`/`0x4004`/`0x400C`/`0x4008` in `stop_all`; `0x4015` with `TONE_CHANNELS = 0x0F` and
  `ALL_CHANNELS = 0x1F`; `0x4010` in `init`; `0x4010..=0x4013` in `play_sfx`.
- `rotj-apu`'s `Apu::write` matches raw addresses to `pulse1.write_ctrl` and the like; `dmc.rs`
  names its four (`CTRL_ADDR`, `LEVEL_ADDR`, `SAMPLE_ADDR_ADDR`, `SAMPLE_LENGTH_ADDR`) and
  `ENABLE_BIT`. Tests in `apu.rs`, `crates/rotj-apu/tests/driver_trace.rs`, the app's `audio.rs` and
  `play.rs`, and `tools/rotj-oracle/tests/music.rs` use literals.
- The driver's voice is a `u8` 0-3 (`TRIANGLE = 2`, `NOISE = 3`), an index into `voices: [u8; 4]`
  (`$4F-$52`, with `VOICE_TAKEN`/`VOICE_RETRIGGER`) and the register base; header voice bytes index
  it raw.

## The change

- In `rotj-apu` (which `rotj-music` already depends on), per-channel modules of register addresses,
  named after the APU's methods: `pulse1`/`pulse2::{CTRL, SWEEP, TIMER_LO, TIMER_HI}`,
  `triangle::{CTRL, TIMER_LO, TIMER_HI}`, `noise::{CTRL, UNUSED, TIMER, LENGTH}` (`UNUSED` is
  `$400D`, doc'd as the driver's idle sweep the APU ignores; `TIMER` is period and mode, `LENGTH`
  the length load), `dmc::{CTRL, LEVEL, ADDRESS, LENGTH}` (replacing `dmc.rs`'s `*_ADDR`), `STATUS`,
  `FRAME_COUNTER`, and `status::{PULSE1, PULSE2, TRIANGLE, NOISE, DMC, TONE, ALL}` (`ENABLE_BIT`
  folded in; `TONE` = the four tone channels, `ALL` = with the DMC; `enable & 0x0F` becomes
  `& status::TONE`). `Apu::write` matches on them; its tests use them.
- In `rotj-music`, `enum Voice { Pulse1, Pulse2, Triangle, Noise }` (`Default` = `Pulse1`) with one
  `match` returning its four register addresses from those consts (the triangle's `+1` never
  written), replacing `voice_base` and `base + n`; `voices` stays `[u8; 4]` indexed by `Voice`; a
  header's voice byte converts with a panic above 3, as the index panics today; `TRIANGLE`/`NOISE`
  go. `stop_all`, `init`, `play_sfx` and the `$4015` writes use the names; `TONE_CHANNELS`/
  `ALL_CHANNELS` become `status::TONE`/`ALL`; `silent_volume`'s `0x10` may be named as `CTRL`'s
  constant-volume bit.
- The tests that assert register writes (`driver_trace.rs`, `audio.rs`, `play.rs`, `music.rs`) use
  the names where they spell an address.
- Out of scope: the commands (the next release), the flag bytes (already `mod flag`), the APU's
  internal register decoding beyond the address match.

## Proof

The music test (`tools/rotj-oracle/tests/music.rs`) mirrors every driver call from power-on and
requires the port's and the oracle's register-write streams equal per frame, in order, over the
physics run, run1, every song id `$00-$2F` for 480 frames and fade-then-song, plus its ignored TAS
run; with a pause-and-resume case added (`[(0, song), (60, 0xFE), (120, 0xFD)]`) so `park_active`'s
`$4015` write is reached. The rotj-core lockstep does not prove the driver (it runs only from the
NMI, so its RAM never changes inside a bracket); it proves the `$40-$43` requests. Plus the APU's
and the app's audio tests, the Classic suites, a sweep and a 300-branch exploration unchanged; tapes
byte-identical.
