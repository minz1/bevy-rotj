# The music driver's residual bytes: the envelope step typed, the other bits named

Status: implemented in 1.1.50. Signed off by owner-proxy (L3) 2026-10-30, after the reviewer's pass: `EnvelopeStep` the
only new type, the other bits named consts and a sign-magnitude helper (stand-in [review]); every
computed step through `From`. Survey #17 (`.orchestration/survey2.md`), HANDOFF roadmap 5, approved
by the owner 2026-10-29. No behaviour change: the oracle's music tests (`every_song_to_its_loop`,
`port_matches_the_driver_for_every_song_and_effect`) prove it write for write; tapes unchanged.

## What is there

`crates/rotj-music/src/driver.rs` still carries four bytes whose bits mean different things:

- `Channel.envelope_step: u8` (`$0695`): 0 = the macro ended and the volume decays; 1-`$7F` = the
  macro position; bit 7 = decay without the release jump (`DECAY_ONLY = $FF`, set by `$ED` and
  the rest). Read as `& 0x80` in `hold` (`$829B`) and `run_envelope` (`$840B`), written as `1`,
  `0`, `DECAY_ONLY`, `position + 1`, and the macro's byte 0 (the release position) in `hold`.
  Every bit-7 value acts the same; nothing else reads the byte (no RAM projection, no lift).
- `Channel.volume: u8` (`$069D`), the `$4000`-style register byte: `Fade::scale` and `decay`
  split it as `(volume & 0xF0) | level`, and `scale` tests the constant-volume bit `& 0x10`.
- The triangle slide (`slide`, `$8394`) and the period-offset macro (`period_offset_macro`)
  read sign-magnitude bytes: bit 7 down, `& 0x7F` the amount (the macro's `$80` is a jump).
- The song table's high byte (`load_song`, `$80DE`): bit 7 = the entry is a driver-bank header
  pointer; otherwise `& 0x0F` is the FME-7 song bank and the low byte a header index.

## The design

1. **`EnvelopeStep`**, a private enum, `Default` = `Decay`:
   `At(u8)` (a macro position, 1-`$7F`), `Decay` (byte 0), `DecayOnly` (bit 7). `From<u8>` is
   the only way in from a byte: `0 → Decay`, `1..=0x7F → At`, `0x80.. → DecayOnly`, so
   `position + 1` past `$7F` and a release position byte with bit 7 land where the ROM's byte
   would. `hold` releases unless `DecayOnly`; `run_envelope` steps the macro only on `At`;
   `envelope_macro` walks from that position. `DECAY_ONLY` goes. The survey's sketch named four
   variants (`Hold`, `Decay`, `At`, `Release`); the byte has three behaviours, and the release is
   the transition in `hold`, not a stored state, so this names three.
2. **The volume byte's bits named**: `CONSTANT_VOLUME = 0x10` and `LEVEL = 0x0F` (on the triangle
   the low nibble is linear-counter bits) in `Fade::scale`, `decay` and `silent_volume`.
3. **Sign-magnitude as `i8`**: one `const fn signed(byte) -> i8` (bit 7 set: minus the low 7 bits).
   `slide` becomes `period.wrapping_add_signed(i16::from(..))`, `period_offset_macro`'s arms one
   `period_lo.saturating_add_signed(..)` after its `$80` jump arm. `$80` gives 0, as the ROM's
   subtract-0.
4. **The song table's bits named**: `IN_DRIVER_BANK = 0x80` and `BANK_BITS = 0x0F` in `load_song`;
   the FME-7 comment moves to `BANK_BITS`'s doc.

Every computed or read step (`position + 1`, the release byte in `hold`) goes through `From`;
only `At(1)`, `Decay` and `DecayOnly` are built directly.

Not here: `Volume`, `Slide` and `SongEntry` as types (the first draft): each is built and used up
within one site, and `Volume` would call the triangle's linear counter a level; named consts and
`signed` say the same with less code (stand-in [review], after the reviewer's pass). The sweep
macro's bit 7 (it is the `$4001` enable bit, written as is), the header's bit-7 terminator, the
channel `flag` bytes (they are the `$44` copy the voices hand-off ORs with). One commit, one patch
release (1.1.50), with item 4's MODERNIZE commit.

## Proof

- Written first: `EnvelopeStep::from` over all 256 bytes; `signed` over all 256 bytes against the
  old two-branch forms (wrapping `u16`, saturating `u8`); an envelope macro that steps from
  `At($7F)` into `DecayOnly` and then neither steps nor releases; a release byte with bit 7 that
  blocks a second release.
- The existing driver unit tests (`fade_volume_scaling` and the rest) unchanged.
- The oracle's music tests (both, `--ignored` included) and the full set: tape `cmp` equal.
